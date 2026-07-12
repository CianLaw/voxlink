// VoxLink - 高精度跨平台语音输入助手
// 主入口：启动 Tauri 应用，初始化音频、光标、注入模块

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    log::info!("[VoxLink] 启动语音输入助手...");

    voxlink_lib::run();
}