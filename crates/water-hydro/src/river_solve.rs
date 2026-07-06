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

use water_core::raster_ops::{binary_erosion, distance_transform_edt, medial_axis};

use crate::cross_section::{cross_section_z_at_skeleton_pixels, CrossSectionParams};
use crate::laplace::solve_laplace_dirichlet;
use crate::river_zsmooth::{
    ffill_bfill, mask_aware_spatial_gauss_clamp, nanmean, numpy_median, spatial_p30_clamp,
};
use crate::skeleton_graph::order_skeleton_pixels_along_flow;
use crate::skeleton_zloc::{
    compute_skeleton_tangents, detect_junction_stations, isotonic_multi_peak, Pixel,
};

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
    let prof = std::env::var("WATER_HYDRO_PROFILE").map(|v| v == "1").unwrap_or(false);
    let mut tk = std::time::Instant::now();
    let mut marks: Vec<(&str, f64)> = Vec::new();
    #[allow(unused_assignments)]
    macro_rules! mark {
        ($name:expr) => {
            if prof {
                marks.push(($name, tk.elapsed().as_secs_f64()));
                tk = std::time::Instant::now();
            }
        };
    }
    let skeleton = medial_axis(poly_mask, medial_tiebreaker);
    mark!("medial_axis");
    let corr_edt = distance_transform_edt(poly_mask).distances;
    mark!("edt");
    let ordered = order_skeleton_pixels_along_flow(&skeleton, dem_loc);
    mark!("order_flow");

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
    mark!("tangent+junction");

    // 岸线环 = 多边形边缘一像素环。
    let eroded = binary_erosion(poly_mask, 1);
    let mut boundary_mask = Array2::<bool>::default((lh, lw));
    for r in 0..lh {
        for c in 0..lw {
            boundary_mask[(r, c)] = poly_mask[(r, c)] && !eroded[(r, c)];
        }
    }
    mark!("boundary");

    // 各站半宽 = 最近骨架像素的 EDT（下限 2）。
    let skel_rc: Vec<(usize, usize)> = skeleton
        .indexed_iter()
        .filter(|(_, &b)| b)
        .map(|((r, c), _)| (r, c))
        .collect();
    let edt_at_skel = nearest_skeleton_edt(&smoothed, &skel_rc, &corr_edt);
    mark!("nearest_skel_edt");

    // 横断面 bank z（boundary-only + EDT 截断）。
    let params = CrossSectionParams {
        max_ray_steps: 500,
        z_ref: None,
        epsilon: None,
        edt_half_widths: Some(&edt_at_skel),
    };
    let xs = cross_section_z_at_skeleton_pixels(&smoothed_px, &tangents, &boundary_mask, dem_loc, &params);
    mark!("cross_section");

    // 多峰等渗 + ffill/bfill。
    let mut z_smooth = isotonic_multi_peak(&xs.z_cross);
    let finite_cnt = z_smooth.iter().filter(|x| x.is_finite()).count();
    if finite_cnt > 0 && finite_cnt < z_smooth.len() {
        ffill_bfill(&mut z_smooth);
    }

    spatial_p30_clamp(&mut z_smooth, &smoothed, lh, lw, &corr_edt, &skeleton);
    mark!("p30_clamp");
    mask_aware_spatial_gauss_clamp(&mut z_smooth, &smoothed, lh, lw);
    mark!("gauss_clamp");

    let (dir_mask, dir_z) = build_dirichlet(&smoothed, &z_smooth, &is_junction, poly_mask);
    let n_dir = dir_mask.iter().filter(|&&b| b).count();
    let out = if n_dir == 0 {
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
    };
    mark!("laplace");
    if prof {
        let total: f64 = marks.iter().map(|(_, t)| t).sum();
        if total > 1.0 {
            let detail: String = marks
                .iter()
                .map(|(n, t)| format!("{n}={t:.2}"))
                .collect::<Vec<_>>()
                .join(" ");
            eprintln!("[profile] river_solve {lh}x{lw} 站点={} 骨架={}: {detail} | 合计={total:.2}s",
                smoothed.len(), skel_rc.len());
        }
    }
    out
}
