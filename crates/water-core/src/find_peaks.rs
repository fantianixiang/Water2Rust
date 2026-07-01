//! 峰值检测（对标 `scipy.signal.find_peaks`，仅 `prominence` 条件）。
//!
//! **替换的 Python 库**：scipy.signal.find_peaks / _peak_prominences / _local_maxima_1d。
//!
//! 复刻链路：局部极大值（含平台取中点）→ 显著度（prominence，全信号 wlen）→ 按下限过滤。
//! NaN 处理与 numpy 一致：涉及 NaN 的 `<` / `<=` / `==` 均为 false（Rust 原生比较同语义）。

/// 局部极大值下标（对应 scipy `_local_maxima_1d`）。
///
/// 峰为严格大于两侧直邻的样本；平台（等值连续）取其中点 `(left+right)/2` 向下取整。
pub fn local_maxima_1d(x: &[f64]) -> Vec<usize> {
    let n = x.len();
    let mut midpoints = Vec::new();
    if n < 3 {
        return midpoints;
    }
    let i_max = n - 1; // 最后一个样本不参与
    let mut i = 1usize;
    while i < i_max {
        if x[i - 1] < x[i] {
            let mut i_ahead = i + 1;
            while i_ahead < i_max && x[i_ahead] == x[i] {
                i_ahead += 1;
            }
            if x[i_ahead] < x[i] {
                let left_edge = i;
                let right_edge = i_ahead - 1;
                midpoints.push((left_edge + right_edge) / 2);
                i = i_ahead;
            }
        }
        i += 1;
    }
    midpoints
}

/// 峰的显著度（对应 scipy `_peak_prominences`，wlen 全信号）。
///
/// 对每个峰，向左 / 向右直到遇到不低于峰高的样本，取两侧路径最低点的较高者作基准，
/// prominence = 峰高 − max(left_min, right_min)。
pub fn peak_prominences(x: &[f64], peaks: &[usize]) -> Vec<f64> {
    let n = x.len();
    let mut prominences = Vec::with_capacity(peaks.len());
    for &peak in peaks {
        let peak_h = x[peak];

        // 向左
        let mut left_min = peak_h;
        let mut i = peak as i64;
        while i >= 0 && x[i as usize] <= peak_h {
            if x[i as usize] < left_min {
                left_min = x[i as usize];
            }
            i -= 1;
        }

        // 向右
        let mut right_min = peak_h;
        let mut j = peak;
        while j <= n - 1 && x[j] <= peak_h {
            if x[j] < right_min {
                right_min = x[j];
            }
            if j == n - 1 {
                break;
            }
            j += 1;
        }

        prominences.push(peak_h - left_min.max(right_min));
    }
    prominences
}

/// `find_peaks(x, prominence=prom_min)` 的等价结果：过滤后的峰下标与其显著度。
///
/// 过滤条件与 scipy 一致：`prominence >= prom_min`（闭区间下限）。
pub fn find_peaks_prominence(x: &[f64], prom_min: f64) -> (Vec<usize>, Vec<f64>) {
    let peaks = local_maxima_1d(x);
    let proms = peak_prominences(x, &peaks);
    let mut kept = Vec::new();
    let mut kept_prom = Vec::new();
    for (p, pr) in peaks.into_iter().zip(proms) {
        if pr >= prom_min {
            kept.push(p);
            kept_prom.push(pr);
        }
    }
    (kept, kept_prom)
}
