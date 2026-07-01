//! 河心线 z_local 管线原语（对应 Python `waters/hydro/hydro_skeleton_zloc.py`）。
//!
//! **替换的 Python 库**：scipy.signal（find_peaks）、scipy.ndimage（median_filter）、numpy。
//!
//! 本模块逐个复刻河流水位剖面的纯数值原语，每个都与 Python 做数值对拍。
//! 当前含：等渗回归（PAVA，非增 / 非减约束）、多峰分段等渗回归。

use water_core::find_peaks::find_peaks_prominence;
use water_core::rank_filter::median_filter_1d;

/// 非增等渗回归（Pool-Adjacent-Violators）。
///
/// 对应 Python `_isotonic_non_increasing`：
/// - 输入 1D 序列，NaN 位置跳过并原样保留；
/// - 对有限值执行「翻转 → 非减 PAVA（权重合并求均值）→ 翻转回」得到非增拟合。
///
/// 有限值数量 ≤ 1 时原样返回。
pub fn isotonic_non_increasing(values: &[f64]) -> Vec<f64> {
    let mut out = values.to_vec();

    // 收集有限值下标（跳过 NaN / ±inf，与 numpy.isfinite 一致）。
    let idx: Vec<usize> = out
        .iter()
        .enumerate()
        .filter(|(_, v)| v.is_finite())
        .map(|(i, _)| i)
        .collect();
    if idx.len() <= 1 {
        return out;
    }

    // 提取有限值并翻转（非增 = 翻转后跑非减 PAVA 再翻转回）。
    let mut v: Vec<f64> = idx.iter().rev().map(|&i| out[i]).collect();

    // 非减 PAVA：块 = (加权均值, 权重)，遇到左块 > 右块则合并。
    let mut blocks: Vec<(f64, f64)> = Vec::with_capacity(v.len());
    blocks.push((v[0], 1.0));
    for &val in v.iter().skip(1) {
        blocks.push((val, 1.0));
        while blocks.len() > 1 && blocks[blocks.len() - 2].0 > blocks[blocks.len() - 1].0 {
            let (v2, w2) = blocks[blocks.len() - 1];
            let (v1, w1) = blocks[blocks.len() - 2];
            let total_w = w1 + w2;
            let merged = (v1 * w1 + v2 * w2) / total_w;
            blocks.pop();
            let last = blocks.len() - 1;
            blocks[last] = (merged, total_w);
        }
    }

    // 展开块 → 结果，再翻转回原序。
    let mut result = Vec::with_capacity(v.len());
    for (val, cnt) in blocks {
        let c = cnt as usize;
        for _ in 0..c {
            result.push(val);
        }
    }
    result.reverse();
    v = result;

    // 写回有限值位置。
    for (k, &i) in idx.iter().enumerate() {
        out[i] = v[k];
    }
    out
}

/// 非减等渗回归。对应 Python `_isotonic_non_increasing(seg[::-1])[::-1]`。
pub fn isotonic_non_decreasing(values: &[f64]) -> Vec<f64> {
    let reversed: Vec<f64> = values.iter().rev().copied().collect();
    let mut fit = isotonic_non_increasing(&reversed);
    fit.reverse();
    fit
}

/// 多峰分段等渗回归。对应 Python `_isotonic_multi_peak`。
///
/// 流程：中值轻度平滑 → `find_peaks`（自适应显著度阈值 `max(5, 0.05*ptp)`）→
/// 端点 ∪ 峰构成锚点 → 逐段以谷为界做非增 / 非减拟合。短或无峰剖面原样返回。
pub fn isotonic_multi_peak(z_cross: &[f64]) -> Vec<f64> {
    let n = z_cross.len();
    if n < 3 {
        return z_cross.to_vec();
    }
    let finite: Vec<f64> = z_cross.iter().copied().filter(|v| v.is_finite()).collect();
    if finite.len() < 3 {
        return z_cross.to_vec();
    }

    // 自适应显著度阈值：max(5, 0.05 * ptp(finite))。
    let fmax = finite.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let fmin = finite.iter().copied().fold(f64::INFINITY, f64::min);
    let ptp = fmax - fmin;
    let prom_thr = 5.0f64.max(0.05 * ptp);

    // 轻度中值平滑后找峰。
    let med_size = 3.min(n);
    let z_smooth = median_filter_1d(z_cross, med_size);
    let (peaks, _proms) = find_peaks_prominence(&z_smooth, prom_thr);
    if peaks.is_empty() {
        return z_cross.to_vec();
    }
    let is_peak = |i: usize| peaks.contains(&i);

    // 锚点：端点 ∪ 峰，升序去重。
    let mut anchors: Vec<usize> = peaks.clone();
    anchors.push(0);
    anchors.push(n - 1);
    anchors.sort_unstable();
    anchors.dedup();

    let mut z_fit = z_cross.to_vec();
    for w in anchors.windows(2) {
        let (a, b) = (w[0], w[1]);
        if b - a < 2 {
            continue;
        }
        let mut seg: Vec<f64> = z_cross[a..=b].to_vec();

        // 段内谷（有限值最小，取首次出现）。
        let mut valley_local: Option<usize> = None;
        let mut valley_val = f64::INFINITY;
        for (k, &v) in seg.iter().enumerate() {
            if v.is_finite() && v < valley_val {
                valley_val = v;
                valley_local = Some(k);
            }
        }
        let valley_local = match valley_local {
            Some(v) => v,
            None => continue,
        };

        let a_is_peak = is_peak(a);
        let b_is_peak = is_peak(b);
        if a_is_peak && b_is_peak {
            let left = isotonic_non_increasing(&seg[..=valley_local]);
            let right = isotonic_non_decreasing(&seg[valley_local..]);
            seg[..=valley_local].copy_from_slice(&left);
            seg[valley_local..].copy_from_slice(&right);
        } else if b_is_peak {
            seg = isotonic_non_decreasing(&seg);
        } else {
            // a_is_peak 或两端皆非峰：非增。
            seg = isotonic_non_increasing(&seg);
        }

        z_fit[a..=b].copy_from_slice(&seg);
    }
    z_fit
}

/// 骨架像素坐标 (row, col)。
pub type Pixel = (i64, i64);

/// 沿有序骨架路径的局部切线（对应 Python `_compute_skeleton_tangents`）。
///
/// 以 `±context_pixels` 做有限差分，但遇到相邻跳变 `>sqrt(2)`（平方 `>2.5`）即停止扩展，
/// 使切线保持"空间局部"而非"列表局部"——分支拼接处的跳变不会把切线拉过多边形。
/// 返回每个像素的单位切线 `[row_dir, col_dir]`；退化时取 `[0, 1]`。
pub fn compute_skeleton_tangents(ordered_pixels: &[Pixel], context_pixels: usize) -> Vec<[f64; 2]> {
    let n = ordered_pixels.len();
    let mut tangents = vec![[0.0f64, 0.0f64]; n];
    const JUMP_SQ: i64 = 2; // 平方 > 2.5 ⟺ 整数平方 ≥ 3，即 > 2

    let jump = |a: Pixel, b: Pixel| -> bool {
        let d = (b.0 - a.0) * (b.0 - a.0) + (b.1 - a.1) * (b.1 - a.1);
        d > JUMP_SQ // 连续 8 邻域 ≤ 2；跳变 > 2.5 ⟺ 整数平方 > 2
    };

    for i in 0..n {
        // 向后走，遇跳变即停。对应 range(i, max(0, i-ctx), -1)。
        let mut i_back = i;
        let lower = i.saturating_sub(context_pixels);
        let mut k = i;
        while k > lower {
            if jump(ordered_pixels[k - 1], ordered_pixels[k]) {
                break;
            }
            i_back = k - 1;
            k -= 1;
        }
        // 向前走，遇跳变即停。对应 range(i, min(n-1, i+ctx))。
        let mut i_fwd = i;
        let upper = std::cmp::min(n.saturating_sub(1), i + context_pixels);
        let mut k = i;
        while k < upper {
            if jump(ordered_pixels[k], ordered_pixels[k + 1]) {
                break;
            }
            i_fwd = k + 1;
            k += 1;
        }
        if i_back == i_fwd {
            tangents[i] = [0.0, 1.0];
            continue;
        }
        let (r_back, c_back) = ordered_pixels[i_back];
        let (r_fwd, c_fwd) = ordered_pixels[i_fwd];
        let dr = (r_fwd - r_back) as f64;
        let dc = (c_fwd - c_back) as f64;
        let length = (dr * dr + dc * dc).sqrt();
        if length < 1e-12 {
            tangents[i] = [0.0, 1.0];
        } else {
            tangents[i] = [dr / length, dc / length];
        }
    }
    tangents
}

/// 标记位于汇流区的骨架站点（对应 Python `_detect_junction_stations`）。
///
/// 若某站点在 `radius_px`（2D 像素欧氏距离）内存在另一站点，其切线与本站点切线的
/// 点积绝对值 `< angle_cos_threshold`（即夹角 > acos(threshold)，默认 60°），则判为
/// 汇流站点——分支交汇处各臂的骨架像素聚集但切线各指其臂，产生交叉"扇形"。
///
/// 切线沿直线方向符号任意，故用点积绝对值。半径查询与 `cKDTree.query_ball_point`
/// 一致（欧氏距离 `<= radius_px`，含端点、跳过自身）。
pub fn detect_junction_stations(
    ordered_pixels: &[Pixel],
    tangents: &[[f64; 2]],
    radius_px: f64,
    angle_cos_threshold: f64,
) -> Vec<bool> {
    let n = ordered_pixels.len();
    let mut is_junction = vec![false; n];
    for i in 0..n {
        let (ri, ci) = ordered_pixels[i];
        for j in 0..n {
            if j == i {
                continue;
            }
            let (rj, cj) = ordered_pixels[j];
            let d2 = ((rj - ri) * (rj - ri) + (cj - ci) * (cj - ci)) as f64;
            if d2.sqrt() > radius_px {
                continue;
            }
            let cos_angle = tangents[i][0] * tangents[j][0] + tangents[i][1] * tangents[j][1];
            if cos_angle.abs() < angle_cos_threshold {
                is_junction[i] = true;
                break;
            }
        }
    }
    is_junction
}
