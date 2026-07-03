//! water-fclass — 水体 fclass 语义分类。
//!
//! 对应原 Python `waters/fclass/`（`assign_water_fclass` / `compute_water_fclass_gdf`）。
//! **替换的 Python 库**：geopandas、shapely、fiona。
//!
//! 将源水体多边形与已准备好的水体参考（GeoPackage：reservoir/lake/dock/glacier
//! 多边形层，river/stream/sea 线层）按优先级做空间关联，赋予语义类别 `fclass`。
//! 分类在工作 CRS（EPSG:3857）下进行，输出保留输入几何与原属性（`fclass` 列替换为分类结果）。

mod classify;
mod reference;

use std::collections::BTreeMap;
use std::path::Path;

use geo::BoundingRect;
use geo_types::{Coord, Geometry, MultiPolygon, Rect};
use serde_json::Value;

use water_core::error::{Result, WaterError};
use water_io::vector::{
    json_to_shp_value, read_shapefile_fields, read_vector, reproject_geometry,
    write_polygons_shapefile, ShpFieldDef, ShpFieldType, ShpValue,
};

use crate::classify::classify_geometry;
use crate::reference::{ReferenceLayers, WORKING_EPSG};

/// 参照读取包围盒外扩（米）。对应 Python `REFERENCE_READ_BBOX_PAD_M`。
const REFERENCE_READ_BBOX_PAD_M: f64 = 100.0;
/// featureid 重基起点。对应 Python `FEATUREID_REBASE_START`。
const FEATUREID_REBASE_START: i64 = 30_000_000;

/// fclass 分类参数。
#[derive(Debug, Clone)]
pub struct FclassOptions {
    /// 准备好的水体参考 GeoPackage 路径。
    pub reference_path: std::path::PathBuf,
    /// 仅检测语义过渡位置（对应 `--fclass-transition-only`），暂未实现。
    pub transition_only: bool,
}

/// 几何 → `MultiPolygon`（Polygon 包装；MultiPolygon 原样；其它 `None`）。
fn to_multipolygon(g: &Geometry<f64>) -> Option<MultiPolygon<f64>> {
    match g {
        Geometry::Polygon(p) => Some(MultiPolygon(vec![p.clone()])),
        Geometry::MultiPolygon(mp) => Some(mp.clone()),
        _ => None,
    }
}

/// 分类并写出结果。对应 `assign_water_fclass`（主路径，非 transition_only）。
pub fn run_fclass(water_path: &Path, output_path: &Path, opts: &FclassOptions) -> Result<()> {
    if opts.transition_only {
        return Err(WaterError::NotImplemented("water_fclass::transition_only"));
    }
    if !opts.reference_path.exists() {
        return Err(WaterError::InvalidInput(format!(
            "参考 GeoPackage 不存在：{}",
            opts.reference_path.display()
        )));
    }

    let fc = read_vector(water_path)?;
    if fc.features.is_empty() {
        return Err(WaterError::InvalidInput("水体输入无任何要素".into()));
    }
    let src_epsg: u16 = fc
        .crs_epsg
        .ok_or_else(|| WaterError::InvalidInput("水体输入缺少 CRS".into()))? as u16;

    // 输出用（原生 CRS）多边形 + 分类用（工作 CRS）多边形。
    let mut out_polys: Vec<MultiPolygon<f64>> = Vec::with_capacity(fc.features.len());
    let mut work_polys: Vec<MultiPolygon<f64>> = Vec::with_capacity(fc.features.len());
    for feat in &fc.features {
        let native = to_multipolygon(&feat.geometry)
            .ok_or_else(|| WaterError::InvalidInput("水体要素含非多边形几何".into()))?;
        let work = if src_epsg == WORKING_EPSG {
            native.clone()
        } else {
            let g =
                reproject_geometry(&Geometry::MultiPolygon(native.clone()), src_epsg, WORKING_EPSG)?;
            to_multipolygon(&g)
                .ok_or_else(|| WaterError::Other(anyhow::anyhow!("重投影后几何非多边形")))?
        };
        out_polys.push(native);
        work_polys.push(work);
    }

    // 工作 CRS 下水体并集包围盒 + 外扩，作为参照读取 AOI。
    let aoi = padded_bbox(&work_polys, REFERENCE_READ_BBOX_PAD_M)
        .ok_or_else(|| WaterError::InvalidInput("水体几何无有效包围盒".into()))?;
    let refs = ReferenceLayers::load(&opts.reference_path, &aoi)?;

    // 逐要素分类（各要素独立，rayon 并行）。
    use rayon::prelude::*;
    let fclasses: Vec<&'static str> = work_polys
        .par_iter()
        .map(|wp| classify_geometry(wp, &refs).0)
        .collect();

    write_output(water_path, output_path, &fc, &out_polys, &fclasses)?;

    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for &c in &fclasses {
        *counts.entry(c).or_insert(0) += 1;
    }
    tracing::info!(
        features = fc.features.len(),
        counts = ?counts,
        output = %output_path.with_extension("shp").display(),
        "water_fclass 分类完成"
    );
    Ok(())
}

/// 工作 CRS 下所有多边形并集包围盒，外扩 `pad` 米。
fn padded_bbox(polys: &[MultiPolygon<f64>], pad: f64) -> Option<Rect<f64>> {
    let mut it = polys.iter().filter_map(|p| p.bounding_rect());
    let first = it.next()?;
    let (mut minx, mut miny) = (first.min().x, first.min().y);
    let (mut maxx, mut maxy) = (first.max().x, first.max().y);
    for r in it {
        minx = minx.min(r.min().x);
        miny = miny.min(r.min().y);
        maxx = maxx.max(r.max().x);
        maxy = maxy.max(r.max().y);
    }
    Some(Rect::new(
        Coord { x: minx - pad, y: miny - pad },
        Coord { x: maxx + pad, y: maxy + pad },
    ))
}

/// 构建输出字段/记录并写出 shapefile（保留输入几何，`fclass` 列替换为分类结果）。
fn write_output(
    water_path: &Path,
    output_path: &Path,
    fc: &water_io::vector::FeatureCollection,
    polygons: &[MultiPolygon<f64>],
    fclasses: &[&'static str],
) -> Result<()> {
    // 原字段去掉 fclass（对应 Python 把 fclass 改名 _input_fclass 后 drop），末尾追加新 fclass。
    let original = read_shapefile_fields(water_path).unwrap_or_default();
    let mut fields: Vec<ShpFieldDef> =
        original.into_iter().filter(|f| f.name != "fclass").collect();

    // featureid：缺列则补，重复/缺值则重基（对应 Python `_ensure_unique_polygon_featureid`）。
    let has_featureid = fields.iter().any(|f| f.name == "featureid");
    if !has_featureid {
        fields.push(ShpFieldDef {
            name: "featureid".into(),
            ty: ShpFieldType::Character { length: 32 },
        });
    }
    let featureids = resolve_featureids(fc, has_featureid);

    fields.push(ShpFieldDef {
        name: "fclass".into(),
        ty: ShpFieldType::Character { length: 32 },
    });

    let mut records: Vec<Vec<ShpValue>> = Vec::with_capacity(fc.features.len());
    for (i, feat) in fc.features.iter().enumerate() {
        let mut vals = Vec::with_capacity(fields.len());
        for f in &fields {
            let v = if f.name == "fclass" {
                ShpValue::Text(Some(fclasses[i].to_string()))
            } else if f.name == "featureid" {
                ShpValue::Text(Some(featureids[i].clone()))
            } else {
                json_to_shp_value(feat.properties.get(&f.name), &f.ty)
            };
            vals.push(v);
        }
        records.push(vals);
    }

    let shp_path = output_path.with_extension("shp");
    if let Some(parent) = shp_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_polygons_shapefile(&shp_path, polygons, &fields, &records, None)?;

    // 复制输入 .prj（输出 CRS 与输入一致）。
    let in_prj = water_path.with_extension("prj");
    if in_prj.exists() {
        let _ = std::fs::copy(&in_prj, shp_path.with_extension("prj"));
    }
    Ok(())
}

/// 计算每要素的 featureid（保证唯一、非空）。对应 Python `_ensure_unique_polygon_featureid`。
fn resolve_featureids(
    fc: &water_io::vector::FeatureCollection,
    has_featureid: bool,
) -> Vec<String> {
    let n = fc.features.len();
    if !has_featureid {
        return (0..n)
            .map(|i| (FEATUREID_REBASE_START + i as i64).to_string())
            .collect();
    }

    let normalized: Vec<Option<String>> = fc
        .features
        .iter()
        .map(|feat| normalize_featureid(feat.properties.get("featureid")))
        .collect();

    // 是否需要修复（缺值或重复）。
    let mut seen = std::collections::HashSet::new();
    let mut needs_fix = false;
    for v in &normalized {
        match v {
            None => needs_fix = true,
            Some(s) => {
                if !seen.insert(s.clone()) {
                    needs_fix = true;
                }
            }
        }
    }
    if !needs_fix {
        return normalized.into_iter().map(|v| v.unwrap()).collect();
    }

    let mut used: std::collections::HashSet<String> =
        normalized.iter().filter_map(|v| v.clone()).collect();
    let mut next = FEATUREID_REBASE_START;
    let mut kept: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(n);
    for v in &normalized {
        if let Some(s) = v {
            if !kept.contains(s) {
                kept.insert(s.clone());
                out.push(s.clone());
                continue;
            }
        }
        while used.contains(&next.to_string()) {
            next += 1;
        }
        let new_value = next.to_string();
        next += 1;
        used.insert(new_value.clone());
        out.push(new_value);
    }
    out
}

/// 规范化 featureid 值：去空白，空/`none` 视为缺失。
fn normalize_featureid(value: Option<&Value>) -> Option<String> {
    let text = match value {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => return None,
    };
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}
