use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};
use tauri_plugin_global_shortcut::{Shortcut, ShortcutState, Modifiers, Code, GlobalShortcutExt};

// ============ 灵动岛窗口管理 ============

const ISLAND_WIDTH: f64 = 260.0;
const ISLAND_HEIGHT: f64 = 56.0;

fn get_island_position(app: &AppHandle) -> (i32, i32) {
    if let Ok(Some(window)) = app.get_webview_window("main") {
        if let Ok(Some(monitor)) = window.current_monitor() {
            let size = monitor.size();
            let scale = monitor.scale_factor();
            let x = ((size.width as f64 / scale) / 2.0 - ISLAND_WIDTH / 2.0) as i32;
            let y = ((size.height as f64 / scale) * 0.78) as i32;
            return (x, y);
        }
    }
    (500, 600)
}

fn position_island_window(app: &AppHandle) -> Result<(), String> {
    if let Ok(Some(window)) = app.get_webview_window("island") {
        let (x, y) = get_island_position(app);
        let _ = window.set_position(tauri::Position::Logical(tauri::LogicalPosition { x: x as f64, y: y as f64 }));
    }
    Ok(())
}

fn show_island(app: &AppHandle) {
    let _ = position_island_window(app);
    if let Ok(Some(window)) = app.get_webview_window("island") {
        let (x, y) = get_island_position(app);
        let _ = window.set_position(tauri::Position::Logical(tauri::LogicalPosition { x: x as f64, y: y as f64 }));
        let _ = window.show();
        let _ = window.set_focus();
        let _ = app.emit("island:show", ());
    }
}

fn hide_island(app: &AppHandle) {
    if let Ok(Some(window)) = app.get_webview_window("island") {
        let _ = window.hide();
        let _ = app.emit("island:hide", ());
    }
}

fn toggle_island(app: &AppHandle) {
    if let Ok(Some(window)) = app.get_webview_window("island") {
        if window.is_visible().unwrap_or(false) {
            hide_island(app);
        } else {
            show_island(app);
        }
    } else {
        show_island(app);
    }
}

// ============ Tauri 命令（前端调用）============

#[tauri::command]
fn cmd_toggle_island(app: AppHandle) {
    toggle_island(&app);
}

#[tauri::command]
fn cmd_hide_island(app: AppHandle) {
    hide_island(&app);
}

#[tauri::command]
fn cmd_show_main(app: AppHandle) {
    if let Ok(Some(window)) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[tauri::command]
fn cmd_hide_main(app: AppHandle) {
    if let Ok(Some(window)) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

#[tauri::command]
fn cmd_get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// ============ 系统托盘 ============

fn build_tray_menu<R: tauri::Runtime>(app: &AppHandle<R>) -> Result<Menu<R>, tauri::Error> {
    let toggle_i = MenuItem::with_id(app, "toggle", "切换语音输入", true, None::<&str>)?;
    let show_i = MenuItem::with_id(app, "show", "打开设置", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit_i = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    Menu::with_items(app, &[&toggle_i, &show_i, &sep, &quit_i])
}

fn setup_tray(app: &AppHandle) -> Result<(), tauri::Error> {
    let menu = build_tray_menu(app)?;

    let _tray = TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("VoxLink 语音输入助手\n快捷键: Ctrl+Shift+V")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            match event.id.as_ref() {
                "toggle" => toggle_island(app),
                "show" => {
                    if let Ok(Some(window)) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
                "quit" => {
                    let _ = app.emit("app:exit", ());
                    app.exit(0);
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, .. } = event {
                let app = tray.app_handle();
                if let Ok(Some(window)) = app.get_webview_window("main") {
                    if window.is_visible().unwrap_or(false) {
                        let _ = window.hide();
                    } else {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
            }
        })
        .build(app)?;

    Ok(())
}

// ============ 主入口 ============

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_process::init())
        .invoke_handler(tauri::generate_handler![
            cmd_toggle_island,
            cmd_hide_island,
            cmd_show_main,
            cmd_hide_main,
            cmd_get_app_version,
        ])
        .setup(|app| {
            // 隐藏主窗口（后台运行）
            if let Ok(Some(window)) = app.get_webview_window("main") {
                let _ = window.hide();
            }

            // 调整灵动岛窗口位置
            let _ = position_island_window(app.handle());

            // 设置系统托盘
            let _ = setup_tray(app.handle());

            // 注册全局快捷键
            let gs = app.global_shortcut();
            let shortcut = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyV);
            let _ = gs.on_shortcut(shortcut, |app, _shortcut, event| {
                if event.state == ShortcutState::Pressed {
                    toggle_island(app);
                }
            });

            Ok(())
        })
        .on_window_event(|app, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if let Ok(Some(window)) = app.get_webview_window("main") {
                    if window.label() == "main" {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running VoxLink");
}
