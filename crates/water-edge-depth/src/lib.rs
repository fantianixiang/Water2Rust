//! water-edge-depth — 水边深度导出。
//!
//! 对应原 Python `waters/pipeline.py`（`export_water_edge_depth` /
//! `export_water_edge_depth_from_gdf`）。
//! **替换的 Python 库**：rasterio、shapely、scipy。
//!
//! 为水体要素附加 edge/depth 字段并写出 shapefile。默认参数见
//! `water_core::settings::default_edge_depth`。
//!
//! 当前为骨架占位。

use std::path::Path;
use water_core::error::{Result, WaterError};
use water_io::vector::FeatureCollection;

/// 水边深度导出参数。
#[derive(Debug, Clone)]
pub struct EdgeDepthOptions {
    pub all_touched: bool,
}

/// 从矢量集合导出水边深度（占位）。对应 `export_water_edge_depth_from_gdf`。
pub fn export_water_edge_depth_from_fc(
    _water: &FeatureCollection,
    _opts: &EdgeDepthOptions,
) -> Result<FeatureCollection> {
    Err(WaterError::NotImplemented("water_edge_depth::export_water_edge_depth_from_fc"))
}

/// 从文件导出水边深度并写出（占位）。对应 `export_water_edge_depth`。
pub fn export_water_edge_depth(
    _water_path: &Path,
    _output_path: &Path,
    _opts: &EdgeDepthOptions,
) -> Result<()> {
    Err(WaterError::NotImplemented("water_edge_depth::export_water_edge_depth"))
}
