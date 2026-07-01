//! 河心线 z_local 管线原语（对应 Python `waters/hydro/hydro_skeleton_zloc.py`）。
//!
//! **替换的 Python 库**：scipy.signal（find_peaks）、scipy.ndimage（median_filter）、numpy。
//!
//! 本模块逐个复刻河流水位剖面的纯数值原语，每个都与 Python 做数值对拍。
//! 当前含：等渗回归（PAVA，非增 / 非减约束）。

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
