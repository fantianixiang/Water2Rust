# CUDA — GPU 加速层（Water2GPU / water-gpu）

本文件是 **Water2GPU** 的 GPU 加速权威规格。Water2GPU 是水体 GIS 项目的**第三代**：

```
MyProject（Python 基线）→ Water2Rust（纯 Rust CPU，rust/shadcn）→ 【本项目】GPU/Project（CUDA+Rust 加速）
```

本分支 `GPU/Project` **继承自 `rust/shadcn`**（完整纯 Rust 程序：CLI + REST API + egui GUI + Tauri 桌面），
在其之上叠加 GPU 加速层，逐模块把 CPU 数值核搬到 GPU。

## 架构：Rust 编排 + CUDA 计算核

- **Rust 负责**：IO / 几何 / CRS / 瓦片编排 / 对拍 / 错误处理（全部继承自 rust/shadcn，不变）。
- **GPU 只做计算核**：热点数值算法用 CUDA C++（`.cu`，含主机端 `extern "C"` launcher），`nvcc` 编成静态库，
  Rust 经 **FFI** 调用（NVIDIA `cuda-samples` 风格，见「CUDA 编程规范」）。
- **对「纯 Rust」的必要例外**：GPU 计算核为 CUDA C++、且**构建期需 CUDA 工具链**（`nvcc` + 链接 `cudart`）。
  这是 GPU 加速的本质要求；除此之外全链路仍为纯 Rust。GPU 为可选加速，CPU faer 保底，故无 GPU 环境
  （或非 NVIDIA）仍能运行。

## `water-gpu` crate

GPU 桥接层，位于 [crates/water-gpu](../crates/water-gpu)：

| 文件 | 职责 |
| --- | --- |
| `cuda/xxx.cu` | CUDA C++ 计算模块：设备核 + **主机端 launcher**（`extern "C"`） |
| `cuda/xxx.cuh` | 声明该 .cu 的 launcher/接口（每个 .cu 必配） |
| `cuda/common.cuh` | 共用宏：`CUDA_CHECK` / `CUDA_CHECK_KERNEL` / `cuda_require_free_mem` / `CudaTimer` |
| `build.rs` | `nvcc -c` 编每 .cu 为对象→归档 `libwater_cuda_kernels.a`→链接（+cudart+stdc++） |
| `src/lib.rs` | 各 launcher 的 Rust FFI 声明 + 安全包装（返回结果 + [`KernelTiming`]）+ 对拍测试 |

- 默认目标架构 `sm_120`（Blackwell，RTX 5070）。其它架构用 `WATER_GPU_ARCH=sm_86` 覆盖。
- `nvcc` 路径默认取 PATH（`NVCC` 覆盖）；CUDA 库目录 `CUDA_LIB_DIR`（默认 `/usr/local/cuda/lib64`）。

## CUDA 编程规范（强制）

新写任何 CUDA（.cu）代码必须遵守，对标 NVIDIA `cuda-samples`（`Common/helper_cuda.h`）：

1. **每个 `xxx.cu` 配同名 `xxx.cuh`**，声明其 kernel / 主机端 launcher（`extern "C"`）接口。
2. **严格类型/显存检查**：
   - `CUDA_CHECK(call)` 包裹**每一个** CUDA 运行时/库调用（检 `cudaError_t`，失败报 文件:行+错误名）；
   - 核启动后 `CUDA_CHECK_KERNEL(name)`（`cudaGetLastError` + `cudaDeviceSynchronize`）；
   - 大块分配前 `cuda_require_free_mem(bytes)`（`cudaMemGetInfo` 校验可用显存）；
   - 固定宽度类型（`int32_t`/`int64_t`/`double`），主机↔设备类型严格对齐（Rust `#[repr(C)]` 镜像）。
3. **分段计时器（毫秒）**：用 `CudaTimer`（`cudaEvent`）分别计 **H2D / kernel / D2H**，
   写入 `WaterKernelTiming`（与 Rust [`KernelTiming`] 对齐）回传给调用方。
4. **风格**对齐 cuda-samples：`extern "C"` 命名、每核单一职责、资源及时释放。

参考实现（模板）：[cuda/vector_add.cu](../crates/water-gpu/cuda/vector_add.cu) +
[cuda/vector_add.cuh](../crates/water-gpu/cuda/vector_add.cuh) + [cuda/common.cuh](../crates/water-gpu/cuda/common.cuh)。

> 架构变更（2026-07-03）：根据上述规范，主机编排移入 .cu（NVIDIA 风格）经 FFI 调用，
> **不再用 cudarc 加载 PTX**（已从 water-gpu 移除 cudarc 依赖）。cuDSS 仍用 cudart 运行时 API。

## 验证方法论（铁律）

- **GPU 实现的首选验证 = 与 CPU-Rust 路径数值对拍**，容差 **< 1e-6**。
- CPU-Rust 路径（本仓库 `water-core` / `water-hydro` 等，即 rust/shadcn）已与 Python 逐位对拍过
  （见 [HYDRO.md](HYDRO.md) / [FCLASS.md](FCLASS.md) / [EDGE_DEPTH.md](EDGE_DEPTH.md)），
  故 **CPU 路径即 GPU 的 golden 基准**。
- 每个上 GPU 的算法都要在 `water-gpu/tests/` 或 `src` 内建对拍，记录最大绝对/相对误差。

### 已验证：GPU 全链路自检 ✅

- 核：`cuda/vector_add.cu`（逐元素向量加，主机端 launcher `water_vector_add`）。
- 测试：[crates/water-gpu/src/lib.rs](../crates/water-gpu/src/lib.rs) `vector_add_matches_cpu`（经 FFI 调用）。
- **证据**：`NVIDIA GeForce RTX 5070`，100 万+ 元素（非 32 整除，验证 `if(i<n)` 边界）
  GPU 结果与 CPU 参考**逐位一致**。证明 `nvcc→PTX→cudarc→GPU→回传` 全链路在 Blackwell sm_120 + CUDA 13.3 可用。
- 复现：`cargo test -p water-gpu --release -- --nocapture`。

### 已验证：Tier 1 cuDSS 稀疏直接解 PoC ✅（2026-07-03）

Laplace 水面求解上 GPU 的关键前置——GPU 直接稀疏 Cholesky 与 CPU faer 的数值 parity。

- 实现：[crates/water-gpu/src/cudss.rs](../crates/water-gpu/src/cudss.rs) `solve_spd_csr`（`cudss` 特性门控，默认关）。
- 求解库：**NVIDIA cuDSS 0.8.0.10**（pip wheel `nvidia-cudss-cu13`，装于 `third_party/cudss`），重排序设 **AMD**（对齐 faer）。
- 上下文：用 **CUDA 运行时 API（cudart）** 统一管理显存/流，规避 cudarc（驱动 API）与 cuDSS（运行时 API）上下文错配。
- 对拍基准：faer AMD 稀疏 Cholesky（与 hydro `solve_spd_faer` 同一实现），同一 SPD 网格 Poisson-Dirichlet 矩阵。
- **证据**（`cargo test -p water-gpu --release --features cudss -- --nocapture`）：
  - n=3600、nnz=10680：cuDSS vs faer **max_abs = 1.11e-14、max_rel = 5.94e-15**（机器精度，判据 <1e-6）。
  - 耗时拆分：H2D 237µs / 分析 148ms（含一次性 cuDSS·CUDA 初始化）/ 分解 60ms / 求解 36ms / D2H 97µs / 总 681ms。
- **洞见**：小矩阵（n≈3.6k）GPU 固定开销远大于 CPU faer。
- **足迹**：cuDSS 运行时依赖 {libcudss, libcublas, libcublasLt}（不需 cusparse/cusolver）。

### 🔴 Tier 1 profile 结论：cuDSS 直接解**不如 faer**（负面结果，2026-07-03）

对 2D 网格 Laplacian 单次直接求解做规模扫描（warm-run，排除一次性 init）：

| n | faer(CPU) ms | cuDSS best ms | 加速比 |
| --- | --- | --- | --- |
| 40,000 | 39 | 105 | 0.37x |
| 250,000 | 270 | 803 | 0.34x |
| 1,000,000 | 1341 | 2901 | **0.46x** |

- cuDSS **全程慢于 faer 2–25×**（n=900→1M）；瓶颈是 cuDSS 的 **analysis 阶段**（reordering+symbolic，1M 时 ~2.8s）。
- **与重排序无关**：DEFAULT / AMD 的 analysis 均 ~2.7–2.8s（DEFAULT 仅让 factorization 变快）；
  **`HOST_NTHREADS=16` 无效**，说明 analysis 不并行/走主机，未利用 GPU。
- **根因（第一性原理）**：稀疏直接 Cholesky 单次解 2D 网格 Laplacian 不适合 GPU——analysis 不规则/串行；
  hydro 每多边形单次解，无法摊薄 cuDSS 昂贵的 analysis（cuDSS 为多-RHS/重分解场景设计）；faer CPU AMD+supernodal 已很优。
- 数值 parity 始终 OK（~1e-14），是**性能**不行。测试：`profile_cudss_vs_faer_scaling`（`--ignored`）。
- **结论**：**不用 cuDSS 替换 faer 做 Laplace**（会 regress）；Laplace 保留 CPU faer。GPU 力量重定向逐像素
  稠密并行核（Tier 2 warp gather + Tier 3 EDT/高斯/形态学）。cuDSS PoC 代码/安装保留作证据与未来选项
  （若 Laplace 日后成剩余瓶颈，可试 GPU 迭代 PCG（无 analysis）或批处理多小多边形）。

### 🟢 路径 B：matrix-free FP64 Jacobi-PCG ✅（有效，2026-07-03）

不装配 CSR，用 5 点 stencil 直接施加算子（`(Mz)=deg·z − 邻居和`），GPU 上跑 Jacobi 预条件共轭梯度（FP64）。
实现：[cuda/laplace_pcg.cu](../crates/water-gpu/cuda/laplace_pcg.cu) + [.cuh](../crates/water-gpu/cuda/laplace_pcg.cuh)，
Rust 包装 [`laplace_pcg`](../crates/water-gpu/src/lib.rs)。只需 cudart（无 cuDSS/cuBLAS 依赖），自研 stencil + 规约核。

profile（vs faer 直接解，rtol=1e-10）：

| n | faer(CPU) ms | PCG(GPU) ms | 加速比 | max_abs(vs faer) | iters |
| --- | --- | --- | --- | --- | --- |
| 40,000 | 35 | 111 | 0.32x | 4.1e-8 | 493 |
| 250,000 | 257 | 245 | **1.05x** | 2.3e-7 | 990 |
| 518,400 | 635 | 358 | **1.77x** | 6.0e-7 | 1224 |
| 1,000,000 | 1307 | **460** | **2.84x** | 2.7e-7 | 1415 |

- **交叉点 ≈ 250k 未知数（Jacobi-PCG）**：≥250k 更快，1M 时 2.84×。⚠️ 此为紧凑方形网格乐观上界，
  真实细长河 Jacobi-PCG 迭代爆炸达不到——须上 **MG 预条件**方能在真实大河反超（见「真实地形三方对比」：
  n=414k MG 端到端 2.9× 反超 faer）。
- **parity**：max_abs vs faer < 6e-7（< 1e-6 判据内）；对米级水面高程为亚微米精度，远超物理需求（需更紧可降 rtol）。
- iters ~ O(m)（Jacobi-PCG 预期）；FP64 在消费卡 5070（FP64=1/64 FP32）仍够快——PCG 内存带宽受限，非算力受限。
- 测试：`laplace_pcg_matches_faer`（parity）、`profile_pcg_vs_faer_scaling`（`--ignored`，规模扫描）。
- **结论**：**路径 B 可行但需真实地形校准**——合成扫描证明 matrix-free PCG 数值正确、规模趋势对；
  但真实细长河须用**紧凑变量 PCG**（免空 bbox 无用功）+ **聚合多重网格预条件**（降迭代数）方能反超。
  分派阈值按真实数据设为 **150k**，见「真实地形三方对比」。
  剩余优化空间：融合核、标量驻留设备（免每迭代 D2H）、smoothed aggregation（进一步降迭代）。

## 真实地形三方对比（方法论 · 强制）

> **每个 GPU 加速点都必须在真实地形数据上做 Python / Rust / GPU 三方对比并记录于本节**——
> 合成基准（规则网格）常过于乐观，唯真实地形能给出可信的加速倍数与交叉点。

- **数据**：`linzhi_clip`（真实 DEM + 已分类水体，`/home/fantianxiang/Water2GPU/linzhi_clip`）；全量林芝（若可用）。
- **Python 环境**：conda `geoai_pack_py310`（scipy 1.15.2，原基线 `spsolve` = SuperLU+COLAMD）。
- **方法**：诊断转储真实计算系统 → 三方在**同一真实系统**上端到端计时（装配 + 求解，best-of-3）。
- **工具**：Rust 转储 `WATER_LAPLACE_DUMP_DIR=<dir>` + 跑真实 hydro；对比
  [examples/bench_laplace_real.rs](../crates/water-hydro/examples/bench_laplace_real.rs)（faer + GPU PCG）、
  [scripts/bench_laplace_real.py](../scripts/bench_laplace_real.py)（spsolve）。

### Laplace 求解 —— 真实地形结果（2026-07-03/04）

**全量林芝**（1.9GB DEM，`~/DATA/linzhi`）跑真实 hydro 导出的最大 Laplace 系统：
**大河 2686×5771，n_int=413,968**（bbox 1550 万像素，**填充率仅 2.7%**——真实河道细长稀疏）。
端到端（装配 + 求解，best-of-3）：

| 真实系统 | Python `spsolve`（e2e/纯解） | Rust faer | GPU 网格 PCG | GPU 紧凑 Jacobi-PCG | GPU 紧凑 **MG-PCG** |
| --- | --- | --- | --- | --- | --- |
| n=413,968（全量林芝大河） | 1431 / 611 | 406 ms | 4708 🔴 | 431 ms（1203 迭代） | **~140 ms ⚡（60 迭代，2.9×）** |
| n=112,243 | — | 161 ms | — | ~200 ms（259 迭代） | **154 ms（28 迭代）** |
| n=73,785 | — | 88 ms | — | ~100 ms（223 迭代） | 93 ms（26 迭代，持平） |
| n=50,364 | — | **26 ms** | — | 135 ms（530 迭代） | 44 ms（43 迭代） |

**关键教训（真实地形 vs 合成）**：
- **合成基准严重误导**：合成 1M 紧凑方形网格 PCG 快过 faer 2.84×（vs Python 17.6×），但真实大河**完全相反**。
- **根因① 稀疏 bbox**：真实河道细长，bbox 填充率仅 2.7%。初版**全网格** matrix-free PCG 在整个空 bbox 上跑
  stencil → 做 ~37× 无用功 → **比 faer 慢 12 倍（4708ms）**。
- **修复：紧凑变量 PCG**（[cuda/laplace_pcg.cu](../crates/water-gpu/cuda/laplace_pcg.cu) `water_laplace_pcg_compact`）——
  只对 n 个变量存邻居下标数组、stencil 只遍历变量，不碰空 bbox。**4708ms → 431ms（11×）**。
- **根因② 细长域收敛慢**：紧凑修复后 GPU kernel 368ms / **1203 迭代**（同规模紧凑方形网格仅 ~640 迭代），
  故 GPU e2e 431ms 仍**略慢于 faer 399ms**——细长真实河的 Jacobi-PCG 收敛慢是剩余瓶颈。
- **突破：聚合多重网格（MG）预条件**（[cuda/laplace_mg.cu](../crates/water-gpu/cuda/laplace_mg.cu)）——用 2×2 聚合
  逐层粗化建 Galerkin 层次（各层仍为加权紧凑 5 点算子），以对称 V-cycle（阻尼 Jacobi 光滑 + 粗网格校正）作
  PCG 预条件。**低频误差经粗网格一举消除**，迭代数降到**网格无关**的 ~30-60（不再随规模爆炸）：
  - 真实林芝大河 n=414k：**1203 迭代 → 60 迭代**，GPU kernel **370ms → 74ms（5×）**，端到端 **~140ms vs faer 406ms = 2.9×** ⚡。
  - 合成方网扫描（MG vs Jacobi 迭代数）：n=4k→1M 时 MG 迭代恒 22-42（Jacobi 132→1415）；n=1M MG kernel 86ms vs faer 1332ms = **15.5×**。
  - SPD 保证：Galerkin 粗算子 + 对称光滑 + R=P^T → 预条件 SPD → CG 有效、保真。聚合天然贴合拓扑，对细长河比节点几何粗化稳健。
- **诚实结论**：紧凑变量（免空 bbox）+ MG 预条件（降迭代）+ 混合精度 + CUDA 图 + 主机并行**多管齐下**，
  GPU 在真实林芝大河上**端到端 3.2× 反超 faer**（123.6ms vs 400ms）——GPU Laplace 的真正价值兑现。
- **阈值**：`GPU_PCG_MIN_VARS=150k`。实测 e2e 交叉点 ~80-110k（n=112k MG 已胜、73k 持平、50k faer 胜——
  小系统 GPU 固定开销 + 主机建层次未摊薄）；设 150k 留保护裕度，小系统走 faer 无回退，大河走 MG 得实质加速。
  数值 parity 已验证（真实 Python 夹具 2e-11、578k 圆盘 vs faer 8.5e-9 < 1e-6）。
- **优化迭代链**（详见 [cuda-exam/迭代日志.md](../cuda-exam/迭代日志.md)，逐次含真实大河三方对比）：
  - **V8 主机并行①**：`build_pcg_compact` rayon 逐行并行 + 前缀和编号 + i32 反查表；层次构建去 HashMap（稠密块表 + 直接粗邻居槽）。装配 64→35ms，e2e 260→200ms。
  - **V9 混合精度**：V-cycle 核模板化为 FP32（外层 CG 保 FP64 → parity 不损）。kernel 82→72ms。**发现 kernel 为 launch-bound**（非带宽受限）。
  - **V11 CUDA 图**：V-cycle 是固定核序列，捕获一次每迭代重放（~96 启动→1 图启动）。**kernel 72→28.6ms（2.5×）**，e2e 187→154ms。
  - **V12 主机并行②**：结果写回改 calloc 起底 + rayon 逐行填。e2e 154→**123.6ms（3.2×）**。
  - V10 合并 slab/pinned 分配：评估后降级（FP32 层次已使 H2D~7ms，非瓶颈）；V13 smoothed aggregation：暂缓（破坏 ≤4 度紧凑核统一性、净收益边际）。
- **累计**：GPU kernel 368ms→28.6ms（12.9×），e2e 从「与 faer 持平」到 **3.2× 反超**。剩余最大主机项 = 层次构建 ~45ms（含串行聚合下标分配），为未来杠杆。

> 合成规模扫描（紧凑方形网格）：MG-PCG n=1M kernel 86ms vs faer 1332ms = 15.5×、vs Jacobi-PCG 508ms = 5.9×。
> **但真实细长河 e2e 受主机装配/建层次限制**（大河 ~140ms 中 GPU 仅 74ms），故实际 2.9×——仍是决定性反超。

## GPU 加速路线（按优先级，均需与 CPU 对拍）

> 修订（2026-07-04，依 profile + 真实地形证据）：Tier 1 **cuDSS 直接解已证伪**（慢于 faer）；
> **Laplace GPU 化 = 紧凑变量 + 聚合多重网格（MG）预条件 matrix-free PCG**，真实林芝大河
> 端到端 **2.9× 反超 faer**（见「真实地形三方对比」）。小水域（<150k）保留 faer。

1. **Laplace 上 GPU —— 紧凑变量 + MG 预条件 matrix-free FP64 PCG（非 cuDSS）** ✅ **已完成**：
   紧凑变量免空 bbox 无用功；聚合多重网格 V-cycle 预条件把细长河迭代数从 1203 降到网格无关的 ~60，
   GPU kernel 5× 提速，真实大河 e2e 2.9× 反超 faer。已接入 `water-hydro::solve_laplace_dirichlet`
   （阈值 150k 分派 + CPU faer fallback），parity < 1e-6 全验证。
   剩余优化：主机建层次并行化/缓存、smoothed aggregation、融合核 + 标量驻留设备。
2. **高——warp / reproject 逐像素核**：天然可并行。需复刻 GDAL 默认 0.125px 近似变换器
   （CPU 版已实现于 `water-io/warp_approx.rs`），GPU 版对拍之。
3. **中——形态学 / 高斯 / 距离变换 / 骨架**：替代 scipy.ndimage / skimage，可用 NPP 或自研核，
   与 `water-core::raster_ops` 对拍。

## Windows 移植与 GPU warp 对拍决策（2026-07-06）

GPU/Project 移植到 Windows（VS 2026 v18 + CUDA 13.3.1 + `--features gpu`）并做真实地形三方对拍
（`linzhi_clip`，与 rust/shadcn CPU 黄金基准 `waters_hydro.tif` 逐位比对）：

| 路径 | vs 黄金基准 max | mean | p99 | >0.1m 像素 |
| --- | --- | --- | --- | --- |
| CPU（GPU off） | **0（逐位一致）** | 0 | 0 | 0 |
| GPU（含 GPU warp） | **76.0 m** | 1.7 mm | 16 mm | 16445（0.2%） |
| GPU（warp 走 CPU，仅 Laplace GPU） | **0（逐位一致）** | 0 | 0 | 0 |

- **根因**：GPU warp 的双线性采样 / nodata 门控在**水陆边界差 ±1 像素**，陡岸处水面高程 vs 岸坡高程
  一翻即数十米。99% 区域仍 mm 级，仅一条边界带（0.2%）出现大差。非 Windows 特有（核与 Linux 同字节）。
- **决策（方案 A）**：**GPU warp 默认关闭**，生产 warp 恒走 CPU（与黄金基准逐位一致）；GPU 仅加速
  主瓶颈 Laplace（对拍 < 6e-7）。warp 上 GPU 仅省 ~2s，却引入边界误差，得不偿失。
  `gpu_warp_applicable`（`water-hydro/src/pipeline.rs`）改为**仅 env `WATER_HYDRO_GPU_WARP=1` 显式开启**（对拍/实验用）。
- **Windows 构建要点**：① CUDA 自定义安装须去掉 **Driver** 与 **Visual Studio Integration**（后者在 VS18 下
  报 `0x80070715` 致整包失败）；② `warp.cu` 在 MSVC 下需 `#define _USE_MATH_DEFINES` 才有 `M_PI`；
  ③ 须在 vcvars64 开发者环境（`cl.exe`/`lib.exe`）+ `%CUDA_PATH%\bin` 上 PATH 下 `cargo build --features gpu`。

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
