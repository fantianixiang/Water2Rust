//! water-io — 栅格与矢量 IO，统一经 eci-gdal（纯 Rust GDAL）。
//!
//! 对应原 Python `waters/io.py`、`hydro/hydro_raster_io.py` 等。
//! **替换的 Python 库**：rasterio（栅格）、geopandas/fiona（矢量）、pyproj（投影）。
//!
//! ⚠️ eci-gdal 尚未接入（见 AGENTS.md「eci-gdal 接入」）。当前为桩实现，
//! 接入后将 `raster` / `vector` 模块切换为真实 eci-gdal 调用。

pub mod raster;
pub mod vector;
