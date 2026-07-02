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

### 阶段 6c 原语：gaussian_filter ✅（已对拍）

河流 z_local 管线的空间平滑（mask-aware 2D 高斯）用它。

- Rust：[crates/water-core/src/raster_ops.rs](../crates/water-core/src/raster_ops.rs) `gaussian_smooth`
- 对应 Python：`scipy.ndimage.gaussian_filter`（默认 mode='reflect'、truncate=4.0、order=0）
- 实现：可分离一维高斯核（`radius = floor(truncate*σ + 0.5)`，归一化）沿两轴依次相关；边界用 half-sample 'reflect'。
- 测试：[crates/water-hydro/tests/stage6c_gaussian_parity.rs](../crates/water-hydro/tests/stage6c_gaussian_parity.rs)
- **对拍证据**：7 例（冲激/随机/斜坡/小数组，σ=0.8~3.0）与 scipy 最大误差 **1.36e-12**（机器精度）。

### 阶段 6c 原语：等渗回归 + 秩滤波 ✅（已对拍）

供河流纵剖面平滑与空间 P30 先验使用。

- **等渗回归（PAVA）**：[crates/water-hydro/src/skeleton_zloc.rs](../crates/water-hydro/src/skeleton_zloc.rs)
  `isotonic_non_increasing` / `isotonic_non_decreasing`
  - 对应 Python：`hydro_skeleton_zloc.py::_isotonic_non_increasing`（Pool-Adjacent-Violators，NaN 跳过并保留）。
  - 测试 [stage6c_isotonic_parity.rs](../crates/water-hydro/tests/stage6c_isotonic_parity.rs)：11 例（含 NaN / 违反序 / 随机）最大误差 **6.82e-13**（加权均值机器精度）。
- **秩滤波**：[crates/water-core/src/rank_filter.rs](../crates/water-core/src/rank_filter.rs)
  `percentile_filter_2d` / `median_filter_1d`
  - 对应 Python：`scipy.ndimage.percentile_filter`（laplace 中 P30 空间先验）、`median_filter`（multi_peak 轻度平滑）。
  - 秩公式与 scipy `_rank_filter` 一致（percentile：`int(fs*p/100)`，100 时 `fs-1`；median：`fs//2`）；mode='reflect'。
  - 测试 [stage6c_rankfilter_parity.rs](../crates/water-hydro/tests/stage6c_rankfilter_parity.rs)：10 例（含 +inf 填充 / 多 size / 多 percentile）**0 误差**（秩选择取同一顺序统计量）。

### 阶段 6c 原语：find_peaks + 多峰等渗回归 ✅（已对拍）

河流纵剖面按「峰-谷」分段做等渗拟合，需先做峰检测。

- **峰检测（含显著度）**：[crates/water-core/src/find_peaks.rs](../crates/water-core/src/find_peaks.rs)
  `find_peaks_prominence`
  - 对应 Python：`scipy.signal.find_peaks(x, prominence=...)`（复刻 `_local_maxima_1d` 平顶取中 + `_peak_prominences` 显著度）。
  - 测试 [stage6c_findpeaks_parity.rs](../crates/water-hydro/tests/stage6c_findpeaks_parity.rs)：10 例（含平顶 / 单调 / NaN / 随机）峰下标**逐个精确**，显著度最大误差 **2.42e-13**。
- **多峰等渗回归**：[crates/water-hydro/src/skeleton_zloc.rs](../crates/water-hydro/src/skeleton_zloc.rs)
  `isotonic_multi_peak`
  - 对应 Python：`hydro_skeleton_zloc.py::_isotonic_multi_peak`（median_filter 轻度平滑 → find_peaks 找显著峰 → 端点+峰为锚点分段 → 段内峰→谷非增 / 谷→峰非减）。
  - 组合了 median_filter、find_peaks、isotonic 三原语。
  - 测试 [stage6c_multipeak_parity.rs](../crates/water-hydro/tests/stage6c_multipeak_parity.rs)：9 例（含 NaN / 多峰 / 河道形 / 随机）**0 误差**——同时作为 `median_filter` 含 NaN 的权威端到端对拍证据。

### 阶段 6c 原语：骨架切线 + 汇流站点检测 ✅（已对拍）

- **切线**：[crates/water-hydro/src/skeleton_zloc.rs](../crates/water-hydro/src/skeleton_zloc.rs) `compute_skeleton_tangents`
  - 对应 Python：`_compute_skeleton_tangents`（沿有序路径 ±context 有限差分，相邻跳变 `>sqrt(2)` 即停）。
- **汇流站点**：`detect_junction_stations`
  - 对应 Python：`_detect_junction_stations`（`radius_px=4` 内他站切线与本站切线点积绝对值 `<0.5` → 汇流；cKDTree 半径查询→暴力等价）。
- 测试 [stage6c_skeleton_geom_parity.rs](../crates/water-hydro/tests/stage6c_skeleton_geom_parity.rs)：8 例（直线/对角/L 拐/分支跳变/T 汇流/短路径）切线 **0 误差**，汇流掩膜**完全一致**。

### 阶段 6c 原语：骨架图构建 + 沿流排序 ✅（已对拍）

- Rust：[crates/water-hydro/src/skeleton_graph.rs](../crates/water-hydro/src/skeleton_graph.rs)
  `build_skeleton_graph` / `bfs_distance_to_outlet` / `normalise_branch_directions` / `order_skeleton_pixels_along_flow`
- 对应 Python：`skeleton_dryrun.py`（图构建）+ `hydro_skeleton_zloc.py::_order_skeleton_pixels_along_flow`
- 实现：度图 → 端点/交汇为节点（行主序索引）→ 沿度 2 像素走支 → 出水口=DEM 最低端点 →
  树边 BFS 定序 → 支按到出水口距离稳定排序 → 去重收集，出水口在前。
- 测试 [stage6c_skeleton_order_parity.rs](../crates/water-hydro/tests/stage6c_skeleton_order_parity.rs)：5 例（出水口左/右、Y 分叉、L 拐、短路径）排序**完全一致**。

### 阶段 6c 原语：横断面水位采样 ✅（已对拍）

- Rust：[crates/water-hydro/src/cross_section.rs](../crates/water-hydro/src/cross_section.rs)
  `cross_section_z_at_skeleton_pixels`（+ `trace_ray_to_boundary` / `trace_ray_to_contour`）
- 对应 Python：`hydro_skeleton_zloc.py::_cross_section_z_at_skeleton_pixels`
- 实现：每站沿切线法向左右投射射线，命中岸线/等高线，取 `min(左岸DEM, 右岸DEM)`；
  支持 EDT 半宽射线截断（`ceil(hw*1.5)`）、EDT≤1 用骨架 DEM、无命中回退骨架 DEM、contour 模式。
  保真复刻 `int(round(x))` 的 banker's rounding。
- 测试 [stage6c_xsec_parity.rs](../crates/water-hydro/tests/stage6c_xsec_parity.rs)：5 例（boundary-only /
  EDT 截断 / EDT≤1 / 短射线 / contour）z_cross **0 误差**、左右命中**完全一致**。

### 阶段 6c 收尾：河流水面数值核 ✅（已对拍）

**逐河流多边形水面求解的完整装配**——把 12 个原语串成一条链：

- Rust：[crates/water-hydro/src/river_solve.rs](../crates/water-hydro/src/river_solve.rs) `solve_river_polygon_surface`
- 对应 Python：`hydro_laplace.py::solve_laplace_per_polygon` 的**单多边形内层块**
- 链路：`medial_axis`(注入种子) → `distance_transform_edt` 半宽 → `order_skeleton_pixels_along_flow`
  → `compute_skeleton_tangents` → `detect_junction_stations` → 最近骨架 EDT → `cross_section_z`
  → `isotonic_multi_peak` → ffill/bfill → 空间 P30(`percentile_filter`) → mask-aware 高斯 → Dirichlet(河心 pin, 排除 junction) → `solve_laplace_dirichlet`；短骨架(<5)走最近骨架 DEM+水深回退。
- **保真关键**：mask 场用 float32（复刻 scipy 对 float32 输入的高斯**轴间 f32 舍入**，
  新增 [gaussian_smooth_f32](../crates/water-core/src/raster_ops.rs)）；`int(round)` banker's rounding；
  medial_axis 固定种子 tiebreaker。
- 测试 [stage6c_river_solve_parity.rs](../crates/water-hydro/tests/stage6c_river_solve_parity.rs)：3 例
  （直河道/L 形/短 blob 回退）z_local **最大误差 6.5e-13**（机器精度，含 Laplace 稀疏直接解）。

> 至此**河流水面求解的全部数值链已 value-exact**。剩余为 GIS 编排层（窗口/光栅化/缝合/CRS/瓦片/IO）。

### 阶段 6 收尾：河流外层循环 + 内存水面编排 ✅（外层循环已对拍）

- Rust：[crates/water-hydro/src/river_pipeline.rs](../crates/water-hydro/src/river_pipeline.rs)
  `window_from_geometry_bounds` / `solve_laplace_per_polygon` / `compute_water_surface`
- 对应 Python：`hydro_laplace.py::solve_laplace_per_polygon` 逐多边形循环 + `generate_hydro_water_dem` 算法段
- **`solve_laplace_per_polygon`**：逐河流多边形 窗口裁剪(逆仿射) → 光栅化(all_touched) →
  `solve_river_polygon_surface` → 缝合到全局 f32 面（湖泊跳过）。
  - 测试 [stage6c_river_pipeline_parity.rs](../crates/water-hydro/tests/stage6c_river_pipeline_parity.rs)：
    2 河流多边形合成场，surface **最大误差 1.42e-14**（value-exact）。
    > 注：本机 `rasterio.windows.from_bounds` 原生崩溃（GDAL DLL），Python 端用**同式纯逆仿射**生成夹具；
    > Rust 侧本就不依赖 rasterio，窗口正确性由光栅化掩膜一致性间接验证。
- **`compute_water_surface`**：河流求解 → 湖泊压平 → 河床抬升 → 输出组合的内存编排
  （复用已各自对拍的 `flatten_lake_polygons_on_surface` / `apply_river_dem_floor_lift` / `compose_water_output_array`）。

> **至此在工作 CRS 网格上的全部水面算法（河流 + 湖泊 + 抬升 + 组合）已就位。**

### 阶段 7（进行中）：CRS 解析 + 局地 UTM 估计 ✅（已对拍）

- Rust：[crates/water-hydro/src/crs.rs](../crates/water-hydro/src/crs.rs)
  `utm_epsg_from_center` / `estimate_local_utm_epsg` / `resolve_working_crs`（经 eci-gdal-proj）
- 对应 Python：`_resolve_hydro_working_crs` + `estimate_local_utm_crs_from_bounds`
- 策略：DEM 为投影坐标系→直接用之（`SourceProjected`）；否则由水体范围估计局地 UTM（`LocalUtm`）。
  UTM 带 `zone=floor((lon+180)/6)+1`，北 `32600+zone` / 南 `32700+zone`；bounds→4326 密化 21 点。
- 测试 [crs_parity.rs](../crates/water-hydro/tests/crs_parity.rs)：UTM 带选 60 例精确；
  局地 UTM 估计 3 例（4326 源 + UTM 32649 源经 proj4rs 重投影）与 pyproj **一致**。

### 阶段 7（进行中）：几何重投影 ✅（已对拍）

- Rust：[crates/water-hydro/src/crs.rs](../crates/water-hydro/src/crs.rs)
  `reproject_polygon` / `reproject_multipolygon` / `proj_from_epsg` / `normalize_fclass`
- 对应 Python：`_read_water_polygons` 的 `to_crs`（逐顶点重投影）+ fclass 规范化。
- 测试 [reproject_parity.rs](../crates/water-hydro/tests/reproject_parity.rs)：4 例
  （4326↔UTM49N、UTM→4326、4326→3857，含内环）逐顶点与 pyproj 最大误差 **1.86e-9 米**（纳米级）。

### 后续阶段（GIS 编排层，较重）

- [ ] CRS 重投影剩余：`warp_transform_bounds` ROI、DEM warp 到工作网格（`calculate_default_transform` + 重采样）。
- [ ] ROI 裁剪 + 瓦片流水线(`HYDRO_MAX_FULL_RASTER_PIXELS`=2.5e8，tile 8192/pad 50，rayon 并行)。
- [ ] `generate_hydro_water_dem` 端到端 IO（读 DEM/矢量 → 计算 → 写 GeoTIFF）+ 林芝真实数据整体对拍。
- [ ] 阶段 7：CRS 解析 + 重投影（工作 CRS ↔ 源网格）
- [ ] 阶段 8：ROI/瓦片流水线与并行
- [ ] 阶段 9：端到端在林芝真实数据上与 Python 整体对拍

## 端到端编排与三个关键工程决策

端到端管线：[crates/water-hydro/src/pipeline.rs](../crates/water-hydro/src/pipeline.rs)
`run_hydro_pipeline`（读矢量 → DEM 元数据 → 工作 CRS → 几何投影 → ROI 窗口 → 读 ROI 原始 DEM →
warp 到工作网格 → `compute_water_surface` → 投回源网格 → 组合写出）。

在林芝真实数据（DEM 26492×18138 EPSG:4326；水体 `result.shp` 41 多边形）上与 Python
`generate_hydro_water_dem` 逐像素对拍时，定位并处理了三个关键问题，逐一记录如下。

### 决策 1：DEM warp 重投影 —— 复刻 GDAL 近似变换器（bit 级一致）

- **现象**：Rust 精确逐像素 PROJ 重投影，与 Python(rasterio) 输出在陡坡处差最大 5.14m、均值 0.28m。
- **根因（决定性证据）**：
  - 我的工作网格 transform 与 rasterio `calculate_default_transform` **逐位一致**；
  - 我的双线性重采样 == 精确逐像素 pyproj + `scipy.ndimage.map_coordinates` 到 **0.0002m**（我方精确）；
  - `gdal.Warp(errorThreshold=0)`（关闭近似）== 我方精确（0.0005m）；`errorThreshold=0.125`（**GDAL 默认**）差 5.14m；
  - 即 Python 参照用了 **GDAL 默认的 0.125 像素多项式近似变换器**（`GDALApproxTransform`），我方为精确。
- **决策（用户拍板，仅重构 → 与旧参照一致 + 更快）**：忠实复刻 GDAL `GDALApproxTransform` 的递归仿射细分算法，
  见 [crates/water-io/src/warp_approx.rs](../crates/water-io/src/warp_approx.rs)。逐目标行细分为若干段，
  每段两端精确变换、线性插值，段中点曼哈顿误差 ≤ `max_error`(0.125) 即接受，否则递归。
- **证据**：Rust 近似重投影 == `gdal.Warp(errorThreshold=0.125)` 到 **0.00024m**（bit 级）。
  管线 DEM warp 用 `max_error=0.125`；`reproject(max_error=0)` 仍为精确路径（既有精确测试不变）。

### 决策 2：`medial_axis` 随机 tiebreaker —— 需固定为确定性顺序

- **现象**：即便 warp 已 bit 级一致，水面在骨架附近仍差最大 18–23m、均值 0.46m。
- **根因**：skimage 0.25 `medial_axis(image, *, rng=None)`，hydro 调用时 `rng=None` ⇒ **每次运行用随机
  tiebreaker**（`np.random.default_rng().permutation`）打破距离/cornerness 相等像素的处理顺序，
  从而**骨架本身不可复现**。实测同输入仅换种子，Python 两次运行水面即差最大 11.8m、p99 3.66m（不可约随机）。
- **决策**：Rust `water_core::medial_axis(mask, tiebreaker)` 接收**显式 tiebreaker**，采用确定性 identity
  顺序（`0..n`，行主序 fg 秩）。对拍时把 Python `medial_axis` 也改为同一 identity tiebreaker
  （见 [scripts/run_py_hydro_seed.py](../scripts/run_py_hydro_seed.py) `deterministic_medial_axis`）。
- **证据**：两边同用 identity tiebreaker 后，水体像素 Rust vs Python **max 0.65m、mean 0.012m、p99 0.117m、
  中位 0.004m**——由随机导致的米级差异全部消除。骨架算法本身早已 value-exact
  （[stage6b_medial_parity.rs](../crates/water-hydro/tests/stage6b_medial_parity.rs) 注入同 tiebreaker 逐像素相等）。

### 决策 3：背景 DEM 处理 —— 方案 B「精确源 DEM 背景」（不重采样）

- **背景**：`water_surface_with_dem` 模式下，输出的非水像素为 DEM「直通」背景。Python 的做法是先在
  **工作网格**上组合 `DEM_work + 水面`，再把整幅组合结果经 GDAL warp **投回源网格**——因此 Python 的背景是
  **双重重采样**（源 DEM → 工作 UTM → 投回源）的结果。
- **为何不 bit 匹配 Python 背景**（已证明不可行）：该双重重采样值依赖 **GDAL warp 的内部分块**——
  - `warp_mem_limit` 不同即结果不同（mem1 vs mem256 差 **5.96m**）；
  - GDAL 默认（=单块）的分块**既非 ROI 窗口宽、也非整幅行宽**（huge vs 窗口差 6.1m、vs 整行宽差 5.6m），
    而是其内部用 21 点边采样算出的包围盒，纯 Rust 无法稳定复刻；
  - 即 Python 背景是**不稳定的 GDAL 实现伪影**（非算法逻辑），bit 匹配既不可行也无意义。
- **决策（用户拍板：方案 B）**：**只把水面 + 掩膜投回源网格，背景直接用未重采样的精确源 DEM**。
  见 [pipeline.rs](../crates/water-hydro/src/pipeline.rs) 步骤 8–10：
  1. `compute_water_surface` 只产**工作网格水面 + 写入掩膜**（不组合 DEM）；
  2. `masked_surface`（掩膜内水面、掩膜外 NaN）双线性投回源 ROI；掩膜最近邻投回；
  3. 源网格组合：水像素用投回水面，其余用**精确源 DEM**（`read_window_f32` 读的原值），否则 nodata。
- **相对 Python 的差异与取舍**：
  - 水面像素：**不受影响**，仍为决策 2 的 parity（max 0.65m）；
  - 背景像素：本实现 == **精确源 DEM**（对源 DEM 窗口 **p50=0**），比 Python「被 GDAL 模糊过的双重重采样」
    背景**更锐利、更正确、完全确定**，且省一次全图反投影（更快）；
  - 与 Python 背景的中位差约 **0.33m**，即 Python 双重重采样引入的模糊量——本方案**有意避免**之。
- **一句话**：方案 B 使非水区严格等于原始 DEM（第一性原理正确 + 确定 + 更快），代价是与旧 Python 参照
  被 GDAL 模糊过的背景相差约 0.33m 中位；水面本身与 Python（固定种子）为 parity。

> **对拍脚本**：[scripts/run_py_hydro_seed.py](../scripts/run_py_hydro_seed.py)（固定 `medial_axis` 种子 /
> identity，并猴子补丁绕过本机 `rasterio.windows.from_bounds` 的 PROJ 原生崩溃，端到端跑出确定性 Python 参照）；
> 逐像素对拍见对应 `scripts/` 分析脚本。

> **重要更正（方案 B 实为与生产参照一致）**：Python 有两条输出路径——**非瓦片**（小场景）先在工作网格
> 组合 `DEM_work+surface` 再整体投回（scheme A，背景双重重采样），**瓦片**（`_process_water_tile_body`，
> 大场景）则**只投回 surface+mask，再与精确源 DEM tile 组合**（正是 scheme B）。林芝全域 `waters.tif`
> 工作网格 276M 超预算，走的是**瓦片路径 = scheme B**。故本仓库方案 B 的背景与真实 `waters.tif`
> **逐位一致**（全域条带对比背景 p50=p99=0，见下「瓦片路径」）。上面 0.33m 仅是与**非瓦片** Python 的差。

### 瓦片路径（`_generate_hydro_water_dem_tiled`）✅（全域跑通 + 背景 bit 级一致）

- Rust：[crates/water-hydro/src/pipeline.rs](../crates/water-hydro/src/pipeline.rs)
  `run_hydro_pipeline_tiled` / `process_window`（提取的可复用「padded 窗口→core」处理）。
- 对应 Python：`hydro_tile_runner.py::_generate_hydro_water_dem_tiled` / `_process_water_tile_body`。
- **分派**：整窗工作网格 ≤ `HYDRO_MAX_FULL_RASTER_PIXELS`(2.5e8) 走单窗，否则瓦片。
- **瓦片**：源网格按 `HYDRO_TILED_TILE_SIZE`=8192 分块，pad=`max(50,64)`=64px。
  与水体相交的瓦片：`process_window`（读 DEM tile±pad → warp 到工作网格 → `compute_water_surface`
  → 投回 padded 源窗口 → 与精确源 DEM 组合 → 提取 core）；dry 瓦片保留源 DEM/nodata。
  峰值工作内存 ~ 单瓦片规模，故可处理超预算场景。
- **对拍证据（林芝全域 41 多边形）**：
  - Rust 与 Python 分块**完全一致**：grid=(3,4)、12 瓦片、**6 水瓦片 + 6 dry**；
  - **与 Python identity 全域参照逐像素对拍**（`full_pyident.tif`，两边同用 identity tiebreaker）：
    4.507 亿重叠有效像素、**0 像素仅单边有效**；**整体 mean 0.00047m（0.5mm）、99.74% 像素 < 1cm**；
    diff>1m 仅 0.0072%、diff>5m 仅 0.0002%（974 像素）；
  - 与真实 `waters.tif`（随机 tiebreak）逐条带对比：**背景 DEM 逐位一致（p50=p99=0）**；
  - 残留 >5m 的 974 像素**0% 在瓦片接缝**（拼接无缝），全部落在 river 的**浮空穹顶/陡岸**局部簇——
    算法固有不稳定：栅格化偶发 1px 边界翻转 → identity 秩偏移 → 骨架微变，在敏感穹顶处放大
    （与 `medial_axis` 随机性同源、不可约，见决策 2 与 `data/tmp/FLOATING_diagnosis.md`）；
  - Rust 全域约 2 分钟（单线程），输出 26492×18138 全幅。
- **已知差异**：Python 瓦片对 `lake`/`water` fclass 有「静水常数 z」快捷（still-water tile）+ 源网格
  湖泊再压平；本 Rust 当前对湖泊按 `compute_water_surface` 内的 `lake_flatten` 处理，跨瓦片大湖的
  常数水位可能与 Python 的全局 `lake_constant_z_map` 略异（河流水面不受影响）。


