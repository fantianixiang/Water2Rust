//! 栅格 IO（GeoTIFF / DEM），经 `eci-gdal-geotiff`。替代 rasterio。
//!
//! DEM 以**惰性**方式打开（像 rasterio.open）：打开只读元数据，不解码像素；
//! `read_window_f32` 经块级解码按需读取窗口；点采样 `sample_*` 首次调用才触发
//! 整幅解码并缓存。避免把整幅（可达数 GB）像素在打开时就载入内存。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use eci_gdal_core::RasterBounds;
use eci_gdal_geotiff::api::load_dem_geotiff_window;
use eci_gdal_geotiff::dem_source::DemSource;
use eci_gdal_geotiff::source::GeoTiffSource;
use water_core::error::{Result, WaterError};

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

/// DEM 数据源封装（惰性：打开只读元数据，窗口/采样按需），经 `eci-gdal-geotiff`。
pub struct Dem {
    path: PathBuf,
    meta: DemMeta,
    /// 仅点采样（sample_*）时按需全量解码并缓存；镶嵌源于打开时预填。
    source: OnceLock<DemSource>,
}

/// 从（镶嵌）`DemSource` 构建元数据。
fn meta_from_source(inner: &DemSource) -> DemMeta {
    let b = inner.bounds();
    let (width, height) = (inner.width(), inner.height());
    DemMeta {
        width,
        height,
        crs_epsg: inner.crs().epsg_code().map(u32::from),
        min_x: b.min_x,
        min_y: b.min_y,
        max_x: b.max_x,
        max_y: b.max_y,
        nodata: inner.nodata_value(),
        pixel_size_x: if width > 0 { b.width() / width as f64 } else { 0.0 },
        pixel_size_y: if height > 0 { b.height() / height as f64 } else { 0.0 },
        is_mosaic: inner.is_mosaic(),
        tile_count: inner.tile_count(),
    }
}

impl Dem {
    /// 打开单个 GeoTIFF 文件或目录镶嵌。单文件仅读元数据（惰性）。
    pub fn open(path: &Path) -> Result<Self> {
        if path.is_dir() {
            // 镶嵌：DemSource 索引轻量，直接用于元数据 + 采样。
            let inner = DemSource::open(path)?;
            let meta = meta_from_source(&inner);
            let source = OnceLock::new();
            let _ = source.set(inner);
            Ok(Self { path: path.to_path_buf(), meta, source })
        } else {
            // 单文件：惰性只读元数据（不解码像素），像 rasterio.open。
            let src = GeoTiffSource::open_with_nodata(path, true)?;
            let b = src.bounds();
            let (width, height) = (src.width(), src.height());
            let meta = DemMeta {
                width,
                height,
                crs_epsg: src.crs().epsg_code().map(u32::from),
                min_x: b.min_x,
                min_y: b.min_y,
                max_x: b.max_x,
                max_y: b.max_y,
                nodata: src.nodata_value,
                pixel_size_x: if width > 0 { b.width() / width as f64 } else { 0.0 },
                pixel_size_y: if height > 0 { b.height() / height as f64 } else { 0.0 },
                is_mosaic: false,
                tile_count: 1,
            };
            Ok(Self { path: path.to_path_buf(), meta, source: OnceLock::new() })
        }
    }

    /// 读取元数据（已在打开时缓存，不触发解码）。
    pub fn meta(&self) -> DemMeta {
        self.meta.clone()
    }

    /// 惰性获取全量解码数据源（首次调用触发整幅解码，供点采样用）。
    fn source(&self) -> Result<&DemSource> {
        if let Some(s) = self.source.get() {
            return Ok(s);
        }
        let s = DemSource::open(&self.path)?;
        let _ = self.source.set(s);
        Ok(self.source.get().expect("just set"))
    }

    /// 以 Web Mercator（EPSG:3857）米坐标双线性采样高程（首次调用触发整幅解码）。
    ///
    /// DEM 为 3857 / 4326 时内部自动换算；其它 CRS 需后续扩展 `CoordTransform`。
    /// 越界或 nodata 返回 `None`。
    pub fn sample_3857_bilinear(&self, x: f64, y: f64) -> Option<f32> {
        self.source().ok()?.sample_mercator_bilinear(x, y, None)
    }

    /// 以 DEM 源（native）CRS 坐标双线性采样高程（首次调用触发整幅解码）。
    ///
    /// 例如 DEM 为 EPSG:4326 时，传入经度 `x`、纬度 `y`。越界或 nodata 返回 `None`。
    pub fn sample_model_bilinear(&self, x: f64, y: f64) -> Option<f32> {
        self.source().ok()?.sample_model_bilinear(x, y)
    }

    /// 读取源像素窗口 `[col0, col0+w) × [row0, row0+h)` 的原始高程为 `Array2<f32>`。
    ///
    /// 与 rasterio `read(1, window=...)` 逻辑一致：经 `load_dem_geotiff_window` **块级解码**
    /// 只读取覆盖窗口的瓦片、直接取原始 f32（不重采样），而非逐像素采样——大幅提速。
    /// nodata 像素置 `f32::NAN`，越界补 `NaN`。行主序 `(h, w)`。
    ///
    /// 返回窗口自身的 rasterio Affine 序变换 `[a,b,c,d,e,f]`（PixelIsArea，
    /// 窗口左上角对齐源像素 `(col0,row0)` 的左上角）。
    pub fn read_window_f32(
        &self,
        col0: u32,
        row0: u32,
        w: u32,
        h: u32,
    ) -> Result<(ndarray::Array2<f32>, [f64; 6])> {
        let a = self.meta.pixel_size_x; // 像素宽
        let ph = self.meta.pixel_size_y; // 像素高（正）
        let (origin_x, origin_y) = (self.meta.min_x, self.meta.max_y);
        let transform = [a, 0.0, origin_x + col0 as f64 * a, 0.0, -ph, origin_y - row0 as f64 * ph];

        // 请求窗口对应的模型坐标范围（对齐像素边界）。
        let win_bounds = RasterBounds {
            min_x: origin_x + col0 as f64 * a,
            max_x: origin_x + (col0 + w) as f64 * a,
            min_y: origin_y - (row0 + h) as f64 * ph,
            max_y: origin_y - row0 as f64 * ph,
        };

        // 块级解码：镶嵌源无单一路径，退回逐像素采样；单文件走快路径。
        if self.meta.is_mosaic {
            let src = self.source()?;
            return Ok((
                read_window_sampled(src, col0, row0, w, h, a, ph, origin_x, origin_y),
                transform,
            ));
        }
        let win = load_dem_geotiff_window(&self.path, win_bounds)
            .map_err(|e| WaterError::Other(anyhow::anyhow!("DEM 窗口读取失败: {e}")))?;

        // 返回窗口在整幅栅格中的像素起点（可能因 <90% 快路径精确对齐，或整幅回退时为 0）。
        let rx0 = ((win.bounds.min_x - origin_x) / a).round() as i64;
        let ry0 = ((origin_y - win.bounds.max_y) / ph).round() as i64;
        let (rw, rh) = (win.width as i64, win.height as i64);
        let nodata = win.nodata_value;
        let is_nodata = |v: f32| match nodata {
            Some(nd) if nd.is_nan() => v.is_nan(),
            Some(nd) => (v as f64 - nd).abs() <= 0.0,
            None => false,
        };

        let mut out = ndarray::Array2::from_elem((h as usize, w as usize), f32::NAN);
        for j in 0..h as i64 {
            let sr = row0 as i64 + j - ry0;
            if sr < 0 || sr >= rh {
                continue;
            }
            for i in 0..w as i64 {
                let sc = col0 as i64 + i - rx0;
                if sc < 0 || sc >= rw {
                    continue;
                }
                let v = win.values[(sr * rw + sc) as usize];
                if !is_nodata(v) {
                    out[(j as usize, i as usize)] = v;
                }
            }
        }
        Ok((out, transform))
    }
}

/// 逐像素最近邻采样回退（仅镶嵌源用；单文件走 `read_window_f32` 块级快路径）。
#[allow(clippy::too_many_arguments)]
fn read_window_sampled(
    src: &DemSource,
    col0: u32,
    row0: u32,
    w: u32,
    h: u32,
    a: f64,
    ph: f64,
    origin_x: f64,
    origin_y: f64,
) -> ndarray::Array2<f32> {
    let mut out = ndarray::Array2::from_elem((h as usize, w as usize), f32::NAN);
    for j in 0..h {
        let y = origin_y - (row0 + j) as f64 * ph;
        for i in 0..w {
            let x = origin_x + (col0 + i) as f64 * a;
            if let Some(v) = src.sample_model_nearest(x, y) {
                out[(j as usize, i as usize)] = v;
            }
        }
    }
    out
}

/// 多边形栅格化为布尔掩膜（含 GDAL `ALL_TOUCHED`）——已上游至 eci-gdal-alg，此处 re-export。
pub use eci_gdal_alg::rasterize_polygon_mask;
