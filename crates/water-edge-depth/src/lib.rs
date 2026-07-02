//! water-edge-depth —— 水边深度导出。
//!
//! 对应原 Python `waters/pipeline.py`（`export_water_edge_depth` /
//! `export_water_edge_depth_from_gdf`）与 `io.py::_enrich_water_gdf_with_edge_depth`。
//! **替换的 Python 库**：geopandas / fiona / shapely。
//!
//! 纯矢量属性富化：按 fclass 给每个水体多边形加 `edgeexpand` / `depth` 字段，写出 shapefile
//! （保留原属性列 + 新增两列）。每 fclass 的 edge/depth **可配置**（`EdgeDepthConfig`，
//! 含 GUI 滑块范围），对应 Python `WATER_FCLASS_EDGE_DEPTH_GUIDANCE`。

use std::collections::BTreeSet;
use std::path::Path;

use geo_types::{Geometry, MultiPolygon};

use water_core::edge_depth::{normalize_fclass, EdgeDepthConfig};
use water_core::error::{Result, WaterError};
use water_io::vector::{
    json_to_shp_value, read_shapefile_fields, read_vector, write_polygons_shapefile,
    FeatureCollection, ShpFieldDef, ShpFieldType, ShpValue,
};

/// 水边深度导出参数。面向 GUI：每 fclass 的 edge/depth 由 [`EdgeDepthConfig`] 配置。
#[derive(Debug, Clone, Default)]
pub struct EdgeDepthOptions {
    /// 每 fclass 的 edge_expand / depth 取值（默认取自 `WATER_FCLASS_EDGE_DEPTH`）。
    pub config: EdgeDepthConfig,
}

/// 从要素属性取 `fclass`（大小写不敏感字段名）。
fn feature_fclass(
    props: &std::collections::BTreeMap<String, serde_json::Value>,
) -> Option<String> {
    props
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("fclass"))
        .and_then(|(_, v)| v.as_str().map(|s| s.to_string()))
}

/// 几何 → `MultiPolygon`（Polygon 包装为单元素；MultiPolygon 原样；其它返回 `None`）。
fn to_multipolygon(g: &Geometry<f64>) -> Option<MultiPolygon<f64>> {
    match g {
        Geometry::Polygon(p) => Some(MultiPolygon(vec![p.clone()])),
        Geometry::MultiPolygon(mp) => Some(mp.clone()),
        _ => None,
    }
}

/// 校验并规范化每个要素的 fclass；任一不支持即整体报错（对应 Python 行为）。
fn resolve_fclasses(fc: &FeatureCollection) -> Result<Vec<&'static str>> {
    let mut out = Vec::with_capacity(fc.features.len());
    let mut unsupported: BTreeSet<String> = BTreeSet::new();
    for feat in &fc.features {
        match feature_fclass(&feat.properties) {
            None => {
                unsupported.insert("<缺少 fclass>".to_string());
            }
            Some(raw) => match normalize_fclass(&raw) {
                Some(c) => out.push(c),
                None => {
                    unsupported.insert(raw);
                }
            },
        }
    }
    if !unsupported.is_empty() {
        return Err(WaterError::InvalidInput(format!(
            "不支持的水体 fclass：{}",
            unsupported.into_iter().collect::<Vec<_>>().join(", ")
        )));
    }
    Ok(out)
}

/// 从文件导出水边深度：读水体 → 按 fclass 加 `edgeexpand`/`depth` → 写 shapefile。
///
/// `output_path` 的扩展名统一为 `.shp`；.prj 直接复制输入（CRS 不变）。
pub fn export_water_edge_depth(
    water_path: &Path,
    output_path: &Path,
    opts: &EdgeDepthOptions,
) -> Result<()> {
    let fc = read_vector(water_path)?;
    if fc.features.is_empty() {
        return Err(WaterError::InvalidInput("水体输入无任何要素".into()));
    }
    let canon = resolve_fclasses(&fc)?;

    // 原字段（保留原属性列）+ 追加 edgeexpand / depth。
    let mut fields = read_shapefile_fields(water_path).unwrap_or_default();
    let has = |n: &str| fields.iter().any(|f| f.name.eq_ignore_ascii_case(n));
    if !has("fclass") {
        fields.push(ShpFieldDef {
            name: "fclass".into(),
            ty: ShpFieldType::Character { length: 32 },
        });
    }
    fields.push(ShpFieldDef {
        name: "edgeexpand".into(),
        ty: ShpFieldType::Numeric { length: 19, decimals: 6 },
    });
    fields.push(ShpFieldDef {
        name: "depth".into(),
        ty: ShpFieldType::Numeric { length: 19, decimals: 6 },
    });

    // 逐要素：几何 + 属性透传 + edge/depth。
    let mut polygons: Vec<MultiPolygon<f64>> = Vec::with_capacity(fc.features.len());
    let mut records: Vec<Vec<ShpValue>> = Vec::with_capacity(fc.features.len());
    for (feat, &c) in fc.features.iter().zip(&canon) {
        let Some(mp) = to_multipolygon(&feat.geometry) else {
            return Err(WaterError::InvalidInput("水体要素含非多边形几何".into()));
        };
        let ed = opts
            .config
            .get(c)
            .ok_or_else(|| WaterError::InvalidInput(format!("fclass {c} 无 edge/depth 配置")))?;
        let no_fclass_field = feature_fclass(&feat.properties).is_none();
        let mut vals = Vec::with_capacity(fields.len());
        for f in &fields {
            let v = if f.name == "edgeexpand" {
                ShpValue::Number(Some(ed.edge_expand))
            } else if f.name == "depth" {
                ShpValue::Number(Some(ed.depth))
            } else if f.name.eq_ignore_ascii_case("fclass") && no_fclass_field {
                ShpValue::Text(Some(c.to_string()))
            } else {
                json_to_shp_value(feat.properties.get(&f.name), &f.ty)
            };
            vals.push(v);
        }
        polygons.push(mp);
        records.push(vals);
    }

    // 写 .shp（统一扩展名）。
    let shp_path = output_path.with_extension("shp");
    if let Some(parent) = shp_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_polygons_shapefile(&shp_path, &polygons, &fields, &records, None)?;

    // 复制输入 .prj（CRS 不变）。
    let in_prj = water_path.with_extension("prj");
    if in_prj.exists() {
        let _ = std::fs::copy(&in_prj, shp_path.with_extension("prj"));
    }

    let uniq: BTreeSet<&str> = canon.iter().copied().collect();
    tracing::info!(
        features = fc.features.len(),
        fclass = ?uniq,
        output = %shp_path.display(),
        "water_edge_depth 导出完成"
    );
    Ok(())
}
