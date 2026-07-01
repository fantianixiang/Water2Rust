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

    /// 读取源像素窗口 `[col0, col0+w) × [row0, row0+h)` 的原始高程为 `Array2<f32>`。
    ///
    /// 不做重采样：在每个像素的源栅格节点上最近邻取值，命中即为存储原值
    /// （eci-gdal 读端 node-at-corner + 最近邻=round，整数节点精确落回该像素）。
    /// 越界或 nodata 像素置 `f32::NAN`。行主序 `(h, w)`。
    ///
    /// 返回窗口自身的 rasterio Affine 序变换 `[a,b,c,d,e,f]`（PixelIsArea，
    /// 窗口左上角对齐源像素 `(col0,row0)` 的左上角）。
    pub fn read_window_f32(
        &self,
        col0: u32,
        row0: u32,
        w: u32,
        h: u32,
    ) -> (ndarray::Array2<f32>, [f64; 6]) {
        let b = self.inner.bounds();
        let width = self.inner.width().max(1) as f64;
        let height = self.inner.height().max(1) as f64;
        let a = (b.max_x - b.min_x) / width; // 像素宽
        let ph = (b.max_y - b.min_y) / height; // 像素高（正）
        let (origin_x, origin_y) = (b.min_x, b.max_y);
        let mut out = ndarray::Array2::from_elem((h as usize, w as usize), f32::NAN);
        for j in 0..h {
            let row = (row0 + j) as f64;
            let y = origin_y - row * ph;
            for i in 0..w {
                let col = (col0 + i) as f64;
                let x = origin_x + col * a;
                if let Some(v) = self.inner.sample_model_nearest(x, y) {
                    out[(j as usize, i as usize)] = v;
                }
            }
        }
        let transform = [a, 0.0, origin_x + col0 as f64 * a, 0.0, -ph, origin_y - row0 as f64 * ph];
        (out, transform)
    }
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
