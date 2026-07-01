//! 中轴骨架（medial axis），纯 Rust 复刻 `skimage.morphology.medial_axis`。

use ndarray::Array2;

use crate::edt::distance_transform_edt;

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
