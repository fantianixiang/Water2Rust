//! Water2Rust 桌面后端（Tauri 2）。
//!
//! 经 `#[tauri::command]` 把纯 Rust 业务能力（water-core/io/fclass/hydro/edge-depth）
//! 暴露给 React 前端。当前仅含 `greet` 自检命令，water 业务命令在后续迁移接入。

/// 后端自检命令：验证前端 ↔ Rust 链路。
#[tauri::command]
fn greet(name: &str) -> String {
    format!("你好，{name}！Water2Rust 纯 Rust 后端已就绪。")
}

/// Tauri 应用入口（供 `main.rs` 与未来移动端复用）。
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![greet])
        .run(tauri::generate_context!())
        .expect("运行 Tauri 应用失败");
}
