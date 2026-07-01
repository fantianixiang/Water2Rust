//! 逐河流多边形的水面数值核（对应 Python `hydro_laplace.py::solve_laplace_per_polygon`
//! 的**单多边形内层块**）。
//!
//! 把已就位的原语按序装配成河流水面求解：
//! medial_axis → EDT 半宽 → 沿流排序 → 切线 → junction → 横断面 z → 多峰等渗 →
//! ffill/bfill → 空间 P30 → mask-aware 高斯 → Dirichlet(河心 pin) → Laplace 求解。
//!
//! 外层窗口裁剪 / 光栅化 / 缝合到全局面属 GIS 编排，另行处理。
//!
//! **保真要点**：`medial_axis` 的 tiebreaker 需外部注入（与 skimage 固定种子对拍）；
//! `int(round(x))` 用 banker's rounding（`cross_section::py_round`）。

use ndarray::Array2;

use water_core::rank_filter::percentile_filter_2d;
use water_core::raster_ops::{
    binary_erosion, distance_transform_edt, gaussian_smooth, gaussian_smooth_f32, medial_axis,
};

use crate::cross_section::{
    cross_section_z_at_skeleton_pixels, py_round, CrossSectionParams,
};
use crate::laplace::solve_laplace_dirichlet;
use crate::skeleton_graph::order_skeleton_pixels_along_flow;
use crate::skeleton_zloc::{
    compute_skeleton_tangents, detect_junction_stations, isotonic_multi_peak, Pixel,
};

/// numpy.median（有限值假设；偶数取两中值均值）。
fn numpy_median(vals: &[f64]) -> f64 {
    if vals.is_empty() {
        return f64::NAN;
    }
    let mut v = vals.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

/// 有限值均值（对应 `np.nanmean`）。
fn nanmean(vals: &[f64]) -> f64 {
    let (mut s, mut cnt) = (0.0f64, 0usize);
    for &x in vals {
        if x.is_finite() {
            s += x;
            cnt += 1;
        }
    }
    if cnt == 0 {
        f64::NAN
    } else {
        s / cnt as f64
    }
}

/// pandas `Series.ffill().bfill()`：先前向填充 NaN，再后向填充剩余前导 NaN。
fn ffill_bfill(v: &mut [f64]) {
    let mut last = f64::NAN;
    for x in v.iter_mut() {
        if x.is_finite() {
            last = *x;
        } else if last.is_finite() {
            *x = last;
        }
    }
    let mut next = f64::NAN;
    for x in v.iter_mut().rev() {
        if x.is_finite() {
            next = *x;
        } else if next.is_finite() {
            *x = next;
        }
    }
}

/// 每个平滑站点最近骨架像素的 EDT 半宽（下限 2.0）。对应 cKDTree.query + `maximum(.,2.0)`。
fn nearest_skeleton_edt(
    smoothed: &[(usize, usize)],
    skel_rc: &[(usize, usize)],
    corr_edt: &Array2<f64>,
) -> Vec<f64> {
    smoothed
        .iter()
        .map(|&(sr, sc)| {
            if skel_rc.is_empty() {
                return 1.0f64.max(2.0);
            }
            // 最近骨架像素（平手取行主序先出现者）。
            let mut best = usize::MAX;
            let mut best_d2 = i64::MAX;
            for (j, &(kr, kc)) in skel_rc.iter().enumerate() {
                let dr = kr as i64 - sr as i64;
                let dc = kc as i64 - sc as i64;
                let d2 = dr * dr + dc * dc;
                if d2 < best_d2 {
                    best_d2 = d2;
                    best = j;
                }
            }
            let (kr, kc) = skel_rc[best];
            corr_edt[(kr, kc)].max(2.0)
        })
        .collect()
}

/// 空间 30 分位下压（对应 P30 spatial filter）。就地更新 `z_smooth`。
fn spatial_p30_clamp(
    z_smooth: &mut [f64],
    smoothed: &[(usize, usize)],
    lh: usize,
    lw: usize,
    corr_edt: &Array2<f64>,
    skeleton: &Array2<bool>,
) {
    let skel_edt: Vec<f64> = skeleton
        .indexed_iter()
        .filter(|(_, &b)| b)
        .map(|((r, c), _)| corr_edt[(r, c)])
        .collect();
    let med_hw_skel = if skel_edt.is_empty() { 3.0 } else { numpy_median(&skel_edt) };
    let pct_radius = std::cmp::max(2, py_round(med_hw_skel)) as usize;
    let pct_size = 2 * pct_radius + 1;

    // 稀疏骨架 z 场，非骨架填 +inf（不干扰低分位）。
    let mut zf_for_pct = Array2::<f64>::from_elem((lh, lw), f64::INFINITY);
    for (k, &(sr, sc)) in smoothed.iter().enumerate() {
        if z_smooth[k].is_finite() && sr < lh && sc < lw {
            zf_for_pct[(sr, sc)] = z_smooth[k];
        }
    }
    let zf_low = percentile_filter_2d(&zf_for_pct, 30.0, pct_size);
    for (k, &(sr, sc)) in smoothed.iter().enumerate() {
        if sr < lh && sc < lw {
            let v = zf_low[(sr, sc)];
            if v.is_finite() && v < f64::INFINITY && z_smooth[k].is_finite() {
                z_smooth[k] = z_smooth[k].min(v);
            }
        }
    }
}

/// 2D mask-aware 高斯（sigma=3），再对原值取 min。对应 gauss 段。就地更新 `z_smooth`。
fn mask_aware_spatial_gauss_clamp(
    z_smooth: &mut [f64],
    smoothed: &[(usize, usize)],
    lh: usize,
    lw: usize,
) {
    let z_pre = z_smooth.to_vec();
    let mut z_field = Array2::<f64>::zeros((lh, lw));
    // mask_field 用 f32（与 Python 一致），使 mask 高斯走 scipy 的 float32 舍入路径。
    let mut mask_field = Array2::<f32>::zeros((lh, lw));
    for (k, &(sr, sc)) in smoothed.iter().enumerate() {
        if z_smooth[k].is_finite() && sr < lh && sc < lw {
            z_field[(sr, sc)] = z_smooth[k];
            mask_field[(sr, sc)] = 1.0;
        }
    }
    let zg = gaussian_smooth(&z_field, 3.0);
    let mg = gaussian_smooth_f32(&mask_field, 3.0);
    for (k, &(sr, sc)) in smoothed.iter().enumerate() {
        if sr < lh && sc < lw && mg[(sr, sc)] > 1e-3 {
            z_smooth[k] = zg[(sr, sc)] / mg[(sr, sc)];
        }
    }
    // 与原值逐元素取 min（NaN 保持）。
    for (x, &pre) in z_smooth.iter_mut().zip(z_pre.iter()) {
        if x.is_finite() && pre.is_finite() {
            *x = x.min(pre);
        } else if !x.is_finite() {
            // 原为 NaN 的位置：np.minimum(nan, pre) = nan
        }
    }
}

/// 构建 Dirichlet 掩膜与值：河心 pin（排除 junction，落在 poly_mask 内）。
fn build_dirichlet(
    smoothed: &[(usize, usize)],
    z_smooth: &[f64],
    is_junction: &[bool],
    poly_mask: &Array2<bool>,
) -> (Array2<bool>, Array2<f64>) {
    let (lh, lw) = poly_mask.dim();
    let mut dir_mask = Array2::<bool>::default((lh, lw));
    let mut dir_z = Array2::<f64>::from_elem((lh, lw), f64::NAN);
    for (k, &(sr, sc)) in smoothed.iter().enumerate() {
        if is_junction[k] {
            continue;
        }
        if sr < lh && sc < lw && poly_mask[(sr, sc)] {
            dir_mask[(sr, sc)] = true;
            dir_z[(sr, sc)] = z_smooth[k];
        }
    }
    (dir_mask, dir_z)
}

/// 骨架过短（<5）时的简单 z_local 回退。
fn fallback_simple_zlocal(
    poly_mask: &Array2<bool>,
    dem_loc: &Array2<f64>,
    skeleton: &Array2<bool>,
    corr_edt: &Array2<f64>,
    pixel_m: f64,
) -> Array2<f64> {
    let (lh, lw) = poly_mask.dim();
    // ~skeleton 的最近特征索引 = 最近骨架像素。
    let not_skel = skeleton.mapv(|b| !b);
    let edt = distance_transform_edt(&not_skel);
    let mut z = Array2::<f64>::zeros((lh, lw));
    for r in 0..lh {
        for c in 0..lw {
            let nr = edt.index_row[(r, c)] as usize;
            let nc = edt.index_col[(r, c)] as usize;
            z[(r, c)] = dem_loc[(nr, nc)];
        }
    }
    let skel_widths: Vec<f64> = skeleton
        .indexed_iter()
        .filter(|(_, &b)| b)
        .map(|((r, c), _)| corr_edt[(r, c)])
        .collect();
    let med_hw = if skel_widths.is_empty() { 3.0 } else { numpy_median(&skel_widths) };
    let depth_m = (med_hw * pixel_m * 0.1).clamp(1.0, 10.0);
    for r in 0..lh {
        for c in 0..lw {
            if poly_mask[(r, c)] {
                z[(r, c)] += depth_m;
            } else {
                z[(r, c)] = f64::NAN;
            }
        }
    }
    z
}

/// 逐河流多边形数值核：给定局部窗口的 `poly_mask` 与 `dem_loc`，产出 `z_local_field`。
///
/// `medial_tiebreaker`：skimage medial_axis 的随机置换（对拍用固定种子注入）。
/// `pixel_m`：像素边长（米），仅回退路径用于水深估计。
pub fn solve_river_polygon_surface(
    poly_mask: &Array2<bool>,
    dem_loc: &Array2<f64>,
    medial_tiebreaker: &[usize],
    pixel_m: f64,
) -> Array2<f64> {
    let (lh, lw) = poly_mask.dim();
    let skeleton = medial_axis(poly_mask, medial_tiebreaker);
    let corr_edt = distance_transform_edt(poly_mask).distances;
    let ordered = order_skeleton_pixels_along_flow(&skeleton, dem_loc);

    if ordered.len() < 5 {
        return fallback_simple_zlocal(poly_mask, dem_loc, &skeleton, &corr_edt, pixel_m);
    }

    // 平滑位置 = 有序像素夹回窗口内（均为整数）。
    let smoothed: Vec<(usize, usize)> = ordered
        .iter()
        .map(|&(r, c)| {
            (
                r.clamp(0, lh as i64 - 1) as usize,
                c.clamp(0, lw as i64 - 1) as usize,
            )
        })
        .collect();
    let smoothed_px: Vec<Pixel> = smoothed.iter().map(|&(r, c)| (r as i64, c as i64)).collect();

    let tangents = compute_skeleton_tangents(&smoothed_px, 3);
    let is_junction = detect_junction_stations(&smoothed_px, &tangents, 4.0, 0.5);

    // 岸线环 = 多边形边缘一像素环。
    let eroded = binary_erosion(poly_mask, 1);
    let mut boundary_mask = Array2::<bool>::default((lh, lw));
    for r in 0..lh {
        for c in 0..lw {
            boundary_mask[(r, c)] = poly_mask[(r, c)] && !eroded[(r, c)];
        }
    }

    // 各站半宽 = 最近骨架像素的 EDT（下限 2）。
    let skel_rc: Vec<(usize, usize)> = skeleton
        .indexed_iter()
        .filter(|(_, &b)| b)
        .map(|((r, c), _)| (r, c))
        .collect();
    let edt_at_skel = nearest_skeleton_edt(&smoothed, &skel_rc, &corr_edt);

    // 横断面 bank z（boundary-only + EDT 截断）。
    let params = CrossSectionParams {
        max_ray_steps: 500,
        z_ref: None,
        epsilon: None,
        edt_half_widths: Some(&edt_at_skel),
    };
    let xs = cross_section_z_at_skeleton_pixels(&smoothed_px, &tangents, &boundary_mask, dem_loc, &params);

    // 多峰等渗 + ffill/bfill。
    let mut z_smooth = isotonic_multi_peak(&xs.z_cross);
    let finite_cnt = z_smooth.iter().filter(|x| x.is_finite()).count();
    if finite_cnt > 0 && finite_cnt < z_smooth.len() {
        ffill_bfill(&mut z_smooth);
    }

    spatial_p30_clamp(&mut z_smooth, &smoothed, lh, lw, &corr_edt, &skeleton);
    mask_aware_spatial_gauss_clamp(&mut z_smooth, &smoothed, lh, lw);

    let (dir_mask, dir_z) = build_dirichlet(&smoothed, &z_smooth, &is_junction, poly_mask);
    let n_dir = dir_mask.iter().filter(|&&b| b).count();
    if n_dir == 0 {
        let mean = nanmean(&z_smooth);
        let mut z_local = Array2::<f64>::from_elem((lh, lw), f64::NAN);
        for r in 0..lh {
            for c in 0..lw {
                if poly_mask[(r, c)] {
                    z_local[(r, c)] = mean;
                }
            }
        }
        z_local
    } else {
        solve_laplace_dirichlet(poly_mask, &dir_mask, &dir_z)
    }
}
