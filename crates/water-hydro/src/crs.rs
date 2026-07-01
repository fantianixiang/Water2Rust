//! 工作 CRS 解析与局地 UTM 估计（对应 Python `_resolve_hydro_working_crs` +
//! `estimate_local_utm_crs_from_bounds`）。
//!
//! **GIS 能力经 eci-gdal-proj 引入**（`RasterCrs::proj()` → proj4rs，`transform_point`）。
//!
//! 策略（`_resolve_hydro_working_crs`）：DEM 为**投影坐标系**时直接用其作工作 CRS
//! （避免工作↔源重投影伪影）；否则（地理坐标，如 4326）由水体范围估计**局地 UTM**。

use anyhow::{bail, Result};
use eci_gdal_core::RasterCrs;
use eci_gdal_proj::transform::transform_point;

/// 工作 CRS 解析策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HydroCrsStrategy {
    /// DEM 原生投影 CRS。
    SourceProjected,
    /// 地理坐标回退到局地 UTM。
    LocalUtm,
}

/// 由中心经纬度确定 UTM 带并给出 EPSG（对应 estimate_local_utm 的带号逻辑）。
///
/// `zone = floor((lon+180)/6)+1`（夹到 `[1,60]`）；北半球 `32600+zone`、南半球 `32700+zone`。
/// 纬度超出标准 UTM 覆盖 `[-80, 84]` 报错。
pub fn utm_epsg_from_center(lon_center: f64, lat_center: f64) -> Result<u16> {
    if !lon_center.is_finite() || !lat_center.is_finite() {
        bail!("无法从非有限的中心经纬度估计局地 UTM");
    }
    if lat_center < -80.0 || lat_center > 84.0 {
        bail!("范围中心纬度 {lat_center} 超出标准 UTM 覆盖 [-80, 84]");
    }
    let mut zone = ((lon_center + 180.0) / 6.0).floor() as i64 + 1;
    zone = zone.clamp(1, 60);
    let epsg = if lat_center >= 0.0 { 32600 + zone } else { 32700 + zone };
    Ok(epsg as u16)
}

/// 将 `[minx, miny, maxx, maxy]` 从 `src`→`dst` 投影，沿四边各密化 `densify_pts` 点取包围盒。
///
/// 对应 rasterio `transform_bounds(..., densify_pts=N)`：非线性投影下边界会弯曲，
/// 密化采样以更准地估计变换后 bbox。
fn transform_bounds_densify(
    src: &eci_gdal_proj::Proj,
    dst: &eci_gdal_proj::Proj,
    bounds: [f64; 4],
    densify_pts: usize,
) -> Result<[f64; 4]> {
    let [minx, miny, maxx, maxy] = bounds;
    let (mut lo_x, mut lo_y) = (f64::INFINITY, f64::INFINITY);
    let (mut hi_x, mut hi_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    let n = densify_pts + 1;
    let mut acc = |x: f64, y: f64| -> Result<()> {
        let (px, py) = transform_point(src, dst, x, y)?;
        lo_x = lo_x.min(px);
        lo_y = lo_y.min(py);
        hi_x = hi_x.max(px);
        hi_y = hi_y.max(py);
        Ok(())
    };
    for i in 0..=n {
        let fx = i as f64 / n as f64;
        let x = minx + (maxx - minx) * fx;
        let y = miny + (maxy - miny) * fx;
        // 上/下边（y 固定）与 左/右边（x 固定）。
        acc(x, miny)?;
        acc(x, maxy)?;
        acc(minx, y)?;
        acc(maxx, y)?;
    }
    if lo_x >= hi_x || lo_y >= hi_y {
        bail!("bounds 投影后退化");
    }
    Ok([lo_x, lo_y, hi_x, hi_y])
}

/// 由水体范围（源 CRS）估计局地 UTM 的 EPSG（对应 `estimate_local_utm_crs_from_bounds`）。
///
/// 将 bounds 转到 4326（densify 21）取中心经纬度，再定 UTM 带。
pub fn estimate_local_utm_epsg(bounds: [f64; 4], source_epsg: u16) -> Result<u16> {
    if !bounds.iter().all(|v| v.is_finite()) {
        bail!("无法从非有限 bounds 估计局地 UTM");
    }
    let src = RasterCrs::Epsg(source_epsg).proj()?;
    let dst = RasterCrs::Epsg(4326).proj()?;
    let [lon_min, lat_min, lon_max, lat_max] = transform_bounds_densify(&src, &dst, bounds, 21)?;
    let lon_center = (lon_min + lon_max) / 2.0;
    let lat_center = (lat_min + lat_max) / 2.0;
    utm_epsg_from_center(lon_center, lat_center)
}

/// 解析 hydro 工作 CRS（对应 `_resolve_hydro_working_crs`）。
///
/// `dem_epsg`：DEM 原生 EPSG（`None` 表示未知/无）。DEM 为投影坐标系时用之；
/// 否则由 `water_bounds`（源 `water_epsg`）估计局地 UTM。返回 `(工作 EPSG, 策略)`。
pub fn resolve_working_crs(
    dem_epsg: Option<u16>,
    water_bounds: [f64; 4],
    water_epsg: u16,
) -> Result<(u16, HydroCrsStrategy)> {
    if let Some(de) = dem_epsg {
        let dem_proj = RasterCrs::Epsg(de).proj()?;
        // 投影坐标系 = 非经纬度。
        if !dem_proj.is_latlong() {
            return Ok((de, HydroCrsStrategy::SourceProjected));
        }
    }
    let utm = estimate_local_utm_epsg(water_bounds, water_epsg)?;
    Ok((utm, HydroCrsStrategy::LocalUtm))
}
