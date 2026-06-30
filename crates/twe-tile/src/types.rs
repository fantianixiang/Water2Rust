//! 瓦片寻址共享类型

/// ZXY 瓦片坐标
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TileCoord {
    pub x: u32,
    pub y: u32,
    pub z: u8,
}

/// 瓦片寻址错误
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TileError {
    /// 空 quadkey
    EmptyQuadkey,
    /// quadkey 超长（最大 22 级）
    QuadkeyTooLong(usize),
    /// 非法 quadkey 字符
    InvalidQuadkeyChar { ch: char, position: usize },
    /// 目标 zoom 比 parent 小
    ZoomTooSmall { target: u8, parent: u8 },
}

impl std::fmt::Display for TileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyQuadkey => write!(f, "empty quadkey"),
            Self::QuadkeyTooLong(len) => {
                write!(f, "quadkey too long: {} chars (max 22)", len)
            }
            Self::InvalidQuadkeyChar { ch, position } => {
                write!(
                    f,
                    "invalid quadkey character '{}' at position {}",
                    ch, position
                )
            }
            Self::ZoomTooSmall { target, parent } => {
                write!(f, "target zoom {} < parent zoom {}", target, parent)
            }
        }
    }
}

impl std::error::Error for TileError {}
