//! 河流纵剖面 z_smooth 的后处理原语（从 `river_solve` 拆出，保持文件 <300 行）。
//!
//! 含：numpy 中位数/均值、pandas ffill/bfill、空间 P30 下压、mask-aware 高斯下压。

use ndarray::Array2;

use water_core::rank_filter::percentile_filter_2d_at;
use water_core::raster_ops::{gaussian_smooth, gaussian_smooth_f32};

use crate::cross_section::py_round;

/// numpy.median（有限值假设；偶数取两中值均值）。
pub(crate) fn numpy_median(vals: &[f64]) -> f64 {
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
pub(crate) fn nanmean(vals: &[f64]) -> f64 {
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
pub(crate) fn ffill_bfill(v: &mut [f64]) {
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

/// 空间 30 分位下压（对应 P30 spatial filter）。就地更新 `z_smooth`。
pub(crate) fn spatial_p30_clamp(
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
    // 仅在站点像素计算 P30 秩滤波（全网格结果只在站点被读取；与全网格版逐位一致）。
    let station_pts: Vec<(usize, usize)> = smoothed
        .iter()
        .map(|&(sr, sc)| (sr.min(lh.saturating_sub(1)), sc.min(lw.saturating_sub(1))))
        .collect();
    let zf_low_at = percentile_filter_2d_at(&zf_for_pct, 30.0, pct_size, &station_pts);
    for (k, &(sr, sc)) in smoothed.iter().enumerate() {
        if sr < lh && sc < lw {
            let v = zf_low_at[k];
            if v.is_finite() && v < f64::INFINITY && z_smooth[k].is_finite() {
                z_smooth[k] = z_smooth[k].min(v);
            }
        }
    }
}

/// 2D mask-aware 高斯（sigma=3），再对原值取 min。对应 gauss 段。就地更新 `z_smooth`。
pub(crate) fn mask_aware_spatial_gauss_clamp(
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
        }
        // 原为 NaN 的位置：np.minimum(nan, pre) = nan，保持不变。
    }
}
