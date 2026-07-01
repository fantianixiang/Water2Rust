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

### 后续阶段（待实现，逐一对拍）

- [ ] 阶段 2：湖泊常数水位（`_iterative_trimmed_median` / `_compute_lake_constant_z_*`）
- [ ] 阶段 3：河床抬升（`apply_river_dem_floor_lift`）
- [ ] 阶段 4：输出组合（`compose_water_output_array`）
- [ ] 阶段 5：多边形栅格化（`rasterio.features.rasterize`，all_touched）+ 形态学（`binary_erosion` 岸线环）
- [ ] 阶段 6：骨架/中轴与河心线 z（`medial_axis` / 横断面 / isotonic）
- [ ] 阶段 7：CRS 解析 + 重投影（工作 CRS ↔ 源网格）
- [ ] 阶段 8：ROI/瓦片流水线与并行
- [ ] 阶段 9：端到端在林芝真实数据上与 Python 整体对拍
