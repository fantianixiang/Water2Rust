//! 秩滤波（对标 `scipy.ndimage` 的 `percentile_filter` / `median_filter`）。
//!
//! **替换的 Python 库**：scipy.ndimage 秩滤波。默认边界 mode='reflect'、origin=0。
//!
//! 秩计算与 scipy `_rank_filter` 一致：
//! - percentile：`rank = int(filter_size * percentile / 100)`（percentile==100 时为 `filter_size-1`）；
//! - median：`rank = filter_size // 2`。
//!
//! 选取第 `rank` 小的值复刻 scipy 的 `NI_Select`（Hoare 快速选择，pivot=窗口首元素），
//! 而非普通排序——对**全有限值窗口**两者结果一致（同一顺序统计量），但 `NI_Select`
//! 更快且与 scipy 走同一路径。
//!
//! **NaN 语义（scipy 实现定义，已核对 C 源 `ni_filters.c`）**：`NI_RankFilter` 把窗口值按
//! `_offsets[]` 顺序填入 buffer，再调 `NI_Select`。因 NaN 参与的 `>`/`<` 比较全为 false，
//! quickselect 的划分路径依赖 NaN 在 buffer 中的位置，可能选中 NaN 也可能选中有限值
//! （同一 `[a, b, NaN]` 三种 NaN 位置结果不同）——这是 quickselect + IEEE-754 的产物，
//! scipy 未定义"含 NaN 的中位数"。真实管线不受影响：`percentile_filter` 恒以 +inf 填充
//! （无 NaN，已 0 误差直测）；`median_filter` 的 NaN 由 `_isotonic_multi_peak` 端到端对拍
//! （`stage6c_multipeak`，0 误差）覆盖。故不对人造孤立 NaN 窗口做 bit 级断言。
//! 窗口为边长 `size` 的方框，相对偏移 `[-size/2, size-1-size/2]`（origin=0）。

use ndarray::Array2;

use crate::raster_ops::reflect_index;

/// scipy `NI_Select` 的忠实移植：在 `buffer[min..=max]` 上就地部分划分，返回第 `rank` 小的值。
///
/// pivot 取 `buffer[min]`；`do jj--; while(buffer[jj] > x)` / `do ii++; while(buffer[ii] < x)`
/// 的 NaN 语义（比较为 false 即停）与 Rust 原生 `>` / `<` 一致，因此 NaN 行为与 scipy 逐位相同。
fn ni_select(buffer: &mut [f64], min: isize, max: isize, rank: isize) -> f64 {
    if min == max {
        return buffer[min as usize];
    }
    let x = buffer[min as usize];
    let mut ii = min - 1;
    let mut jj = max + 1;
    loop {
        loop {
            jj -= 1;
            if !(buffer[jj as usize] > x) {
                break;
            }
        }
        loop {
            ii += 1;
            if !(buffer[ii as usize] < x) {
                break;
            }
        }
        if ii < jj {
            buffer.swap(ii as usize, jj as usize);
        } else {
            break;
        }
    }
    let cnt = jj - min + 1;
    if rank < cnt {
        ni_select(buffer, min, jj, rank)
    } else {
        ni_select(buffer, jj + 1, max, rank - cnt)
    }
}

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

/// 二维秩滤波核：每个像素取方框窗口内第 `rank` 小的值（NI_Select）。
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
            let n = buf.len() as isize;
            out[(r, c)] = ni_select(&mut buf, 0, n - 1, rank as isize);
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
        let m = buf.len() as isize;
        out[i] = ni_select(&mut buf, 0, m - 1, rank as isize);
    }
    out
}

