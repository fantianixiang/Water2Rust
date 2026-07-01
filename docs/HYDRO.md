# HYDRO — 水面 DEM 生成（`water-hydro`）

将原 Python `modules/waters/hydro/`（`generate_hydro_water_dem`）忠实复刻为纯 Rust。
本文件记录**分阶段实现进度**与**每个被替换算法的数值容差与对拍证据**（AGENTS.md 要求）。

## 算法总览（原 Python 流水线）

输入 DEM(GeoTIFF) + 水体多边形 → 输出水面 DEM(GeoTIFF)。核心阶段：

1. CRS 解析：优先 DEM 原生投影 CRS；地理坐标(如 4326)回退到局地 UTM，处理后再投回源栅格。
2. 读取水体域 + fclass，计算并集，按并集范围裁 ROI（5% padding，下限 64px）。
3. 预算判定：ROI 像素 > `HYDRO_MAX_FULL_RASTER_PIXELS`(2.5e8) 走瓦片流水线，否则整幅内存处理。
4. **逐多边形 Laplace 求解**（河流）：5 点差分 ∇²z=0 + Dirichlet 边界（岸线环 + 河心 pin），稀疏直接解。
5. **湖泊压平**：迭代截尾中位数求常数水位，覆盖求解面。
6. **河床抬升**：河道内像素若解低于 DEM 则夹回 DEM；漫滩像素保留。
7. 输出组合：`water_surface_only` / `water_surface_with_dem` 两种模式；必要时重投影回源网格并写出。

## 对拍方法

- 参考实现：`myproject_py310` 环境下 MyProject 的**真实** Python 函数。
- 夹具生成：`scripts/parity/gen_laplace_fixtures.py` 调用真实 Python 函数产出输入+期望输出 JSON。
- Rust 比对：`crates/water-hydro/tests/*_parity.rs` 加载夹具、运行 Rust 实现、逐像素比对。

## 实现进度与对拍证据

### 阶段 1：Laplace Dirichlet 求解器 ✅（已对拍）

- Rust：[crates/water-hydro/src/laplace.rs](../crates/water-hydro/src/laplace.rs) `solve_laplace_dirichlet`
- 对应 Python：`hydro/hydro_laplace.py::solve_laplace_dirichlet`
- 实现说明：Python 组装对称负定系统 `A z = rhs` 后用 `scipy.sparse.linalg.spsolve` 直接解；
  Rust 改解等价 SPD 系统 `M z = b`（`M = -A`），用 `nalgebra-sparse` 的 `CscCholesky` 直接分解。
  线性系统解唯一，故两者在数值容差内一致。
- 测试：[crates/water-hydro/tests/laplace_parity.rs](../crates/water-hydro/tests/laplace_parity.rs)（5 个构造用例）
- **对拍证据（最大绝对误差 vs scipy spsolve）**：

  | 用例 | 尺寸 | 最大绝对误差 |
  | --- | --- | --- |
  | rect_linear（线性平面调和延拓） | 12×16 | 1.85e-13 |
  | rect_lr（左右边界一维插值） | 10×20 | 2.98e-13 |
  | blob_ring_random（圆盘环边界随机） | 24×24 | 2.84e-13 |
  | interior_pin_line（内部 Dirichlet 线） | 18×30 | 1.95e-14 |
  | irregular_random（不规则连通域） | 20×28 | 1.31e-12 |
  | **总体** | — | **1.31e-12** |

  容差判据 `< 1e-6`，实测机器精度量级（~1e-12）。

### 阶段 2：湖泊常数水位内核 `iterative_trimmed_median` ✅（已对拍）

- Rust：[crates/water-hydro/src/lake.rs](../crates/water-hydro/src/lake.rs) `iterative_trimmed_median`
- 对应 Python：`hydro/hydro_lake_flatten.py::_iterative_trimmed_median`
- 说明：迭代丢弃高于当前中位数的值再重算中位数（`values <= z` 即等高线方程，无阈值），
  numpy 风格中位数（偶数取中间两者均值）。完整逐多边形/连通体常数水位（依赖栅格化 + 岸线环腐蚀）留待阶段 5。
- 测试：[crates/water-hydro/tests/hydro_stage234_parity.rs](../crates/water-hydro/tests/hydro_stage234_parity.rs)（6 例）
- **对拍证据**：最大绝对误差 **2.84e-14**（含高离群/悬崖侵入/含 NaN 用例，判据 1e-9）。

### 阶段 3：河床抬升 `apply_river_dem_floor_lift`（数组逻辑）✅（已对拍）

- Rust：[crates/water-hydro/src/postprocess.rs](../crates/water-hydro/src/postprocess.rs) `apply_river_dem_floor_lift`
- 对应 Python：`hydro/hydro_raster_postprocess.py::apply_river_dem_floor_lift`
- 说明：忠实复刻漫滩判定（`river ∩ dem>surface`）与河道内抬升（`write ∩ surface<dem ∩ ~lake ∩ ~overbank → =dem`）。
  原函数内部的多边形栅格化留待阶段 5；本阶段以 Python 用相同 `rasterize(all_touched)` 复算并 dump 的
  `river_mask`/`lake_mask` 作为 Rust 输入，隔离栅格化专测数组逻辑。
- **对拍证据**：3 例（含全高于 DEM、含 NaN 水面），返回计数 `(n_lifted,n_lake_excluded,n_overbank)`
  与抬升后数组逐像素**完全一致**（如 mixed 例 `(37,20,20)`）。

### 阶段 4：输出组合 `compose_water_output_array` ✅（已对拍）

- Rust：[crates/water-hydro/src/output.rs](../crates/water-hydro/src/output.rs) `compose_water_output_array`
- 对应 Python：`output.py::compose_water_output_array`
- 说明：完全自包含。`water_surface_with_dem` 先填底 DEM 再覆盖水面；`water_surface_only` 仅写水面。
  同时复刻 5 项 metrics 计数。f32 拷贝无算术，逐像素位一致。
- **对拍证据**：4 例（两模式、含 DEM NaN、含空洞水体），输出数组 + 全部 5 项 metrics **完全一致**。

### 阶段 5（部分）：多边形栅格化 + 形态学腐蚀 ✅（已对拍）

- **binary_erosion**：[crates/water-core/src/raster_ops.rs](../crates/water-core/src/raster_ops.rs) `binary_erosion`
  - 对应 scipy 默认 `binary_erosion(mask, iterations=n)`：4 邻域十字结构、`border_value=0`。
  - 对拍：6 例（实心矩形/随机块/圆盘/细条，1~3 次迭代）与 scipy **逐像素 0 不一致**。
- **多边形栅格化(all_touched=False)**：[crates/water-io/src/raster.rs](../crates/water-io/src/raster.rs) `rasterize_polygon_mask`
  - 经 `eci-gdal-alg::rasterize_scope_mask`（扫描线 even-odd、像素中心采样，对标 GDAL burn-value）。
  - 对拍 rasterio(all_touched=False) 5 例：rect/triangle/pentagon/rect_with_hole **精确一致**；
    `slanted`（分数顶点斜边）1/504 像素差异。
  - **容差**：整数/轴对齐多边形精确一致；分数斜边因像素中心恰压边的浮点 tie-break，
    每多边形容许 ≤1 个边界像素、占比 < 0.5%。rasterio 在世界坐标做扫描线、eci-gdal 在像素坐标做，
    代数等价但 ~1e-13 舍入在压边处会翻转一个边界像素；对水面掩膜物理影响可忽略（连不同 GDAL 版本亦有此差异）。
  - 测试：[crates/water-hydro/tests/stage5_parity.rs](../crates/water-hydro/tests/stage5_parity.rs)
- **多边形栅格化(all_touched=True)**：[crates/water-io/src/raster.rs](../crates/water-io/src/raster.rs) `rasterize_polygon_mask(.., all_touched=true)`
  - eci-gdal 不提供 all_touched=True。**GeoRust 调研**：`geo-rasterize` 是 GDAL 直接移植（匹配 ALL_TOUCHED=TRUE），
    但其依赖 `geo ^0.18` / `ndarray ^0.15` / `euclid ^0.22`，接入会引入重复旧版本，与 AGENTS「最小依赖」冲突。
  - 按 AGENTS 核心第一定律「参考 GDAL 源码以纯 Rust 补齐」：**忠实移植 GDAL `alg/llrasterize.cpp` 的
    `GDALdllImageLineAllTouched`**（含 `bIntersectOnly=TRUE` 对轴对齐整数边的 EPSILON 跳过，gdal #7523/#6414），
    与 eci-gdal 的内部填充求并即得 GDAL `ALL_TOUCHED=TRUE`。
  - 关键细节：世界→像素坐标采用 GDAL 逆地理变换的**求值顺序** `(-c/a)+x*(1/a)`，避免 `(x-c)/a` 在整数顶点处因舍入落到 1.999…。
  - **对拍证据**：5 多边形（rect/triangle/pentagon/rect_with_hole/slanted）与 rasterio(all_touched=True) **全部逐像素 0 不一致**（含分数斜边）。

### 输入检查（按需求新增）

`generate_hydro_water_dem` 入口 `validate_hydro_inputs`（[lib.rs](../crates/water-hydro/src/lib.rs)）：
**强制要求 DEM 与已分类(fclass)水体齐全**——DEM/水体文件须存在，且水体矢量须含 `fclass` 字段；
缺失则明确报错并提示先运行 fclass 流程。已在林芝真实数据验证：`waters.shp`(无 fclass) 被正确拒绝。

### 阶段 5 续：湖泊 DEM 观测（内部/岸线环中位数）端到端 ✅（已对拍）

首个**完整 Python 函数级**端到端复刻——组合窗口计算 + 栅格化(all_touched=False) + 腐蚀取环 + （截尾）中位数：

- Rust：[crates/water-hydro/src/lake.rs](../crates/water-hydro/src/lake.rs)
  `sample_polygon_interior_dem_median` / `sample_polygon_boundary_ring_dem_median`
- 对应 Python：`hydro/hydro_lake_flatten.py::_sample_polygon_interior_dem_median` / `_sample_polygon_boundary_ring_dem_median`
- 复刻要点：多边形 bbox 四角反算局部窗口（各向外扩 1 px、夹到栅格），`local_transform = transform·translation(col_off,row_off)`；
  内部 = 整掩膜中位数；岸线环 = `掩膜 ∩ ~腐蚀(掩膜)`（过细多边形回退整掩膜）后迭代截尾中位数；有限值过滤（NaN/nodata）。
- 测试：[crates/water-hydro/tests/stage5b_parity.rs](../crates/water-hydro/tests/stage5b_parity.rs)（5 多边形，含带洞、极扁回退、悬崖高脊）
- **对拍证据**：内部与岸线环的中位数、像素数**全部 diff = 0（精确一致）**（整数像素对齐，规避栅格化 tie-break）。

### 阶段 5c：湖泊路径端到端（常数水位 + 压平回盖）✅（已对拍）

- Rust：[crates/water-hydro/src/lake_flatten.rs](../crates/water-hydro/src/lake_flatten.rs)
  `is_lake_fclass` / `compute_lake_constant_z_for_polygon` / `compute_lake_constant_z_for_component` / `flatten_lake_polygons_on_surface`
- 对应 Python：`hydro/hydro_lake_flatten.py` 同名函数
- 复刻要点：按 fclass 过滤湖泊多边形（河流忽略）；按 component 分组（无 component 者各自成孤立组）；
  单多边形取岸线环截尾中位数(回退内部中位数)，多多边形组汇集各多边形环样本再截尾中位数(共享一个水位，消除碎片接缝台阶)；
  栅格化用 **all_touched=True**，`surface[mask] = constant_z`(f32) 就地覆盖。
  说明：原 Python 分层还含 tier 1/2（求解节点 z / 剖面样本 z），当前**无河网**流水线中恒为空，故实现 DEM 观测的 tier 0/3（与无网络运行等价）。
- 测试：[crates/water-hydro/tests/stage5c_parity.rs](../crates/water-hydro/tests/stage5c_parity.rs)（孤立湖 + component 分组两场景，含非湖多边形忽略、悬崖高脊）
- **对拍证据**：与真实 Python `_flatten_lake_polygons_on_surface` 对拍——压平后 surface 逐像素一致、
  summary 计数（lake/filled/skipped/filled_pixel）全等、逐多边形常数水位全等（如 component 场景碎片共享 1046.5）。

### 阶段 6a：精确欧氏距离变换 EDT ✅（已对拍）

河流路径的基础原语（河道半宽、最近骨架像素均依赖它）。

- Rust：[crates/water-core/src/raster_ops.rs](../crates/water-core/src/raster_ops.rs) `distance_transform_edt`
- 对应 Python：`scipy.ndimage.distance_transform_edt`（含 `return_indices`）
- 实现：Felzenszwalb–Huttenlocher 两遍（列 + 行）**精确**平方距离变换（下包络法），并跟踪最近背景像素的行/列索引。
- 测试：[crates/water-hydro/tests/stage6_edt_parity.rs](../crates/water-hydro/tests/stage6_edt_parity.rs)（rect/disk/ring/random/thin_line/single_bg）
- **对拍证据**：距离场与 scipy **精确一致**（最大误差 1.78e-15，sqrt 机器精度）；最近特征索引校验为
  「所选特征确为背景像素且欧氏距离等于 scipy 距离」（并列平手时允许选不同的等距特征）。

### 阶段 6b：中轴骨架 medial_axis ✅（已对拍，固定随机种子）

- Rust：[crates/water-core/src/raster_ops.rs](../crates/water-core/src/raster_ops.rs) `medial_axis`
- 对应 Python：`skimage.morphology.medial_axis`（0.25.2）
- 算法：512 项查表（`keep = 中心前景 且 (去掉中心改变 8 连通分量数 或 邻域前景<3)`）+ 距离变换（阶段 6a EDT）
  + cornerness（`9 - 邻域前景数`）+ 按 `(distance, corner_score, tiebreaker)` 升序单遍细化（`table[邻域index]==0` 则删）。
- **随机种子处理（关键）**：skimage 用 PCG64 随机 permutation 作并列 tiebreaker，**默认非确定性**。
  为可对拍，Rust `medial_axis` 接受**外部注入的 tiebreaker**；对拍时由 Python 用**固定种子** `rng=SEED`
  运行，并复现其内部同种子 permutation（`default_rng(SEED).permutation(arange(n))`）一并 dump，Rust 注入同一序列
  → 双方在**同一随机种子**下逐像素一致。
- 测试：[crates/water-hydro/tests/stage6b_medial_parity.rs](../crates/water-hydro/tests/stage6b_medial_parity.rs)
- **对拍证据**：6 例（文档方块 / 矩形 / 圆盘 / L 形 / 河道 blob / 随机团块）与 skimage **全部 0 不一致**。

### 后续阶段（待实现，逐一对拍）

- [ ] 阶段 6c：河心线横断面 z + isotonic + 河流 Laplace 组装（`solve_laplace_per_polygon`：岸线环 + 河心 pin 作 Dirichlet）
- [ ] 阶段 7：CRS 解析 + 重投影（工作 CRS ↔ 源网格）
- [ ] 阶段 8：ROI/瓦片流水线与并行
- [ ] 阶段 9：端到端在林芝真实数据上与 Python 整体对拍
