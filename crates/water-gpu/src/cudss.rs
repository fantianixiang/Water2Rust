//! Tier 1 PoC —— cuDSS GPU 直接稀疏 Cholesky 求解器（对拍 CPU faer）。
//!
//! 目的：验证「GPU 直接稀疏解（带 fill-reducing 重排序）能达到 CPU faer 的数值 parity」，
//! 并拆分耗时（H2D / 分析 / 数值分解 / 求解 / D2H）。这是 hydro Laplace 水面求解上 GPU 的
//! 关键前置证据（见 GPU 改造清单 Tier 1）。
//!
//! 设计：
//! - **求解库**：NVIDIA cuDSS（直接稀疏 Cholesky/LDLᵀ），重排序设为 **AMD**，与 faer 对齐。
//! - **显存/流**：用 CUDA 运行时 API（cudart）统一管理，规避 cudarc（驱动 API 上下文）与
//!   cuDSS（运行时 API）之间的上下文错配。
//! - **矩阵**：CSR，`mtype=SPD`、`mview=LOWER`（只给下三角 + 对角），`base=0`，值 f64、索引 i32。
//!
//! 仅在启用 `cudss` 特性时编译（默认关，保持无 cuDSS 环境也可构建 water-gpu）。

#![cfg(feature = "cudss")]

use std::ffi::c_void;
use std::ptr;
use std::time::{Duration, Instant};

// ── cudss 数据类型/枚举常量（对应 cudss_data_types.h） ──
const CUDSS_R_64F: i32 = 1; // = CUDA_R_64F
const CUDSS_R_32I: i32 = 10; // = CUDA_R_32I
const CUDSS_MTYPE_SPD: i32 = 3;
const CUDSS_MVIEW_LOWER: i32 = 1;
const CUDSS_BASE_ZERO: i32 = 0;
const CUDSS_LAYOUT_COL_MAJOR: i32 = 0;
const CUDSS_CONFIG_REORDERING_ALG: i32 = 0;
const CUDSS_CONFIG_HOST_NTHREADS: i32 = 14;
/// cuDSS 重排序算法（cudssReorderingAlg_t）。
pub const CUDSS_REORDERING_ALG_DEFAULT: i32 = 0;
pub const CUDSS_REORDERING_ALG_AMD: i32 = 3;
pub const CUDSS_REORDERING_ALG_NESTED_DISSECTION: i32 = 4;
// phase 位掩码（cudssPhase_t）
const CUDSS_PHASE_ANALYSIS: i32 = (1 << 0) | (1 << 1); // reordering | symbolic = 3
const CUDSS_PHASE_FACTORIZATION: i32 = 1 << 2; // 4
const CUDSS_PHASE_SOLVE: i32 =
    (1 << 4) | (1 << 5) | (1 << 6) | (1 << 7) | (1 << 8) | (1 << 9); // 1008
const CUDSS_STATUS_SUCCESS: i32 = 0;
// cudaMemcpyKind
const CUDA_MEMCPY_H2D: i32 = 1;
const CUDA_MEMCPY_D2H: i32 = 2;
const CUDA_SUCCESS: i32 = 0;

#[allow(non_camel_case_types)]
type cudssHandle_t = *mut c_void;
#[allow(non_camel_case_types)]
type cudssConfig_t = *mut c_void;
#[allow(non_camel_case_types)]
type cudssData_t = *mut c_void;
#[allow(non_camel_case_types)]
type cudssMatrix_t = *mut c_void;
#[allow(non_camel_case_types)]
type cudaStream_t = *mut c_void;

#[allow(non_snake_case)]
extern "C" {
    fn cudssCreate(handle: *mut cudssHandle_t) -> i32;
    fn cudssDestroy(handle: cudssHandle_t) -> i32;
    fn cudssSetStream(handle: cudssHandle_t, stream: cudaStream_t) -> i32;
    fn cudssConfigCreate(cfg: *mut cudssConfig_t) -> i32;
    fn cudssConfigDestroy(cfg: cudssConfig_t) -> i32;
    fn cudssConfigSet(cfg: cudssConfig_t, param: i32, value: *const c_void, size: usize) -> i32;
    fn cudssDataCreate(handle: cudssHandle_t, data: *mut cudssData_t) -> i32;
    fn cudssDataDestroy(handle: cudssHandle_t, data: cudssData_t) -> i32;
    fn cudssExecute(
        handle: cudssHandle_t,
        phase: i32,
        cfg: cudssConfig_t,
        data: cudssData_t,
        a: cudssMatrix_t,
        x: cudssMatrix_t,
        b: cudssMatrix_t,
    ) -> i32;
    #[allow(clippy::too_many_arguments)]
    fn cudssMatrixCreateCsr(
        m: *mut cudssMatrix_t,
        nrows: i64,
        ncols: i64,
        nnz: i64,
        row_start: *const c_void,
        row_end: *const c_void,
        col_idx: *const c_void,
        values: *const c_void,
        offset_type: i32,
        index_type: i32,
        value_type: i32,
        mtype: i32,
        mview: i32,
        base: i32,
    ) -> i32;
    fn cudssMatrixCreateDn(
        m: *mut cudssMatrix_t,
        nrows: i64,
        ncols: i64,
        ld: i64,
        values: *const c_void,
        value_type: i32,
        layout: i32,
    ) -> i32;
    fn cudssMatrixDestroy(m: cudssMatrix_t) -> i32;
}

#[allow(non_snake_case)]
extern "C" {
    fn cudaMalloc(ptr: *mut *mut c_void, size: usize) -> i32;
    fn cudaFree(ptr: *mut c_void) -> i32;
    fn cudaMemcpy(dst: *mut c_void, src: *const c_void, count: usize, kind: i32) -> i32;
    fn cudaStreamCreate(stream: *mut cudaStream_t) -> i32;
    fn cudaStreamSynchronize(stream: cudaStream_t) -> i32;
    fn cudaStreamDestroy(stream: cudaStream_t) -> i32;
}

/// cuDSS/CUDA 调用错误。
#[derive(Debug, thiserror::Error)]
pub enum CudssError {
    #[error("CUDA 运行时调用失败: {0} (code {1})")]
    Cuda(&'static str, i32),
    #[error("cuDSS 调用失败: {0} (status {1})")]
    Cudss(&'static str, i32),
}

/// 一次 SPD CSR 求解的耗时拆分（用于 PoC profile）。
#[derive(Debug, Clone, Default)]
pub struct SolveTiming {
    pub h2d: Duration,
    pub analysis: Duration,
    pub factorization: Duration,
    pub solve: Duration,
    pub d2h: Duration,
    pub total: Duration,
}

macro_rules! cuda_try {
    ($call:expr, $name:literal) => {{
        let code = unsafe { $call };
        if code != CUDA_SUCCESS {
            return Err(CudssError::Cuda($name, code));
        }
    }};
}

macro_rules! cudss_try {
    ($call:expr, $name:literal) => {{
        let status = unsafe { $call };
        if status != CUDSS_STATUS_SUCCESS {
            return Err(CudssError::Cudss($name, status));
        }
    }};
}

/// 在 GPU 上用 cuDSS 直接稀疏 Cholesky 求解 SPD 系统 `A x = b`。
///
/// 输入为 **下三角 + 对角** 的 0-based CSR（`mview=LOWER`、`mtype=SPD`）：
/// - `row_offsets`：长度 `n+1`（i32）
/// - `col_indices`：长度 `nnz`（i32，每行按列升序）
/// - `values`：长度 `nnz`（f64，与 col_indices 对应）
/// - `b`：长度 `n`（f64）
///
/// 返回解向量 `x`（长度 `n`）与耗时拆分。`reordering` 指定 fill-reducing 重排序算法
/// （`CUDSS_REORDERING_ALG_DEFAULT` / `_AMD` / `_NESTED_DISSECTION`）。解唯一，与重排序选择无关。
pub fn solve_spd_csr(
    n: usize,
    row_offsets: &[i32],
    col_indices: &[i32],
    values: &[f64],
    b: &[f64],
    reordering: i32,
) -> Result<(Vec<f64>, SolveTiming), CudssError> {
    assert_eq!(row_offsets.len(), n + 1, "row_offsets 长度应为 n+1");
    let nnz = col_indices.len();
    assert_eq!(values.len(), nnz, "values 与 col_indices 长度应一致");
    assert_eq!(b.len(), n, "b 长度应为 n");

    let mut timing = SolveTiming::default();
    let t_all = Instant::now();

    // ── 设备内存分配 ──
    let mut d_offsets: *mut c_void = ptr::null_mut();
    let mut d_cols: *mut c_void = ptr::null_mut();
    let mut d_vals: *mut c_void = ptr::null_mut();
    let mut d_b: *mut c_void = ptr::null_mut();
    let mut d_x: *mut c_void = ptr::null_mut();
    cuda_try!(
        cudaMalloc(&mut d_offsets, (n + 1) * std::mem::size_of::<i32>()),
        "cudaMalloc(offsets)"
    );
    cuda_try!(
        cudaMalloc(&mut d_cols, nnz * std::mem::size_of::<i32>()),
        "cudaMalloc(cols)"
    );
    cuda_try!(
        cudaMalloc(&mut d_vals, nnz * std::mem::size_of::<f64>()),
        "cudaMalloc(vals)"
    );
    cuda_try!(
        cudaMalloc(&mut d_b, n * std::mem::size_of::<f64>()),
        "cudaMalloc(b)"
    );
    cuda_try!(
        cudaMalloc(&mut d_x, n * std::mem::size_of::<f64>()),
        "cudaMalloc(x)"
    );

    // ── H2D 拷贝 ──
    let t_h2d = Instant::now();
    cuda_try!(
        cudaMemcpy(
            d_offsets,
            row_offsets.as_ptr() as *const c_void,
            (n + 1) * std::mem::size_of::<i32>(),
            CUDA_MEMCPY_H2D
        ),
        "cudaMemcpy(offsets H2D)"
    );
    cuda_try!(
        cudaMemcpy(
            d_cols,
            col_indices.as_ptr() as *const c_void,
            nnz * std::mem::size_of::<i32>(),
            CUDA_MEMCPY_H2D
        ),
        "cudaMemcpy(cols H2D)"
    );
    cuda_try!(
        cudaMemcpy(
            d_vals,
            values.as_ptr() as *const c_void,
            nnz * std::mem::size_of::<f64>(),
            CUDA_MEMCPY_H2D
        ),
        "cudaMemcpy(vals H2D)"
    );
    cuda_try!(
        cudaMemcpy(
            d_b,
            b.as_ptr() as *const c_void,
            n * std::mem::size_of::<f64>(),
            CUDA_MEMCPY_H2D
        ),
        "cudaMemcpy(b H2D)"
    );
    timing.h2d = t_h2d.elapsed();

    // ── cuDSS 句柄 / 流 / 配置 / 数据 ──
    let mut handle: cudssHandle_t = ptr::null_mut();
    cudss_try!(cudssCreate(&mut handle), "cudssCreate");
    let mut stream: cudaStream_t = ptr::null_mut();
    cuda_try!(cudaStreamCreate(&mut stream), "cudaStreamCreate");
    cudss_try!(cudssSetStream(handle, stream), "cudssSetStream");

    let mut config: cudssConfig_t = ptr::null_mut();
    cudss_try!(cudssConfigCreate(&mut config), "cudssConfigCreate");
    cudss_try!(
        cudssConfigSet(
            config,
            CUDSS_CONFIG_REORDERING_ALG,
            &reordering as *const i32 as *const c_void,
            std::mem::size_of::<i32>()
        ),
        "cudssConfigSet(REORDERING)"
    );
    // 尝试并行化主机端 analysis（reordering/symbolic）；不支持则忽略。
    let host_threads: i32 = 16;
    unsafe {
        cudssConfigSet(
            config,
            CUDSS_CONFIG_HOST_NTHREADS,
            &host_threads as *const i32 as *const c_void,
            std::mem::size_of::<i32>(),
        );
    }

    let mut data: cudssData_t = ptr::null_mut();
    cudss_try!(cudssDataCreate(handle, &mut data), "cudssDataCreate");

    // ── 矩阵对象 ──
    let mut mat_a: cudssMatrix_t = ptr::null_mut();
    cudss_try!(
        cudssMatrixCreateCsr(
            &mut mat_a,
            n as i64,
            n as i64,
            nnz as i64,
            d_offsets,
            ptr::null(),
            d_cols,
            d_vals,
            CUDSS_R_32I,
            CUDSS_R_32I,
            CUDSS_R_64F,
            CUDSS_MTYPE_SPD,
            CUDSS_MVIEW_LOWER,
            CUDSS_BASE_ZERO
        ),
        "cudssMatrixCreateCsr"
    );
    let mut mat_b: cudssMatrix_t = ptr::null_mut();
    cudss_try!(
        cudssMatrixCreateDn(
            &mut mat_b,
            n as i64,
            1,
            n as i64,
            d_b,
            CUDSS_R_64F,
            CUDSS_LAYOUT_COL_MAJOR
        ),
        "cudssMatrixCreateDn(b)"
    );
    let mut mat_x: cudssMatrix_t = ptr::null_mut();
    cudss_try!(
        cudssMatrixCreateDn(
            &mut mat_x,
            n as i64,
            1,
            n as i64,
            d_x,
            CUDSS_R_64F,
            CUDSS_LAYOUT_COL_MAJOR
        ),
        "cudssMatrixCreateDn(x)"
    );

    // ── 三阶段执行（每阶段后同步以拆分耗时） ──
    let t_an = Instant::now();
    cudss_try!(
        cudssExecute(handle, CUDSS_PHASE_ANALYSIS, config, data, mat_a, mat_x, mat_b),
        "cudssExecute(ANALYSIS)"
    );
    cuda_try!(cudaStreamSynchronize(stream), "sync(analysis)");
    timing.analysis = t_an.elapsed();

    let t_fac = Instant::now();
    cudss_try!(
        cudssExecute(handle, CUDSS_PHASE_FACTORIZATION, config, data, mat_a, mat_x, mat_b),
        "cudssExecute(FACTORIZATION)"
    );
    cuda_try!(cudaStreamSynchronize(stream), "sync(factorization)");
    timing.factorization = t_fac.elapsed();

    let t_sol = Instant::now();
    cudss_try!(
        cudssExecute(handle, CUDSS_PHASE_SOLVE, config, data, mat_a, mat_x, mat_b),
        "cudssExecute(SOLVE)"
    );
    cuda_try!(cudaStreamSynchronize(stream), "sync(solve)");
    timing.solve = t_sol.elapsed();

    // ── D2H 取回解 ──
    let mut x = vec![0.0f64; n];
    let t_d2h = Instant::now();
    cuda_try!(
        cudaMemcpy(
            x.as_mut_ptr() as *mut c_void,
            d_x,
            n * std::mem::size_of::<f64>(),
            CUDA_MEMCPY_D2H
        ),
        "cudaMemcpy(x D2H)"
    );
    timing.d2h = t_d2h.elapsed();

    // ── 释放 ──
    unsafe {
        cudssMatrixDestroy(mat_a);
        cudssMatrixDestroy(mat_b);
        cudssMatrixDestroy(mat_x);
        cudssDataDestroy(handle, data);
        cudssConfigDestroy(config);
        cudssDestroy(handle);
        cudaStreamDestroy(stream);
        cudaFree(d_offsets);
        cudaFree(d_cols);
        cudaFree(d_vals);
        cudaFree(d_b);
        cudaFree(d_x);
    }

    timing.total = t_all.elapsed();
    Ok((x, timing))
}

#[cfg(test)]
mod tests {
    use super::*;
    use faer::dyn_stack::{MemBuffer, MemStack};
    use faer::linalg::cholesky::llt::factor::LltRegularization;
    use faer::reborrow::*;
    use faer::sparse::linalg::cholesky::{
        factorize_symbolic_cholesky, CholeskySymbolicParams, SymmetricOrdering,
    };
    use faer::sparse::{SparseColMat, Triplet};
    use faer::{Conj, Mat, Par, Side};

    /// 构造 m×m 网格 5 点 Dirichlet Laplacian（SPD），返回下三角 CSR + faer 下三角三元组。
    ///
    /// 节点行主序 idx = r*m + c；**对角恒为 4**（域外邻居视作 Dirichlet 边界，贡献对角但无非对角），
    /// 下三角非对角 = 上/左域内邻居各 -1。边界节点因缺失域内邻居而严格对角占优 → SPD。
    /// 这与 hydro `solve_laplace_dirichlet` 组装的 SPD 系统同类（Dirichlet 邻居并入对角）。
    fn build_grid_laplacian(
        m: usize,
    ) -> (usize, Vec<i32>, Vec<i32>, Vec<f64>, Vec<Triplet<usize, usize, f64>>) {
        let n = m * m;
        let mut row_offsets = Vec::with_capacity(n + 1);
        let mut col_indices: Vec<i32> = Vec::new();
        let mut values: Vec<f64> = Vec::new();
        let mut triplets: Vec<Triplet<usize, usize, f64>> = Vec::new();
        row_offsets.push(0i32);
        for r in 0..m {
            for c in 0..m {
                let idx = r * m + c;
                // 对角恒为 4：域外的上下左右邻居均为 Dirichlet 边界（并入对角，SPD 关键）。
                let deg = 4.0f64;
                // 下三角非对角：上邻居(idx-m) 与 左邻居(idx-1)，列升序
                if r > 0 {
                    let j = idx - m;
                    col_indices.push(j as i32);
                    values.push(-1.0);
                    triplets.push(Triplet::new(idx, j, -1.0));
                }
                if c > 0 {
                    let j = idx - 1;
                    col_indices.push(j as i32);
                    values.push(-1.0);
                    triplets.push(Triplet::new(idx, j, -1.0));
                }
                // 对角
                col_indices.push(idx as i32);
                values.push(deg);
                triplets.push(Triplet::new(idx, idx, deg));
                row_offsets.push(col_indices.len() as i32);
            }
        }
        (n, row_offsets, col_indices, values, triplets)
    }

    /// CPU 参考：faer AMD 稀疏 Cholesky 解同一 SPD 系统（复刻 hydro `solve_spd_faer`）。
    fn solve_faer(n: usize, triplets: &[Triplet<usize, usize, f64>], b: &[f64]) -> Vec<f64> {
        let a = SparseColMat::<usize, f64>::try_new_from_triplets(n, n, triplets).unwrap();
        let symbolic = factorize_symbolic_cholesky(
            a.symbolic(),
            Side::Lower,
            SymmetricOrdering::Amd,
            CholeskySymbolicParams::default(),
        )
        .unwrap();
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
            .unwrap();
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

    /// Tier 1 PoC 证据：cuDSS GPU 直接稀疏解与 CPU faer 在同一 SPD 网格 Laplacian 上 parity。
    #[test]
    fn cudss_matches_faer_grid_laplacian() {
        let m = 60; // n = 3600 内部变量，代表中等河流窗口
        let (n, row_offsets, col_indices, values, triplets) = build_grid_laplacian(m);
        // 确定性 RHS
        let b: Vec<f64> = (0..n).map(|i| ((i * 7 + 3) % 13) as f64 - 6.0).collect();

        let cpu = solve_faer(n, &triplets, &b);
        let (gpu, timing) = solve_spd_csr(n, &row_offsets, &col_indices, &values, &b, CUDSS_REORDERING_ALG_AMD)
            .expect("cuDSS 求解失败");

        let max_abs = cpu
            .iter()
            .zip(&gpu)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f64, f64::max);
        let denom = cpu.iter().map(|v| v.abs()).fold(1e-30, f64::max);
        let max_rel = max_abs / denom;

        eprintln!(
            "[Tier1 PoC] n={n} nnz={} | max_abs={max_abs:.3e} max_rel={max_rel:.3e}",
            col_indices.len()
        );
        eprintln!(
            "[Tier1 PoC] 耗时: H2D={:?} 分析={:?} 分解={:?} 求解={:?} D2H={:?} 总={:?}",
            timing.h2d, timing.analysis, timing.factorization, timing.solve, timing.d2h, timing.total
        );

        assert!(
            max_abs < 1e-6,
            "cuDSS 与 faer 不一致: max_abs={max_abs:.3e} (判据 <1e-6)"
        );
    }

    /// GPU/CPU 交叉阈值 profile：不同规模网格 Laplacian 上 cuDSS(warm) vs faer 耗时。
    ///
    /// 默认忽略（较久）；显式运行：
    /// `cargo test -p water-gpu --release --features cudss profile_cudss_vs_faer -- --ignored --nocapture`
    ///
    /// cuDSS 计时取 分析+分解+求解（compute，排除 H2D/D2H）；先 warm 一次排除 CUDA/cuDSS 一次性 init。
    #[test]
    #[ignore]
    fn profile_cudss_vs_faer_scaling() {
        // 预热：初始化 CUDA/cuDSS 上下文，排除一次性 init 开销。
        {
            let (n, ro, ci, v, _) = build_grid_laplacian(16);
            let b = vec![1.0f64; n];
            let _ = solve_spd_csr(n, &ro, &ci, &v, &b, CUDSS_REORDERING_ALG_DEFAULT).expect("warmup");
        }
        for (tag, reorder) in [
            ("DEFAULT", CUDSS_REORDERING_ALG_DEFAULT),
            ("AMD", CUDSS_REORDERING_ALG_AMD),
        ] {
            eprintln!("\n== cuDSS 重排序 = {tag} vs faer(AMD) ==");
            eprintln!(
                "{:>9} {:>11} {:>11} {:>10} {:>10} {:>10} {:>11} {:>9}",
                "n", "nnz", "faer_ms", "an_ms", "fac_ms", "sol_ms", "gpu_ms", "speedup"
            );
            for &m in &[30usize, 70, 120, 200, 320, 500, 720, 1000] {
                let (n, ro, ci, v, tri) = build_grid_laplacian(m);
                let b: Vec<f64> = (0..n).map(|i| ((i * 7 + 3) % 13) as f64 - 6.0).collect();

                // CPU faer
                let t = Instant::now();
                let cpu = solve_faer(n, &tri, &b);
                let faer_ms = t.elapsed().as_secs_f64() * 1e3;

                // GPU cuDSS（warm）
                let (gpu, tim) =
                    solve_spd_csr(n, &ro, &ci, &v, &b, reorder).expect("cudss 求解失败");
                let an = tim.analysis.as_secs_f64() * 1e3;
                let fac = tim.factorization.as_secs_f64() * 1e3;
                let sol = tim.solve.as_secs_f64() * 1e3;
                let gpu_ms = an + fac + sol;

                let max_abs = cpu
                    .iter()
                    .zip(&gpu)
                    .map(|(a, b)| (a - b).abs())
                    .fold(0.0f64, f64::max);
                assert!(max_abs < 1e-6, "n={n} parity 失败 max_abs={max_abs:.3e}");

                eprintln!(
                    "{:>9} {:>11} {:>11.2} {:>10.2} {:>10.2} {:>10.2} {:>11.2} {:>8.2}x",
                    n,
                    ci.len(),
                    faer_ms,
                    an,
                    fac,
                    sol,
                    gpu_ms,
                    faer_ms / gpu_ms
                );
            }
        }
    }
}
