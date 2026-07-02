//! 矢量 IO（Shapefile / GeoJSON / GeoPackage 矢量），经 `eci-gdal-vector`。
//! 替代 geopandas / fiona / shapely。

use std::collections::BTreeMap;
use std::path::Path;

use eci_gdal_vector::{
    write_polygon_shapefile, FieldDefn, VectorCrsStatus, VectorFieldType, VectorFieldValue,
    open_vector_source, read_dbf_records, read_dbf_schema, read_shapefile_crs_metadata,
    to_vector_field_value,
};
use serde_json::Value;
use water_core::error::{Result, WaterError};

/// 一个带属性的矢量要素。
#[derive(Debug, Clone)]
pub struct Feature {
    pub geometry: geo_types::Geometry<f64>,
    pub properties: BTreeMap<String, Value>,
}

/// 一个图层：要素集合 + CRS。
#[derive(Debug, Clone, Default)]
pub struct FeatureCollection {
    pub features: Vec<Feature>,
    pub crs_epsg: Option<u32>,
}

/// 读取矢量图层：几何 + 属性 + CRS。
///
/// 几何经 `eci-gdal-vector` 的 `load_geometries`，属性经 dbf（仅 shapefile），
/// 二者按要素顺序 1:1 对齐。
pub fn read_vector(path: &Path) -> Result<FeatureCollection> {
    let source = open_vector_source(path)?;
    let geoms = source.load_geometries()?;

    // 属性：仅 shapefile 有独立 .dbf；读取时须用 .dbf 路径（不能把 .shp 交给 dbf 解析器）。
    let dbf_path = path.with_extension("dbf");
    let (records, schema) = if dbf_path.exists() {
        (read_dbf_records(&dbf_path).ok(), read_dbf_schema(&dbf_path).ok())
    } else {
        (None, None)
    };

    let mut features = Vec::with_capacity(geoms.len());
    for (i, geometry) in geoms.into_iter().enumerate() {
        let mut properties = BTreeMap::new();
        if let (Some(records), Some(schema)) = (records.as_ref(), schema.as_ref()) {
            if let Some(record) = records.get(i) {
                for field in &schema.fields {
                    if let Some(fv) = record.get(field.name.as_str()) {
                        properties.insert(
                            field.name.clone(),
                            field_value_to_json(&to_vector_field_value(fv)),
                        );
                    }
                }
            }
        }
        features.push(Feature { geometry, properties });
    }

    let crs_epsg = read_shapefile_crs_metadata(path)
        .ok()
        .and_then(|m| match m.status {
            VectorCrsStatus::Identified(rc) => rc.epsg_code().map(u32::from),
            VectorCrsStatus::AssumedWgs84(_) => Some(4326),
            _ => None,
        });

    Ok(FeatureCollection { features, crs_epsg })
}

/// 将要素集合写出为 GeoJSON。
pub fn write_geojson(path: &Path, fc: &FeatureCollection) -> Result<()> {
    let features: Vec<geojson::Feature> = fc
        .features
        .iter()
        .map(|f| {
            let geometry = geojson::Geometry::new(geojson::Value::from(&f.geometry));
            let properties: serde_json::Map<String, Value> =
                f.properties.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            geojson::Feature {
                bbox: None,
                geometry: Some(geometry),
                id: None,
                properties: Some(properties),
                foreign_members: None,
            }
        })
        .collect();

    let collection = geojson::FeatureCollection {
        bbox: None,
        features,
        foreign_members: None,
    };
    std::fs::write(path, collection.to_string())?;
    Ok(())
}

fn field_value_to_json(value: &VectorFieldValue) -> Value {
    match value {
        VectorFieldValue::Character(Some(s)) | VectorFieldValue::Date(Some(s)) => {
            Value::String(s.clone())
        }
        VectorFieldValue::Memo(s) => Value::String(s.clone()),
        VectorFieldValue::DateTime { date, .. } => Value::String(date.clone()),
        VectorFieldValue::Numeric(Some(f))
        | VectorFieldValue::Currency(f)
        | VectorFieldValue::Double(f) => json_num(*f),
        VectorFieldValue::Float(Some(f)) => json_num(*f as f64),
        VectorFieldValue::Integer(i) => Value::from(*i),
        VectorFieldValue::Logical(Some(b)) => Value::Bool(*b),
        _ => Value::Null,
    }
}

fn json_num(f: f64) -> Value {
    serde_json::Number::from_f64(f)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

// ── Shapefile 写入（经 eci-gdal-vector），供 water-edge-depth 属性富化后写出 ──

/// 写入用字段/值类型 re-export（经 eci-gdal-vector）。
pub use eci_gdal_vector::{ShpFieldDef, ShpFieldType, ShpValue};

/// dbf 字段类型 → 写入用 `ShpFieldType`（字符类归 Character，数值类归 Numeric）。
fn map_dbf_field(f: &FieldDefn) -> ShpFieldDef {
    let ty = match f.field_type {
        VectorFieldType::Character
        | VectorFieldType::Date
        | VectorFieldType::Logical
        | VectorFieldType::Memo
        | VectorFieldType::DateTime => ShpFieldType::Character { length: f.width.max(1) },
        VectorFieldType::Numeric
        | VectorFieldType::Float
        | VectorFieldType::Currency
        | VectorFieldType::Integer
        | VectorFieldType::Double => ShpFieldType::Numeric {
            length: f.width.max(1),
            decimals: f.precision.unwrap_or(0),
        },
    };
    ShpFieldDef { name: f.name.clone(), ty }
}

/// 读取 shapefile 的 dbf 字段定义（保留原字段名/类型/宽度），供属性透传写出。
pub fn read_shapefile_fields(path: &Path) -> Result<Vec<ShpFieldDef>> {
    let dbf = path.with_extension("dbf");
    let schema = read_dbf_schema(&dbf).map_err(WaterError::Other)?;
    Ok(schema.fields.iter().map(map_dbf_field).collect())
}

/// 将属性 JSON 值按目标字段类型转为写入用 `ShpValue`。
pub fn json_to_shp_value(value: Option<&Value>, ty: &ShpFieldType) -> ShpValue {
    match ty {
        ShpFieldType::Numeric { .. } => {
            let n = match value {
                Some(Value::Number(n)) => n.as_f64(),
                Some(Value::String(s)) => s.trim().parse::<f64>().ok(),
                Some(Value::Bool(b)) => Some(if *b { 1.0 } else { 0.0 }),
                _ => None,
            };
            ShpValue::Number(n)
        }
        ShpFieldType::Character { .. } => {
            let s = match value {
                Some(Value::String(s)) => Some(s.clone()),
                Some(Value::Number(n)) => Some(n.to_string()),
                Some(Value::Bool(b)) => Some(b.to_string()),
                _ => None,
            };
            ShpValue::Text(s)
        }
    }
}

/// 写多边形 shapefile（经 eci-gdal-vector）。`polygons[i]` 与 `records[i]` 平行。
pub fn write_polygons_shapefile(
    path: &Path,
    polygons: &[geo_types::MultiPolygon<f64>],
    fields: &[ShpFieldDef],
    records: &[Vec<ShpValue>],
    prj_wkt: Option<&str>,
) -> Result<()> {
    write_polygon_shapefile(path, polygons, fields, records, prj_wkt).map_err(WaterError::Other)
}
