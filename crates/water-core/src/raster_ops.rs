//! 栅格数值算法（纯 Rust，替代 numpy / scipy.ndimage / skimage.morphology）。
//!
//! 本模块汇集原 Python 实现中依赖第三方库的数值算法，逐个改造为纯 Rust：
//!
//! - `binary_closing` / `binary_opening` —— 替代 `scipy.ndimage` 形态学
//! - `gaussian_smooth` —— 替代 `scipy.ndimage.gaussian_filter`
//! - `distance_transform_edt` —— 替代 `scipy.ndimage.distance_transform_edt`
//! - `skeletonize` —— 替代 `skimage.morphology.skeletonize`
//!
//! 每个函数都**必须**在实现后与对应 Python 库做数值对拍，并在 `docs/` 留存证据。
//!
//! 当前为骨架占位，待逐项实现。

use crate::error::{Result, WaterError};
use ndarray::Array2;

/// 二值形态学腐蚀（scipy.ndimage.binary_erosion 默认语义）。
///
/// 结构元为 4 邻域十字（`generate_binary_structure(2, 1)`：中心 + 上下左右），
/// `border_value = 0`（越界视作 0，故边界像素会被腐蚀）。忠实复刻 scipy 默认调用
/// `binary_erosion(mask, iterations=n)`。
pub fn binary_erosion(mask: &Array2<bool>, iterations: usize) -> Array2<bool> {
    let (h, w) = mask.dim();
    let mut cur = mask.clone();
    for _ in 0..iterations {
        let mut out = Array2::<bool>::from_elem((h, w), false);
        for r in 0..h {
            for c in 0..w {
                if !cur[(r, c)] {
                    continue;
                }
                // 中心为真，且上下左右四邻居均为真（越界记为假 → 腐蚀）
                let up = r > 0 && cur[(r - 1, c)];
                let down = r + 1 < h && cur[(r + 1, c)];
                let left = c > 0 && cur[(r, c - 1)];
                let right = c + 1 < w && cur[(r, c + 1)];
                out[(r, c)] = up && down && left && right;
            }
        }
        cur = out;
    }
    cur
}

/// 二值形态学闭运算（占位）。
pub fn binary_closing(_mask: &Array2<bool>, _iterations: u32) -> Result<Array2<bool>> {
    Err(WaterError::NotImplemented("raster_ops::binary_closing"))
}

/// 高斯平滑（占位）。
pub fn gaussian_smooth(_data: &Array2<f64>, _sigma: f64) -> Result<Array2<f64>> {
    Err(WaterError::NotImplemented("raster_ops::gaussian_smooth"))
}

/// 欧氏距离变换结果：距离场 + 最近背景像素的行/列索引（对应 scipy 的 return_indices）。
#[derive(Debug, Clone)]
pub struct EdtResult {
    pub distances: Array2<f64>,
    pub index_row: Array2<i64>,
    pub index_col: Array2<i64>,
}

/// 一维平方距离变换（Felzenszwalb–Huttenlocher 下包络法），返回 `(平方距离, 取得最小的源下标)`。
fn edt_1d(f: &[f64]) -> (Vec<f64>, Vec<usize>) {
    let n = f.len();
    let mut d = vec![0.0f64; n];
    let mut arg = vec![0usize; n];
    if n == 0 {
        return (d, arg);
    }
    let mut v = vec![0usize; n];
    let mut z = vec![0.0f64; n + 1];
    let mut k = 0usize;
    v[0] = 0;
    z[0] = f64::NEG_INFINITY;
    z[1] = f64::INFINITY;
    let sq = |i: usize| f[i] + (i as f64) * (i as f64);
    for q in 1..n {
        let mut s;
        loop {
            let vk = v[k];
            s = (sq(q) - sq(vk)) / (2.0 * q as f64 - 2.0 * vk as f64);
            if s <= z[k] {
                // z[0] = -inf 保证 k 不会低于 0
                k -= 1;
            } else {
                break;
            }
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = f64::INFINITY;
    }
    k = 0;
    for q in 0..n {
        while z[k + 1] < q as f64 {
            k += 1;
        }
        let dq = q as f64 - v[k] as f64;
        d[q] = dq * dq + f[v[k]];
        arg[q] = v[k];
    }
    (d, arg)
}

/// 精确欧氏距离变换（对标 `scipy.ndimage.distance_transform_edt`）。
///
/// `mask` 中 `true` 为前景：对每个前景像素返回到**最近 `false`（背景）像素**的欧氏距离，
/// 背景像素距离为 0；并返回每个像素最近背景像素的行/列索引。
/// 采用 Felzenszwalb–Huttenlocher 两遍（列 + 行）精确算法。
pub fn distance_transform_edt(mask: &Array2<bool>) -> EdtResult {
    let (h, w) = mask.dim();
    let big = (h * h + w * w) as f64 * 4.0 + 1.0;

    // 列遍：每列做 1D 变换，得到到本列最近背景的平方纵距 + 源行。
    let mut d1 = Array2::<f64>::zeros((h, w));
    let mut src_row = Array2::<usize>::zeros((h, w));
    let mut col = vec![0.0f64; h];
    for c in 0..w {
        for r in 0..h {
            col[r] = if mask[(r, c)] { big } else { 0.0 };
        }
        let (d, arg) = edt_1d(&col);
        for r in 0..h {
            d1[(r, c)] = d[r];
            src_row[(r, c)] = arg[r];
        }
    }

    // 行遍：对每行以 d1 为 f 做 1D 变换，合成平方欧氏距离 + 源列。
    let mut distances = Array2::<f64>::zeros((h, w));
    let mut index_row = Array2::<i64>::zeros((h, w));
    let mut index_col = Array2::<i64>::zeros((h, w));
    let mut row = vec![0.0f64; w];
    for r in 0..h {
        for c in 0..w {
            row[c] = d1[(r, c)];
        }
        let (d, arg) = edt_1d(&row);
        for c in 0..w {
            distances[(r, c)] = d[c].max(0.0).sqrt();
            let sc = arg[c];
            index_row[(r, c)] = src_row[(r, sc)] as i64;
            index_col[(r, c)] = sc as i64;
        }
    }

    EdtResult {
        distances,
        index_row,
        index_col,
    }
}

/// 3×3 邻域内 8 连通分量数（`cells` 为 9 位掩膜，bit = r*3+c）。
fn count_components_8(cells: u16) -> u32 {
    let get = |r: i32, c: i32| r >= 0 && r < 3 && c >= 0 && c < 3 && (cells >> (r * 3 + c) & 1) != 0;
    let mut visited = 0u16;
    let mut count = 0u32;
    for r in 0..3i32 {
        for c in 0..3i32 {
            let b = (r * 3 + c) as u16;
            if (cells >> b & 1) != 0 && (visited >> b & 1) == 0 {
                count += 1;
                let mut stack = vec![(r, c)];
                visited |= 1 << b;
                while let Some((cr, cc)) = stack.pop() {
                    for dr in -1..=1 {
                        for dc in -1..=1 {
                            if dr == 0 && dc == 0 {
                                continue;
                            }
                            let (nr, nc) = (cr + dr, cc + dc);
                            if get(nr, nc) {
                                let nb = (nr * 3 + nc) as u16;
                                if (visited >> nb & 1) == 0 {
                                    visited |= 1 << nb;
                                    stack.push((nr, nc));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    count
}

/// 构建 512 项查表：`keep = 中心前景 且 (去掉中心改变 8 连通分量数 或 邻域前景数<3)`。
/// 对应 skimage.morphology.medial_axis 的 table。
fn build_medial_axis_table() -> [u8; 512] {
    let mut table = [0u8; 512];
    for index in 0u16..512 {
        let center_fg = (index >> 4) & 1 != 0;
        let sum = index.count_ones();
        let without_center = index & !(1 << 4);
        let conn_change = count_components_8(index) != count_components_8(without_center);
        table[index as usize] = u8::from(center_fg && (conn_change || sum < 3));
    }
    table
}

/// 计算像素 `(r,c)` 的 3×3 邻域 9 位索引（越界记为背景 0）。
/// bit 位序：`bit = (dr+1)*3 + (dc+1)`，中心为 bit4。
fn neighborhood_index(grid: &Array2<bool>, r: usize, c: usize, h: usize, w: usize) -> u16 {
    let mut idx = 0u16;
    for dr in -1i32..=1 {
        for dc in -1i32..=1 {
            let (nr, nc) = (r as i32 + dr, c as i32 + dc);
            if nr >= 0
                && nr < h as i32
                && nc >= 0
                && nc < w as i32
                && grid[(nr as usize, nc as usize)]
            {
                idx |= 1 << (((dr + 1) * 3 + (dc + 1)) as u16);
            }
        }
    }
    idx
}

/// 中轴骨架（medial axis），忠实复刻 `skimage.morphology.medial_axis`。
///
/// skimage 用 PCG64 随机 permutation 作并列 tiebreaker（默认非确定性）。为可对拍，
/// 本函数接受**外部注入的 `tiebreaker`**（行主序对齐前景像素，长度 = 前景像素数）——
/// 对拍时注入 skimage 同种子生成的同一 permutation，即得逐像素一致结果。
///
/// 顺序 = 按 `(distance, corner_score, tiebreaker)` 升序；`distance` 为 EDT，
/// `corner_score = 9 - 邻域前景数`；单遍按序细化，`table[邻域index]==0` 则删除（用当前 result 查邻域）。
pub fn medial_axis(mask: &Array2<bool>, tiebreaker: &[usize]) -> Array2<bool> {
    let (h, w) = mask.dim();
    let table = build_medial_axis_table();
    let dist = distance_transform_edt(mask).distances;

    // 行主序枚举前景像素
    let mut fg: Vec<(usize, usize)> = Vec::new();
    for r in 0..h {
        for c in 0..w {
            if mask[(r, c)] {
                fg.push((r, c));
            }
        }
    }
    let n = fg.len();
    assert_eq!(tiebreaker.len(), n, "tiebreaker 长度须等于前景像素数");

    // corner_score = 9 - 邻域前景数
    let corner: Vec<u32> = fg
        .iter()
        .map(|&(r, c)| 9 - neighborhood_index(mask, r, c, h, w).count_ones())
        .collect();

    // 按 (distance, corner_score, tiebreaker) 升序
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        let (ra, ca) = fg[a];
        let (rb, cb) = fg[b];
        dist[(ra, ca)]
            .partial_cmp(&dist[(rb, cb)])
            .unwrap()
            .then(corner[a].cmp(&corner[b]))
            .then(tiebreaker[a].cmp(&tiebreaker[b]))
    });

    // 单遍细化
    let mut result = mask.clone();
    for &k in &order {
        let (r, c) = fg[k];
        if result[(r, c)] {
            let idx = neighborhood_index(&result, r, c, h, w);
            if table[idx as usize] == 0 {
                result[(r, c)] = false;
            }
        }
    }
    result
}
