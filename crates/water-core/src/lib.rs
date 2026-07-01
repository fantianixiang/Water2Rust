//! water-core — 共享类型、错误、配置与栅格算法。
//!
//! 本 crate 对应原 Python `waters` 的 `settings.py` / `hydro_settings.py`
//! 以及散落在各模块的 numpy / scipy / skimage 数值算法（改造为纯 Rust）。

pub mod error;
pub mod find_peaks;
pub mod rank_filter;
pub mod raster_ops;
pub mod settings;

pub use error::{Result, WaterError};
