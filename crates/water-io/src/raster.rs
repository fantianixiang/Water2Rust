//! 栅格 IO（GeoTIFF / DEM）。替代 rasterio。接入后改用 `eci-gdal-geotiff` + `eci-gdal-proj`。

use std::path::Path;
use water_core::error::{Result, WaterError};

/// 栅格地理参考信息（仿射变换 + CRS + 尺寸）。
#[derive(Debug, Clone)]
pub struct RasterMeta {
    pub width: usize,
    pub height: usize,
    /// 6 元仿射变换 [a, b, c, d, e, f]（GDAL/rasterio 约定）。
    pub transform: [f64; 6],
    /// CRS 的 EPSG 代码。
    pub crs_epsg: Option<u32>,
    /// nodata 值。
    pub nodata: Option<f64>,
}

/// 读取的 DEM 栅格：元数据 + 单波段 f32 数据。
#[derive(Debug, Clone)]
pub struct DemRaster {
    pub meta: RasterMeta,
    pub data: ndarray::Array2<f32>,
}

/// 读取 GeoTIFF DEM（占位）。
pub fn read_dem(_path: &Path) -> Result<DemRaster> {
    Err(WaterError::NotImplemented("water_io::raster::read_dem (待接入 eci-gdal-geotiff)"))
}

/// 写出 GeoTIFF（占位）。
pub fn write_geotiff(_path: &Path, _raster: &DemRaster) -> Result<()> {
    Err(WaterError::NotImplemented("water_io::raster::write_geotiff (待接入 eci-gdal-geotiff)"))
}
