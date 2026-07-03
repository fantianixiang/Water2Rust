//! 逐多边形 Laplace 水面求解（∇²z = 0，Dirichlet 边界）。
//!
//! 忠实复刻 Python `hydro/hydro_laplace.py::solve_laplace_dirichlet`。
//!
//! 离散化：5 点差分。多边形外的邻居视作 Neumann ∂z/∂n = 0（ghost cell 取中心值，
//! 对差分贡献 0，等价于从方程中丢弃该邻居）。
//!
//! Python 组装的线性系统 `A z = rhs`：
//! - `A[idx, idx] = -n_nb`（n_nb 为在域内的邻居数，含内部与 Dirichlet）
//! - `A[idx, 内部邻居] = +1`
//! - `rhs[idx] -= dirichlet_z[Dirichlet 邻居]`
//!
//! 该 `A` 为对称负定；本实现改解等价的 SPD 系统 `M z = b`（`M = -A`，`b = -rhs`），
//! 用 **faer** 的稀疏 Cholesky（内置 AMD fill-reducing 重排序）直接分解求解。
//! 线性系统解唯一，故与 scipy `spsolve`（SuperLU + COLAMD 重排序）在数值容差内一致。
//! 相比 `nalgebra-sparse` 无重排序的 Cholesky（2D 网格填充 O(n³) 内存、大水域算爆），
//! AMD 重排序把填充压到 ~O(N log N)，是大水面能在合理时间/内存内求解的关键。

use faer::dyn_stack::{MemBuffer, MemStack};
use faer::linalg::cholesky::llt::factor::LltRegularization;
use faer::reborrow::*;
use faer::sparse::linalg::cholesky::{
    factorize_symbolic_cholesky, CholeskySymbolicParams, SymmetricOrdering,
};
use faer::sparse::{SparseColMat, Triplet};
use faer::{Conj, Mat, Par, Side};
use ndarray::Array2;

const OFFSETS: [(i64, i64); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];

/// 求解 SPD 系统 `M z = b`（`M` 以下三角三元组给出），用 faer AMD 稀疏 Cholesky。
///
/// `triplets` 含对角与**下三角**非对角项（每条无向边只存一次，row > col），
/// AMD 重排序自动最小化填充。返回长度 `n` 的解向量。
fn solve_spd_faer(n: usize, triplets: &[Triplet<usize, usize, f64>], b: &[f64]) -> Vec<f64> {
    let a = SparseColMat::<usize, f64>::try_new_from_triplets(n, n, triplets)
        .expect("组装 Dirichlet Laplacian 稀疏矩阵");
    let symbolic = factorize_symbolic_cholesky(
        a.symbolic(),
        Side::Lower,
        SymmetricOrdering::Amd,
        CholeskySymbolicParams::default(),
    )
    .expect("Dirichlet Laplacian 符号分解");
    let mut l_val = vec![0.0f64; symbolic.len_val()];
    let llt = symbolic
        .factorize_numeric_llt::<f64>(
            &mut l_val,
            a.rb(),
            Side::Lower,
            LltRegularization::default(),
            Par::Seq,
            MemStack::new(&mut MemBuffer::new(
                symbolic.factorize_numeric_llt_scratch::<f64>(Par::Seq, Default::default()),
            )),
            Default::default(),
        )
        .expect("Dirichlet Laplacian 应为对称正定");
    let mut x = Mat::<f64>::zeros(n, 1);
    for (i, &bi) in b.iter().enumerate() {
        x[(i, 0)] = bi;
    }
    llt.solve_in_place_with_conj(
        Conj::No,
        x.as_mut(),
        Par::Seq,
        MemStack::new(&mut MemBuffer::new(
            symbolic.solve_in_place_scratch::<f64>(1, Par::Seq),
        )),
    );
    (0..n).map(|i| x[(i, 0)]).collect()
}

/// 在 `poly_mask` 上求解 ∇²z = 0，`dirichlet_mask` 处施加 Dirichlet 边界（值取 `dirichlet_z`）。
///
/// 返回 f64 数组，形状同 `poly_mask`，`poly_mask` 外为 NaN。
pub fn solve_laplace_dirichlet(
    poly_mask: &Array2<bool>,
    dirichlet_mask: &Array2<bool>,
    dirichlet_z: &Array2<f64>,
) -> Array2<f64> {
    let (h, w) = poly_mask.dim();
    let mut result = Array2::<f64>::from_elem((h, w), f64::NAN);

    // 先写入 Dirichlet 值
    for r in 0..h {
        for c in 0..w {
            if dirichlet_mask[(r, c)] {
                result[(r, c)] = dirichlet_z[(r, c)];
            }
        }
    }

    // interior = poly_mask & ~dirichlet_mask，并给每个内部像素编号
    let mut var_idx = Array2::<i64>::from_elem((h, w), -1);
    let mut int_rc: Vec<(usize, usize)> = Vec::new();
    for r in 0..h {
        for c in 0..w {
            if poly_mask[(r, c)] && !dirichlet_mask[(r, c)] {
                var_idx[(r, c)] = int_rc.len() as i64;
                int_rc.push((r, c));
            }
        }
    }
    let n_int = int_rc.len();
    if n_int == 0 {
        return result;
    }

    // 组装 SPD 系统 M z = b（M = -A）：对角 + 下三角非对角三元组，右端项 b。
    let mut triplets: Vec<Triplet<usize, usize, f64>> = Vec::with_capacity(n_int * 3);
    let mut b = vec![0.0f64; n_int];

    for (idx, &(r, c)) in int_rc.iter().enumerate() {
        let mut n_nb = 0.0f64;
        for (dr, dc) in OFFSETS {
            let nr = r as i64 + dr;
            let nc = c as i64 + dc;
            if nr < 0 || nr >= h as i64 || nc < 0 || nc >= w as i64 {
                continue;
            }
            let (nr, nc) = (nr as usize, nc as usize);
            if poly_mask[(nr, nc)] && !dirichlet_mask[(nr, nc)] {
                // 内部邻居：M[idx, nb] = -1，只存下三角（每条无向边一次）
                let j = var_idx[(nr, nc)] as usize;
                if j < idx {
                    triplets.push(Triplet::new(idx, j, -1.0));
                }
                n_nb += 1.0;
            } else if dirichlet_mask[(nr, nc)] {
                // Dirichlet 邻居：并入右端项
                b[idx] += dirichlet_z[(nr, nc)];
                n_nb += 1.0;
            }
        }
        triplets.push(Triplet::new(idx, idx, n_nb));
    }

    let z = solve_spd_faer(n_int, &triplets, &b);

    for (idx, &(r, c)) in int_rc.iter().enumerate() {
        result[(r, c)] = z[idx];
    }
    result
}

