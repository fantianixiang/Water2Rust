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
use rayon::prelude::*;

use water_io::raster::{Dem, DemMeta};
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
/// 瓦片边长（源像素）。Python `HYDRO_TILED_TILE_SIZE`。
const HYDRO_TILED_TILE_SIZE: u32 = 8192;
/// 瓦片 padding（源像素）。Python `max(HYDRO_TILED_SURFACE_PADDING_PX, 64)`。
const HYDRO_TILED_PAD_PX: u32 = 64;
/// 瓦片级并行线程数（保持全分辨率，仅瓦片之间并行；faer 求解内部为 `Par::Seq`，不嵌套竞争）。
const HYDRO_TILE_WORKERS: usize = 4;

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

/// 处理一个 padded 源窗口，返回其 **core（去 padding）子窗口** 的组合结果 `(core_h, core_w)`。
///
/// 流程（对应 Python `_process_water_tile_body` / 非瓦片主路径的算法段）：
/// 读 padded DEM → warp 到工作网格 → `compute_water_surface`（solve+lake+floor+skirt）→
/// 只投回水面+掩膜到 padded 源窗口 → 与**精确源 DEM** 组合（方案 B）→ 提取 core。
/// NaN → `OUTPUT_NODATA`。`pad == core` 时即非瓦片整窗处理。
#[allow(clippy::too_many_arguments)]
fn process_window(
    dem: &Dem,
    m: &DemMeta,
    src_epsg: u16,
    target_epsg: u16,
    polys_target: &[Polygon<f64>],
    fclass: &[Option<String>],
    all_touched: bool,
    output_mode: OutputMode,
    pad: (u32, u32, u32, u32),  // (col0, row0, w, h)
    core: (u32, u32, u32, u32), // (col0, row0, w, h)，须为 pad 的子窗口
) -> Result<Array2<f32>> {
    let (pad_col0, pad_row0, pad_w, pad_h) = pad;
    let (core_col0, core_row0, core_w, core_h) = core;

    // 读 padded 原始 DEM（源网格，不重采样）。
    let (dem_src, src_win_t) = dem.read_window_f32(pad_col0, pad_row0, pad_w, pad_h)?;

    // warp 到工作网格。
    let warp = suggested_warp_output(src_epsg, target_epsg, src_win_t, pad_w as usize, pad_h as usize)?;
    let work_pixels = warp.width as u64 * warp.height as u64;
    if work_pixels > HYDRO_MAX_FULL_RASTER_PIXELS {
        return Err(WaterError::InvalidInput(format!(
            "瓦片工作网格 {work_pixels} px 超出预算（瓦片边长应更小）",
        )));
    }
    let dem_work = reproject_with_max_error(
        &dem_src, src_win_t, src_epsg, m.nodata,
        warp.transform, warp.width, warp.height, target_epsg,
        Resampling::Bilinear, GDAL_WARP_MAX_ERROR,
    )?
    .mapv(|v| v as f64);

    // 工作网格水面 + 写入掩膜（含裙边）。
    let (surface_work, mask_work) = compute_water_surface(
        &warp.transform, &dem_work, polys_target, fclass,
        all_touched, HYDRO_WATER_SKIRT_PIXELS,
        |_idx, n| (0..n).collect::<Vec<usize>>(),
    );

    // 只把水面 + 掩膜投回 padded 源窗口（方案 B）。
    let masked_surface = Array2::from_shape_fn(surface_work.dim(), |(r, c)| {
        if mask_work[(r, c)] { surface_work[(r, c)] } else { f32::NAN }
    });
    let surf_src = reproject_with_max_error(
        &masked_surface, warp.transform, target_epsg, None,
        src_win_t, pad_w as usize, pad_h as usize, src_epsg,
        Resampling::Bilinear, GDAL_WARP_MAX_ERROR,
    )?;
    let mask_f32 = mask_work.mapv(|b| if b { 1.0f32 } else { 0.0f32 });
    let mask_src = reproject_with_max_error(
        &mask_f32, warp.transform, target_epsg, None,
        src_win_t, pad_w as usize, pad_h as usize, src_epsg,
        Resampling::Nearest, GDAL_WARP_MAX_ERROR,
    )?;

    // 提取 core：与精确源 DEM 组合。
    let with_dem = matches!(output_mode, OutputMode::WaterSurfaceWithDem);
    let dr = (core_row0 - pad_row0) as usize;
    let dc = (core_col0 - pad_col0) as usize;
    let core_arr = Array2::from_shape_fn((core_h as usize, core_w as usize), |(r, c)| {
        let (pr, pc) = (dr + r, dc + c);
        let is_water = mask_src[(pr, pc)] >= 0.5 && surf_src[(pr, pc)].is_finite();
        if is_water {
            surf_src[(pr, pc)]
        } else if with_dem && dem_src[(pr, pc)].is_finite() {
            dem_src[(pr, pc)]
        } else {
            OUTPUT_NODATA as f32
        }
    });
    Ok(core_arr)
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

    // 5) 源网格 ROI 窗口（覆盖全部水体）。
    let (col0, row0, ww, hh) =
        compute_source_roi(tbounds, target_epsg, src_epsg, full_t, m.width as usize, m.height as usize)?;

    // 6) 预算分派：整窗工作网格 ≤ 预算走单窗；否则走瓦片路径（对应 Python 整幅 vs tile runner）。
    let roi_win_t = window_transform(full_t, col0, row0);
    let warp = suggested_warp_output(src_epsg, target_epsg, roi_win_t, ww as usize, hh as usize)?;
    let work_pixels = warp.width as u64 * warp.height as u64;
    if work_pixels > HYDRO_MAX_FULL_RASTER_PIXELS
        || (ww as u64 * hh as u64) > HYDRO_MAX_FULL_RASTER_PIXELS
    {
        return run_hydro_pipeline_tiled(
            job, &dem, &m, src_epsg, target_epsg, &polys_src, &polys_target, &fclass,
        );
    }

    // 7) 单窗处理（pad == core == ROI）+ 写 ROI GeoTIFF。
    tracing::info!("[hydro] 单窗口处理整个水域（{ww}×{hh} 源像素）…");
    let core = process_window(
        &dem, &m, src_epsg, target_epsg, &polys_target, &fclass,
        job.all_touched, job.output_mode,
        (col0, row0, ww, hh), (col0, row0, ww, hh),
    )?;
    let is_geo = proj_from_epsg(src_epsg)?.is_latlong();
    write_geotiff_f32(&job.output_path, &core, roi_win_t, src_epsg, is_geo, Some(OUTPUT_NODATA))?;
    tracing::info!("[hydro] 水面 DEM 写出完成 → {}", job.output_path.display());
    Ok(())
}

/// 由整幅仿射 `full_t` 与像素窗口起点算窗口仿射（Affine 序 `[a,b,c,d,e,f]`）。
fn window_transform(full_t: [f64; 6], col0: u32, row0: u32) -> [f64; 6] {
    [
        full_t[0],
        0.0,
        full_t[2] + col0 as f64 * full_t[0],
        0.0,
        full_t[4],
        full_t[5] + row0 as f64 * full_t[4],
    ]
}

/// 瓦片路径：逐 8192 源瓦片处理并拼进全幅输出（对应 Python `_generate_hydro_water_dem_tiled`）。
///
/// 每个与水体相交的瓦片按 `process_window` 独立 warp+求解（tile ± 64px padding），
/// 与精确源 DEM 组合后写入全幅输出的 core 区；不相交（dry）瓦片保留 DEM/nodata 底。
/// 峰值工作内存 ~ 单瓦片规模（非整幅水域），故可处理超预算场景。
#[allow(clippy::too_many_arguments)]
fn run_hydro_pipeline_tiled(
    job: &HydroJob,
    dem: &Dem,
    m: &DemMeta,
    src_epsg: u16,
    target_epsg: u16,
    polys_src: &[Polygon<f64>],
    polys_target: &[Polygon<f64>],
    fclass: &[Option<String>],
) -> Result<()> {
    let full_t = [m.pixel_size_x, 0.0, m.min_x, 0.0, -m.pixel_size_y, m.max_y];
    let (fw, fh) = (m.width, m.height);
    let with_dem = matches!(job.output_mode, OutputMode::WaterSurfaceWithDem);

    // 全幅输出：with_dem 用整幅精确源 DEM 铺底；only 用 nodata。
    let mut out: Array2<f32> = if with_dem {
        let (mut full_dem, _t) = dem.read_window_f32(0, 0, fw, fh)?;
        full_dem.mapv_inplace(|v| if v.is_finite() { v } else { OUTPUT_NODATA as f32 });
        full_dem
    } else {
        Array2::from_elem((fh as usize, fw as usize), OUTPUT_NODATA as f32)
    };

    // 各源多边形 bbox（源 CRS 坐标），用于瓦片重叠判定。
    let src_bboxes: Vec<[f64; 4]> = polys_src
        .iter()
        .filter_map(|p| {
            p.bounding_rect()
                .map(|r| [r.min().x, r.min().y, r.max().x, r.max().y])
        })
        .collect();

    let (a, e, ox, oy) = (full_t[0], full_t[4], full_t[2], full_t[5]);
    let (tile, pad) = (HYDRO_TILED_TILE_SIZE, HYDRO_TILED_PAD_PX);
    // 真实进度：按瓦片计数（含水+无水），供 UI 显示「第 N / 共 M」。
    let total_tiles = fw.div_ceil(tile) * fh.div_ceil(tile);

    // ── 一遍枚举瓦片：分含水/无水，收集含水瓦片作业（padded 窗口 + core 窗口）──
    type Win = (u32, u32, u32, u32); // (col0, row0, w, h)
    let mut jobs: Vec<(Win, Win)> = Vec::new();
    let mut n_dry = 0u32;
    let mut tile_idx = 0u32;
    let mut trow0 = 0u32;
    while trow0 < fh {
        let th = tile.min(fh - trow0);
        let mut tcol0 = 0u32;
        while tcol0 < fw {
            let tw = tile.min(fw - tcol0);
            tile_idx += 1;
            // 瓦片源 bbox（north-up：e<0）。
            let tminx = ox + tcol0 as f64 * a;
            let tmaxx = ox + (tcol0 + tw) as f64 * a;
            let tmaxy = oy + trow0 as f64 * e;
            let tminy = oy + (trow0 + th) as f64 * e;
            let overlaps = src_bboxes
                .iter()
                .any(|b| b[0] <= tmaxx && b[2] >= tminx && b[1] <= tmaxy && b[3] >= tminy);
            if !overlaps {
                n_dry += 1;
                tracing::info!("[hydro] 瓦片 {tile_idx}/{total_tiles} 跳过（区域内无水体）");
            } else {
                // padded 窗口（tile ± pad，clamp 到整幅）。
                let pcol0 = tcol0.saturating_sub(pad);
                let prow0 = trow0.saturating_sub(pad);
                let pcol1 = (tcol0 + tw + pad).min(fw);
                let prow1 = (trow0 + th + pad).min(fh);
                jobs.push((
                    (pcol0, prow0, pcol1 - pcol0, prow1 - prow0),
                    (tcol0, trow0, tw, th),
                ));
            }
            tcol0 += tile;
        }
        trow0 += tile;
    }

    // ── 含水瓦片 4 线程并行处理（全分辨率不变）──
    let n_water = jobs.len() as u32;
    tracing::info!(
        "[hydro] 并行处理 {n_water} 个含水瓦片（{HYDRO_TILE_WORKERS} 线程，全分辨率）…"
    );
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(HYDRO_TILE_WORKERS)
        .build()
        .map_err(|err| WaterError::Other(anyhow::anyhow!("rayon 线程池构建失败: {err}")))?;
    let done = std::sync::atomic::AtomicU32::new(0);
    let results: Vec<(Win, Array2<f32>)> = pool.install(|| {
        jobs.par_iter()
            .map(|&(pad_win, core_win)| {
                let core = process_window(
                    dem, m, src_epsg, target_epsg, polys_target, fclass,
                    job.all_touched, job.output_mode, pad_win, core_win,
                )?;
                let k = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                tracing::info!("[hydro] 含水瓦片 {k}/{n_water} 完成");
                Ok((core_win, core))
            })
            .collect::<Result<Vec<_>>>()
    })?;

    // ── 顺序写回全幅输出（各 core 区互不重叠，与串行逐位一致）──
    for ((tcol0, trow0, tw, th), core) in results {
        for r in 0..th as usize {
            for c in 0..tw as usize {
                out[(trow0 as usize + r, tcol0 as usize + c)] = core[(r, c)];
            }
        }
    }
    tracing::info!(
        "[hydro] 全部 {total_tiles} 个瓦片处理完成（含水 {n_water}，无水 {n_dry}），输出 {fh}×{fw}"
    );

    let is_geo = proj_from_epsg(src_epsg)?.is_latlong();
    write_geotiff_f32(&job.output_path, &out, full_t, src_epsg, is_geo, Some(OUTPUT_NODATA))?;
    Ok(())
}

