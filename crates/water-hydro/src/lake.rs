//! 湖泊常数水位相关数值内核。
//!
//! 本阶段实现纯数值内核 `iterative_trimmed_median`（忠实复刻
//! Python `hydro/hydro_lake_flatten.py::_iterative_trimmed_median`）。
//! 完整的逐多边形/逐连通体常数水位（依赖栅格化 + 岸线环腐蚀）留待阶段 5。

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
