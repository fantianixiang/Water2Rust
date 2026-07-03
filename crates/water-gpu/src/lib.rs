//! `water-gpu` — GPU 桥接层（Rust 编排 → CUDA 计算核）。
//!
//! 架构：业务 crate 经本 crate 调度 GPU 计算核。计算核为 `.cu`，编译期由 [`build.rs`]
//! 经 `nvcc` 编译成 PTX，运行期由 [`cudarc`] 加载进 CUDA 上下文并启动。
//!
//! 本文件当前提供：
//! - [`GpuContext`]：封装设备上下文 + 默认流 + 已加载模块，供上层复用（避免重复初始化）。
//! - [`vector_add`]：最小端到端示例/自检——把两个向量搬到 GPU 相加再取回，用于验证
//!   `nvcc→PTX→cudarc→GPU→回传` 全链路在本机（Blackwell sm_120 + CUDA 13.3）可用。
//!
//! 后续将在此基础上加入 Laplace 稀疏求解、warp 重投影等业务核，并与 `water-core` / CPU
//! 实现逐一数值对拍（容差 < 1e-6，见 AGENTS.md）。

use std::sync::Arc;

use cudarc::driver::{CudaContext, CudaModule, CudaStream, LaunchConfig, PushKernelArg};
use cudarc::nvrtc::Ptx;

/// 内嵌的向量加 PTX（由 build.rs 经 nvcc 生成到 OUT_DIR）。
const VECTOR_ADD_PTX: &str = include_str!(concat!(env!("OUT_DIR"), "/vector_add.ptx"));

/// GPU 桥接错误。
#[derive(Debug, thiserror::Error)]
pub enum GpuError {
    /// cudarc 驱动/加载/启动层错误。
    #[error("CUDA 驱动错误: {0}")]
    Driver(#[from] cudarc::driver::DriverError),
    /// 输入不满足核的前置约束（如长度不一致）。
    #[error("输入无效: {0}")]
    InvalidInput(String),
}

/// 结果别名。
pub type Result<T> = std::result::Result<T, GpuError>;

/// GPU 设备上下文：持有 CUDA 上下文、默认流与已加载的 PTX 模块。
///
/// 一次初始化、多次复用；`Arc` 使其可安全跨线程共享（配合 rayon 多流并发）。
pub struct GpuContext {
    ctx: Arc<CudaContext>,
    stream: Arc<CudaStream>,
    vector_add_module: Arc<CudaModule>,
}

impl GpuContext {
    /// 在给定设备序号（通常 0）上创建上下文，并加载内置计算核。
    pub fn new(ordinal: usize) -> Result<Self> {
        let ctx = CudaContext::new(ordinal)?;
        let stream = ctx.default_stream();
        let vector_add_module = ctx.load_module(Ptx::from_src(VECTOR_ADD_PTX))?;
        Ok(Self {
            ctx,
            stream,
            vector_add_module,
        })
    }

    /// 设备名称（用于日志/诊断）。
    pub fn device_name(&self) -> Result<String> {
        Ok(self.ctx.name()?)
    }

    /// 在 GPU 上逐元素相加 `a + b`，返回结果向量。
    ///
    /// 主要用于全链路自检；两向量长度必须一致。
    pub fn vector_add(&self, a: &[f32], b: &[f32]) -> Result<Vec<f32>> {
        if a.len() != b.len() {
            return Err(GpuError::InvalidInput(format!(
                "向量长度不一致: a={}, b={}",
                a.len(),
                b.len()
            )));
        }
        let n = a.len();
        if n == 0 {
            return Ok(Vec::new());
        }

        let d_a = self.stream.clone_htod(a)?;
        let d_b = self.stream.clone_htod(b)?;
        let mut d_out = self.stream.alloc_zeros::<f32>(n)?;

        let func = self.vector_add_module.load_function("vector_add")?;
        let cfg = LaunchConfig::for_num_elems(n as u32);
        let n_arg = n;
        let mut builder = self.stream.launch_builder(&func);
        builder.arg(&d_a).arg(&d_b).arg(&mut d_out).arg(&n_arg);
        unsafe { builder.launch(cfg)? };

        Ok(self.stream.clone_dtoh(&d_out)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 端到端证据：GPU 向量加与 CPU 参考逐元素一致（bit-exact，纯加法无舍入差）。
    #[test]
    fn vector_add_matches_cpu() {
        let ctx = GpuContext::new(0).expect("创建 GPU 上下文失败");
        let name = ctx.device_name().expect("取设备名失败");
        eprintln!("GPU 设备: {name}");

        let n = 1_000_003usize; // 非 32 整除，验证边界判定 `if (i < n)`
        let a: Vec<f32> = (0..n).map(|i| i as f32 * 0.5).collect();
        let b: Vec<f32> = (0..n).map(|i| (i as f32).sin()).collect();

        let gpu = ctx.vector_add(&a, &b).expect("GPU 向量加失败");
        let cpu: Vec<f32> = a.iter().zip(&b).map(|(x, y)| x + y).collect();

        assert_eq!(gpu.len(), cpu.len());
        for (i, (g, c)) in gpu.iter().zip(&cpu).enumerate() {
            assert_eq!(g, c, "第 {i} 个元素不一致: gpu={g} cpu={c}");
        }
    }
}
