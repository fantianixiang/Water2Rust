//! 输出组合：把求解水面与 DEM 按模式合成最终输出数组。
//!
//! 忠实复刻 Python `output.py::compose_water_output_array`（完全自包含，无栅格化）。

use ndarray::Array2;

use crate::OutputMode;

/// 组合统计（对应 Python 返回的 metrics dict）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComposeMetrics {
    pub surface_write_pixels: usize,
    pub water_dem_fill_pixels: usize,
    pub background_dem_pixels: usize,
    pub water_remaining_nodata_pixels: usize,
    pub remaining_nodata_pixels: usize,
}

/// 合成输出数组（f32，NaN 为 nodata）。
///
/// - `WaterSurfaceWithDem`：先用有限 DEM 填底，再用求解水面覆盖水体像素。
/// - `WaterSurfaceOnly`：仅写求解水面，其余为 NaN。
pub fn compose_water_output_array(
    dem: &Array2<f32>,
    water_surface: &Array2<f32>,
    water_mask: &Array2<bool>,
    output_mode: OutputMode,
) -> (Array2<f32>, ComposeMetrics) {
    let dim = dem.dim();
    assert_eq!(dim, water_surface.dim(), "dem 与 water_surface 形状须一致");
    assert_eq!(dim, water_mask.dim(), "dem 与 water_mask 形状须一致");

    let (h, w) = dim;
    let mut output = Array2::<f32>::from_elem(dim, f32::NAN);

    let with_dem = matches!(output_mode, OutputMode::WaterSurfaceWithDem);

    let mut surface_write_pixels = 0usize;
    for r in 0..h {
        for c in 0..w {
            let d = dem[(r, c)];
            let s = water_surface[(r, c)];
            // 先填底 DEM
            if with_dem && d.is_finite() {
                output[(r, c)] = d;
            }
            // 求解水面覆盖
            if water_mask[(r, c)] && s.is_finite() {
                output[(r, c)] = s;
                surface_write_pixels += 1;
            }
        }
    }

    // 统计
    let mut water_dem_fill_pixels = 0usize;
    let mut background_dem_pixels = 0usize;
    let mut water_remaining_nodata_pixels = 0usize;
    let mut remaining_nodata_pixels = 0usize;
    for r in 0..h {
        for c in 0..w {
            let d = dem[(r, c)];
            let s = water_surface[(r, c)];
            let m = water_mask[(r, c)];
            let finite_dem = d.is_finite();
            let surface_write = m && s.is_finite();
            let finite_output = output[(r, c)].is_finite();

            if m && !surface_write && finite_dem && finite_output {
                water_dem_fill_pixels += 1;
            }
            if !m && finite_dem && finite_output {
                background_dem_pixels += 1;
            }
            if m && !finite_output {
                water_remaining_nodata_pixels += 1;
            }
            if !finite_output {
                remaining_nodata_pixels += 1;
            }
        }
    }

    let metrics = ComposeMetrics {
        surface_write_pixels,
        water_dem_fill_pixels,
        background_dem_pixels,
        water_remaining_nodata_pixels,
        remaining_nodata_pixels,
    };
    (output, metrics)
}
