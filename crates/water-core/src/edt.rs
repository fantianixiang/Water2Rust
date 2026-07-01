//! 精确欧氏距离变换（纯 Rust，替代 `scipy.ndimage.distance_transform_edt`）。
//!
//! 采用 Felzenszwalb–Huttenlocher 两遍（列 + 行）下包络法，含 `return_indices` 语义。

use ndarray::Array2;

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
