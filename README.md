# Water2Rust

将 `MyProject` 的 Python `waters` 模块改造为**纯 Rust** 的水体处理工具链。
**不依赖** `gdal` / `fiona` / `shapely` 等 Python 第三方库，GIS 底层能力统一经
[`eci-gdal`](crates/eci-gdal)（纯 Rust GDAL，git submodule）引入。

> 工作准则与架构规则以 [AGENTS.md](AGENTS.md) 为唯一来源。AI 助手请先读取 AGENTS.md。

## 能力

| 子命令 | 能力 | 对应原 Python |
| --- | --- | --- |
| `hydro` | 生成水面 DEM | `generate_hydro_water_dem` |
| `fclass` | 水体 fclass 语义分类 | `assign_water_fclass` |
| `edge-depth` | 水边深度导出 | `export_water_edge_depth` |

提供 **CLI** 与 **REST API** 两种入口。API 形态仿照 `GeoAI_Toolkit/workshop`。

## 工程结构

```text
apps/
  water_cli/         # CLI 主入口（bin: water2rust）
  water_api/         # Axum REST API（默认 127.0.0.1:8000）
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

# API（http://127.0.0.1:8000，OpenAPI 端点见 routes）
cargo run --release -p water_api
```

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
- [ ] 落地 `water-io` 真实栅格/矢量 IO（调用 eci-gdal-geotiff/vector/proj）
- [ ] `water-core::raster_ops` 数值算法（与 scipy/skimage 对拍）
- [ ] `water-fclass` / `water-hydro` / `water-edge-depth` 业务逻辑
