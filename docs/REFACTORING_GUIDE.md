# MyProject → 纯 Rust 重构方法论（Refactoring Playbook）

> 本文沉淀自 `waters` 模块（fclass / hydro / edge-depth）从 Python 改造为纯 Rust 的完整经验，
> 供后续把 MyProject 其它模块（`building`、`building_extract`、`geometry_pretraining`、
> `poi`、`remote_sensing`、`road`、`utils`）改造为纯 Rust 时**直接照搬流程**。
>
> **适用前提**：纯 CPU 纯 Rust 路径（对应 `rust/shadcn` 血脉）。GPU 加速是**独立、可选**的后续话题
> （见 `GPU/Project` 分支与 [docs/CUDA.md](CUDA.md)），本次改造**不涉及 GPU**。
>
> 唯一规则源头仍是 [AGENTS.md](../AGENTS.md)；本文是把其中原则落到「如何改一个新模块」的操作手册。

---

## 0. 一句话心法

> **纯 Rust、零 C/Python 绑定优先；GIS 能力必经 eci-gdal；每一步算法都要与原 Python 数值对拍并留证据。**
> 编译通过 ≠ 完成；**拿到对拍证据才算完成**。

---

## 1. 第一性原理与铁律（不可违背）

1. **纯 Rust，零绑定优先**：所有业务 crate 纯 Rust。能力缺口时参考 GDAL/scipy 源码用纯 Rust 补齐，**NEVER 退回 Python 库**。
2. **禁止引入 Python 第三方 GIS 库**：`gdal`/`fiona`/`shapely`/`geopandas`/`rasterio`/`pyproj` 一律不引入。
3. **GIS 底层能力必须经 `eci-gdal` 子 crate 引入**，不得直接依赖底层库（见 §4 映射表）。
4. **避免造轮子**：能用库就用库（GIS 库清单见 <https://georust.org/>）；GIS 行业无现成纯 Rust 库时才自研（并对拍）。
5. **代码体量纪律**：单 `.rs` > 300 行考虑拆文件；单方法 > 50 行需反思。
6. **改造保真**：Rust 与原 Python 在相同输入下产生**等价结果**，每个被替换算法记录数值容差与对拍证据。
7. **面向用户变更必更新文档**：能力变更更新 `README.md` + 对应 `docs/<模块>.md`。

### 已获豁免的通用库（可直接用，非 GIS 底层）
| 库 | 替代的 Python | 用途 |
| --- | --- | --- |
| `geo` / `geo-types` | shapely 几何 | 通用几何类型与算子 |
| `ndarray` | numpy | N 维数组 |
| `rstar` | scipy.spatial.cKDTree | 空间索引（R\*-tree） |
| `rusqlite`（libsqlite3-sys, bundled） | — | **唯一 C 豁免**，仅 GeoPackage 读取；需本机 C 工具链 |

---

## 2. 能力映射表（Python → Rust）★ 改造前先查此表

| 原 Python 能力 | Rust 落点 | 说明 |
| --- | --- | --- |
| GeoTIFF / DEM 栅格读写（gdal, rasterio） | **`eci-gdal-geotiff`** | 经 `water-io` 封装调用 |
| 投影 / CRS 变换（pyproj, rasterio.warp） | **`eci-gdal-proj`** | 底层 proj4rs/proj4wkt |
| 矢量解析 Shapefile/GeoJSON/GeoPackage（fiona, geopandas, shapely IO） | **`eci-gdal-vector`** | GeoPackage 读取走 rusqlite |
| 重采样 / 栅格化 / 色彩（rasterio） | **`eci-gdal-alg`** | warp 近似变换器、rasterize |
| 通用几何运算（shapely） | `geo` / `geo-types` | 面积、缓冲、相交等 |
| 数组运算（numpy） | `ndarray` | |
| KD 树 / 最近邻（scipy.spatial） | `rstar` | |
| 稀疏线性求解（scipy.sparse.linalg） | `faer` | **务必带 fill-reducing 重排序**（见 §7 坑） |
| 形态学/高斯/距离变换（scipy.ndimage） | **自研**，放 `water-core::raster_ops` / `edt.rs` | 必须与 scipy 对拍 |
| 骨架提取 skeletonize（skimage.morphology） | **自研**，放 `water-core` | 必须与 skimage 对拍 |
| CPU 并行（multiprocessing/线程） | `rayon`（计算）/ `tokio`（I/O） | 瓦片级并行优先 |
| 配置常量（settings.py） | `water-core::settings`（或各模块 settings 模块） | 常量逐一搬运并对齐 |

> **新模块若出现表中没有的能力**：先判断 georust 生态有无现成纯 Rust 库；有则用（版本与 `[workspace.dependencies]` 对齐），
> 无则自研到 `*-core` 并补对拍。**任何情况都不得退回 Python 库或加 GDAL C 绑定。**

---

## 3. 工程结构与新模块落位

### 3.1 现有 workspace 布局
```
Cargo.toml                 # workspace root（成员、依赖版本、profile 双轨）
apps/
    water_cli/             # CLI 主入口（clap），bin = water2rust
    water_api/             # Axum REST API（仿 GeoAI workshop：tasks/reports/events + SSE）
gui/                       # 纯 Rust 原生 GUI（egui/eframe，可选）
desktop/src-tauri/         # Tauri + React GUI（shadcn，可选）
crates/
    water-core/            # 共享：错误 / 配置(settings) / 几何与栅格算法(raster_ops, edt)
    water-io/              # 栅格/矢量 IO，统一封装 eci-gdal 调用
    water-fclass/          # 业务模块示例（分类）
    water-hydro/           # 业务模块示例（最大，含分阶段解算）
    water-edge-depth/      # 业务模块示例
    eci-gdal/              # 纯 Rust GDAL（git submodule，作为 workspace 成员编译）
    twe-tile/              # eci-gdal proj 的 tile 特性依赖（vendored）
```

### 3.2 新增一个业务模块 crate 的步骤
以把 `modules/road` 改造为 `water-road`（示意，命名可按域调整，如 `geoai-road`）为例：

1. **建目录**：`crates/<new-crate>/{Cargo.toml, src/lib.rs}`（+ 后续 `src/<子模块>.rs`、`tests/`）。
2. **注册 workspace 成员**：在根 `Cargo.toml` 的 `[workspace] members` 加入 `"crates/<new-crate>"`；
   若要能被 `cargo build`（无 `--workspace`）默认编到，酌情加进 `default-members`。
3. **声明内部依赖别名**：在根 `[workspace.dependencies]` 加 `<new-crate> = { path = "crates/<new-crate>" }`。
4. **crate 的 `Cargo.toml` 模板**（照抄 water-fclass）：
   ```toml
   [package]
   name = "<new-crate>"
   version.workspace = true
   edition.workspace = true
   license.workspace = true

   [dependencies]
   water-core.workspace = true      # 复用错误/配置/栅格算子
   water-io.workspace = true        # 栅格/矢量 IO（经 eci-gdal）
   anyhow.workspace = true
   geo.workspace = true
   geo-types.workspace = true
   rstar.workspace = true           # 需空间索引时
   rayon.workspace = true           # 需并行时
   ndarray.workspace = true         # 需数组时
   serde_json.workspace = true
   tracing.workspace = true
   # 仅在确有需要时再加 eci-gdal-* 子 crate（一般经 water-io 间接使用）
   ```
5. **分层原则**：
   - **通用可复用**（错误、配置、纯栅格/几何算子）→ 放 `water-core`（跨模块共享）。
   - **IO**（读 DEM/矢量、写结果）→ 一律经 `water-io`（内部调 eci-gdal），业务 crate 不直接碰 eci-gdal-geotiff/vector。
   - **业务算法** → 放本模块 crate。
6. **接入入口**：CLI 加 `water_cli` 子命令；API 加 `water_api` 路由；（GUI 可选）。

---

## 4. 对拍验证方法论（本方法论的核心，务必照做）

> 「改造保真」= 每个被替换的算法都要有**与原 Python 数值等价的证据**。参考 `water-hydro/tests/` 的 20+ 个 `*_parity.rs`。

### 4.1 分阶段分解（stage-by-stage）
把 Python 算法按数据流拆成**可独立验证的阶段**（如 hydro：warp → EDT → 排序 → 横断面 → 等渗 → 高斯 → Laplace → 组合）。
每个阶段一个 parity 测试，逐段锁定，避免「整体不一致但不知哪出错」。

### 4.2 生成 Python 黄金基准（golden fixtures）
- 在 `scripts/` 写 Python 脚本，用**原 MyProject 模块**在固定输入上产出中间量/最终结果，落盘为 fixture：
  - 结构化数值 → `.json` / `.npy` / `.bin`；栅格 → `.tif`；矢量 → `.shp`/`.geojson`。
  - 存到 `crates/<crate>/tests/fixtures/`。
- 参考现有 `scripts/`：`cmp_fclass.py`、`bench_laplace_real.py`、`run_py_hydro_single.py` 等。

### 4.3 Rust 侧对拍测试
- 在 `crates/<crate>/tests/<stage>_parity.rs`：加载同一输入 + Python fixture，运行 Rust 实现，比对并断言容差。
- **容差分级**（在测试与文档里写明用哪一档）：
  | 档 | 判据 | 适用 |
  | --- | --- | --- |
  | 逐位一致 | MD5 / bit-exact | 确定性整数/查表/纯搬运逻辑、全流程回归 |
  | < 1e-6 | max_abs / max_rel | 浮点数值算法（求解器、变换）★ 默认标准 |
  | mm / 亚微米级 | 物理量级 | 高程/坐标等有物理意义的量 |
- **全流程对拍**：除分阶段外，跑一次端到端并与 Python 最终产物比 MD5（如 hydro 的 `waters_hydro.tif` 黄金基准）。

### 4.4 测试执行规范
- **所有测试用 release build**（`cargo test --workspace --release`），不要用 debug（数值/性能都可能不同）。
- 没有现成测试时，**先思考测试方法**（与 Python 对拍是首选），再写实现。

---

## 5. 入口形态（CLI + API +（可选）GUI）

- **CLI**（`apps/water_cli`，clap derive）：每个业务能力一个子命令（`inspect`/`hydro`/`fclass`/`edge-depth` 是范例）。
  bin 名统一 `water2rust`。参数用 `--kebab-case`，`--help` 中文描述。
- **REST API**（`apps/water_api`，axum + tokio）：形态仿 GeoAI `workshop`（tasks/reports/events + SSE 进度）。
- **GUI**（可选）：`gui/`（egui/eframe 纯 Rust）或 `desktop/`（Tauri + React）。新模块不强制做 GUI。

---

## 6. 构建、测试、性能规范

```powershell
# 日常构建（release：opt-level=2，无 LTO，编译快）
cargo build --workspace --release
# 正式产物（prod：opt-level=3 + LTO fat + codegen-units=1）
cargo build --workspace --profile prod
# 全部测试（release）
cargo test --workspace --release
# 跑 CLI / API
cargo run --release -p water_cli -- --help
cargo run --release -p water_api
```

- **Profile 双轨**（根 `Cargo.toml`，勿随意改）：`release`（日常/冒烟/`cargo test`）vs `prod`（基准/正式）。
- **性能数据必须标注 profile**（release vs prod 差异可达数倍，混用会得错误结论）。
- **无畏并发**：`rayon`（CPU）+ `tokio`（I/O），**瓦片级并行优先**，保证单文件输出足够快。

---

## 7. 关键库经验与踩坑速查（lessons learned）

- **faer 稀疏求解务必带 fill-reducing 重排序**：`nalgebra-sparse` 的 `CscCholesky` 无重排序，大稀疏系统填充 O(n³) **内存爆炸**（本项目历史最大坑）。`faer` 0.24 内置 AMD 重排序，用它。
- **eci-gdal 版本对齐**：根 `[workspace.dependencies]` 中 `geo`/`geo-types` 等必须与 eci-gdal 来源仓库（AesMetaTool）**同版本**，跨 crate 传几何类型才不冲突；已复刻 `[patch.crates-io] tiff = tiff-patch`（GeoTIFF 兼容补丁），勿动。
- **CRS 识别**：ESRI WKT 与 EPSG 的匹配交给 `eci-gdal-proj`；日志会打印 `CRS matched ... → EPSG:xxxx`，对拍前先确认源/目标 EPSG 一致。
- **自研 scipy/skimage 替代放 `water-core`**：形态学/高斯/距离变换(EDT)/骨架 skeletonize，全部要有 scipy/skimage 对拍测试（见 `tests/stage6*_parity.rs`）。
- **换行符**：仓库在 Windows 上编辑易触发 LF↔CRLF 警告；提交前 `git diff --stat` 确认不是纯换行改动混入。
- **跨平台数值/编译**（即使暂不做 GPU 也需注意）：Windows/MSVC 与 Linux 的 libm 常量、默认宏不同（如 `M_PI` 在 MSVC 下需 `_USE_MATH_DEFINES`）；纯 Rust 代码基本无此问题，涉及 C/CUDA 才有。
- **submodule**：eci-gdal 是 git submodule，克隆用 `git clone --recursive` 或 `git submodule update --init --recursive`。

---

## 8. 文档与 Git 规范

- **每个模块一份 `docs/<模块大写>.md`**：技术规格 + 对 Python 的数值对拍报告（参考 `HYDRO.md`/`FCLASS.md`/`EDGE_DEPTH.md`）。
- **能力/参数/行业知识变更**：同步更新 `README.md` 与对应模块文档。
- **commit message 中文、面向用户**：说明关键参数修正、行业知识层面的修改、使用者能力变更；**禁止写成改动代码的流水账**。
- **Git 流程**：`git add -A && git commit -m "<中文说明>" && git push`（远端 origin=GitHub，geoai=内网）。
- **NEVER 全局杀进程**：按 PID 精准关闭，禁止按 image-name 杀（会误杀用户其它 Node/Cargo/Python 实例）。

---

## 9. 新模块落地 Checklist（逐项打勾）

- [ ] 读原 Python 模块，画出**数据流/阶段图**，列出每个阶段用到的 Python 库能力。
- [ ] 对照 §2 映射表，确定每个能力的 Rust 落点；有缺口先找 georust 库，无则规划自研 + 对拍。
- [ ] 建 crate、注册 workspace 成员与依赖别名（§3.2）。
- [ ] 搬运 `settings.py` 常量到 `*-core::settings`，逐一核对数值。
- [ ] 分阶段实现，每阶段：Python 出 golden fixture → Rust `*_parity.rs` 对拍（§4）。
- [ ] 端到端全流程对拍（MD5 / 容差）。
- [ ] 接 CLI 子命令（+ 可选 API 路由）。
- [ ] 写 `docs/<模块>.md`（规格 + 对拍报告）+ 更新 `README.md`。
- [ ] `cargo test --workspace --release` 全绿；`git commit`（中文面向用户）+ push。

---

## 10. 待改造模块清单与初步预判

> 正式改造前需先读各模块 Python 源码确认，以下为**基于目录名的初步预判**，供起步参考。

| MyProject 模块 | 预判性质 | 可能主要能力（待核实） | 大概率 Rust 落点 |
| --- | --- | --- | --- |
| `utils/` | 通用工具 | IO/几何/数组辅助 | 拆入 `water-core` / `water-io` 复用 |
| `poi/` | 矢量点处理 | 点要素读写、空间查询、属性 | `eci-gdal-vector` + `rstar` + `geo` |
| `road/` | 矢量线网 | 线要素、拓扑、缓冲、栅格化 | `geo` + `eci-gdal-vector`/`-alg` |
| `building/` | 建筑面 | 多边形处理、栅格化 | `geo` + `eci-gdal-alg` |
| `building_extract/` | 从栅格提取建筑 | 影像分割/形态学/矢量化 | 自研 raster_ops + `eci-gdal-alg`（可能重难点） |
| `remote_sensing/` | 遥感栅格 | 波段运算、重采样、指数 | `eci-gdal-geotiff`/`-proj`/`-alg` + `ndarray` |
| `geometry_pretraining/` | 几何/训练数据 | 几何变换、样本生成 | `geo` + `ndarray`（需先看用途） |

> **起步建议顺序**：先 `utils`（沉淀公共能力到 core/io，为后续铺路）→ 再挑一个 IO 密集但算法简单的（如 `poi`）跑通全流程范式 → 再攻算法重的（`building_extract` / `remote_sensing`）。

---

### 附：关键参考文件
- [AGENTS.md](../AGENTS.md) — 唯一规则源头
- [docs/HYDRO.md](HYDRO.md) / [docs/FCLASS.md](FCLASS.md) / [docs/EDGE_DEPTH.md](EDGE_DEPTH.md) — 模块规格 + 对拍报告范例
- `crates/water-hydro/tests/*_parity.rs` — 分阶段对拍测试范例（照抄这套）
- `scripts/*.py` — Python 黄金基准生成脚本范例
- [docs/benchmark_py_vs_rust.md](benchmark_py_vs_rust.md) — Python vs Rust 性能对比范例
