> 你是我的 **CTO**，擅长使用**第一性原理**与**行业最佳实践**来制定方案。所有结论必须有**明确证据**支持，不能是臆想，工作量不参与决策。

> git commit message 必须中文，且面向用户的解释：关键参数的修正、针对行业知识的修改、面向使用者的能力变更。**不能**写成改动代码的流水账。

- **NEVER 全局杀进程**：禁止 `taskkill //IM <name> //F`、`taskkill /F /IM <name>`、`pkill <name>`、`killall <name>` 等任何按 image-name 关闭进程的命令——会误杀用户其它工作中的 Node/Cargo/Python 实例。**必须**按 PID 精准关闭：启动时记录 PID（`echo "started PID=$!"`），结束时用 `taskkill //PID <pid> //T //F`（`//T` 杀整棵子进程树）。

# Water2Rust — 纯 Rust 水体处理工具链

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
- **自研算法**（GIS 行业无现成纯 Rust 库时）：形态学 / 高斯滤波 / 距离变换（替代 scipy.ndimage）、骨架提取 skeletonize（替代 skimage.morphology）。这些放在 `water-core` 的 `raster_ops` 模块，**必须**与 scipy/skimage 做数值对拍。
- **无畏并发**：`rayon`（CPU）+ `tokio`（I/O）。瓦片级并行优先，保证单文件输出足够快。

### eci-gdal 接入（前置步骤，尚未完成）

> ⚠️ 当前状态：`eci-gdal` 是 `AesMetaTool` 仓库的 git submodule（`crates/eci-gdal`），**在本机尚未初始化、本地无源码**。在接入前，`water-io` 等 crate 以**桩实现**占位、可独立编译，但不具备真实 IO 能力。

接入步骤（任选其一，推荐 A）：

- **A. 作为本仓库 git submodule**（推荐，可独立构建）：
  ```bash
  git submodule add <eci-gdal.git 地址> crates/eci-gdal
  git submodule update --init --recursive
  ```
  然后在根 `Cargo.toml` 的 `[workspace.dependencies]` 取消 `eci-gdal-*` 注释，路径改为 `crates/eci-gdal/<sub>`。
- **B. 相对路径引用 AesMetaTool 的 submodule**：先在 `AesMetaTool` 内 `git submodule update --init` 拉取 eci-gdal 源码，再在根 `Cargo.toml` 启用形如 `../AesMetaTool/dev/crates/eci-gdal/<sub>` 的路径依赖（已预置注释模板）。

接入完成后，把对应 crate 的 `eci-gdal-*` workspace 依赖在各 `Cargo.toml` 打开，并将 `water-io` 桩实现替换为真实调用。

### Rust 工具链（前置步骤）

> ⚠️ 当前状态：本机**未检测到 `rustc` / `cargo`**。需先安装工具链才能构建与测试。

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
    water-io/              # 栅格/矢量 IO，经 eci-gdal（当前为桩实现）
    water-fclass/          # 水体 fclass 分类（OSM / GeoPackage 参考）
    water-hydro/           # 水面 DEM 生成（原 hydro 子模块，最大模块）
    water-edge-depth/      # 水边深度导出（原 pipeline.export_water_edge_depth）
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
