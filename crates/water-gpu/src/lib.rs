//! `water-gpu` — GPU 桥接层（Rust 编排 → CUDA C++ 计算核）。
//!
//! 架构（Water2GPU CUDA 规范）：每个计算模块是一份 `cuda/xxx.cu`，含**主机端 launcher**
//! （`extern "C"`，内部完成 显存检查 → 分配 → H2D → kernel → D2H，全程 `CUDA_CHECK` 校验，
//! 并用 `cudaEvent` 分段计时 H2D/kernel/D2H）；同名 `cuda/xxx.cuh` 声明其接口。
//! `build.rs` 用 `nvcc` 编成静态库链接，Rust 经 FFI 调用 launcher。
//! 编程规范对标 NVIDIA `cuda-samples`（`helper_cuda.h` 的 `checkCudaErrors` 与 event 计时）。
//!
//! 本文件提供：
//! - [`KernelTiming`]：与 CUDA `WaterKernelTiming` 对齐的分段耗时（ms）。
//! - [`vector_add`]：最小端到端示例/自检——调用 `water_vector_add`，返回结果 + 分段耗时。
//!
//! 后续加入 Laplace 稀疏求解（见 [`cudss`]）、warp 重投影、形态学/高斯/EDT 等业务核，
//! 均与 `water-core` / CPU 实现逐一数值对拍（容差见 AGENTS.md / docs/CUDA.md）。

/// cuDSS GPU 直接稀疏 Cholesky 求解（Tier 1 Laplace PoC，需启用 `cudss` 特性）。
#[cfg(feature = "cudss")]
pub mod cudss;

/// 每个计算模块的分段耗时（毫秒），与 CUDA `common.cuh` 的 `WaterKernelTiming` 内存对齐。
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct KernelTiming {
    /// 主机 → 设备拷贝耗时。
    pub h2d_ms: f64,
    /// 核执行耗时。
    pub kernel_ms: f64,
    /// 设备 → 主机拷贝耗时。
    pub d2h_ms: f64,
}

extern "C" {
    /// CUDA 侧 `water_vector_add`（cuda/vector_add.cu），返回 cudaError_t（0=成功）。
    fn water_vector_add(
        h_a: *const f32,
        h_b: *const f32,
        h_out: *mut f32,
        n: i64,
        timing: *mut KernelTiming,
    ) -> i32;

    /// CUDA 侧 `water_laplace_pcg`（cuda/laplace_pcg.cu）：无矩阵 FP64 Jacobi-PCG。
    #[allow(clippy::too_many_arguments)]
    fn water_laplace_pcg(
        h_deg: *const f64,
        h_b: *const f64,
        h_z: *mut f64,
        height: i32,
        width: i32,
        rtol: f64,
        max_iter: i32,
        out_iters: *mut i32,
        out_res: *mut f64,
        timing: *mut KernelTiming,
    ) -> i32;
}

/// PCG 求解结果：解向量 + 实际迭代数 + 最终相对残差 + 分段耗时。
#[derive(Debug, Clone)]
pub struct PcgResult {
    /// 解 `z`（行主序 `height*width`）。
    pub z: Vec<f64>,
    /// 实际迭代次数。
    pub iters: i32,
    /// 最终相对残差 `||r||/||b||`。
    pub residual: f64,
    /// 分段耗时（H2D / 求解循环 / D2H）。
    pub timing: KernelTiming,
}

/// GPU 桥接错误。
#[derive(Debug, thiserror::Error)]
pub enum GpuError {
    /// CUDA 运行时/核调用失败，携带 `cudaError_t` 码。
    #[error("CUDA 调用失败 (cudaError {0})")]
    Cuda(i32),
    /// 输入不满足核的前置约束（如长度不一致）。
    #[error("输入无效: {0}")]
    InvalidInput(String),
}

/// 结果别名。
pub type Result<T> = std::result::Result<T, GpuError>;

/// 在 GPU 上逐元素相加 `a + b`，返回结果向量与分段耗时（H2D/kernel/D2H, ms）。
///
/// 主要用于全链路自检；两向量长度必须一致。
pub fn vector_add(a: &[f32], b: &[f32]) -> Result<(Vec<f32>, KernelTiming)> {
    if a.len() != b.len() {
        return Err(GpuError::InvalidInput(format!(
            "向量长度不一致: a={}, b={}",
            a.len(),
            b.len()
        )));
    }
    let n = a.len();
    let mut out = vec![0.0f32; n];
    let mut timing = KernelTiming::default();
    if n == 0 {
        return Ok((out, timing));
    }
    let code = unsafe {
        water_vector_add(
            a.as_ptr(),
            b.as_ptr(),
            out.as_mut_ptr(),
            n as i64,
            &mut timing,
        )
    };
    if code != 0 {
        return Err(GpuError::Cuda(code));
    }
    Ok((out, timing))
}

/// 无矩阵 FP64 Jacobi-PCG 求解 5 点 Dirichlet Laplacian `M z = b`（GPU）。
///
/// `deg`/`b` 为 `height*width` 行主序全网格数组：`deg` 是度/对角场（非内部像素置 0，兼作掩膜），
/// `b` 是 RHS（非内部置 0）。迭代到相对残差 `< rtol` 或达 `max_iter`。
/// 见 [cuda/laplace_pcg.cu](../cuda/laplace_pcg.cu)。
pub fn laplace_pcg(
    deg: &[f64],
    b: &[f64],
    height: usize,
    width: usize,
    rtol: f64,
    max_iter: i32,
) -> Result<PcgResult> {
    let n = height * width;
    if deg.len() != n || b.len() != n {
        return Err(GpuError::InvalidInput(format!(
            "deg/b 长度应为 height*width={n}，实为 deg={}, b={}",
            deg.len(),
            b.len()
        )));
    }
    let mut z = vec![0.0f64; n];
    let mut iters: i32 = 0;
    let mut residual: f64 = 0.0;
    let mut timing = KernelTiming::default();
    if n == 0 {
        return Ok(PcgResult {
            z,
            iters,
            residual,
            timing,
        });
    }
    let code = unsafe {
        water_laplace_pcg(
            deg.as_ptr(),
            b.as_ptr(),
            z.as_mut_ptr(),
            height as i32,
            width as i32,
            rtol,
            max_iter,
            &mut iters,
            &mut residual,
            &mut timing,
        )
    };
    if code != 0 {
        return Err(GpuError::Cuda(code));
    }
    Ok(PcgResult {
        z,
        iters,
        residual,
        timing,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 端到端证据：GPU 向量加与 CPU 参考逐元素一致（bit-exact，纯加法无舍入差）。
    #[test]
    fn vector_add_matches_cpu() {
        let n = 1_000_003usize; // 非 32 整除，验证边界判定 `if (i < n)`
        let a: Vec<f32> = (0..n).map(|i| i as f32 * 0.5).collect();
        let b: Vec<f32> = (0..n).map(|i| (i as f32).sin()).collect();

        let (gpu, timing) = vector_add(&a, &b).expect("GPU 向量加失败");
        let cpu: Vec<f32> = a.iter().zip(&b).map(|(x, y)| x + y).collect();

        eprintln!(
            "[vector_add] n={n} 分段耗时: H2D={:.3}ms kernel={:.3}ms D2H={:.3}ms",
            timing.h2d_ms, timing.kernel_ms, timing.d2h_ms
        );

        assert_eq!(gpu.len(), cpu.len());
        for (i, (g, c)) in gpu.iter().zip(&cpu).enumerate() {
            assert_eq!(g, c, "第 {i} 个元素不一致: gpu={g} cpu={c}");
        }
    }

    // ── 无矩阵 PCG 对拍 / profile（vs faer 直接解） ──
    use faer::dyn_stack::{MemBuffer, MemStack};
    use faer::linalg::cholesky::llt::factor::LltRegularization;
    use faer::reborrow::*;
    use faer::sparse::linalg::cholesky::{
        factorize_symbolic_cholesky, CholeskySymbolicParams, SymmetricOrdering,
    };
    use faer::sparse::{SparseColMat, Triplet};
    use faer::{Conj, Mat, Par, Side};
    use std::time::Instant;

    /// m×m 网格 5 点 Poisson-Dirichlet：返回 (deg 全 4, RHS b, faer 下三角三元组)。
    fn build_grid_poisson(m: usize) -> (Vec<f64>, Vec<f64>, Vec<Triplet<usize, usize, f64>>) {
        let n = m * m;
        let deg = vec![4.0f64; n];
        let b: Vec<f64> = (0..n).map(|i| ((i * 7 + 3) % 13) as f64 - 6.0).collect();
        let mut tri = Vec::new();
        for r in 0..m {
            for c in 0..m {
                let idx = r * m + c;
                if r > 0 {
                    tri.push(Triplet::new(idx, idx - m, -1.0));
                }
                if c > 0 {
                    tri.push(Triplet::new(idx, idx - 1, -1.0));
                }
                tri.push(Triplet::new(idx, idx, 4.0));
            }
        }
        (deg, b, tri)
    }

    /// CPU 参考：faer AMD 稀疏 Cholesky 直接解（与 hydro `solve_spd_faer` 同一实现）。
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

    /// PCG 数值对拍：收敛到紧容差后与 faer 直接解一致（<1e-6）。
    #[test]
    fn laplace_pcg_matches_faer() {
        let m = 48;
        let n = m * m;
        let (deg, b, tri) = build_grid_poisson(m);
        let cpu = solve_faer(n, &tri, &b);
        let res = laplace_pcg(&deg, &b, m, m, 1e-12, 50_000).expect("PCG 失败");
        let max_abs = cpu
            .iter()
            .zip(&res.z)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f64, f64::max);
        eprintln!(
            "[PCG parity] n={n} iters={} rel_res={:.2e} max_abs(vs faer)={max_abs:.3e} \
             (H2D={:.3} solve={:.3} D2H={:.3} ms)",
            res.iters, res.residual, res.timing.h2d_ms, res.timing.kernel_ms, res.timing.d2h_ms
        );
        assert!(max_abs < 1e-6, "PCG 与 faer 不一致: max_abs={max_abs:.3e}");
    }

    /// GPU PCG vs CPU faer profile（规模扫描）。默认忽略；显式运行：
    /// `cargo test -p water-gpu --release profile_pcg_vs_faer -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn profile_pcg_vs_faer_scaling() {
        // 预热 GPU。
        let (deg, b, _) = build_grid_poisson(16);
        let _ = laplace_pcg(&deg, &b, 16, 16, 1e-8, 1000).expect("warmup");

        eprintln!(
            "{:>9} {:>11} {:>9} {:>8} {:>11} {:>11} {:>9} {:>11}",
            "n", "faer_ms", "pcg_ms", "iters", "rel_res", "max_abs", "speedup", "pcg_total"
        );
        for &m in &[30usize, 70, 120, 200, 320, 500, 720, 1000] {
            let n = m * m;
            let (deg, b, tri) = build_grid_poisson(m);

            let t = Instant::now();
            let cpu = solve_faer(n, &tri, &b);
            let faer_ms = t.elapsed().as_secs_f64() * 1e3;

            let res = laplace_pcg(&deg, &b, m, m, 1e-10, 100_000).expect("PCG 失败");
            let pcg_ms = res.timing.kernel_ms;
            let pcg_total = res.timing.h2d_ms + res.timing.kernel_ms + res.timing.d2h_ms;
            let max_abs = cpu
                .iter()
                .zip(&res.z)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0f64, f64::max);

            eprintln!(
                "{:>9} {:>11.2} {:>9.2} {:>8} {:>11.2e} {:>11.3e} {:>8.2}x {:>10.2}",
                n,
                faer_ms,
                pcg_ms,
                res.iters,
                res.residual,
                max_abs,
                faer_ms / pcg_ms,
                pcg_total
            );
        }
    }
}
