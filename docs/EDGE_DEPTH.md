# EDGE_DEPTH — 水边深度导出（`water-edge-depth`）

将原 Python `waters/pipeline.py`（`export_water_edge_depth` / `export_water_edge_depth_from_gdf`）
与 `io.py::_enrich_water_gdf_with_edge_depth` 忠实复刻为纯 Rust。**替换的 Python 库**：geopandas / fiona / shapely。

## 功能总览

**纯矢量属性富化**（无栅格）：读入**已分类**水体多边形（含 `fclass`）→ 按 fclass 给每个多边形
追加 `edgeexpand` / `depth` 两个字段 → 写出 shapefile（**保留原属性列** + 新增两列，CRS 不变）。

- 输入：水体矢量（shapefile / GeoJSON / GeoPackage），须含 `fclass` 字段。
- 输出：`.shp`/`.shx`/`.dbf`（+ 复制输入 `.prj`）。
- **任一要素 fclass 不受支持即整体报错**（对应 Python 行为）。

## 每 fclass 可配置（GUI 就绪）

新增需求：`edgeexpand` / `depth` 每 fclass **可自定义调试**（最终为 GUI 滑块）。对应 Python
`settings.py::WATER_FCLASS_EDGE_DEPTH_GUIDANCE`——每项含**默认值 + 推荐范围**（滑块 min/max）。

- Rust：[crates/water-core/src/edge_depth.rs](../crates/water-core/src/edge_depth.rs)
  - `edge_depth_guidance()`：8 类的**规格表**（默认值 + `edge_expand_range` / `depth_range`），供 GUI/CLI/API 构建调参界面；
  - `EdgeDepthConfig`：每 fclass 的**实际取值**（`default()` = 默认值），`set(fclass, edge, depth)` 调参；
  - `normalize_fclass`：fclass 别名规范化（对应 `WATER_FCLASS_ALIASES`，含全角括号/中文别名）。
- 设计参照 Python `postprocess` 的「任务选项 + roof hue-card palette」**声明式 spec 驱动** GUI 模式。

### 默认值 + 推荐范围（与 Python `WATER_FCLASS_EDGE_DEPTH_GUIDANCE` 逐项一致）

| fclass | edgeexpand（默认 / 范围） | depth（默认 / 范围） |
|---|---|---|
| stream | 1.0 / (0.5, 1.5) | 1.5 / (0.2, 0.5) |
| dock | 3.0 / (15, 75) | 5.0 / (5, 18) |
| water | 5.0 / (5, 20) | 3.0 / (1, 3) |
| river | 10.0 / (3, 20) | 2.8 / (1, 3) |
| lake | 15.0 / (20, 200) | 4.5 / (2, 10) |
| reservoir | 3.0 / (20, 100) | 6.0 / (3, 8) |
| glacier | 10.0 / (10, 50) | 6.0 / (5, 15) |
| sea | 15.0 / (500, ∞) | 8.0 / (20, ∞) |

> 忠实复刻原值——部分默认值**不在**其推荐范围内（如 `stream.depth=1.5` 而范围 (0.2,0.5)、
> `dock.edgeexpand=3.0` 而范围 (15,75)），此处**不做修正**（范围仅为 GUI 提示，`set` 也不强制夹取）。

## 用法

### CLI
```powershell
# 默认配置
water2rust edge-depth --water result.shp --output edge.shp
# 每 fclass 覆盖（可重复）：fclass:edgeexpand:depth
water2rust edge-depth --water result.shp --output edge.shp --set river:12.5:3.1 --set lake:20:5
```

### API（`water_api`）
`POST /tasks/edge-depth`，请求体含 `overrides: [{fclass, edgeexpand, depth}]`（GUI 调参结果）；
未列出的 fclass 用默认。滑块范围提示由 `water_core::edge_depth::edge_depth_guidance()` 提供。

## 对拍证据

- **默认值 + 范围**：8 类 `edgeexpand`/`depth` 默认值与 Python `WATER_FCLASS_EDGE_DEPTH`、
  `edge_expand_range`/`depth_range` 与 `WATER_FCLASS_EDGE_DEPTH_GUIDANCE` **逐项一致**
  （[scripts/cmp_edge_depth.py](../scripts/cmp_edge_depth.py)）。
- **端到端**：林芝 `result.shp`（41 要素，fclass=river/lake/water）经 Rust CLI 导出：
  - 输出列 = 原属性（osm_id/code/hide/fclass/featureid）+ `edgeexpand` + `depth`，CRS EPSG:4326 保留；
  - 各 fclass 赋值正确：river{10,2.8}、lake{15,4.5}、water{5,3.0}；
  - `--set river:12.5:3.1 --set lake:20:5` 覆盖生效、未指定的 water 保持默认。
- **配置模块单测**：3 例（8 类规格、别名规范化、默认/覆盖）通过。

## 依赖的 eci-gdal 补充

edge 需**写 shapefile**，而 eci-gdal-vector 原**只读不写**。已补齐：
- `eci-gdal-vector::write_polygon_shapefile`（多边形 + 属性写 `.shp`/`.shx`/`.dbf`）；
- 该能力同时归档于 [eci-gdal-contrib/vector/shapefile_write.rs](../eci-gdal-contrib/vector/shapefile_write.rs)，待回流 eci-gdal 库。
- water-io 侧薄封装：`vector::{read_shapefile_fields, write_polygons_shapefile, json_to_shp_value}`。
