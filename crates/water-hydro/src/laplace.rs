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

/// 诊断转储：若设了环境变量 `WATER_LAPLACE_DUMP_DIR`，把每个真实 Laplace 系统
/// `(poly_mask, dirichlet_mask, dirichlet_z)` 以紧凑二进制写盘，供 Python/Rust/GPU
/// 三方在**真实地形系统**上做加速对比（见 scripts/bench_laplace_real.py）。
///
/// 格式（小端）：`i64 h`, `i64 w`, `h*w u8 poly`, `h*w u8 dmask`, `h*w f64 dz`。
/// 文件名 `sys_<序号>_n<内部变量数>.bin`。
fn maybe_dump_laplace_system(
    poly_mask: &Array2<bool>,
    dirichlet_mask: &Array2<bool>,
    dirichlet_z: &Array2<f64>,
) {
    let Ok(dir) = std::env::var("WATER_LAPLACE_DUMP_DIR") else {
        return;
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    static CTR: AtomicUsize = AtomicUsize::new(0);
    let id = CTR.fetch_add(1, Ordering::Relaxed);
    let (h, w) = poly_mask.dim();
    let n_int = (0..h * w)
        .filter(|&i| poly_mask[(i / w, i % w)] && !dirichlet_mask[(i / w, i % w)])
        .count();
    let mut buf: Vec<u8> = Vec::with_capacity(16 + h * w * 10);
    buf.extend_from_slice(&(h as i64).to_le_bytes());
    buf.extend_from_slice(&(w as i64).to_le_bytes());
    for r in 0..h {
        for c in 0..w {
            buf.push(poly_mask[(r, c)] as u8);
        }
    }
    for r in 0..h {
        for c in 0..w {
            buf.push(dirichlet_mask[(r, c)] as u8);
        }
    }
    for r in 0..h {
        for c in 0..w {
            buf.extend_from_slice(&dirichlet_z[(r, c)].to_le_bytes());
        }
    }
    let path = format!("{dir}/sys_{id:04}_n{n_int}.bin");
    if let Err(e) = std::fs::write(&path, &buf) {
        tracing::warn!("转储 Laplace 系统失败 {path}: {e}");
    } else {
        tracing::info!("转储真实 Laplace 系统 {path}（{h}x{w}, n_int={n_int}）");
    }
}

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
    // 诊断：真实地形对比时按需转储真实 Laplace 系统。
    maybe_dump_laplace_system(poly_mask, dirichlet_mask, dirichlet_z);

    // GPU 分派（feature `gpu`）：内部变量数达阈值时用 matrix-free FP64 PCG（GPU），
    // 未收敛/失败自动回退 CPU faer。阈值依 profile 交叉点（见 docs/CUDA.md 路径 B）。
    #[cfg(feature = "gpu")]
    {
        let n_int = count_interior_vars(poly_mask, dirichlet_mask);
        if n_int >= GPU_PCG_MIN_VARS {
            if let Some(gpu_result) =
                solve_laplace_dirichlet_gpu(poly_mask, dirichlet_mask, dirichlet_z)
            {
                return gpu_result;
            }
        }
    }
    solve_laplace_dirichlet_cpu(poly_mask, dirichlet_mask, dirichlet_z)
}

/// CPU faer 直接稀疏 Cholesky 求解（原实现）。始终可用；GPU 未启用或回退时走此路径。
pub fn solve_laplace_dirichlet_cpu(
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

/// 统计内部变量数（poly & ~dirichlet），用于 GPU/CPU 分派判定。
#[cfg(feature = "gpu")]
fn count_interior_vars(poly_mask: &Array2<bool>, dirichlet_mask: &Array2<bool>) -> usize {
    let (h, w) = poly_mask.dim();
    let mut n = 0usize;
    for r in 0..h {
        for c in 0..w {
            if poly_mask[(r, c)] && !dirichlet_mask[(r, c)] {
                n += 1;
            }
        }
    }
    n
}

/// GPU PCG 分派阈值：内部变量数 ≥ 此值才走 GPU。
///
/// **真实地形标定（保护性）**：全量林芝最大真实 Laplace 系统 n≈414k 时，紧凑 PCG（Jacobi）
/// 与 faer **持平/略慢**（GPU kernel 368ms + 装配，vs faer 405ms；细长河需 ~1203 迭代）。
/// 故阈值设在观测最大值之上（500k），使当前真实数据**全部走 CPU faer，不产生回退**；
/// GPU 仅对更大水域启用（PCG 近线性 vs faer 超线性，规模越大越有利）。
/// 决定性反超需 multigrid 预条件（降迭代数），见 docs/CUDA.md 待办。
#[cfg(feature = "gpu")]
pub const GPU_PCG_MIN_VARS: usize = 500_000;

/// 由 `(poly_mask, dirichlet_mask, dirichlet_z)` 构建**紧凑变量** PCG 输入：
/// 只对内部变量建索引，避免在稀疏细长水域的空 bounding box 上做无用功。
///
/// 返回 `(diag, nbr, b, int_rc)`：
/// - `diag[i]` = 第 i 变量的对角（poly 邻居数，内部+Dirichlet）；
/// - `nbr[i*4+k]` = 第 i 变量第 k 邻居的变量下标（非内部邻居 = -1）；
/// - `b[i]` = Σ dirichlet_z(Dirichlet 邻居)；
/// - `int_rc[i]` = 第 i 变量的像素坐标（写回用）。
/// 与 [`solve_laplace_dirichlet`] 的 CPU 装配**同一算子**。
#[cfg(feature = "gpu")]
pub fn build_pcg_compact(
    poly_mask: &Array2<bool>,
    dirichlet_mask: &Array2<bool>,
    dirichlet_z: &Array2<f64>,
) -> (Vec<f64>, Vec<i32>, Vec<f64>, Vec<(usize, usize)>) {
    let (h, w) = poly_mask.dim();
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
    let n = int_rc.len();
    let mut diag = vec![0.0f64; n];
    let mut b = vec![0.0f64; n];
    let mut nbr = vec![-1i32; n * 4];
    for (i, &(r, c)) in int_rc.iter().enumerate() {
        let mut n_nb = 0.0f64;
        for (k, (dr, dc)) in OFFSETS.iter().enumerate() {
            let nr = r as i64 + dr;
            let nc = c as i64 + dc;
            if nr < 0 || nr >= h as i64 || nc < 0 || nc >= w as i64 {
                continue; // 域外：nbr 保持 -1
            }
            let (nr, nc) = (nr as usize, nc as usize);
            if poly_mask[(nr, nc)] && !dirichlet_mask[(nr, nc)] {
                nbr[i * 4 + k] = var_idx[(nr, nc)] as i32; // 内部邻居
                n_nb += 1.0;
            } else if dirichlet_mask[(nr, nc)] {
                b[i] += dirichlet_z[(nr, nc)]; // Dirichlet 邻居定值并入 b
                n_nb += 1.0;
            }
        }
        diag[i] = n_nb;
    }
    (diag, nbr, b, int_rc)
}

/// 用 GPU **紧凑变量** matrix-free FP64 Jacobi-PCG 求解 ∇²z=0 Dirichlet 系统
/// （与 [`solve_laplace_dirichlet`] 等价）。
///
/// 返回 `Some(result)` 当且仅当 GPU 求解成功且**收敛到目标残差**；否则 `None`（由调用方回退 faer）。
/// 只在数值可靠时采用 GPU 解，兼顾稳定性与 parity（rtol 足够紧，vs faer < 1e-6）。
#[cfg(feature = "gpu")]
pub fn solve_laplace_dirichlet_gpu(
    poly_mask: &Array2<bool>,
    dirichlet_mask: &Array2<bool>,
    dirichlet_z: &Array2<f64>,
) -> Option<Array2<f64>> {
    let (h, w) = poly_mask.dim();
    let (diag, nbr, b, int_rc) = build_pcg_compact(poly_mask, dirichlet_mask, dirichlet_z);
    let n = int_rc.len();

    // rtol 紧到保证与 faer parity < 1e-6；max_iter 依变量规模放宽（细长域迭代数偏多），
    // 未收敛则回退 faer 保稳定。
    let rtol = 1e-11;
    let max_iter = (40 * (n as f64).sqrt() as usize + 5000) as i32;
    let res = water_gpu::laplace_pcg_compact(&diag, &nbr, &b, rtol, max_iter).ok()?;
    if !res.residual.is_finite() || res.residual > rtol {
        tracing::warn!(
            "Laplace GPU PCG(紧凑) 未收敛（n={n}, iters={}, res={:.2e} > rtol={:.0e}），回退 faer",
            res.iters,
            res.residual,
            rtol
        );
        return None;
    }
    tracing::debug!(
        "Laplace GPU PCG(紧凑) 收敛：n={n} iters={} res={:.2e} solve={:.2}ms",
        res.iters,
        res.residual,
        res.timing.kernel_ms
    );

    let mut result = Array2::<f64>::from_elem((h, w), f64::NAN);
    for r in 0..h {
        for c in 0..w {
            if dirichlet_mask[(r, c)] {
                result[(r, c)] = dirichlet_z[(r, c)];
            }
        }
    }
    for (i, &(r, c)) in int_rc.iter().enumerate() {
        result[(r, c)] = res.z[i];
    }
    Some(result)
}

