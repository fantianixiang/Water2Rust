//! 把 `tracing` 日志桥接为 Tauri 事件 `pipeline://log`，供前端实时显示。
//!
//! 与 egui GUI 的 ChannelWriter 同思路：tracing 写入器 → 通道 → setup 里 drain → emit。

use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter};
use tracing_subscriber::fmt::MakeWriter;

/// tracing 格式化输出转发到通道的写入器（`Sender` 非 `Sync`，用 `Arc<Mutex>` 包裹）。
#[derive(Clone)]
pub struct ChannelWriter {
    tx: Arc<Mutex<Sender<String>>>,
}

impl ChannelWriter {
    pub fn new(tx: Sender<String>) -> Self {
        Self { tx: Arc::new(Mutex::new(tx)) }
    }
}

impl std::io::Write for ChannelWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let text = String::from_utf8_lossy(buf).to_string();
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(text);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for ChannelWriter {
    type Writer = ChannelWriter;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// 初始化 tracing（info 级）并返回日志接收端，交由 setup 转发为 Tauri 事件。
pub fn init_tracing() -> Receiver<String> {
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let _ = tracing_subscriber::fmt()
        .with_writer(ChannelWriter::new(tx))
        .with_ansi(false)
        .with_target(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .try_init();
    rx
}

/// 在后台线程把日志通道逐行 emit 到前端。
pub fn spawn_log_forwarder(app: AppHandle, rx: Receiver<String>) {
    std::thread::spawn(move || {
        for line in rx.iter() {
            let _ = app.emit("pipeline://log", line);
        }
    });
}
