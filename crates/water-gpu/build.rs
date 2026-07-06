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
    "cuda/nvtx_util.cu",
    "cuda/warp.cu",
];

fn main() {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR 未设置"));
    // 目标 OS（构建脚本用 CARGO_CFG_TARGET_OS，而非 host cfg!）。
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let windows = target_os == "windows";
    let nvcc = std::env::var("NVCC").unwrap_or_else(|_| "nvcc".to_string());
    let arch = std::env::var("WATER_GPU_ARCH").unwrap_or_else(|_| "sm_120".to_string());
    // CUDA 库目录：Linux 默认 /usr/local/cuda/lib64；Windows 默认 %CUDA_PATH%\lib\x64。
    let cuda_dir = std::env::var("CUDA_LIB_DIR").unwrap_or_else(|_| {
        if windows {
            let root = std::env::var("CUDA_PATH").expect(
                "Windows 下需 CUDA_PATH（CUDA Toolkit 安装时自动设置），或用 CUDA_LIB_DIR 指定库目录",
            );
            format!("{root}\\lib\\x64")
        } else {
            "/usr/local/cuda/lib64".to_string()
        }
    });

    // 通用头变更时重编。
    println!("cargo:rerun-if-changed=cuda/common.cuh");

    // ── 逐 .cu 编译为对象文件（含主机端 launcher + 设备核） ──
    let obj_ext = if windows { "obj" } else { "o" };
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
        let obj = out_dir.join(format!("{stem}.{obj_ext}"));
        let mut cmd = Command::new(&nvcc);
        cmd.args(["-c", "-O3", &format!("-arch={arch}"), "-Icuda"]);
        // Linux：位置无关代码 -fPIC；Windows：nvcc 用 cl.exe，无需 -fPIC。
        if !windows {
            cmd.args(["--compiler-options", "-fPIC"]);
        }
        cmd.arg(src).arg("-o").arg(&obj);
        let status = cmd.status()
            .unwrap_or_else(|e| {
                panic!(
                    "调用 nvcc 失败（{e}）。请确认已安装 CUDA Toolkit 且 `{nvcc}` 在 PATH 上，\
                     或用环境变量 NVCC 指定其绝对路径。"
                )
            });
        assert!(status.success(), "nvcc 编译 {src} 失败（arch={arch}）");
        objects.push(obj);
    }

    // ── 归档为静态库（Linux: ar→.a；Windows: lib.exe→.lib） ──
    let lib = if windows {
        out_dir.join("water_cuda_kernels.lib")
    } else {
        out_dir.join("libwater_cuda_kernels.a")
    };
    let _ = std::fs::remove_file(&lib);
    if windows {
        // MSVC 库工具 lib.exe（需在 Visual Studio 开发者环境/PATH 中；
        // 在 “x64 Native Tools Command Prompt for VS” 里构建或已 vcvars64 激活）。
        let mut libcmd = Command::new("lib.exe");
        libcmd.arg("/nologo").arg(format!("/OUT:{}", lib.display()));
        for obj in &objects {
            libcmd.arg(obj);
        }
        let status = libcmd
            .status()
            .expect("调用 lib.exe 失败（需在 Visual Studio x64 开发者命令行/环境中构建）");
        assert!(status.success(), "lib.exe 归档 CUDA 对象失败");
    } else {
        let mut ar = Command::new("ar");
        ar.arg("crs").arg(&lib);
        for obj in &objects {
            ar.arg(obj);
        }
        let status = ar.status().expect("调用 ar 失败");
        assert!(status.success(), "ar 归档 CUDA 对象失败");
    }

    // ── 链接：静态核库 + CUDA 运行时（+ Linux C++ 运行时/rpath） ──
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-search=native={cuda_dir}");
    println!("cargo:rustc-link-lib=static=water_cuda_kernels");
    println!("cargo:rustc-link-lib=dylib=cudart");
    if !windows {
        // Linux：nvcc 主机代码需 libstdc++；以 rpath 定位 CUDA 运行时。
        // Windows：MSVC 自动链 C++ 运行时；cudart.dll 由 PATH/CUDA bin 定位（无 rpath）。
        println!("cargo:rustc-link-lib=dylib=stdc++");
        println!("cargo:rustc-link-arg=-Wl,-rpath,{cuda_dir}");
    }

    println!("cargo:rerun-if-env-changed=NVCC");
    println!("cargo:rerun-if-env-changed=WATER_GPU_ARCH");
    println!("cargo:rerun-if-env-changed=CUDA_LIB_DIR");
    println!("cargo:rerun-if-env-changed=CUDA_PATH");

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
    // 链接需要非版本化的 libcudss.so；wheel 只带 libcudss.so.0，补一个软链（仅 Unix）。
    #[cfg(unix)]
    {
        let unversioned = cudss_dir.join("libcudss.so");
        let versioned = cudss_dir.join("libcudss.so.0");
        if !unversioned.exists() && versioned.exists() {
            let _ = std::os::unix::fs::symlink(&versioned, &unversioned);
        }
    }
    println!("cargo:rustc-link-search=native={}", cudss_dir.display());
    println!("cargo:rustc-link-lib=dylib=cudss");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", cudss_dir.display());
    println!("cargo:rerun-if-env-changed=CUDSS_LIB_DIR");
}

