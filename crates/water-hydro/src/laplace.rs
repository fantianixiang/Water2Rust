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
//! 用 `nalgebra-sparse` 的 Cholesky 直接分解求解。线性系统解唯一，故与 scipy `spsolve`
//! 在数值容差内一致。

use nalgebra::DVector;
use nalgebra_sparse::factorization::CscCholesky;
use nalgebra_sparse::{CooMatrix, CscMatrix};
use ndarray::Array2;

const OFFSETS: [(i64, i64); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];

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

    // 组装 SPD 系统 M z = b（M = -A）
    let mut coo = CooMatrix::<f64>::new(n_int, n_int);
    let mut b = DVector::<f64>::zeros(n_int);

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
                // 内部邻居：M[idx, nb] = -1
                coo.push(idx, var_idx[(nr, nc)] as usize, -1.0);
                n_nb += 1.0;
            } else if dirichlet_mask[(nr, nc)] {
                // Dirichlet 邻居：并入右端项
                b[idx] += dirichlet_z[(nr, nc)];
                n_nb += 1.0;
            }
        }
        coo.push(idx, idx, n_nb);
    }

    let csc = CscMatrix::from(&coo);
    let chol = CscCholesky::factor(&csc).expect("Dirichlet Laplacian 应为对称正定");
    let z = chol.solve(&b);

    for (idx, &(r, c)) in int_rc.iter().enumerate() {
        result[(r, c)] = z[(idx, 0)];
    }
    result
}
