//! 栅格 IO（GeoTIFF / DEM），经 `eci-gdal-geotiff`。替代 rasterio。
//!
//! DEM 以惰性数据源（`DemSource`）方式打开：只读元数据 + 按需瓦片采样，
//! 不把整幅（可达数 GB）像素载入内存。

use std::path::Path;

use eci_gdal_geotiff::dem_source::DemSource;
use water_core::error::Result;

/// DEM 元数据（惰性，不含像素）。
#[derive(Debug, Clone)]
pub struct DemMeta {
    pub width: u32,
    pub height: u32,
    /// CRS 的 EPSG 代码（若可识别）。
    pub crs_epsg: Option<u32>,
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
    /// nodata 值。
    pub nodata: Option<f64>,
    /// 像素尺寸（源 CRS 单位）。
    pub pixel_size_x: f64,
    pub pixel_size_y: f64,
    /// 是否为多瓦片镶嵌。
    pub is_mosaic: bool,
    pub tile_count: usize,
}

/// DEM 数据源封装（惰性瓦片访问 + 采样），经 `eci-gdal-geotiff`。
pub struct Dem {
    inner: DemSource,
}

impl Dem {
    /// 打开单个 GeoTIFF 文件或目录镶嵌。
    pub fn open(path: &Path) -> Result<Self> {
        let inner = DemSource::open(path)?;
        Ok(Self { inner })
    }

    /// 读取元数据。
    pub fn meta(&self) -> DemMeta {
        let b = self.inner.bounds();
        let width = self.inner.width();
        let height = self.inner.height();
        DemMeta {
            width,
            height,
            crs_epsg: self.inner.crs().epsg_code().map(u32::from),
            min_x: b.min_x,
            min_y: b.min_y,
            max_x: b.max_x,
            max_y: b.max_y,
            nodata: self.inner.nodata_value(),
            pixel_size_x: if width > 0 { b.width() / width as f64 } else { 0.0 },
            pixel_size_y: if height > 0 { b.height() / height as f64 } else { 0.0 },
            is_mosaic: self.inner.is_mosaic(),
            tile_count: self.inner.tile_count(),
        }
    }

    /// 以 Web Mercator（EPSG:3857）米坐标双线性采样高程。
    ///
    /// DEM 为 3857 / 4326 时内部自动换算；其它 CRS 需后续扩展 `CoordTransform`。
    /// 越界或 nodata 返回 `None`。
    pub fn sample_3857_bilinear(&self, x: f64, y: f64) -> Option<f32> {
        self.inner.sample_mercator_bilinear(x, y, None)
    }

    /// 以 DEM 源（native）CRS 坐标双线性采样高程。
    ///
    /// 例如 DEM 为 EPSG:4326 时，传入经度 `x`、纬度 `y`。越界或 nodata 返回 `None`。
    pub fn sample_model_bilinear(&self, x: f64, y: f64) -> Option<f32> {
        self.inner.sample_model_bilinear(x, y)
    }
}
