//! 秩滤波（对标 `scipy.ndimage` 的 `percentile_filter` / `median_filter`）。
//!
//! **替换的 Python 库**：scipy.ndimage 秩滤波。默认边界 mode='reflect'、origin=0。
//!
//! 秩计算与 scipy `_rank_filter` 一致：
//! - percentile：`rank = int(filter_size * percentile / 100)`（percentile==100 时为 `filter_size-1`）；
//! - median：`rank = filter_size // 2`。
//!
//! 秩滤波在窗口内取第 `rank` 小（0 起）的值。窗口为边长 `size` 的方框，
//! 相对偏移 `[-size/2, size-1-size/2]`（origin=0）。

use ndarray::Array2;

use crate::raster_ops::reflect_index;

/// 由 filter_size 与 percentile 计算秩（与 scipy `_rank_filter` 的 percentile 分支一致）。
fn percentile_rank(filter_size: usize, percentile: f64) -> usize {
    let mut p = percentile;
    if p < 0.0 {
        p += 100.0;
    }
    assert!((0.0..=100.0).contains(&p), "invalid percentile: {percentile}");
    if p == 100.0 {
        filter_size - 1
    } else {
        // int() 对正数向零截断，等价 as usize。
        (filter_size as f64 * p / 100.0) as usize
    }
}

/// 二维百分位秩滤波（方框窗口，边长 `size`，mode='reflect'）。
///
/// 对应 `scipy.ndimage.percentile_filter(data, percentile, size=size)`。
pub fn percentile_filter_2d(data: &Array2<f64>, percentile: f64, size: usize) -> Array2<f64> {
    assert!(size >= 1, "size 必须 ≥ 1");
    let fs = size * size;
    let rank = percentile_rank(fs, percentile);
    rank_filter_2d(data, size, rank)
}

/// 二维秩滤波核：每个像素取方框窗口内第 `rank` 小的值。
fn rank_filter_2d(data: &Array2<f64>, size: usize, rank: usize) -> Array2<f64> {
    let (h, w) = data.dim();
    let s = size as i64;
    let lo = -(s / 2); // origin=0 的窗口下界
    let hi = s - 1 - s / 2; // 上界
    let mut out = Array2::<f64>::zeros((h, w));
    let mut buf: Vec<f64> = Vec::with_capacity(size * size);
    for r in 0..h {
        for c in 0..w {
            buf.clear();
            for dr in lo..=hi {
                let rr = reflect_index(r as i64 + dr, h as i64);
                for dc in lo..=hi {
                    let cc = reflect_index(c as i64 + dc, w as i64);
                    buf.push(data[(rr, cc)]);
                }
            }
            // 与 numpy 排序一致：升序，NaN 视为最大（本用途调用方已用 +inf 填充）。
            buf.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Greater));
            out[(r, c)] = buf[rank];
        }
    }
    out
}

/// 一维中值滤波（方框窗口，长度 `size`，mode='reflect'）。
///
/// 对应 `scipy.ndimage.median_filter(data, size=size)`（median 秩 = `size // 2`）。
pub fn median_filter_1d(data: &[f64], size: usize) -> Vec<f64> {
    assert!(size >= 1, "size 必须 ≥ 1");
    let rank = size / 2;
    let n = data.len() as i64;
    let s = size as i64;
    let lo = -(s / 2);
    let hi = s - 1 - s / 2;
    let mut out = vec![0.0f64; data.len()];
    let mut buf: Vec<f64> = Vec::with_capacity(size);
    for i in 0..data.len() {
        buf.clear();
        for d in lo..=hi {
            let ii = reflect_index(i as i64 + d, n);
            buf.push(data[ii]);
        }
        buf.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Greater));
        out[i] = buf[rank];
    }
    out
}
