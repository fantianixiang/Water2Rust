# CUDA — GPU 加速层（Water2GPU / water-gpu）

本文件是 **Water2GPU** 的 GPU 加速权威规格。Water2GPU 是水体 GIS 项目的**第三代**：

```
MyProject（Python 基线）→ Water2Rust（纯 Rust CPU，rust/shadcn）→ 【本项目】GPU/Project（CUDA+Rust 加速）
```

本分支 `GPU/Project` **继承自 `rust/shadcn`**（完整纯 Rust 程序：CLI + REST API + egui GUI + Tauri 桌面），
在其之上叠加 GPU 加速层，逐模块把 CPU 数值核搬到 GPU。

## 架构：Rust 编排 + CUDA 计算核

- **Rust 负责**：IO / 几何 / CRS / 瓦片编排 / 对拍 / 错误处理（全部继承自 rust/shadcn，不变）。
- **GPU 只做计算核**：热点数值算法用 CUDA C++（`.cu`）实现，`nvcc` 编译为 PTX，`cudarc`（纯 Rust CUDA 绑定）
  运行时加载并启动。
- **对「纯 Rust」的唯一必要例外**：GPU 计算核为 CUDA C++。这是 GPU 加速的本质要求；除此之外全链路仍为纯 Rust。
  `cudarc` 用 `dynamic-loading`，**构建期不需要 CUDA 库**，运行期加载 NVIDIA 驱动（libcuda）。

## `water-gpu` crate

GPU 桥接层，位于 [crates/water-gpu](../crates/water-gpu)：

| 文件 | 职责 |
| --- | --- |
| `cuda/*.cu` | CUDA C++ 计算核源码 |
| `build.rs` | 编译期调 `nvcc --ptx -arch=$WATER_GPU_ARCH` 把每个核编成 PTX，落 `OUT_DIR` |
| `src/lib.rs` | `GpuContext`（cudarc 上下文/默认流/已加载模块）+ 各核的安全 Rust 包装 + 对拍测试 |

- 默认目标架构 `sm_120`（Blackwell，RTX 5070）。其它架构用环境变量 `WATER_GPU_ARCH=sm_86` 覆盖。
- `nvcc` 路径默认取 PATH；可用 `NVCC=/usr/local/cuda/bin/nvcc` 覆盖。
- cudarc 用 `cuda-13030` 特性，精确匹配本机 **CUDA Toolkit 13.3**。

### cudarc 0.19 关键 API（备忘）

```rust
let ctx = CudaContext::new(0)?;                 // Arc<CudaContext>
let stream = ctx.default_stream();              // Arc<CudaStream>
let module = ctx.load_module(Ptx::from_src(PTX))?;
let func = module.load_function("kernel_name")?;
let d_a = stream.clone_htod(&host_slice)?;      // host→device（非弃用；memcpy_stod 已弃用）
let mut d_out = stream.alloc_zeros::<f32>(n)?;
let mut b = stream.launch_builder(&func);
b.arg(&d_a).arg(&mut d_out).arg(&n);
unsafe { b.launch(LaunchConfig::for_num_elems(n as u32))? };
let out: Vec<f32> = stream.clone_dtoh(&d_out)?; // device→host
```

`PushKernelArg` trait 需 `use` 才能用 `.arg()`。

## 验证方法论（铁律）

- **GPU 实现的首选验证 = 与 CPU-Rust 路径数值对拍**，容差 **< 1e-6**。
- CPU-Rust 路径（本仓库 `water-core` / `water-hydro` 等，即 rust/shadcn）已与 Python 逐位对拍过
  （见 [HYDRO.md](HYDRO.md) / [FCLASS.md](FCLASS.md) / [EDGE_DEPTH.md](EDGE_DEPTH.md)），
  故 **CPU 路径即 GPU 的 golden 基准**。
- 每个上 GPU 的算法都要在 `water-gpu/tests/` 或 `src` 内建对拍，记录最大绝对/相对误差。

### 已验证：GPU 全链路自检 ✅

- 核：`cuda/vector_add.cu`（逐元素向量加）。
- 测试：[crates/water-gpu/src/lib.rs](../crates/water-gpu/src/lib.rs) `vector_add_matches_cpu`。
- **证据**：`NVIDIA GeForce RTX 5070`，100 万+ 元素（非 32 整除，验证 `if(i<n)` 边界）
  GPU 结果与 CPU 参考**逐位一致**。证明 `nvcc→PTX→cudarc→GPU→回传` 全链路在 Blackwell sm_120 + CUDA 13.3 可用。
- 复现：`cargo test -p water-gpu --release -- --nocapture`。

## GPU 加速路线（按优先级，均需与 CPU 对拍）

1. **最高——Laplace 稀疏水面求解上 GPU**：CPU 版最大瓶颈（超线性）。候选 **cuDSS**（GPU 直接稀疏 Cholesky，
   对拍友好）/ cuSOLVER / AMGX。**务必带 fill-reducing 重排序**——CPU 版 faer 用 AMD 重排序是本项目最大的坑
   （无重排序会 O(n³) 填充爆内存，见 [HYDRO.md](HYDRO.md) 阶段 1 与 MEMORY 记录）。对拍目标：
   `water-hydro::solve_laplace_dirichlet`（faer）。
2. **高——warp / reproject 逐像素核**：天然可并行。需复刻 GDAL 默认 0.125px 近似变换器
   （CPU 版已实现于 `water-io/warp_approx.rs`），GPU 版对拍之。
3. **中——形态学 / 高斯 / 距离变换 / 骨架**：替代 scipy.ndimage / skimage，可用 NPP 或自研核，
   与 `water-core::raster_ops` 对拍。

## 性能基线（GPU 要超越，来自 rust/shadcn CPU 版）

| 数据集 | 规模 | CPU-Rust hydro |
| --- | --- | --- |
| `linzhi_clip`（裁剪大中小） | 3036×3076 ≈9.3M px，6 要素 | 单线程 2.9 s |
| `linzhi`（全量） | 26492×18138 ≈4.8亿 px，41 要素 | 4 线程全分辨率 **48.5 s**（单线程 104.7 s） |

GPU 目标：Laplace 求解上 GPU（主瓶颈）、warp/重采样上 GPU 核、大批量多瓦片多流并发。

## 环境（本机，2026-07-03，Linux/WSL）

- Rust stable 1.96.1（`~/.cargo/env` 随 `.bashrc` 加载）。工具链需 ≥ 1.85（eci-gdal edition 2024）。
- CUDA Toolkit 13.3（`nvcc` 在 `/usr/local/cuda/bin`），GPU RTX 5070（Blackwell sm_120），驱动 610.47。
- eci-gdal submodule 固定 **feature 分支** `49dc33e`（与 rust/shadcn gitlink 一致；含 warp/GeoTIFF 写/Shapefile 写）。

## 构建与测试

```bash
# GPU 桥接端到端自检（nvcc→PTX→cudarc→GPU→回传，与 CPU 对拍）
cargo test -p water-gpu --release -- --nocapture

# 只构建 GPU + CLI（避开 Tauri 桌面的系统 webkit 依赖）
cargo build -p water-gpu -p water_cli --release

# 全量构建（含 gui/desktop，需系统 GUI 依赖）
cargo build --workspace --release
```
