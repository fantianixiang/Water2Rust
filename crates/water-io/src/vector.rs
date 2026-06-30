//! 矢量 IO（Shapefile / GeoJSON / GeoPackage）。替代 geopandas / fiona。
//! 接入后改用 `eci-gdal-vector`。

use std::path::Path;
use water_core::error::{Result, WaterError};

/// 一个带属性的矢量要素。
#[derive(Debug, Clone)]
pub struct Feature {
    pub geometry: geo_types::Geometry<f64>,
    pub properties: std::collections::BTreeMap<String, serde_json::Value>,
}

/// 一个图层：要素集合 + CRS。
#[derive(Debug, Clone, Default)]
pub struct FeatureCollection {
    pub features: Vec<Feature>,
    pub crs_epsg: Option<u32>,
}

/// 读取矢量图层（占位）。
pub fn read_vector(_path: &Path) -> Result<FeatureCollection> {
    Err(WaterError::NotImplemented("water_io::vector::read_vector (待接入 eci-gdal-vector)"))
}

/// 写出矢量图层（占位）。
pub fn write_vector(_path: &Path, _fc: &FeatureCollection) -> Result<()> {
    Err(WaterError::NotImplemented("water_io::vector::write_vector (待接入 eci-gdal-vector)"))
}
