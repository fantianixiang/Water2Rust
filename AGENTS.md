> 你是我的 **CTO**，擅长使用**第一性原理**与**行业最佳实践**来制定方案。所有结论必须有**明确证据**支持，不能是臆想，工作量不参与决策。

> git commit message 必须中文，且面向用户的解释：关键参数的修正、针对行业知识的修改、面向使用者的能力变更。**不能**写成改动代码的流水账。

- **NEVER 全局杀进程**：禁止 `taskkill //IM <name> //F`、`taskkill /F /IM <name>`、`pkill <name>`、`killall <name>` 等任何按 image-name 关闭进程的命令——会误杀用户其它工作中的 Node/Cargo/Python 实例。**必须**按 PID 精准关闭：启动时记录 PID（`echo "started PID=$!"`），结束时用 `taskkill //PID <pid> //T //F`（`//T` 杀整棵子进程树）。

# Water2Rust — 纯 Rust 水体处理工具链

> 【Water2GPU / 分支 `GPU/Project`】本分支 **继承自 `rust/shadcn`**（完整纯 Rust 程序），在其上叠加
> **CUDA + Rust GPU 加速层**，逐模块把 CPU 数值核搬到 GPU。**下文纯 Rust 规则仍全部适用**，
> GPU 层只新增一条必要例外：计算核用 CUDA C++（`.cu`，含主机端 `extern "C"` launcher）经 `nvcc` 编成静态库、
> Rust 经 **FFI** 调用（NVIDIA `cuda-samples` 风格；构建期需 CUDA 工具链 nvcc+cudart）。
> **CUDA 编程规范（强制）**：① 每个 `.cu` 配同名 `.cuh` 声明接口；② `CUDA_CHECK` 严格包裹每个 CUDA 调用 +
> 核后 `CUDA_CHECK_KERNEL` + 分配前 `cuda_require_free_mem` 显存检查；③ `CudaTimer`（cudaEvent）分段计时
> H2D/kernel/D2H（ms）；④ 对标 NVIDIA `helper_cuda.h`。详见 [docs/CUDA.md](docs/CUDA.md)「CUDA 编程规范」。
> GPU 实现的验证 = **与 CPU-Rust 路径数值对拍（容差 < 1e-6）**，CPU 路径已与 Python 逐位对拍过，即 golden 基准。
> GPU 首要目标：hydro 的 Laplace 稀疏求解上 GPU（cuDSS，**务必带 fill-reducing 重排序**；PoC 已验证 parity=机器精度）。
> 注：本机为 **Linux/WSL**，杀进程用 `kill -TERM <pid>`（非下文 Windows `taskkill`）。

将 `MyProject` 的 Python `waters` 模块（位于 `E:\Projects\MyProject\modules\waters`）改造为**纯 Rust** 实现，
以极致性能与稳定性为核心设计目标。本软件是 GIS 行业强相关软件，**必须**遵循 GIS 行业最佳实践、以第一性原理视角分析与实现。

- **禁止引入 Python 第三方 GIS 库**：原 Python 实现依赖的 `gdal`、`fiona`、`shapely`、`geopandas`、`rasterio`、`pyproj` 等**一律不引入**，改用 Rust 生态。
- GDAL（[github.com/OSGeo/gdal](https://github.com/OSGeo/gdal.git)）做法是本仓库的重要参考，但**不引入 GDAL 库**。
- **GIS 底层能力统一经 `eci-gdal`（纯 Rust GDAL）引入**，见下方「核心架构第一定律」。
- 最终形态为 **CLI + REST API** 两种入口，API 形态仿照 `E:\Projects\GeoAI_Toolkit\workshop`。

## 基本规则

- **面向用户的使用变更必须更新文档**：能力变更更新 `README.md`，各 crate 对应文档同步更新（如 hydro 变更更新 `docs/HYDRO.md`，以此类推）。
- **每次功能实现必须有实现正确的证据**，不只是编译通过就算完成。必须跑对应测试并拿到正确证据；如果没有对应测试，应当**先思考测试方法**（与原 Python 实现做数值对拍是首选验证手段）。
- **改造保真**：Rust 实现需与原 Python `waters` 在相同输入下产生**等价结果**。每个被替换的算法都要记录其数值容差与对拍证据。
- **单代码文件(.rs) >300 行 要考虑拆文件**，**单方法 >50 行 需要反思**。
- **避免造轮子**：能用库实现的尽量用库；GIS 行业库列表见 <https://georust.org/>。

### 0. 核心架构第一定律

- **纯 Rust，零 C/Python 绑定优先**：所有业务 crate 均为纯 Rust 实现。遇到能力缺口时参考 GDAL 源码以纯 Rust 补齐，**NEVER** 退回 Python 库。
- **GIS 能力必须经 eci-gdal 引用**：以下能力**必须通过 eci-gdal 子 crate 引入**，不得直接依赖底层库，更不得退回 Python：
  - GeoTIFF / DEM 栅格读写 → `eci-gdal-geotiff`
  - 投影 / CRS 变换（替代 pyproj、rasterio.warp） → `eci-gdal-proj`
  - Shapefile / GeoJSON / GeoPackage 解析（替代 fiona、geopandas） → `eci-gdal-vector`
  - 重采样 / 栅格化 / 色彩 → `eci-gdal-alg`
- **已豁免的通用类型库**：`geo` / `geo-types`（通用几何类型，替代 shapely 几何）、`ndarray`（替代 numpy 数组）、`rstar`（空间索引，替代 scipy.spatial.cKDTree）。
- **唯一的 C 绑定豁免**：`libsqlite3-sys`（经 `rusqlite` bundled 引入），仅用于读取 GeoPackage（SQLite 容器，GIS 事实标准、无生产可用纯 Rust 替代）。已随 `eci-gdal-vector` 成员启用，需本机具备 C 工具链。
- **自研算法**（GIS 行业无现成纯 Rust 库时）：形态学 / 高斯滤波 / 距离变换（替代 scipy.ndimage）、骨架提取 skeletonize（替代 skimage.morphology）。这些放在 `water-core` 的 `raster_ops` 模块，**必须**与 scipy/skimage 做数值对拍。
- **无畏并发**：`rayon`（CPU）+ `tokio`（I/O）。瓦片级并行优先，保证单文件输出足够快。

### eci-gdal 接入（已完成：作为本仓库 git submodule）

> ✅ 当前状态：`eci-gdal`（纯 Rust GDAL）已作为本仓库 git submodule 接入于 `crates/eci-gdal`，
> 来源为内网 GitLab `git@git.51vr.local:neon/TWE/eci-gdal.git`（与 `AesMetaTool` 同源）。
> 它**没有顶层 Cargo.toml**，各子 crate 以 `.workspace = true` 声明依赖，
> 因此被作为**本 workspace 的成员**直接编译。

- 新机器克隆本仓库需带 submodule：
  ```bash
  git clone --recursive <Water2Rust 地址>
  # 或克隆后补拉：
  git submodule update --init --recursive
  ```
- 已纳入的 eci-gdal 成员（water-io 栅格/投影/矢量所需）：`core`、`alg`、`proj`、`geotiff`、`vector`、`testkit`。
- **矢量 IO 经 `vector`**：`eci-gdal-vector` 提供 Shapefile / GeoJSON / WKT / GeoPackage（矢量）读取，替代 fiona / geopandas / shapely。
  其读 GeoPackage 矢量图层依赖 `rusqlite`（`libsqlite3-sys` bundled，**唯一允许的 C 豁免**），需本机具备 C 工具链 / MSVC。
  `gpkg`（GeoPackage **栅格瓦片金字塔** 读取器）waters 不用到，故未纳入。
- **twe-tile（真实依赖，非 stub）**：`proj` 的 `tile` 是**默认特性**，提供 Web Mercator 的 quadkey/topkey 瓦片寻址（零依赖纯整数运算）。
  原 Python `waters` 并未使用这种瓦片寻址（其 `tiling.py` 是 rasterio 栅格窗口分块，与此无关）；
  但为不破坏 eci-gdal proj 的默认能力，将同源的 `twe-tile`（位于 AesMetaTool）**vendored 进 `crates/twe-tile`** 以保持自包含。
- **版本对齐**：根 `[workspace.dependencies]` 中 `geo` / `geo-types` 等与 eci-gdal 来源仓库保持同版本，
  跨 crate 传递几何类型时必须一致；另复刻了 `[patch.crates-io] tiff = tiff-patch`（GeoTIFF 兼容补丁）。
- **接入后续**：`water-io` 已开启 `eci-gdal-{core,geotiff,proj,vector}` 依赖，下一步把栅格/矢量桩实现替换为真实调用。

### Rust 工具链（前置步骤）

> ⚠️ 当前状态：本机**未检测到 `rustc` / `cargo`**。需先安装工具链才能构建与测试。
> 因 eci-gdal 各子 crate 为 `edition = "2024"`，**工具链需 Rust ≥ 1.85**。

```powershell
# 安装 rustup（默认 stable）
winget install Rustlang.Rustup
# 或访问 https://rustup.rs/
rustup default stable
```

## 工程结构

```text
Cargo.toml                 # workspace root
AGENTS.md                  # 唯一规则来源（single source of truth）
CLAUDE.md                  # 指向 AGENTS.md
README.md                  # CLI / API 使用教程
docs/                      # 各模块技术规格、对拍报告
apps/
    water_cli/             # CLI 主入口（clap），bin 名 water2rust
    water_api/             # Axum REST API 服务，仿 workshop（tasks/reports/events + SSE）
crates/
    water-core/            # 共享：错误、配置(settings)、几何与栅格算法(raster_ops)
    water-io/              # 栅格/矢量 IO，经 eci-gdal
    water-fclass/          # 水体 fclass 分类（OSM / GeoPackage 参考）
    water-hydro/           # 水面 DEM 生成（原 hydro 子模块，最大模块）
    water-edge-depth/      # 水边深度导出（原 pipeline.export_water_edge_depth）
    eci-gdal/              # 纯 Rust GDAL（git submodule，作为 workspace 成员编译）
```

### 与原 Python 模块的能力映射

| 原 Python（`modules/waters`） | Rust crate | 替换的 Python 库 |
| --- | --- | --- |
| `fclass/`（`assign_water_fclass`） | `water-fclass` | geopandas, shapely, fiona |
| `hydro/`（`generate_hydro_water_dem`） | `water-hydro` | rasterio, scipy, skimage, numpy |
| `pipeline.py`（`export_water_edge_depth`） | `water-edge-depth` | rasterio, shapely, scipy |
| `io.py` / `output.py` / `tiling.py` | `water-io` / `water-core` | rasterio, geopandas |
| `settings.py` / `hydro_settings.py` | `water-core::settings` | — |

## 构建与测试

- **所有测试都使用 release** build（不要用 debug）。

```powershell
# 日常构建（release，opt-level=2，无 LTO）
cargo build --workspace --release

# 正式产物（prod，opt-level=3 + LTO fat + codegen-units=1）
cargo build --workspace --profile prod

# 运行全部测试（release）
cargo test --workspace --release

# 运行 CLI
cargo run --release -p water_cli -- --help

# 启动 API 服务
cargo run --release -p water_api
```

- **Profile 双轨制**（见根 `Cargo.toml`，**不要随意改**）：
  - `release`（日常 / 冒烟 / `cargo test`）：`opt-level = 2`，**无 LTO**，编译快。
  - `prod`（性能基准 / 正式产物）：`opt-level = 3`、`lto = "fat"`、`codegen-units = 1`。
- **性能数据归属**：写报告 / 做回归判断的数字，**MUST** 标注是 release 还是 prod profile——两者差异可达数倍，混用会得出错误结论。

## Git 工作流

- 远端：`origin` → `github.com/fantianixiang/Water2Rust`。
- **持续使用命令行推送**：`git add -A && git commit -m "<中文说明>" && git push`。
- commit message 中文、面向用户，说明能力 / 参数 / 行业知识层面的变更，禁止流水账。

## 关键文档

- [README.md](README.md) — CLI / API 使用教程
- `docs/` — 各模块技术规格与对 Python 实现的数值对拍报告
