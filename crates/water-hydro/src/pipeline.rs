//! 端到端编排（对应 Python `generate_hydro_water_dem` 的**非瓦片 / 无网络**主路径）。
//!
//! 流程：读水体矢量 → DEM 元数据 → 解析工作 CRS → 水体几何投影到工作 CRS →
//! 由水体范围推源网格 ROI 窗口 → 读 ROI 原始 DEM → warp 到工作网格 →
//! 逐多边形 Laplace 水面 + 湖泊压平 + 河床抬升 + 裙边 + 组合 → 重投影回源网格 → 写 GeoTIFF。
//!
//! 说明：默认走无网络模式（无骨架图/纵剖面输入），与 Python 一致；瓦片降采样分支（超预算）
//! 与湖泊源网格再压平（跨 CRS 边缘混叠修补）暂不在此实现，超预算时报错而非静默降采样。

use eci_gdal_core::RasterBounds;
use geo::BoundingRect;
use geo_types::{Geometry, Polygon};
use ndarray::Array2;

use water_io::raster::Dem;
use water_io::vector::read_vector;
use water_io::warp::{reproject_with_max_error, suggested_warp_output, Resampling};
use water_io::geotiff_write::write_geotiff_f32;
use water_core::error::WaterError;
use water_core::Result;

use crate::crs::{proj_from_epsg, reproject_polygon, resolve_working_crs};
use crate::river_pipeline::compute_water_surface;
use crate::{HydroJob, OutputMode};

/// 输出阶段裙边带宽（Python `HYDRO_WATER_SKIRT_PIXELS`，模块常量）。
const HYDRO_WATER_SKIRT_PIXELS: usize = 5;
/// GDAL warp 默认近似变换误差阈值（像素）。复刻 GDAL `errorThreshold=0.125`，
/// 使 DEM 重投影与原 Python(GDAL) 参照 bit 级一致（见 docs/HYDRO.md）。
const GDAL_WARP_MAX_ERROR: f64 = 0.125;
/// 内存整幅栅格像素上限（Python `HYDRO_MAX_FULL_RASTER_PIXELS`）。
const HYDRO_MAX_FULL_RASTER_PIXELS: u64 = 250_000_000;
/// 输出 nodata。
const OUTPUT_NODATA: f64 = -9999.0;

/// 从要素集合抽取多边形 + fclass（对应 Python `_extract_polygons` 展开顺序）。
///
/// `MultiPolygon` 拆为各子多边形，`Polygon` 取自身；每个子多边形继承要素的 fclass。
fn extract_polygons_fclass(fc: &water_io::vector::FeatureCollection) -> (Vec<Polygon<f64>>, Vec<Option<String>>) {
    let mut polys = Vec::new();
    let mut fclass = Vec::new();
    for feat in &fc.features {
        let fc_val = feat
            .properties
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("fclass"))
            .and_then(|(_, v)| v.as_str())
            .and_then(|s| crate::crs::normalize_fclass(Some(s)));
        match &feat.geometry {
            Geometry::Polygon(p) => {
                polys.push(p.clone());
                fclass.push(fc_val);
            }
            Geometry::MultiPolygon(mp) => {
                for p in &mp.0 {
                    polys.push(p.clone());
                    fclass.push(fc_val.clone());
                }
            }
            _ => {}
        }
    }
    (polys, fclass)
}

/// 多边形集合的整体包围盒 `[min_x, min_y, max_x, max_y]`。
fn polygons_bounds(polys: &[Polygon<f64>]) -> Option<[f64; 4]> {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for p in polys {
        if let Some(r) = p.bounding_rect() {
            b[0] = b[0].min(r.min().x);
            b[1] = b[1].min(r.min().y);
            b[2] = b[2].max(r.max().x);
            b[3] = b[3].max(r.max().y);
        }
    }
    if b[0] <= b[2] && b[1] <= b[3] {
        Some(b)
    } else {
        None
    }
}

/// 由工作 CRS 下的水体范围推算源网格 ROI 窗口 `(col0, row0, w, h)`（含 5% / 64px 填充）。
fn compute_source_roi(
    bounds_target: [f64; 4],
    target_epsg: u16,
    src_epsg: u16,
    full_t: [f64; 6],
    full_w: usize,
    full_h: usize,
) -> Result<(u32, u32, u32, u32)> {
    let tp = proj_from_epsg(target_epsg)?;
    let sp = proj_from_epsg(src_epsg)?;
    let sb = eci_gdal_proj::transform::transform_bounds_with_proj(
        RasterBounds { min_x: bounds_target[0], min_y: bounds_target[1], max_x: bounds_target[2], max_y: bounds_target[3] },
        &tp,
        &sp,
    )?;
    // 逆仿射（north-up）：col=(x-c)/a，row=(y-f)/e。
    let (a, e, c, f) = (full_t[0], full_t[4], full_t[2], full_t[5]);
    let col_of = |x: f64| (x - c) / a;
    let row_of = |y: f64| (y - f) / e;
    let (ca, ra) = (col_of(sb.min_x), row_of(sb.max_y));
    let (cb, rb) = (col_of(sb.max_x), row_of(sb.min_y));
    let col0 = ca.min(cb).floor() as i64;
    let row0 = ra.min(rb).floor() as i64;
    let col1 = ca.max(cb).ceil() as i64;
    let row1 = ra.max(rb).ceil() as i64;
    let span = (col1 - col0).max(row1 - row0);
    let pad = (0.05 * span as f64) as i64;
    let pad = pad.max(64);
    let col0 = (col0 - pad).max(0);
    let row0 = (row0 - pad).max(0);
    let col1 = (col1 + pad).min(full_w as i64);
    let row1 = (row1 + pad).min(full_h as i64);
    if col1 <= col0 || row1 <= row0 {
        return Err(WaterError::InvalidInput("水体范围与 DEM 不相交".into()));
    }
    Ok((col0 as u32, row0 as u32, (col1 - col0) as u32, (row1 - row0) as u32))
}

/// 运行端到端 hydro 管线。
pub fn run_hydro_pipeline(job: &HydroJob) -> Result<()> {
    // 1) 读水体矢量 → 多边形 + fclass + 源 CRS。
    let fc = read_vector(&job.water_path)?;
    let water_epsg = fc
        .crs_epsg
        .ok_or_else(|| WaterError::InvalidInput("水体缺少 CRS".into()))? as u16;
    let (polys_src, fclass) = extract_polygons_fclass(&fc);
    if polys_src.is_empty() {
        return Err(WaterError::InvalidInput("水体无多边形要素".into()));
    }

    // 2) DEM 元数据（惰性，不载入像素）。
    let dem = Dem::open(&job.dem_path)?;
    let m = dem.meta();
    let src_epsg = m
        .crs_epsg
        .ok_or_else(|| WaterError::InvalidInput("DEM 缺少 CRS".into()))? as u16;
    let full_t = [m.pixel_size_x, 0.0, m.min_x, 0.0, -m.pixel_size_y, m.max_y];

    // 3) 解析工作 CRS（DEM 为投影坐标系则用之，否则按水体中心估算本地 UTM）。
    let wbounds = polygons_bounds(&polys_src)
        .ok_or_else(|| WaterError::InvalidInput("水体范围为空".into()))?;
    let (target_epsg, _strategy) = resolve_working_crs(Some(src_epsg), wbounds, water_epsg)?;

    // 4) 水体几何投影到工作 CRS。
    let sproj = proj_from_epsg(water_epsg)?;
    let tproj = proj_from_epsg(target_epsg)?;
    let polys_target: Vec<Polygon<f64>> = polys_src
        .iter()
        .map(|p| reproject_polygon(p, &sproj, &tproj))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let tbounds = polygons_bounds(&polys_target)
        .ok_or_else(|| WaterError::InvalidInput("投影后水体范围为空".into()))?;

    // 5) 源网格 ROI 窗口。
    let (col0, row0, ww, hh) =
        compute_source_roi(tbounds, target_epsg, src_epsg, full_t, m.width as usize, m.height as usize)?;

    // 6) 读 ROI 原始 DEM（源网格，不重采样）。
    let (dem_src, src_win_t) = dem.read_window_f32(col0, row0, ww, hh)?;

    // 7) warp 到工作网格（建议输出网格 + 双线性重采样）。
    let warp = suggested_warp_output(src_epsg, target_epsg, src_win_t, ww as usize, hh as usize)?;
    let work_pixels = warp.width as u64 * warp.height as u64;
    if work_pixels > HYDRO_MAX_FULL_RASTER_PIXELS || (ww as u64 * hh as u64) > HYDRO_MAX_FULL_RASTER_PIXELS {
        return Err(WaterError::InvalidInput(format!(
            "ROI 超出内存预算（工作网格 {work_pixels} px），瓦片路径尚未实现",
        )));
    }
    let dem_work_f32 = reproject_with_max_error(
        &dem_src,
        src_win_t,
        src_epsg,
        m.nodata,
        warp.transform,
        warp.width,
        warp.height,
        target_epsg,
        Resampling::Bilinear,
        GDAL_WARP_MAX_ERROR,
    )?;
    let dem_work = dem_work_f32.mapv(|v| v as f64);

    // 8) 工作网格水面 + 写入掩膜（含裙边）。生产用恒等置换作为 medial_axis tiebreaker。
    let (surface_work, mask_work) = compute_water_surface(
        &warp.transform,
        &dem_work,
        &polys_target,
        &fclass,
        job.all_touched,
        HYDRO_WATER_SKIRT_PIXELS,
        |_idx, n| (0..n).collect::<Vec<usize>>(),
    );

    // 9) 只把水面 + 掩膜投回源 ROI 网格（B 方案）。
    //    背景 DEM **不重采样**：非水像素直接用精确源 DEM（下一步组合），
    //    避免 Python「工作网格组合后整体投回」带来的双重重采样 + GDAL 分块伪影
    //    （详见 docs/HYDRO.md「背景 DEM 处理（方案 B）」）。
    let masked_surface = Array2::from_shape_fn(surface_work.dim(), |(r, c)| {
        if mask_work[(r, c)] {
            surface_work[(r, c)]
        } else {
            f32::NAN
        }
    });
    let surf_src = reproject_with_max_error(
        &masked_surface,
        warp.transform,
        target_epsg,
        None,
        src_win_t,
        ww as usize,
        hh as usize,
        src_epsg,
        Resampling::Bilinear,
        GDAL_WARP_MAX_ERROR,
    )?;
    let mask_f32 = mask_work.mapv(|b| if b { 1.0f32 } else { 0.0f32 });
    let mask_src = reproject_with_max_error(
        &mask_f32,
        warp.transform,
        target_epsg,
        None,
        src_win_t,
        ww as usize,
        hh as usize,
        src_epsg,
        Resampling::Nearest,
        GDAL_WARP_MAX_ERROR,
    )?;

    // 10) 源网格组合：水像素用投回水面；其余按模式用**精确源 DEM** 或 nodata。写 ROI GeoTIFF。
    let with_dem = matches!(job.output_mode, OutputMode::WaterSurfaceWithDem);
    let out_final = Array2::from_shape_fn((hh as usize, ww as usize), |(r, c)| {
        let is_water = mask_src[(r, c)] >= 0.5 && surf_src[(r, c)].is_finite();
        if is_water {
            surf_src[(r, c)]
        } else if with_dem && dem_src[(r, c)].is_finite() {
            dem_src[(r, c)]
        } else {
            OUTPUT_NODATA as f32
        }
    });
    let is_geo = proj_from_epsg(src_epsg)?.is_latlong();
    write_geotiff_f32(&job.output_path, &out_final, src_win_t, src_epsg, is_geo, Some(OUTPUT_NODATA))?;
    Ok(())
}
