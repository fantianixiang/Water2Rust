//! water-fclass — 水体 fclass 语义分类。
//!
//! 对应原 Python `waters/fclass/`（`assign_water_fclass` / `compute_water_fclass_gdf`）。
//! **替换的 Python 库**：geopandas、shapely、fiona。
//!
//! 将源水体多边形与已准备好的水体参考（GeoPackage：stream/dock/river/lake/
//! reservoir/sea/untyped 图层）做空间关联，赋予语义类别。
//!
//! 当前为骨架占位。

use std::path::Path;
use water_core::error::{Result, WaterError};
use water_io::vector::FeatureCollection;

/// fclass 分类参数。
#[derive(Debug, Clone)]
pub struct FclassOptions {
    /// 准备好的水体参考 GeoPackage 路径。
    pub reference_path: std::path::PathBuf,
    /// 仅检测语义过渡位置（对应 `--fclass-transition-only`）。
    pub transition_only: bool,
}

/// 为水体多边形赋予 fclass（占位）。
/// 对应 `assign_water_fclass` / `compute_water_fclass_gdf`。
pub fn assign_water_fclass(
    _water: &FeatureCollection,
    _opts: &FclassOptions,
) -> Result<FeatureCollection> {
    Err(WaterError::NotImplemented("water_fclass::assign_water_fclass"))
}

/// 分类并写出结果（占位）。对应 `write_water_fclass_output`。
pub fn run_fclass(_water_path: &Path, _output_path: &Path, _opts: &FclassOptions) -> Result<()> {
    Err(WaterError::NotImplemented("water_fclass::run_fclass"))
}
