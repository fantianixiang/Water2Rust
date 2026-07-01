//! 骨架横断面水位采样（对应 Python `hydro_skeleton_zloc.py` 的横断面部分）。
//!
//! 每个骨架站点沿切线**法向**向左右投射射线，命中岸线（`boundary_mask`）或等高线，
//! 取 `min(左岸 DEM, 右岸 DEM)` 作该站的水位上界 `z_cross`。
//!
//! **替换的 Python 库**：numpy。保真要点：`int(round(x))` 为 banker's rounding
//! （四舍六入五取偶），射线步进取整逐位复刻。

use ndarray::Array2;

use crate::skeleton_zloc::Pixel;

/// Python `int(round(x))`：四舍六入五取偶（round-half-to-even）。
fn py_round(x: f64) -> i64 {
    let f = x.floor();
    let diff = x - f;
    if diff < 0.5 {
        f as i64
    } else if diff > 0.5 {
        f as i64 + 1
    } else {
        // 恰为 .5 → 取最近偶数
        let fi = f as i64;
        if fi.rem_euclid(2) == 0 {
            fi
        } else {
            fi + 1
        }
    }
}

/// 横断面采样参数。对应 Python 的关键字参数。
#[derive(Clone)]
pub struct CrossSectionParams<'a> {
    /// 单侧最大射线步数（Python 默认 500）。
    pub max_ray_steps: usize,
    /// 等高线模式的参考水位；`None` 走 boundary-only 模式。
    pub z_ref: Option<f64>,
    /// 等高线模式的容差。
    pub epsilon: Option<f64>,
    /// 每站 EDT 半宽（存在时按 `ceil(hw*1.5)` 截断射线长度）。
    pub edt_half_widths: Option<&'a [f64]>,
}

impl<'a> Default for CrossSectionParams<'a> {
    fn default() -> Self {
        Self { max_ray_steps: 500, z_ref: None, epsilon: None, edt_half_widths: None }
    }
}

/// 横断面采样结果。
pub struct CrossSectionResult {
    /// 每站 `z_cross`；无命中时为 NaN（或回退骨架 DEM）。
    pub z_cross: Vec<f64>,
    /// 每站左/右命中像素（`None` 表示未命中）。
    pub left_hits: Vec<Option<Pixel>>,
    pub right_hits: Vec<Option<Pixel>>,
}

/// 从 `(start_r, start_c)` 沿 `(dir_r, dir_c)` 整数步进，命中 `boundary_mask` 的 True 像素。
///
/// 对应 `_trace_ray_to_boundary`。返回命中 `(row, col)` 或 `None`。
fn trace_ray_to_boundary(
    start_r: f64,
    start_c: f64,
    dir_r: f64,
    dir_c: f64,
    boundary_mask: &Array2<bool>,
    max_steps: usize,
) -> Option<Pixel> {
    let (h, w) = boundary_mask.dim();
    for step in 1..=max_steps {
        let r_step = start_r + dir_r * step as f64;
        let c_step = start_c + dir_c * step as f64;
        let ri = py_round(r_step);
        let ci = py_round(c_step);
        if ri < 0 || ri >= h as i64 || ci < 0 || ci >= w as i64 {
            return None;
        }
        if boundary_mask[(ri as usize, ci as usize)] {
            return Some((ri, ci));
        }
    }
    None
}

/// 从 `(start_r, start_c)` 沿 `(dir_r, dir_c)` 亚像素步进采样 DEM，
/// 停在首个 `DEM > z_ref + epsilon` 的位置。对应 `_trace_ray_to_contour`。
///
/// 返回 `(r_stop, c_stop, z_stop)`（取整后的整数位置）或 `None`。
fn trace_ray_to_contour(
    dem: &Array2<f64>,
    start_r: f64,
    start_c: f64,
    dir_r: f64,
    dir_c: f64,
    z_ref: f64,
    epsilon: f64,
    max_len_px: usize,
    step_size: f64,
) -> Option<(f64, f64, f64)> {
    let (h, w) = dem.dim();
    let threshold = z_ref + epsilon;
    let n_steps = (max_len_px as f64 / step_size) as usize;
    for step in 1..=n_steps {
        let dist = step as f64 * step_size;
        let r_pos = start_r + dir_r * dist;
        let c_pos = start_c + dir_c * dist;
        let ri = py_round(r_pos);
        let ci = py_round(c_pos);
        if ri < 0 || ri >= h as i64 || ci < 0 || ci >= w as i64 {
            return None;
        }
        let z_val = dem[(ri as usize, ci as usize)];
        if !z_val.is_finite() {
            continue;
        }
        if z_val > threshold {
            return Some((ri as f64, ci as f64, z_val));
        }
    }
    None
}

/// 逐骨架站点横断面水位采样（对应 `_cross_section_z_at_skeleton_pixels`）。
pub fn cross_section_z_at_skeleton_pixels(
    ordered_pixels: &[Pixel],
    tangents: &[[f64; 2]],
    boundary_mask: &Array2<bool>,
    dem_window: &Array2<f64>,
    params: &CrossSectionParams,
) -> CrossSectionResult {
    let use_contour = params.z_ref.is_some() && params.epsilon.is_some();
    let n = ordered_pixels.len();
    let (h, w) = dem_window.dim();
    let mut z_cross = vec![f64::NAN; n];
    let mut left_hits: Vec<Option<Pixel>> = Vec::with_capacity(n);
    let mut right_hits: Vec<Option<Pixel>> = Vec::with_capacity(n);

    for i in 0..n {
        let (r, c) = (ordered_pixels[i].0 as f64, ordered_pixels[i].1 as f64);
        let (tr, tc) = (tangents[i][0], tangents[i][1]);
        // 法向：切线旋转 90°（左法向）。
        let (nr, nc) = (-tc, tr);

        // EDT 太小（骨架贴边界）：直接用骨架 DEM。
        if let Some(edt) = params.edt_half_widths {
            if edt[i] <= 1.0 {
                let ri = py_round(r);
                let ci = py_round(c);
                if ri >= 0 && ri < h as i64 && ci >= 0 && ci < w as i64 {
                    let z_skel = dem_window[(ri as usize, ci as usize)];
                    if z_skel.is_finite() {
                        z_cross[i] = z_skel;
                    }
                }
                left_hits.push(None);
                right_hits.push(None);
                continue;
            }
        }

        // 逐站射线长度上限（基于 EDT 半宽）。
        let local_max = if let Some(edt) = params.edt_half_widths {
            let local_hw = edt[i];
            let lm = std::cmp::max((local_hw * 1.5).ceil() as i64, 2) as usize;
            std::cmp::min(lm, params.max_ray_steps)
        } else {
            params.max_ray_steps
        };

        let mut hit_left: Option<Pixel> = None;
        let mut hit_right: Option<Pixel> = None;
        let mut z_left = f64::NAN;
        let mut z_right = f64::NAN;

        if use_contour {
            let z_ref = params.z_ref.unwrap();
            let eps = params.epsilon.unwrap();
            let left = trace_ray_to_contour(dem_window, r, c, nr, nc, z_ref, eps, local_max, 0.5);
            let right = trace_ray_to_contour(dem_window, r, c, -nr, -nc, z_ref, eps, local_max, 0.5);

            if let Some((rl, cl, zl)) = left {
                hit_left = Some((rl as i64, cl as i64));
                z_left = zl;
            } else if let Some(fb) = trace_ray_to_boundary(r, c, nr, nc, boundary_mask, local_max) {
                hit_left = Some(fb);
                z_left = dem_window[(fb.0 as usize, fb.1 as usize)];
            }
            if let Some((rr, cr, zr)) = right {
                hit_right = Some((rr as i64, cr as i64));
                z_right = zr;
            } else if let Some(fb) = trace_ray_to_boundary(r, c, -nr, -nc, boundary_mask, local_max) {
                hit_right = Some(fb);
                z_right = dem_window[(fb.0 as usize, fb.1 as usize)];
            }
        } else {
            // boundary-only 模式。
            if let Some(fb) = trace_ray_to_boundary(r, c, nr, nc, boundary_mask, local_max) {
                hit_left = Some(fb);
                z_left = dem_window[(fb.0 as usize, fb.1 as usize)];
            }
            if let Some(fb) = trace_ray_to_boundary(r, c, -nr, -nc, boundary_mask, local_max) {
                hit_right = Some(fb);
                z_right = dem_window[(fb.0 as usize, fb.1 as usize)];
            }
        }

        left_hits.push(hit_left);
        right_hits.push(hit_right);

        let mut z_vals: Vec<f64> = Vec::new();
        if hit_left.is_some() && z_left.is_finite() {
            z_vals.push(z_left);
        }
        if hit_right.is_some() && z_right.is_finite() {
            z_vals.push(z_right);
        }
        if !z_vals.is_empty() {
            z_cross[i] = z_vals.iter().cloned().fold(f64::INFINITY, f64::min);
        } else {
            // 回退：无命中时用骨架 DEM。
            let ri = py_round(r);
            let ci = py_round(c);
            if ri >= 0 && ri < h as i64 && ci >= 0 && ci < w as i64 {
                let z_skel = dem_window[(ri as usize, ci as usize)];
                if z_skel.is_finite() {
                    z_cross[i] = z_skel;
                }
            }
        }
    }

    CrossSectionResult { z_cross, left_hits, right_hits }
}
