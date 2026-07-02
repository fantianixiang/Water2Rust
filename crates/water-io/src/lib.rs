//! water-io — 栅格与矢量 IO，统一经 eci-gdal（纯 Rust GDAL）。
//!
//! 对应原 Python `waters/io.py`、`hydro/hydro_raster_io.py` 等。
//! **替换的 Python 库**：rasterio（栅格）、geopandas/fiona（矢量）、pyproj（投影）。
//!
//! warp / 重投影 / GeoTIFF 写出 / 多边形栅格化等 GIS 能力已**上游至 eci-gdal**
//! （`eci-gdal-alg` / `eci-gdal-geotiff`），本 crate 仅保留 IO 门面并 re-export，
//! 避免重复实现（单一来源）。

pub mod raster;
pub mod vector;

/// warp：建议输出网格 + 重投影重采样 —— 已上游至 `eci-gdal-alg`，此处 re-export 以保持既有调用路径。
pub mod warp {
    pub use eci_gdal_alg::{
        reproject, reproject_with_max_error, suggested_warp_output, Resampling, WarpOutput,
    };
}

/// GeoTIFF float32 写出 —— 已上游至 `eci-gdal-geotiff`，此处 re-export。
pub mod geotiff_write {
    pub use eci_gdal_geotiff::write_geotiff_f32;
}
