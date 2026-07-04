//! 编译期把 CUDA 计算模块（`cuda/*.cu`，含主机端 launcher）用 `nvcc` 编译为对象文件，
//! 归档为静态库 `libwater_cuda_kernels.a` 并链接进 `water-gpu`；主机端 launcher 经
//! Rust FFI 调用（Water2GPU CUDA 规范：NVIDIA 风格 .cu + .cuh + FFI）。
//!
//! 目标架构默认 `sm_120`（Blackwell，RTX 5070），可用环境变量 `WATER_GPU_ARCH` 覆盖。
//! `nvcc` 路径可用 `NVCC` 覆盖，默认取 PATH 上的 `nvcc`；CUDA 库目录 `CUDA_LIB_DIR`
//! 默认 `/usr/local/cuda/lib64`。

use std::path::PathBuf;
use std::process::Command;

/// 需要编译的计算模块 .cu 清单（新增核在此登记）。
const CU_SOURCES: &[&str] = &[
    "cuda/vector_add.cu",
    "cuda/laplace_pcg.cu",
    "cuda/laplace_mg.cu",
];

fn main() {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR 未设置"));
    let nvcc = std::env::var("NVCC").unwrap_or_else(|_| "nvcc".to_string());
    let arch = std::env::var("WATER_GPU_ARCH").unwrap_or_else(|_| "sm_120".to_string());
    let cuda_dir =
        std::env::var("CUDA_LIB_DIR").unwrap_or_else(|_| "/usr/local/cuda/lib64".to_string());

    // 通用头变更时重编。
    println!("cargo:rerun-if-changed=cuda/common.cuh");

    // ── 逐 .cu 编译为对象文件（含主机端 launcher + 设备核） ──
    let mut objects = Vec::new();
    for src in CU_SOURCES {
        println!("cargo:rerun-if-changed={src}");
        // 同名 .cuh 也纳入重编触发。
        let cuh = src.replace(".cu", ".cuh");
        println!("cargo:rerun-if-changed={cuh}");

        let stem = PathBuf::from(src)
            .file_stem()
            .expect("非法 .cu 路径")
            .to_string_lossy()
            .into_owned();
        let obj = out_dir.join(format!("{stem}.o"));
        let status = Command::new(&nvcc)
            .args([
                "-c",
                "-O3",
                &format!("-arch={arch}"),
                "--compiler-options",
                "-fPIC",
                "-Icuda",
                src,
                "-o",
            ])
            .arg(&obj)
            .status()
            .unwrap_or_else(|e| {
                panic!(
                    "调用 nvcc 失败（{e}）。请确认已安装 CUDA Toolkit 且 `{nvcc}` 在 PATH 上，\
                     或用环境变量 NVCC 指定其绝对路径。"
                )
            });
        assert!(status.success(), "nvcc 编译 {src} 失败（arch={arch}）");
        objects.push(obj);
    }

    // ── 归档为静态库 ──
    let lib = out_dir.join("libwater_cuda_kernels.a");
    let _ = std::fs::remove_file(&lib);
    let mut ar = Command::new("ar");
    ar.arg("crs").arg(&lib);
    for obj in &objects {
        ar.arg(obj);
    }
    let status = ar.status().expect("调用 ar 失败");
    assert!(status.success(), "ar 归档 CUDA 对象失败");

    // ── 链接：静态核库 + CUDA 运行时 + C++ 运行时 ──
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-search=native={cuda_dir}");
    println!("cargo:rustc-link-lib=static=water_cuda_kernels");
    println!("cargo:rustc-link-lib=dylib=cudart");
    println!("cargo:rustc-link-lib=dylib=stdc++");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{cuda_dir}");

    println!("cargo:rerun-if-env-changed=NVCC");
    println!("cargo:rerun-if-env-changed=WATER_GPU_ARCH");
    println!("cargo:rerun-if-env-changed=CUDA_LIB_DIR");

    // 仅在启用 `cudss` 特性时额外链接 cuDSS（Tier 1 Laplace GPU 直接稀疏解）。
    if std::env::var("CARGO_FEATURE_CUDSS").is_ok() {
        link_cudss();
    }
}

/// 配置 cuDSS 的链接与 rpath（cudart 已由主流程链接）。
///
/// cuDSS 库目录默认取项目内 `third_party/cudss/nvidia/cu13/lib`（pip wheel 安装位置），
/// 可用 `CUDSS_LIB_DIR` 覆盖。
fn link_cudss() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let default_cudss = manifest.join("../../third_party/cudss/nvidia/cu13/lib");
    let cudss_dir = std::env::var("CUDSS_LIB_DIR")
        .map(PathBuf::from)
        .unwrap_or(default_cudss);
    let cudss_dir = cudss_dir.canonicalize().unwrap_or(cudss_dir);
    // 链接需要非版本化的 libcudss.so；wheel 只带 libcudss.so.0，补一个软链。
    let unversioned = cudss_dir.join("libcudss.so");
    let versioned = cudss_dir.join("libcudss.so.0");
    if !unversioned.exists() && versioned.exists() {
        let _ = std::os::unix::fs::symlink(&versioned, &unversioned);
    }
    println!("cargo:rustc-link-search=native={}", cudss_dir.display());
    println!("cargo:rustc-link-lib=dylib=cudss");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", cudss_dir.display());
    println!("cargo:rerun-if-env-changed=CUDSS_LIB_DIR");
}

