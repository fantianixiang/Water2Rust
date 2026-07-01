//! 湖泊常数水位相关数值内核与 DEM 采样。
//!
//! - `iterative_trimmed_median`：迭代截尾中位数（复刻 `_iterative_trimmed_median`）。
//! - `sample_polygon_interior_dem_median`：多边形内部 DEM 中位数（复刻同名 Python 函数）。
//! - `sample_polygon_boundary_ring_dem_median`：岸线环 DEM 迭代截尾中位数（复刻同名 Python 函数）。
//!
//! 栅格化经 `water-io`（eci-gdal-alg，all_touched=False），腐蚀经 `water-core::raster_ops`。

use geo::BoundingRect;
use geo_types::Polygon;
use ndarray::Array2;
use water_core::raster_ops::binary_erosion;
use water_io::raster::rasterize_polygon_mask;

/// numpy 风格中位数：升序排序后，奇数取中间、偶数取中间两者均值。
/// `vals` 不含 NaN。
fn numpy_median(vals: &mut [f64]) -> f64 {
    let n = vals.len();
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if n % 2 == 1 {
        vals[n / 2]
    } else {
        0.5 * (vals[n / 2 - 1] + vals[n / 2])
    }
}

/// 迭代截尾中位数：丢弃高于当前中位数的值，重算幸存者中位数，直到收敛（≤ `max_iter`）。
///
/// 忠实复刻 Python `_iterative_trimmed_median`：比较 `values <= z` 即等高线方程本身，
/// 无可调阈值；中位数对 ≤49% 离群稳健。空输入返回 NaN。默认 `max_iter = 5`。
pub fn iterative_trimmed_median(values: &[f64], max_iter: usize) -> f64 {
    let finite: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if finite.is_empty() {
        return f64::NAN;
    }
    let mut z = {
        let mut tmp = finite.clone();
        numpy_median(&mut tmp)
    };
    for _ in 0..max_iter {
        let mut kept: Vec<f64> = finite.iter().copied().filter(|&v| v <= z).collect();
        if kept.is_empty() {
            break;
        }
        let z_new = numpy_median(&mut kept);
        if (z_new - z).abs() < 1e-6 {
            z = z_new;
            break;
        }
        z = z_new;
    }
    z
}

/// 复刻 Python 的多边形局部窗口计算：由多边形 bbox 四角反算像素范围，各向外扩 1 像素并夹到栅格内。
///
/// `transform` 为北向上 GDAL 仿射 `[a,0,c,0,e,f]`。返回
/// `(col_off, row_off, col_end, row_end, local_transform)`，窗口为空时 `None`。
fn local_window(
    polygon: &Polygon<f64>,
    transform: &[f64; 6],
    height: usize,
    width: usize,
) -> Option<(usize, usize, usize, usize, [f64; 6])> {
    let rect = polygon.bounding_rect()?;
    let (minx, miny, maxx, maxy) = (rect.min().x, rect.min().y, rect.max().x, rect.max().y);
    let [a, _b, c, _d, e, f] = *transform;
    let (col0, col1) = ((minx - c) / a, (maxx - c) / a);
    let (row0, row1) = ((miny - f) / e, (maxy - f) / e);
    let min_col = col0.min(col1);
    let max_col = col0.max(col1);
    let min_row = row0.min(row1);
    let max_row = row0.max(row1);
    let col_off = ((min_col.floor() as i64) - 1).max(0) as usize;
    let row_off = ((min_row.floor() as i64) - 1).max(0) as usize;
    let col_end = (((max_col.ceil() as i64) + 1).min(width as i64)).max(0) as usize;
    let row_end = (((max_row.ceil() as i64) + 1).min(height as i64)).max(0) as usize;
    if col_end <= col_off || row_end <= row_off {
        return None;
    }
    let local_transform = [a, 0.0, c + a * col_off as f64, 0.0, e, f + e * row_off as f64];
    Some((col_off, row_off, col_end, row_end, local_transform))
}

/// 收集局部掩膜内、对应全局 DEM 有限的值（转 f64）。
fn masked_finite_values(
    dem: &Array2<f32>,
    col_off: usize,
    row_off: usize,
    mask: &Array2<bool>,
) -> Vec<f64> {
    let (lh, lw) = mask.dim();
    let mut out = Vec::new();
    for lr in 0..lh {
        for lc in 0..lw {
            if mask[(lr, lc)] {
                let v = dem[(row_off + lr, col_off + lc)];
                if v.is_finite() {
                    out.push(v as f64);
                }
            }
        }
    }
    out
}

/// 栅格化多边形到局部窗口，返回 `(col_off, row_off, mask)`；窗口空或掩膜空时 `None`。
fn polygon_local_mask(
    polygon: &Polygon<f64>,
    dem: &Array2<f32>,
    transform: &[f64; 6],
) -> Option<(usize, usize, Array2<bool>)> {
    let (h, w) = dem.dim();
    let (col_off, row_off, col_end, row_end, lt) = local_window(polygon, transform, h, w)?;
    let (lw, lh) = ((col_end - col_off) as u32, (row_end - row_off) as u32);
    let mask = rasterize_polygon_mask(polygon, &lt, lw, lh, false);
    if !mask.iter().any(|&b| b) {
        return None;
    }
    Some((col_off, row_off, mask))
}

/// 多边形内部 DEM 中位数。忠实复刻 `_sample_polygon_interior_dem_median`。
///
/// 返回 `(median, finite_pixel_count)`；窗口/掩膜/有限值为空时 `(NaN, 0)`。
pub fn sample_polygon_interior_dem_median(
    polygon: &Polygon<f64>,
    dem: &Array2<f32>,
    transform: &[f64; 6],
) -> (f64, usize) {
    let Some((col_off, row_off, mask)) = polygon_local_mask(polygon, dem, transform) else {
        return (f64::NAN, 0);
    };
    let mut vals = masked_finite_values(dem, col_off, row_off, &mask);
    if vals.is_empty() {
        return (f64::NAN, 0);
    }
    let n = vals.len();
    (numpy_median(&mut vals), n)
}

/// 多边形岸线环（内一像素带）DEM 迭代截尾中位数。忠实复刻 `_sample_polygon_boundary_ring_dem_median`。
///
/// 岸线环 = 掩膜 ∩ ~腐蚀(掩膜)；多边形过细（腐蚀后为空）时回退整掩膜。
/// 返回 `(median, finite_pixel_count)`；空时 `(NaN, 0)`。
pub fn sample_polygon_boundary_ring_dem_median(
    polygon: &Polygon<f64>,
    dem: &Array2<f32>,
    transform: &[f64; 6],
) -> (f64, usize) {
    let Some((col_off, row_off, mask)) = polygon_local_mask(polygon, dem, transform) else {
        return (f64::NAN, 0);
    };
    let eroded = binary_erosion(&mask, 1);
    let mut ring = Array2::<bool>::from_elem(mask.dim(), false);
    let mut ring_any = false;
    for (idx, &m) in mask.indexed_iter() {
        if m && !eroded[idx] {
            ring[idx] = true;
            ring_any = true;
        }
    }
    if !ring_any {
        ring = mask;
    }
    let vals = masked_finite_values(dem, col_off, row_off, &ring);
    if vals.is_empty() {
        return (f64::NAN, 0);
    }
    let n = vals.len();
    (iterative_trimmed_median(&vals, 5), n)
}
