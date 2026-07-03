# Water2Rust

> 【Water2GPU / 分支 `GPU/Project`】本分支继承自 `rust/shadcn`，在完整纯 Rust 程序上叠加
> **CUDA + Rust GPU 加速**（逐模块把 CPU 数值核搬到 GPU，Rust 编排 + CUDA C++ 计算核）。
> 新增 GPU 桥接 crate [`water-gpu`](crates/water-gpu)；架构与路线见 [docs/CUDA.md](docs/CUDA.md)。
> 已验证：`nvcc→PTX→cudarc→GPU` 全链路在 RTX 5070（sm_120 / CUDA 13.3）与 CPU 逐位一致。
> 快速自检：`cargo test -p water-gpu --release -- --nocapture`。

将 `MyProject` 的 Python `waters` 模块改造为**纯 Rust** 的水体处理工具链。
**不依赖** `gdal` / `fiona` / `shapely` 等 Python 第三方库，GIS 底层能力统一经
[`eci-gdal`](crates/eci-gdal)（纯 Rust GDAL，git submodule）引入。

> 工作准则与架构规则以 [AGENTS.md](AGENTS.md) 为唯一来源。AI 助手请先读取 AGENTS.md。

## 能力

| 子命令 | 能力 | 对应原 Python |
| --- | --- | --- |
| `inspect` | 轻量检查：读水体矢量(+可选 DEM)，报告信息、采样质心高程、导出 GeoJSON | —（验证用） |
| `hydro` | 生成水面 DEM | `generate_hydro_water_dem` |
| `fclass` | 水体 fclass 语义分类 | `assign_water_fclass` |
| `edge-depth` | 水边深度导出 | `export_water_edge_depth` |

提供 **CLI**、**REST API** 与 **桌面 GUI** 三种入口。API 形态仿照 `GeoAI_Toolkit/workshop`；
GUI 为纯 Rust（eframe/egui），直接链接业务 crate，页面布局参考 `MyProject` 的 water 页。

## 工程结构

```text
apps/
  water_cli/         # CLI 主入口（bin: water2rust）
  water_api/         # Axum REST API（默认 127.0.0.1:8000）
gui/                 # 桌面 GUI（bin: water2rust-gui，eframe/egui，纯 Rust）
crates/
  water-core/        # 错误 / 配置 / 栅格算法（替代 numpy/scipy/skimage）
  water-io/          # 栅格·矢量 IO（经 eci-gdal）
  water-fclass/      # 水体分类
  water-hydro/       # 水面 DEM 生成
  water-edge-depth/  # 水边深度导出
  eci-gdal/          # 纯 Rust GDAL（git submodule）
```

## 前置准备

本仓库目前是**骨架阶段**，构建前需完成两项准备（详见 [AGENTS.md](AGENTS.md)）：

1. **拉取 eci-gdal submodule**（纯 Rust GDAL，已作为本仓库 submodule 接入于 `crates/eci-gdal`）：

   ```bash
   git clone --recursive <Water2Rust 地址>
   # 或克隆后补拉：
   git submodule update --init --recursive
   ```

2. **安装 Rust 工具链**（本机尚未检测到 `cargo`；eci-gdal 为 edition 2024，需 **Rust ≥ 1.85**）：

   ```powershell
   winget install Rustlang.Rustup
   rustup default stable
   ```

## 构建与运行

```powershell
# 构建（release，opt-level=2）
cargo build --workspace --release

# 正式产物（prod，opt-level=3 + LTO）
cargo build --workspace --profile prod

# CLI
cargo run --release -p water_cli -- hydro --dem dem.tif --water water.shp --output waters.tif
# 注：hydro 强制要求输入齐全——DEM 与**已分类(含 fclass 字段)**的水体缺一不可；
#     若水体尚未分类，请先运行 fclass 流程。

# fclass：按水体参考库（GeoPackage）为水体多边形赋予语义类别（river/lake/sea/...）
cargo run --release -p water_cli -- fclass --water water.shp --output classified.shp --reference-path waters_china.gpkg

# edge-depth：为已分类水体按 fclass 追加 edgeexpand/depth 字段
cargo run --release -p water_cli -- edge-depth --water classified.shp --output with_depth.shp

# 轻量验证：读水体矢量 + DEM 元数据，采样质心高程并导出 GeoJSON
cargo run --release -p water_cli -- inspect --water waters.shp --dem dem.tif --output out.geojson

# API（http://127.0.0.1:8000，OpenAPI 端点见 routes）
cargo run --release -p water_api

# 桌面 GUI（纯 Rust，eframe/egui）
cargo run --release -p water_gui
```

### 桌面 GUI

`water_gui`（bin `water2rust-gui`）是纯 Rust 图形界面，直接链接业务 crate，无子进程、无 Python。
页面布局参考 `MyProject` 的 water 页：左侧参数区（水体输入 / 输出基名 / DEM / 分类参考库 + 任务勾选），
右侧实时日志（由 `tracing` 汇入）。

- 任务勾选：`fclass` / `edge` / `hydro`。**`fclass` 是 `edge` / `hydro` 的前置条件，自动注入**。
- 输出基名派生产物：`<名>_fclass.shp`、`<名>_edge.shp`、`<名>_hydro.tif`。
- 分类参考库默认指向 `waters_china.gpkg`，可在界面替换；`hydro` 需填 DEM。
- Windows 自动加载微软雅黑等系统字体以正确显示中文。

### REST API 端点

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| `POST` | `/api/v1/tasks/hydro` | 提交水面 DEM 任务 |
| `POST` | `/api/v1/tasks/fclass` | 提交 fclass 分类任务 |
| `POST` | `/api/v1/tasks/edge-depth` | 提交水边深度任务 |
| `GET` | `/api/v1/tasks/{task_id}` | 轮询任务状态（权威终态） |
| `GET` | `/api/v1/events/status/{task_id}` | SSE 实时进度 |
| `GET` | `/api/v1/reports/{task_id}` | 任务结果报告 |
| `GET` | `/health` | 健康检查 |

## 改造进度

- [x] 工程骨架 + AGENTS.md / CLAUDE.md + CLI/API 框架
- [x] 接入 eci-gdal（git submodule，作为 workspace 成员）
- [x] `water-io` 真实 IO：DEM 惰性元数据/采样（eci-gdal-geotiff）+ 矢量含属性读取（eci-gdal-vector）+ GeoJSON 导出
- [x] 轻量 `inspect` 流程，已用林芝真实数据验证（读 41 个水体多边形 + 1.9GB DEM采样，高程 826–4580m）
- [ ] `water-core::raster_ops` 数值算法（与 scipy/skimage 对拍）
- [x] `water-fclass`（水体语义分类，41/41 要素与 Python 逐一致，见 [docs/FCLASS.md](docs/FCLASS.md)）
- [x] `water-edge-depth`（水边深度，每 fclass 可配置，见 [docs/EDGE_DEPTH.md](docs/EDGE_DEPTH.md)）
- [x] `water-hydro`（水面 DEM，最大，见 [docs/HYDRO.md](docs/HYDRO.md)）
- [x] 桌面 GUI `water_gui`（纯 Rust eframe/egui，串联 fclass/edge/hydro，布局参考 MyProject water 页）
