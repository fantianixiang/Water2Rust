//! 骨架图构建与沿流排序（对应 Python `waters/hydro/skeleton_dryrun.py` 的图部分）。
//!
//! **替换的 Python 库**：numpy（`nonzero` 行主序遍历）、collections（BFS 队列）。
//!
//! 供河流 z_local 管线把 medial-axis 骨架从出水口向上游排序。保真要点：
//! - 节点索引按 `np.nonzero` 的**行主序**（先行后列）建立；
//! - 走支/求度按固定 8 邻域顺序 `_NEIGHBORS_8`；
//! - BFS、`sorted`、`min` 均为稳定序，平手取先出现者。

use std::collections::{HashSet, VecDeque};

use ndarray::Array2;

use crate::skeleton_zloc::Pixel;

/// 8 邻域偏移，顺序与 Python `_NEIGHBORS_8` 完全一致（行主序扫描）。
const NEIGHBORS_8: [(i64, i64); 8] = [
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, -1),
    (0, 1),
    (1, -1),
    (1, 0),
    (1, 1),
];

/// 骨架图的一条支：`pixels[0]` = 节点 `node_a`，`pixels[last]` = 节点 `node_b`。
#[derive(Debug, Clone)]
pub struct Branch {
    pub node_a: usize,
    pub node_b: usize,
    pub pixels: Vec<Pixel>,
}

/// 逐骨架像素的 8 邻域度（对应 `_skeleton_pixel_degree`）。
pub fn skeleton_pixel_degree(skel: &Array2<bool>) -> Array2<i32> {
    let (h, w) = skel.dim();
    let mut deg = Array2::<i32>::zeros((h, w));
    for r in 0..h {
        for c in 0..w {
            if !skel[(r, c)] {
                continue;
            }
            let mut d = 0;
            for (dr, dc) in NEIGHBORS_8 {
                let nr = r as i64 + dr;
                let nc = c as i64 + dc;
                if nr >= 0 && nr < h as i64 && nc >= 0 && nc < w as i64 && skel[(nr as usize, nc as usize)] {
                    d += 1;
                }
            }
            deg[(r, c)] = d;
        }
    }
    deg
}

/// 从起点沿度为 2 的像素走到端点/交汇点（对应 `_walk_branch`）。返回含两端的有序像素。
fn walk_branch(
    skel: &Array2<bool>,
    deg: &Array2<i32>,
    visited: &mut Array2<bool>,
    start: Pixel,
    first_step: Pixel,
) -> Vec<Pixel> {
    let (h, w) = skel.dim();
    let mut path = vec![start, first_step];
    visited[(first_step.0 as usize, first_step.1 as usize)] = true;
    let (mut cr, mut cc) = first_step;
    let (mut pr, mut pc) = start;

    while deg[(cr as usize, cc as usize)] == 2 {
        let mut moved = false;
        for (dr, dc) in NEIGHBORS_8 {
            let nr = cr + dr;
            let nc = cc + dc;
            if (nr, nc) == (pr, pc) {
                continue;
            }
            if nr >= 0 && nr < h as i64 && nc >= 0 && nc < w as i64 && skel[(nr as usize, nc as usize)] {
                pr = cr;
                pc = cc;
                cr = nr;
                cc = nc;
                path.push((cr, cc));
                visited[(cr as usize, cc as usize)] = true;
                moved = true;
                break;
            }
        }
        if !moved {
            break;
        }
    }
    path
}

/// 构建骨架图（对应 `_build_skeleton_graph`）。
///
/// 返回 `(node_pixels, branches, deg)`：节点为端点（度 1）与交汇（度 ≥ 3）。
pub fn build_skeleton_graph(skel: &Array2<bool>) -> (Vec<Pixel>, Vec<Branch>, Array2<i32>) {
    let deg = skeleton_pixel_degree(skel);
    let (h, w) = skel.dim();

    let mut node_pixels: Vec<Pixel> = Vec::new();
    let mut node_idx_map: std::collections::HashMap<Pixel, usize> = std::collections::HashMap::new();
    // 行主序遍历（与 np.nonzero 一致）。
    for r in 0..h {
        for c in 0..w {
            if skel[(r, c)] && deg[(r, c)] != 2 {
                let p = (r as i64, c as i64);
                node_idx_map.insert(p, node_pixels.len());
                node_pixels.push(p);
            }
        }
    }

    let mut visited_edge = Array2::<bool>::default((h, w));
    let mut branches: Vec<Branch> = Vec::new();

    for &(sr, sc) in &node_pixels {
        for (dr, dc) in NEIGHBORS_8 {
            let nr = sr + dr;
            let nc = sc + dc;
            if nr < 0 || nr >= h as i64 || nc < 0 || nc >= w as i64 {
                continue;
            }
            let (nru, ncu) = (nr as usize, nc as usize);
            if !skel[(nru, ncu)] || visited_edge[(nru, ncu)] {
                continue;
            }
            let path = walk_branch(skel, &deg, &mut visited_edge, (sr, sc), (nr, nc));
            if path.len() < 2 {
                continue;
            }
            let end_pixel = *path.last().unwrap();
            let Some(&nb) = node_idx_map.get(&end_pixel) else {
                continue;
            };
            branches.push(Branch {
                node_a: node_idx_map[&(sr, sc)],
                node_b: nb,
                pixels: path,
            });
        }
    }

    (node_pixels, branches, deg)
}

/// 出水口树边 BFS 距离，不可达为 -1（对应 `_bfs_distance_to_outlet`）。
pub fn bfs_distance_to_outlet(n_nodes: usize, branches: &[Branch], outlet_idx: usize) -> Vec<i64> {
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n_nodes];
    for br in branches {
        adj[br.node_a].push(br.node_b);
        adj[br.node_b].push(br.node_a);
    }
    let mut dist = vec![-1i64; n_nodes];
    dist[outlet_idx] = 0;
    let mut queue: VecDeque<usize> = VecDeque::new();
    queue.push_back(outlet_idx);
    while let Some(u) = queue.pop_front() {
        for &v in &adj[u] {
            if dist[v] == -1 {
                dist[v] = dist[u] + 1;
                queue.push_back(v);
            }
        }
    }
    dist
}

/// 归一化支方向：使 `node_a` 恒为靠出水口一侧（对应 `_normalise_branch_directions`）。
pub fn normalise_branch_directions(branches: &mut [Branch], distance: &[i64]) {
    for br in branches.iter_mut() {
        let (a, b) = (br.node_a, br.node_b);
        if distance[a] == -1 || distance[b] == -1 {
            continue;
        }
        if distance[a] > distance[b] {
            br.pixels.reverse();
            br.node_a = b;
            br.node_b = a;
        }
    }
}

/// 从出水口（DEM 最低端点）向上游排序骨架像素（对应 `_order_skeleton_pixels_along_flow`）。
///
/// 无节点时回退：按 DEM 升序排序全部骨架像素。
pub fn order_skeleton_pixels_along_flow(skel: &Array2<bool>, dem_window: &Array2<f64>) -> Vec<Pixel> {
    let (node_pixels, mut branches, deg) = build_skeleton_graph(skel);
    let (h, w) = skel.dim();

    // 收集骨架像素（行主序，等价 np.nonzero）。
    let mut skel_pixels: Vec<Pixel> = Vec::new();
    for r in 0..h {
        for c in 0..w {
            if skel[(r, c)] {
                skel_pixels.push((r as i64, c as i64));
            }
        }
    }

    if node_pixels.is_empty() {
        // 回退：按 DEM 升序（稳定，平手取先出现）。
        let mut idx: Vec<usize> = (0..skel_pixels.len()).collect();
        idx.sort_by(|&a, &b| {
            let (ra, ca) = skel_pixels[a];
            let (rb, cb) = skel_pixels[b];
            dem_window[(ra as usize, ca as usize)]
                .partial_cmp(&dem_window[(rb as usize, cb as usize)])
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        return idx.into_iter().map(|i| skel_pixels[i]).collect();
    }

    // 出水口 = DEM 最低的端点（度 1）；无端点则取全部节点。平手取先出现。
    let mut endpoint_indices: Vec<usize> = (0..node_pixels.len())
        .filter(|&i| {
            let (r, c) = node_pixels[i];
            deg[(r as usize, c as usize)] == 1
        })
        .collect();
    if endpoint_indices.is_empty() {
        endpoint_indices = (0..node_pixels.len()).collect();
    }
    let outlet_idx = *endpoint_indices
        .iter()
        .min_by(|&&i, &&j| {
            let (ri, ci) = node_pixels[i];
            let (rj, cj) = node_pixels[j];
            dem_window[(ri as usize, ci as usize)]
                .partial_cmp(&dem_window[(rj as usize, cj as usize)])
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap();

    let bfs_dist = bfs_distance_to_outlet(node_pixels.len(), &branches, outlet_idx);
    normalise_branch_directions(&mut branches, &bfs_dist);

    // 支优先级：两端到出水口的最小 BFS 距离（-1 视为 999999）。稳定排序。
    let priority = |br: &Branch| -> i64 {
        let da = if bfs_dist[br.node_a] >= 0 { bfs_dist[br.node_a] } else { 999999 };
        let db = if bfs_dist[br.node_b] >= 0 { bfs_dist[br.node_b] } else { 999999 };
        da.min(db)
    };
    let mut order_branches: Vec<usize> = (0..branches.len()).collect();
    order_branches.sort_by_key(|&bi| priority(&branches[bi]));

    let mut visited: HashSet<Pixel> = HashSet::new();
    let mut ordered: Vec<Pixel> = Vec::new();
    for &bi in &order_branches {
        for &p in &branches[bi].pixels {
            if visited.insert(p) {
                ordered.push(p);
            }
        }
    }
    // 追加未被支覆盖的孤立骨架像素（行主序）。
    for &p in &skel_pixels {
        if visited.insert(p) {
            ordered.push(p);
        }
    }

    ordered
}
