//! 河流水面外层编排（对应 `hydro_laplace.py::solve_laplace_per_polygon` 的**逐多边形循环**
//! 与 `compose` 收尾）。
//!
//! 内层数值核见 [`crate::river_solve::solve_river_polygon_surface`]；本模块负责：
//! 窗口裁剪(`window_from_geometry_bounds`) → 光栅化(all_touched) → 调核 → 缝合到全局面；
//! 并接入湖泊压平 / 河床抬升 / 输出组合。

use std::collections::HashMap;

use geo::BoundingRect;
use geo_types::Polygon;
use ndarray::Array2;

use water_io::raster::rasterize_polygon_mask;

use crate::lake_flatten::{flatten_lake_polygons_on_surface, is_lake_fclass};
use crate::output::{compose_water_output_array, ComposeMetrics};
use crate::postprocess::apply_river_dem_floor_lift;
use crate::river_solve::solve_river_polygon_surface;
use crate::skirt::apply_water_surface_skirt;
use crate::OutputMode;

/// 由几何 bounds 计算像素窗口（对应 `_window_from_geometry_bounds` + rasterio `from_bounds`）。
///
/// `transform = [a, b, c, d, e, f]`（rasterio Affine：x=a·col+b·row+c，y=d·col+e·row+f）。
/// 返回 `(row_off, col_off, win_h, win_w, win_transform)`；窗口空或在栅格外返回 `None`。
pub fn window_from_geometry_bounds(
    polygon: &Polygon<f64>,
    transform: &[f64; 6],
    height: usize,
    width: usize,
    pad_pixels: i64,
) -> Option<(usize, usize, usize, usize, [f64; 6])> {
    let rect = polygon.bounding_rect()?;
    let (minx, miny) = (rect.min().x, rect.min().y);
    let (maxx, maxy) = (rect.max().x, rect.max().y);
    let [a, b, c, d, e, f] = *transform;
    // 逆仿射（north-up：b=d=0）：col=(x-c)/a，row=(y-f)/e。
    let col_start = (minx - c) / a;
    let row_start = (maxy - f) / e;
    let col_stop = (maxx - c) / a;
    let row_stop = (miny - f) / e;

    let row_off = std::cmp::max(0, row_start.floor() as i64 - pad_pixels);
    let col_off = std::cmp::max(0, col_start.floor() as i64 - pad_pixels);
    let row_end = std::cmp::min(height as i64, row_stop.ceil() as i64 + pad_pixels);
    let col_end = std::cmp::min(width as i64, col_stop.ceil() as i64 + pad_pixels);
    let win_h = std::cmp::max(0, row_end - row_off);
    let win_w = std::cmp::max(0, col_end - col_off);
    if win_w <= 0 || win_h <= 0 {
        return None;
    }
    // 窗口变换：平移原点到窗口左上（north-up：new_c=c+col_off·a，new_f=f+row_off·e）。
    let win_c = c + col_off as f64 * a + row_off as f64 * b;
    let win_f = f + col_off as f64 * d + row_off as f64 * e;
    let win_transform = [a, b, win_c, d, e, win_f];
    Some((
        row_off as usize,
        col_off as usize,
        win_h as usize,
        win_w as usize,
        win_transform,
    ))
}

/// 逐河流多边形求解水面并缝合到全局面（对应 `solve_laplace_per_polygon` 的循环）。
///
/// 湖泊多边形跳过（由 [`flatten_lake_polygons_on_surface`] 另行填充）；返回 f32 河流水面
/// （非水像素为 NaN）。`tiebreaker_for(poly_idx, n)` 给出每个多边形 medial_axis 的 tiebreaker
/// （对拍时注入 Python dump 的固定种子序列；生产可用恒等置换）。
pub fn solve_laplace_per_polygon<F>(
    transform: &[f64; 6],
    height: usize,
    width: usize,
    water_polygons: &[Polygon<f64>],
    water_fclass: &[Option<String>],
    dem: &Array2<f64>,
    all_touched: bool,
    tiebreaker_for: F,
) -> Array2<f32>
where
    F: Fn(usize, usize) -> Vec<usize>,
{
    let mut surface = Array2::<f32>::from_elem((height, width), f32::NAN);

    for (idx, polygon) in water_polygons.iter().enumerate() {
        if polygon.exterior().0.is_empty() {
            continue;
        }
        let fclass = water_fclass.get(idx).and_then(|o| o.as_deref());
        if is_lake_fclass(fclass) {
            continue;
        }
        let Some((r0, c0, lh, lw, win_t)) =
            window_from_geometry_bounds(polygon, transform, height, width, 2)
        else {
            continue;
        };

        let mut dem_loc = Array2::<f64>::zeros((lh, lw));
        for r in 0..lh {
            for c in 0..lw {
                dem_loc[(r, c)] = dem[(r0 + r, c0 + c)];
            }
        }
        let poly_mask = rasterize_polygon_mask(polygon, &win_t, lw as u32, lh as u32, all_touched);
        let n = poly_mask.iter().filter(|&&b| b).count();
        if n == 0 {
            continue;
        }
        let tb = tiebreaker_for(idx, n);
        let pixel_m = win_t[0].abs();
        let z_local = solve_river_polygon_surface(&poly_mask, &dem_loc, &tb, pixel_m);

        // 直接盖印：poly_mask ∩ 有限 z_local → surface。
        for r in 0..lh {
            for c in 0..lw {
                if poly_mask[(r, c)] && z_local[(r, c)].is_finite() {
                    surface[(r0 + r, c0 + c)] = z_local[(r, c)] as f32;
                }
            }
        }
    }
    surface
}

/// 端到端内存水面：河流求解 → 湖泊压平 → 河床抬升 → 输出组合。
///
/// 对应 `generate_hydro_water_dem` 中「已在工作 CRS 网格上」的算法段（不含 CRS/瓦片/IO）。
/// 返回 `(output_surface, metrics)`。
#[allow(clippy::too_many_arguments)]
pub fn compute_water_surface<F>(
    transform: &[f64; 6],
    dem: &Array2<f64>,
    water_polygons: &[Polygon<f64>],
    water_fclass: &[Option<String>],
    all_touched: bool,
    output_mode: OutputMode,
    skirt_pixels: usize,
    tiebreaker_for: F,
) -> (Array2<f32>, ComposeMetrics)
where
    F: Fn(usize, usize) -> Vec<usize>,
{
    let (height, width) = dem.dim();

    // 1) 河流水面（f32，湖泊为 NaN）。
    let mut surface = solve_laplace_per_polygon(
        transform,
        height,
        width,
        water_polygons,
        water_fclass,
        dem,
        all_touched,
        tiebreaker_for,
    );

    // 2) 湖泊压平（就地填入常数水位）。
    let dem_f32 = dem.mapv(|v| v as f32);
    let _lake_summary = flatten_lake_polygons_on_surface(
        &mut surface,
        transform,
        water_polygons,
        water_fclass,
        &dem_f32,
        all_touched,
        None,
    );

    // 3) 河流 / 湖泊掩膜（用于河床抬升与组合）。
    let mut river_mask = Array2::<bool>::default((height, width));
    let mut lake_mask = Array2::<bool>::default((height, width));
    let mut comp_map: HashMap<usize, i64> = HashMap::new();
    let _ = &mut comp_map;
    for (idx, polygon) in water_polygons.iter().enumerate() {
        if polygon.exterior().0.is_empty() {
            continue;
        }
        let Some((r0, c0, lh, lw, win_t)) =
            window_from_geometry_bounds(polygon, transform, height, width, 2)
        else {
            continue;
        };
        let pm = rasterize_polygon_mask(polygon, &win_t, lw as u32, lh as u32, all_touched);
        let is_lake = is_lake_fclass(water_fclass.get(idx).and_then(|o| o.as_deref()));
        for r in 0..lh {
            for c in 0..lw {
                if pm[(r, c)] {
                    if is_lake {
                        lake_mask[(r0 + r, c0 + c)] = true;
                    } else {
                        river_mask[(r0 + r, c0 + c)] = true;
                    }
                }
            }
        }
    }

    // 4) 水面写入掩膜（有限水面像素）。
    let write_mask = surface.mapv(|s| s.is_finite());

    // 5) 河床抬升（河道内解低于 DEM 则夹回 DEM）。
    apply_river_dem_floor_lift(&mut surface, &write_mask, &dem_f32, &river_mask, &lake_mask);

    // 6) 输出裙边（内 N 平铺水位、外 N 过渡到 DEM，掩膜外扩至 2N）。`skirt_pixels=0` 时为恒等。
    let mut output_mask = write_mask.clone();
    apply_water_surface_skirt(&mut surface, &mut output_mask, &dem_f32, skirt_pixels);

    // 7) 输出组合（写入掩膜含裙边）。
    let (output, metrics) =
        compose_water_output_array(&dem_f32, &surface, &output_mask, output_mode);
    (output, metrics)
}
