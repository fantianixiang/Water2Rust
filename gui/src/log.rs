//! GUI 事件通道：把 `tracing` 日志与流水线结果汇入同一通道，供 UI 轮询。

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use tracing_subscriber::fmt::MakeWriter;

/// UI 线程消费的事件。
pub enum GuiEvent {
    /// 一行（或一段）日志文本。
    Log(String),
    /// 某任务阶段开始（`key` ∈ fclass / edge / hydro）。
    TaskStart(String),
    /// 某任务阶段完成。
    TaskDone(String),
    /// 流水线成功，附各任务的产物路径。
    Done(Vec<(String, PathBuf)>),
    /// 流水线失败，附错误描述。
    Failed(String),
}

/// 把 `tracing` 格式化输出转发到 [`GuiEvent::Log`] 的写入器。
///
/// `Sender` 本身非 `Sync`，而全局 subscriber 要求 `MakeWriter` 为 `Send + Sync`，
/// 故用 `Arc<Mutex<..>>` 包裹。日志量不大，短锁可接受。
#[derive(Clone)]
pub struct ChannelWriter {
    tx: Arc<Mutex<Sender<GuiEvent>>>,
}

impl ChannelWriter {
    pub fn new(tx: Sender<GuiEvent>) -> Self {
        Self { tx: Arc::new(Mutex::new(tx)) }
    }
}

impl std::io::Write for ChannelWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let text = String::from_utf8_lossy(buf).to_string();
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(GuiEvent::Log(text));
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
