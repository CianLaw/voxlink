// VoxLink Tauri 入口及插件配置
// 负责：窗口管理、全局快捷键、Tray、模块协调
// 支持桌面端（macOS/Windows/Linux）和移动端（Android/iOS）

mod audio;
mod caret;
mod injector;

use parking_lot::Mutex;
use serde::Serialize;
use std::sync::Arc;
use tauri::{
    Manager, Emitter, PhysicalPosition, PhysicalSize,
    WebviewWindow,
};

#[cfg(desktop)]
use tauri::{
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    menu::{MenuBuilder, MenuItemBuilder},
};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AppState {
    Idle,
    Listening,
    Processing,
    Injecting,
    Error(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct StatePayload {
    pub state: AppState,
    pub transcript: String,
    #[serde(rename = "errorMessage")]
    pub error_message: String,
}

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

#[cfg(desktop)]
fn create_floating_window(app: &tauri::AppHandle) -> Result<WebviewWindow, tauri::Error> {
    let window = tauri::WebviewWindowBuilder::new(
        app,
        "floating",
        tauri::WebviewUrl::App("index.html".into()),
    )
    .title("VoxLink")
    .inner_size(400.0, 120.0)
    .resizable(false)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .visible(false)
    .focused(false)
    .build()?;

    #[cfg(target_os = "macos")]
    {
        use cocoa::appkit::NSWindowCollectionBehavior;
        use cocoa::base::YES;
        use objc::{msg_send, sel, sel_impl};

        unsafe {
            let ns_window = window.ns_window().unwrap() as *mut objc::runtime::Object;
            let _: () = msg_send![ns_window, setLevel: 1000];
            let behavior = NSWindowCollectionBehavior::NSWindowCollectionBehaviorCanJoinAllSpaces
                | NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenAuxiliary
                | NSWindowCollectionBehavior::NSWindowCollectionBehaviorStationary
                | NSWindowCollectionBehavior::NSWindowCollectionBehaviorIgnoresCycle;
            let _: () = msg_send![ns_window, setCollectionBehavior: behavior];
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

#[cfg(mobile)]
fn create_mobile_window(app: &tauri::AppHandle) -> Result<WebviewWindow, tauri::Error> {
    tauri::WebviewWindowBuilder::new(
        app,
        "main",
        tauri::WebviewUrl::App("index.html".into()),
    )
    .title("VoxLink")
    .build()
}

#[cfg(desktop)]
fn position_window_top_center(window: &WebviewWindow) {
    if let Ok(Some(monitor)) = window.available_monitors().map(|m| m.into_iter().next()) {
        let size = monitor.size();
        let ws = window.outer_size().unwrap_or(PhysicalSize::new(400, 120));
        let x = (size.width as i32 - ws.width as i32) / 2;
        let _ = window.set_position(PhysicalPosition::new(x, 40));
    }
}

#[cfg(desktop)]
fn toggle_floating(window: &WebviewWindow, shared: &SharedState) {
    if window.is_visible().unwrap_or(false) {
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

fn emit_state(app: &tauri::AppHandle, shared: &SharedState) {
    let state = shared.state.lock().clone();
    let transcript = shared.transcript.lock().clone();
    let error_message = match &state {
        AppState::Error(msg) => msg.clone(),
        _ => String::new(),
    };
    let payload = StatePayload { state, transcript, error_message };
    #[cfg(desktop)]
    {
        if let Some(window) = app.get_webview_window("floating") {
            let _ = window.emit("voxlink-state", &payload);
        }
    }
    #[cfg(mobile)]
    {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.emit("voxlink-state", &payload);
        }
    }
}

async fn run_recognition_pipeline(app: tauri::AppHandle, shared: Arc<SharedState>) {
    {
        let mut state = shared.state.lock();
        *state = AppState::Listening;
        shared.transcript.lock().clear();
    }
    emit_state(&app, &shared);

    let caret_context = caret::get_caret_context().unwrap_or_else(|e| {
        log::warn!("[VoxLink] 获取光标上下文失败: {}", e);
        caret::CaretContext::default()
    });

    let vad_result = audio::capture_with_vad().await;

    match vad_result {
        Ok(audio_samples) => {
            log::info!("[VoxLink] 语音捕获完成，样本数: {}", audio_samples.len());
            {
                let mut state = shared.state.lock();
                *state = AppState::Processing;
            }
            emit_state(&app, &shared);

            let raw_text = simulate_asr(&audio_samples).await;
            let corrected_text = simulate_llm_correction(
                &raw_text, &caret_context.before_text, &caret_context.after_text,
            ).await;

            log::info!("[VoxLink] 纠错后文本: {}", corrected_text);
            {
                let mut state = shared.state.lock();
                *state = AppState::Injecting;
                *shared.transcript.lock() = corrected_text.clone();
            }
            emit_state(&app, &shared);

            match injector::inject_text(&corrected_text).await {
                Ok(()) => log::info!("[VoxLink] 文本注入成功"),
                Err(e) => {
                    log::error!("[VoxLink] 文本注入失败: {}", e);
                    let mut state = shared.state.lock();
                    *state = AppState::Error(format!("注入失败: {}", e));
                    emit_state(&app, &shared);
                    return;
                }
            }

            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
            {
                let mut state = shared.state.lock();
                *state = AppState::Idle;
                shared.transcript.lock().clear();
            }
            emit_state(&app, &shared);
            #[cfg(desktop)]
            {
                if let Some(window) = app.get_webview_window("floating") {
                    let _ = window.hide();
                }
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

async fn simulate_asr(_samples: &[f32]) -> String {
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    "你好这是一段语音输入的测试文本".to_string()
}

async fn simulate_llm_correction(raw_text: &str, _before: &str, _after: &str) -> String {
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    let mut corrected = raw_text.to_string();
    if !corrected.ends_with('。') && !corrected.ends_with('！') && !corrected.ends_with('？') {
        corrected.push('。');
    }
    corrected
}

#[cfg(desktop)]
fn build_tray_menu(app: &tauri::AppHandle) -> Result<tauri::menu::Menu<tauri::Wry>, tauri::Error> {
    let toggle = MenuItemBuilder::with_id("toggle", "开始语音输入")
        .accelerator("Alt+Space")
        .build(app)?;
    let separator = tauri::menu::PredefinedMenuItem::separator(app)?;
    let quit = MenuItemBuilder::with_id("quit", "退出 VoxLink")
        .accelerator("CmdOrCtrl+Q")
        .build(app)?;
    MenuBuilder::new(app).item(&toggle).item(&separator).item(&quit).build()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let shared = Arc::new(SharedState::new());

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_shell::init());

    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_global_shortcut::Builder::new().build());
    }

    builder = builder.setup(move |app| {
        #[cfg(desktop)]
        {
            let window = create_floating_window(app.handle())?;
            log::info!("[VoxLink] 浮动窗口创建成功");

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
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .build(app)?;

            // 全局快捷键：Alt+Space 切换浮窗
            let app_handle = app.handle().clone();
            let shared_clone = shared.clone();
            app.handle().plugin(
                tauri_plugin_global_shortcut::Builder::new()
                    .with_handler(move |_app, shortcut, _event| {
                        if shortcut.to_string() == "Alt+Space" {
                            if let Some(win) = app_handle.get_webview_window("floating") {
                                toggle_floating(&win, &shared_clone);
                            }
                        }
                    })
                    .build(),
            )?;
        }

        #[cfg(mobile)]
        {
            let _window = create_mobile_window(app.handle())?;
            log::info!("[VoxLink] 移动端主窗口创建成功");
        }

        app.manage(shared.clone());
        Ok(())
    });

    #[cfg(desktop)]
    {
        builder = builder.on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                if window.label() == "floating" {
                    let _ = window.hide();
                }
            }
        });
    }

    builder
        .invoke_handler(tauri::generate_handler![start_recognition, stop_recognition, get_state])
        .run(tauri::generate_context!())
        .expect("VoxLink 启动失败");
}

#[tauri::command]
async fn start_recognition(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<SharedState>>,
) -> Result<(), String> {
    let shared = state.inner().clone();
    let app_handle = app.clone();
    #[cfg(desktop)]
    {
        if let Some(window) = app.get_webview_window("floating") {
            position_window_top_center(&window);
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
    #[cfg(mobile)]
    {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
        }
    }
    tokio::spawn(async move { run_recognition_pipeline(app_handle, shared).await });
    Ok(())
}

#[tauri::command]
async fn stop_recognition(state: tauri::State<'_, Arc<SharedState>>) -> Result<(), String> {
    let mut s = state.state.lock();
    *s = AppState::Idle;
    state.transcript.lock().clear();
    Ok(())
}

#[tauri::command]
async fn get_state(state: tauri::State<'_, Arc<SharedState>>) -> Result<StatePayload, String> {
    let s = state.state.lock().clone();
    let t = state.transcript.lock().clone();
    let err = match &s {
        AppState::Error(msg) => msg.clone(),
        _ => String::new(),
    };
    Ok(StatePayload { state: s, transcript: t, error_message: err })
}
