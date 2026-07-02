# FCLASS — 水体语义分类（`water-fclass`）

将原 Python `waters/fclass/`（`assign_water_fclass` / `compute_water_fclass_gdf`）忠实复刻为纯 Rust。
**替换的 Python 库**：geopandas / shapely / fiona。

## 功能总览

读入水体多边形，与**已准备好的水体参考库**（GeoPackage）做空间关联，按优先级为每个多边形赋予语义类别 `fclass`。

- 输入：水体矢量（shapefile / GeoJSON / GeoPackage），任意 CRS。
- 参考：GeoPackage，含图层 `reservoir` / `lake` / `dock` / `glacier`（多边形）与 `river` / `stream` / `sea`（线）。
- 输出：`.shp`/`.shx`/`.dbf`（+ 复制输入 `.prj`），**保留输入几何与 CRS**，原 `fclass` 列替换为分类结果。
- 分类在**工作 CRS（EPSG:3857）**下进行；输入若非 3857 会临时重投影用于判定，输出几何保持原生 CRS。

> `--transition-only`（语义过渡检测）为独立可选特性，当前未实现，调用会返回 `NotImplemented`。

## 分类算法（与 Python 逐要素一致）

对每个水体多边形，按 **优先级** `FCLASS_PRIORITY` 依次判定，命中即止；全不命中回退 `water`：

| 优先级 | fclass | 参考类型 | 判定（对应 Python） |
|---|---|---|---|
| 1 | sea | 线 | `_has_line_clip_match`：任一参考线 ∩ 多边形的裁剪长度 ≥ 20m |
| 2 | lake | 多边形 | `_any_polygon_match`：参考多边形含质心，或重叠面积/水体面积 ≥ 0.2 |
| 3 | glacier | 多边形 | 同上 |
| 4 | reservoir | 多边形 | 同上 |
| 5 | dock | 多边形 | 同上 |
| 6 | river | 线 | `_has_line_clip_match`（长度 ≥ 20m） |
| 7 | stream | 线 | 同上 |

关键常量（对应 `fclass/settings.py`）：`OSM_LINE_MIN_LENGTH_M = 20.0`、`POLYGON_OVERLAP_THRESHOLD = 0.2`、
`REFERENCE_READ_BBOX_PAD_M = 100.0`（参考按水体并集包围盒外扩 100m 预过滤）。

### 参考层读取（对应 `fclass/io.py`）

- 多边形层（reservoir/lake/dock/glacier）：按**多边形族**读取，`MultiPolygon` 拆成 `Polygon`。
- 线层（river/stream/sea）：按**线族**读取，`MultiLineString` 拆成 `LineString`；`sea` 图层亦按线族过滤（对应 Python `sea_lines`）。
- 均重投影到工作 CRS，并以水体外扩包围盒预过滤 + `rstar` R 树空间索引加速候选查询。

### 输出列（对应 `fclass/io.py::_write_fclass_output`）

- 原 `fclass` 列改名为内部列后 **drop**，新 `fclass`（分类结果）追加到末尾。
- `featureid`：缺列则从 `30_000_000` 起顺序补齐；有列但存在缺值/重复则重基（`_ensure_unique_polygon_featureid`）。
- 内部列 `_input_fclass` / `_fclass_reason` 不写出。

## 使用

```powershell
# CLI
cargo run --release -p water_cli -- fclass `
  --water water.shp `
  --output classified.shp `
  --reference-path waters_china.gpkg
```

## 对拍证据

脚本 [scripts/cmp_fclass.py](../scripts/cmp_fclass.py) 用同一输入分别跑 Python `assign_water_fclass`
与 Rust `water-fclass`，按 `featureid` 逐要素比对 `fclass`。

真实数据（林芝 `data/reslut/result.shp`，41 个多边形，EPSG:4326；参考 `waters_china.gpkg`，EPSG:3857）：

| 指标 | Python | Rust |
|---|---|---|
| 输出列顺序 | `osm_id, code, hide, featureid, fclass` | 一致 |
| 分类计数 | `{river: 32, water: 7, lake: 2}` | `{river: 32, water: 7, lake: 2}` |
| 逐要素差异（按 featureid） | — | **0** |

即 **41/41 要素分类结果完全一致**。
