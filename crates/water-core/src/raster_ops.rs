//! 栅格数值算法（纯 Rust，替代 numpy / scipy.ndimage / skimage.morphology）。
//!
//! 本模块汇集原 Python 实现中依赖第三方库的数值算法，逐个改造为纯 Rust：
//!
//! - `binary_closing` / `binary_opening` —— 替代 `scipy.ndimage` 形态学
//! - `gaussian_smooth` —— 替代 `scipy.ndimage.gaussian_filter`
//! - `distance_transform_edt` —— 替代 `scipy.ndimage.distance_transform_edt`
//! - `skeletonize` —— 替代 `skimage.morphology.skeletonize`
//!
//! 每个函数都**必须**在实现后与对应 Python 库做数值对拍，并在 `docs/` 留存证据。
//!
//! 当前为骨架占位，待逐项实现。

use crate::error::{Result, WaterError};
use ndarray::Array2;

/// 二值形态学闭运算（占位）。
pub fn binary_closing(_mask: &Array2<bool>, _iterations: u32) -> Result<Array2<bool>> {
    Err(WaterError::NotImplemented("raster_ops::binary_closing"))
}

/// 高斯平滑（占位）。
pub fn gaussian_smooth(_data: &Array2<f64>, _sigma: f64) -> Result<Array2<f64>> {
    Err(WaterError::NotImplemented("raster_ops::gaussian_smooth"))
}

/// 欧氏距离变换（占位）。
pub fn distance_transform_edt(_mask: &Array2<bool>) -> Result<Array2<f64>> {
    Err(WaterError::NotImplemented("raster_ops::distance_transform_edt"))
}

/// 骨架提取（占位）。
pub fn skeletonize(_mask: &Array2<bool>) -> Result<Array2<bool>> {
    Err(WaterError::NotImplemented("raster_ops::skeletonize"))
}
