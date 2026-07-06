//! Water2Rust 桌面后端（Tauri 2）。
//!
//! 经 `#[tauri::command]` 把纯 Rust 业务能力（water-core/io/fclass/hydro/edge-depth）
//! 暴露给 React 前端；`tracing` 日志经事件 `pipeline://log` 实时推送。

mod commands;
mod logbridge;
mod pipeline;

/// Tauri 应用入口（供 `main.rs` 与未来移动端复用）。
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // tracing → 通道；进入 setup 后转发为前端事件。
    let log_rx = logbridge::init_tracing();
    let log_rx = std::sync::Mutex::new(Some(log_rx));

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            if let Some(rx) = log_rx.lock().unwrap().take() {
                logbridge::spawn_log_forwarder(app.handle().clone(), rx);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::greet,
            commands::get_edge_guidance,
            commands::gpu_available,
            pipeline::run_pipeline,
        ])
        .run(tauri::generate_context!())
        .expect("运行 Tauri 应用失败");
}
