//! 统一错误类型。

use thiserror::Error;

/// Water2Rust 统一错误。
#[derive(Debug, Error)]
pub enum WaterError {
    #[error("I/O 错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("输入数据无效: {0}")]
    InvalidInput(String),

    #[error("尚未实现: {0}")]
    NotImplemented(&'static str),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// crate 统一 Result 别名。
pub type Result<T> = std::result::Result<T, WaterError>;
