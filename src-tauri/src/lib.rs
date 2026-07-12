// VoxLink Tauri 入口及插件配置
// 负责：窗口管理、全局快捷键、Tray、模块协调

mod audio;
mod caret;
mod injector;

use parking_lot::Mutex;
use serde::Serialize;
use std::sync::Arc;
use tauri::{
    Manager, Window, Emitter, PhysicalPosition, PhysicalSize,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    menu::{MenuBuilder, MenuItemBuilder},
};

/// 应用状态枚举
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AppState {
    Idle,
    Listening,
    Processing,
    Injecting,
    Error(String),
}

/// 前向声明：传给前端的应用状态数据
#[derive(Debug, Clone, Serialize)]
pub struct StatePayload {
    pub state: AppState,
    pub transcript: String,
    #[serde(rename = "errorMessage")]
    pub error_message: String,
}

/// 共享应用状态
pub struct SharedState {
    pub state: Arc<Mutex<AppState>>,
    pub transcript: Arc<Mutex<String>>,
}

impl SharedState {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(AppState::Idle)),
            transcript: Arc::new(Mutex::new(String::new())),
        }
    }
}

/// 创建灵动岛悬浮窗口
/// macOS: 配置为 NSPanel 风格，不夺取焦点
/// Windows: 配置 WS_EX_TOPMOST | WS_EX_NOACTIVATE
fn create_floating_window(app: &tauri::AppHandle) -> Result<Window, tauri::Error> {
    let window = tauri::WebviewWindowBuilder::new(
        app,
        "floating",
        tauri::WebviewUrl::App("index.html".into()),
    )
    .title("VoxLink")
    .inner_size(400.0, 120.0)
    .resizable(false)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .visible(false)
    .focused(false)
    .build()?;

    #[cfg(target_os = "macos")]
    {
        use cocoa::appkit::{NSWindow, NSWindowCollectionBehavior};
        use cocoa::base::YES;
        use objc::{msg_send, sel, sel_impl};

        unsafe {
            let ns_window = window.ns_window().unwrap() as *mut objc::runtime::Object;

            // 设置为 NSPanel 行为：不夺取焦点
            let _: () = msg_send![ns_window, setLevel: 1000]; // NSFloatingWindowLevel

            // 设置 collectionBehavior：跨空间、全屏跟随
            let behavior = NSWindowCollectionBehavior::NSWindowCollectionBehaviorCanJoinAllSpaces
                | NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenAuxiliary
                | NSWindowCollectionBehavior::NSWindowCollectionBehaviorStationary
                | NSWindowCollectionBehavior::NSWindowCollectionBehaviorIgnoresCycle;
            let _: () = msg_send![ns_window, setCollectionBehavior: behavior];

            // 设置 hasShadow = NO 以减少视觉干扰
            #[allow(deprecated)]
            let _: () = msg_send![ns_window, setHasShadow: YES];
        }
    }

    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::WindowsAndMessaging::{
            SetWindowLongW, GetWindowLongW, GWL_EXSTYLE,
            WS_EX_TOPMOST, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
        };
        use windows::Win32::Foundation::HWND;

        unsafe {
            let hwnd = window.hwnd().unwrap().0 as isize;
            let hwnd = HWND(hwnd as *mut _);

            let mut ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
            ex_style |= WS_EX_TOPMOST.0 as i32;
            ex_style |= WS_EX_NOACTIVATE.0 as i32;
            ex_style |= WS_EX_TOOLWINDOW.0 as i32;
            SetWindowLongW(hwnd, GWL_EXSTYLE, ex_style);
        }
    }

    Ok(window)
}

/// 将窗口居中于屏幕顶部
fn position_window_top_center(window: &Window) {
    if let Some(monitor) = window.available_monitors().ok().and_then(|m| m.into_iter().next()) {
        let size = monitor.size();
        let window_size = window.outer_size().unwrap_or(PhysicalSize::new(400, 120));

        let x = (size.width as i32 - window_size.width as i32) / 2;
        let y = 40i32; // 距离顶部 40px

        let _ = window.set_position(PhysicalPosition::new(x, y));
    }
}

/// 显示/隐藏灵动岛窗口
fn toggle_floating(window: &Window, shared: &SharedState) {
    let is_visible = window.is_visible().unwrap_or(false);

    if is_visible {
        let _ = window.hide();
        let mut state = shared.state.lock();
        *state = AppState::Idle;
    } else {
        position_window_top_center(window);
        let _ = window.show();
        let _ = window.set_focus();
        let mut state = shared.state.lock();
        *state = AppState::Listening;
    }
}

/// 发送状态更新到前端
fn emit_state(app: &tauri::AppHandle, shared: &SharedState) {
    let state = shared.state.lock().clone();
    let transcript = shared.transcript.lock().clone();

    let error_message = match &state {
        AppState::Error(msg) => msg.clone(),
        _ => String::new(),
    };

    let payload = StatePayload {
        state,
        transcript,
        error_message,
    };

    if let Some(window) = app.get_webview_window("floating") {
        let _ = window.emit("voxlink-state", &payload);
    }
}

/// 完整的语音识别管道
async fn run_recognition_pipeline(
    app: tauri::AppHandle,
    shared: Arc<SharedState>,
) {
    // 1. 设置状态为 Listening
    {
        let mut state = shared.state.lock();
        *state = AppState::Listening;
        shared.transcript.lock().clear();
    }
    emit_state(&app, &shared);

    // 2. 获取光标上下文
    let caret_context = caret::get_caret_context().unwrap_or_else(|e| {
        log::warn!("[VoxLink] 获取光标上下文失败: {}", e);
        caret::CaretContext::default()
    });

    log::info!("[VoxLink] 光标上下文: before={}, after={}",
        caret_context.before_text.len(), caret_context.after_text.len());

    // 3. 捕获音频并执行 VAD
    let vad_result = audio::capture_with_vad().await;

    match vad_result {
        Ok(audio_samples) => {
            log::info!("[VoxLink] 语音捕获完成，样本数: {}", audio_samples.len());

            // 4. 设置状态为 Processing
            {
                let mut state = shared.state.lock();
                *state = AppState::Processing;
            }
            emit_state(&app, &shared);

            // 5. 模拟 ASR 识别（实际项目中替换为 Whisper/ASR 引擎调用）
            let raw_text = simulate_asr(&audio_samples).await;

            // 6. 模拟大模型纠错（实际项目中替换为 LLM API 调用）
            let corrected_text = simulate_llm_correction(
                &raw_text,
                &caret_context.before_text,
                &caret_context.after_text,
            ).await;

            log::info!("[VoxLink] 纠错后文本: {}", corrected_text);

            // 7. 设置状态为 Injecting
            {
                let mut state = shared.state.lock();
                *state = AppState::Injecting;
                *shared.transcript.lock() = corrected_text.clone();
            }
            emit_state(&app, &shared);

            // 8. 注入文本到光标位置
            match injector::inject_text(&corrected_text).await {
                Ok(()) => {
                    log::info!("[VoxLink] 文本注入成功");
                }
                Err(e) => {
                    log::error!("[VoxLink] 文本注入失败: {}", e);
                    let mut state = shared.state.lock();
                    *state = AppState::Error(format!("注入失败: {}", e));
                    emit_state(&app, &shared);
                    return;
                }
            }

            // 9. 完成，恢复 Idle
            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
            {
                let mut state = shared.state.lock();
                *state = AppState::Idle;
                shared.transcript.lock().clear();
            }
            emit_state(&app, &shared);

            // 隐藏窗口
            if let Some(window) = app.get_webview_window("floating") {
                let _ = window.hide();
            }
        }
        Err(e) => {
            log::error!("[VoxLink] 音频捕获失败: {}", e);
            let mut state = shared.state.lock();
            *state = AppState::Error(format!("音频捕获失败: {}", e));
            emit_state(&app, &shared);
        }
    }
}

/// 模拟 ASR 识别（实际项目中替换为真正的 ASR 引擎）
async fn simulate_asr(_samples: &[f32]) -> String {
    // 在实际项目中，这里会调用 Whisper.cpp、SenseVoice 或其他 ASR 引擎
    // 将 16kHz mono f32 音频样本送入模型进行识别
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    "你好这是一段语音输入的测试文本".to_string()
}

/// 模拟大模型纠错（实际项目中替换为 LLM API 调用）
async fn simulate_llm_correction(
    raw_text: &str,
    context_before: &str,
    context_after: &str,
) -> String {
    // 在实际项目中，这里会调用 LLM API 进行纠错
    // System Prompt 见项目文档
    log::info!("[VoxLink] LLM纠错 - 上下文前: {:?}, 后: {:?}", context_before, context_after);

    tokio::time::sleep(std::time::Duration::from_millis(600)).await;

    // 模拟纠错：添加标点、修正错别字
    let mut corrected = raw_text.to_string();
    if !corrected.ends_with('。') && !corrected.ends_with('！') && !corrected.ends_with('？') {
        corrected.push('。');
    }
    corrected
}

/// 构建 Tray 菜单
fn build_tray_menu(app: &tauri::AppHandle) -> Result<tauri::menu::Menu<tauri::Wry>, tauri::Error> {
    let toggle = MenuItemBuilder::with_id("toggle", "开始语音输入")
        .accelerator("Alt+Space")
        .build(app)?;
    let separator = tauri::menu::PredefinedMenuItem::separator(app)?;
    let quit = MenuItemBuilder::with_id("quit", "退出 VoxLink")
        .accelerator("CmdOrCtrl+Q")
        .build(app)?;

    let menu = MenuBuilder::new(app)
        .item(&toggle)
        .item(&separator)
        .item(&quit)
        .build()?;

    Ok(menu)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let shared = Arc::new(SharedState::new());

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_shell::init())
        .setup(move |app| {
            // 创建浮动窗口
            let window = create_floating_window(app.handle())?;
            log::info!("[VoxLink] 浮动窗口创建成功");

            // 检查 macOS 权限
            #[cfg(target_os = "macos")]
            {
                use objc::{msg_send, sel, sel_impl};
                use objc::runtime::Object;
                use objc_foundation::INSString;
                use objc_foundation::NSString;

                let trusted: bool = unsafe {
                    let options: *mut Object = msg_send![objc::class!(NSDictionary), dictionary];
                    msg_send![objc::class!(AXIsProcessTrusted), AXIsProcessTrustedWithOptions: options]
                };

                if !trusted {
                    log::warn!("[VoxLink] 辅助功能权限未授予，打开系统设置...");
                    unsafe {
                        let url_str = NSString::from_str(
                            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
                        );
                        let workspace: *mut Object = msg_send![objc::class!(NSWorkspace), sharedWorkspace];
                        let url: *mut Object = msg_send![objc::class!(NSURL), URLWithString: url_str];
                        let _: () = msg_send![workspace, openURL: url];
                    }
                } else {
                    log::info!("[VoxLink] 辅助功能权限已授予");
                }
            }

            // 构建 Tray 图标
            let tray_menu = build_tray_menu(app.handle())?;
            let _tray = TrayIconBuilder::new()
                .menu(&tray_menu)
                .tooltip("VoxLink - 语音输入助手")
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("floating") {
                            let shared = app.state::<Arc<SharedState>>();
                            toggle_floating(&window, &shared);
                        }
                    }
                })
                .on_menu_event(move |app, event| {
                    match event.id().as_ref() {
                        "toggle" => {
                            if let Some(window) = app.get_webview_window("floating") {
                                let shared = app.state::<Arc<SharedState>>();
                                toggle_floating(&window, &shared);
                            }
                        }
                        "quit" => {
                            log::info!("[VoxLink] 用户请求退出");
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .build(app)?;

            // 注册全局快捷键回调
            let app_handle = app.handle().clone();
            let shared_clone = shared.clone();
            app.handle().plugin(
                tauri_plugin_global_shortcut::Builder::new()
                    .with_handler(move |_app, shortcut, event| {
                        if shortcut.matches("Alt+Space") && event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                            if let Some(win) = app_handle.get_webview_window("floating") {
                                toggle_floating(&win, &shared_clone);
                            }
                        }
                    })
                    .build(),
            )?;

            // 注册全局快捷键
            #[cfg(desktop)]
            {
                use tauri_plugin_global_shortcut::GlobalShortcutExt;
                let _ = app.handle().plugin(
                    tauri_plugin_global_shortcut::Builder::new().build(),
                );
                app.global_shortcut().register("Alt+Space")?;
            }

            // 管理状态
            app.manage(shared.clone());
            app.manage(window.clone());

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                if window.label() == "floating" {
                    let _ = window.hide();
                    // 阻止关闭，仅隐藏
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            start_recognition,
            stop_recognition,
            get_state,
        ])
        .run(tauri::generate_context!())
        .expect("VoxLink 启动失败");
}

/// Tauri Command: 开始语音识别
#[tauri::command]
async fn start_recognition(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<SharedState>>,
) -> Result<(), String> {
    let shared = state.inner().clone();
    let app_handle = app.clone();

    // 显示窗口
    if let Some(window) = app.get_webview_window("floating") {
        position_window_top_center(&window);
        let _ = window.show();
        let _ = window.set_focus();
    }

    // 在后台执行识别管道
    tokio::spawn(async move {
        run_recognition_pipeline(app_handle, shared).await;
    });

    Ok(())
}

/// Tauri Command: 停止语音识别
#[tauri::command]
async fn stop_recognition(
    state: tauri::State<'_, Arc<SharedState>>,
) -> Result<(), String> {
    let mut s = state.state.lock();
    *s = AppState::Idle;
    state.transcript.lock().clear();
    Ok(())
}

/// Tauri Command: 获取当前状态
#[tauri::command]
async fn get_state(
    state: tauri::State<'_, Arc<SharedState>>,
) -> Result<StatePayload, String> {
    let s = state.state.lock().clone();
    let t = state.transcript.lock().clone();
    let err = match &s {
        AppState::Error(msg) => msg.clone(),
        _ => String::new(),
    };
    Ok(StatePayload {
        state: s,
        transcript: t,
        error_message: err,
    })
}