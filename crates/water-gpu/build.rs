//! 编译期把 CUDA 计算核（`cuda/*.cu`）经 `nvcc` 编译为 PTX，产物落在 `OUT_DIR`，
//! 由 `src/kernels.rs` 用 `include_str!` 内嵌。
//!
//! 目标架构默认 `sm_120`（Blackwell，RTX 5070），可用环境变量 `WATER_GPU_ARCH` 覆盖
//! （如 `sm_90` / `sm_86`）。`nvcc` 路径可用 `NVCC` 覆盖，默认取 PATH 上的 `nvcc`。

use std::path::PathBuf;
use std::process::Command;

/// 需要编译的核清单：(源文件, 产物名)。
const KERNELS: &[(&str, &str)] = &[("cuda/vector_add.cu", "vector_add.ptx")];

fn main() {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR 未设置"));
    let nvcc = std::env::var("NVCC").unwrap_or_else(|_| "nvcc".to_string());
    let arch = std::env::var("WATER_GPU_ARCH").unwrap_or_else(|_| "sm_120".to_string());

    for (src, ptx_name) in KERNELS {
        println!("cargo:rerun-if-changed={src}");
        let ptx_path = out_dir.join(ptx_name);
        let status = Command::new(&nvcc)
            .args(["--ptx", &format!("-arch={arch}"), src, "-o"])
            .arg(&ptx_path)
            .status()
            .unwrap_or_else(|e| {
                panic!(
                    "调用 nvcc 失败（{e}）。请确认已安装 CUDA Toolkit 且 `{nvcc}` 在 PATH 上，\
                     或用环境变量 NVCC 指定其绝对路径。"
                )
            });
        assert!(status.success(), "nvcc 编译 {src} 失败（arch={arch}）");
    }

    println!("cargo:rerun-if-env-changed=NVCC");
    println!("cargo:rerun-if-env-changed=WATER_GPU_ARCH");
}
