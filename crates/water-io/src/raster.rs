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

/// 将单个多边形栅格化为布尔掩膜。
///
/// - `all_touched = false`：像素中心 even-odd 填充（经 `eci-gdal-alg`，对标 GDAL 默认 burn）。
/// - `all_touched = true`：在上述填充基础上，叠加**边界所经全部像素**——忠实移植 GDAL
///   `GDALdllImageLineAllTouched`（`alg/llrasterize.cpp`），得到与 GDAL `ALL_TOUCHED=TRUE` 一致的结果。
///
/// `transform` 为 GDAL 6 元仿射 `[a, b, c, d, e, f]`（要求北向上，即 b = d = 0）。
/// 返回形状 `(height, width)` 的布尔数组，行主序。
pub fn rasterize_polygon_mask(
    polygon: &geo_types::Polygon<f64>,
    transform: &[f64; 6],
    width: u32,
    height: u32,
    all_touched: bool,
) -> ndarray::Array2<bool> {
    let [a, _b, c, _d, e, f] = *transform;
    let bounds = eci_gdal_core::RasterBounds {
        min_x: c,
        min_y: f + height as f64 * e,
        max_x: c + width as f64 * a,
        max_y: f,
    };
    let mp = geo_types::MultiPolygon(vec![polygon.clone()]);
    let flat = eci_gdal_alg::rasterize_scope_mask(&mp, bounds, width, height);

    let (w, h) = (width as usize, height as usize);
    let mut out = ndarray::Array2::<bool>::from_elem((h, w), false);
    for (i, &v) in flat.iter().enumerate() {
        if v != 0 {
            out[(i / w, i % w)] = true;
        }
    }

    if all_touched {
        burn_polygon_edges_all_touched(polygon, transform, w, h, &mut out);
    }
    out
}

/// GDAL `GDALdllImageLineAllTouched` 的检测常量（对齐像素坐标的几何判定阈值）。
const EPSILON_INTERSECT_ONLY: f64 = 1e-4;

/// 烧录多边形所有环边界所经的像素（GDAL all_touched 边界层）。
fn burn_polygon_edges_all_touched(
    polygon: &geo_types::Polygon<f64>,
    transform: &[f64; 6],
    width: usize,
    height: usize,
    mask: &mut ndarray::Array2<bool>,
) {
    let [a, _b, c, _d, e, f] = *transform;
    // 用 GDAL 逆地理变换的求值顺序 col = (-c/a) + x*(1/a)，与 rasterio/GDAL 保持一致的浮点结果，
    // 避免 (x-c)/a 在整数顶点处因舍入落到 1.999… → floor 少一格。
    let (inv_a, inv_e) = (1.0 / a, 1.0 / e);
    let (inv_c, inv_f) = (-c / a, -f / e);
    let to_px = |x: f64, y: f64| -> (f64, f64) { (inv_c + x * inv_a, inv_f + y * inv_e) };

    let mut rings: Vec<&geo_types::LineString<f64>> = vec![polygon.exterior()];
    rings.extend(polygon.interiors().iter());
    for ring in rings {
        let pts: Vec<(f64, f64)> = ring.coords().map(|co| to_px(co.x, co.y)).collect();
        for seg in pts.windows(2) {
            burn_segment_all_touched(seg[0], seg[1], width, height, mask);
        }
    }
}

/// 忠实移植 GDAL `GDALdllImageLineAllTouched` 的单段像素烧录（像素坐标，二值掩膜）。
fn burn_segment_all_touched(
    p0: (f64, f64),
    p1: (f64, f64),
    width: usize,
    height: usize,
    mask: &mut ndarray::Array2<bool>,
) {
    let (nx, ny) = (width as f64, height as f64);
    let (mut dfx, mut dfy) = p0;
    let (mut dfxe, mut dfye) = p1;

    // 完全在目标区域外的段直接跳过
    if (dfy < 0.0 && dfye < 0.0)
        || (dfy > ny && dfye > ny)
        || (dfx < 0.0 && dfxe < 0.0)
        || (dfx > nx && dfxe > nx)
    {
        return;
    }

    // 交换使 dfx <= dfxe（从左到右推进）
    if dfx > dfxe {
        std::mem::swap(&mut dfx, &mut dfxe);
        std::mem::swap(&mut dfy, &mut dfye);
    }

    let mut burn = |iy: i64, ix: i64| {
        if ix >= 0 && (ix as usize) < width && iy >= 0 && (iy as usize) < height {
            mask[(iy as usize, ix as usize)] = true;
        }
    };

    // 竖直线特例
    if (dfx - dfxe).abs() < 0.01 {
        // bIntersectOnly=TRUE：与像素网格对齐的整数轴向边由填充层处理，此处跳过
        // （对应 GDAL issue #7523/#6414 的修复）。
        if (dfx - dfx.round()).abs() < EPSILON_INTERSECT_ONLY
            && (dfxe - dfxe.round()).abs() < EPSILON_INTERSECT_ONLY
        {
            return;
        }
        if dfye < dfy {
            std::mem::swap(&mut dfy, &mut dfye);
        }
        let ix = dfxe.floor() as i64;
        if ix < 0 || ix >= width as i64 {
            return;
        }
        let mut iy = (dfy.floor() as i64).max(0);
        let iyend = ((dfye - EPSILON_INTERSECT_ONLY).floor() as i64).min(height as i64 - 1);
        while iy <= iyend {
            burn(iy, ix);
            iy += 1;
        }
        return;
    }

    // 水平线特例
    if (dfy - dfye).abs() < 0.01 {
        // bIntersectOnly=TRUE：跳过与像素网格对齐的整数水平边。
        if (dfy - dfy.round()).abs() < EPSILON_INTERSECT_ONLY
            && (dfye - dfye.round()).abs() < EPSILON_INTERSECT_ONLY
        {
            return;
        }
        let iy = dfy.floor() as i64;
        if iy < 0 || iy >= height as i64 {
            return;
        }
        let mut ix = (dfx.floor() as i64).max(0);
        let ixend = ((dfxe - EPSILON_INTERSECT_ONLY).floor() as i64).min(width as i64 - 1);
        while ix <= ixend {
            burn(iy, ix);
            ix += 1;
        }
        return;
    }

    // 一般斜线：先按 X、Y 裁剪到栅格范围，再逐像素步进
    let slope = (dfye - dfy) / (dfxe - dfx);
    if dfxe > nx {
        dfye -= (dfxe - nx) * slope;
        dfxe = nx;
    }
    if dfx < 0.0 {
        dfy += (0.0 - dfx) * slope;
        dfx = 0.0;
    }
    if dfye > dfy {
        if dfy < 0.0 {
            dfx += (0.0 - dfy) / slope;
            dfy = 0.0;
        }
        if dfye >= ny {
            dfxe += (dfye - ny) / slope;
            if dfxe > nx {
                dfxe = nx;
            }
        }
    } else {
        if dfy >= ny {
            dfx += (ny - dfy) / slope;
            dfy = ny;
        }
        if dfye < 0.0 {
            dfxe -= (dfye - 0.0) / slope;
        }
    }

    while dfx >= 0.0 && dfx < dfxe {
        let ix = dfx.floor() as i64;
        let iy = dfy.floor() as i64;
        if iy >= 0 && iy < height as i64 {
            burn(iy, ix);
        }
        let mut step_x = (dfx + 1.0).floor() - dfx;
        let mut step_y = step_x * slope;
        if (dfy + step_y).floor() as i64 == iy {
            dfx += step_x;
            dfy += step_y;
        } else if slope < 0.0 {
            step_y = iy as f64 - dfy;
            if step_y > -0.000_000_001 {
                step_y = -0.000_000_001;
            }
            step_x = step_y / slope;
            dfx += step_x;
            dfy += step_y;
        } else {
            step_y = (iy + 1) as f64 - dfy;
            if step_y < 0.000_000_001 {
                step_y = 0.000_000_001;
            }
            step_x = step_y / slope;
            dfx += step_x;
            dfy += step_y;
        }
    }
}
