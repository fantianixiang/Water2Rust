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
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use water_io::raster::{Dem, DemMeta};
use water_io::vector::read_vector;
use water_io::warp::{reproject_masked, suggested_warp_output, Resampling};
use water_io::geotiff_write::{write_geotiff_f32, write_geotiff_f32_banded};
use water_core::error::WaterError;
use water_core::Result;

use crate::crs::{proj_from_epsg, reproject_polygon, resolve_working_crs};
use crate::river_pipeline::{compute_water_surface, window_from_geometry_bounds};
use crate::{HydroJob, OutputMode};

/// 输出阶段裙边带宽（Python `HYDRO_WATER_SKIRT_PIXELS`，模块常量）。
const HYDRO_WATER_SKIRT_PIXELS: usize = 5;

/// 正向 warp 掩膜膨胀半径（像素）。须 ≥ 裙边 ramp 读取带 `2·skirt` + 双线性 halo + solve
/// 边界 halo，保证掩膜覆盖 `compute_water_surface` 消费的**全部** dem_work 像元（parity 由
/// 全量 md5 逐位一致背书）。取 `2·5 + 6 = 16` 留足裕度。
const HYDRO_WARP_MASK_DILATE: usize = 2 * HYDRO_WATER_SKIRT_PIXELS + 6;
/// GDAL warp 默认近似变换误差阈值（像素）。复刻 GDAL `errorThreshold=0.125`，
/// 使 DEM 重投影与原 Python(GDAL) 参照 bit 级一致（见 docs/HYDRO.md）。
const GDAL_WARP_MAX_ERROR: f64 = 0.125;
/// 内存整幅栅格像素上限（Python `HYDRO_MAX_FULL_RASTER_PIXELS`）。
const HYDRO_MAX_FULL_RASTER_PIXELS: u64 = 250_000_000;

/// 整幅**输出**像素上限：≤ 此值走「全幅铺底 + 单条带写」（与既有文件级一致，含林芝）；
/// 超过则走「流式逐条带写」，峰值内存仅一个瓦片行带，避免超大图（如 NJ ~150 亿像元）OOM。
const HYDRO_FULLFRAME_OUT_MAX_PIXELS: u64 = 700_000_000;
/// 输出 nodata。
const OUTPUT_NODATA: f64 = -9999.0;
/// 瓦片边长（源像素）。Python `HYDRO_TILED_TILE_SIZE`。
const HYDRO_TILED_TILE_SIZE: u32 = 8192;
/// 瓦片 padding（源像素）。Python `max(HYDRO_TILED_SURFACE_PADDING_PX, 64)`。
const HYDRO_TILED_PAD_PX: u32 = 64;
/// 瓦片级**并发数上限**（限峰值内存；线程池另取满核，瓦片内层并行借空闲线程铺满 CPU）。
/// 每并发瓦片 ~4–6GB（含内层并行临时量）；24 核 / 31GB 机上取 3（~19GB，安全裕度足）。
/// 提高可略降墙钟但显著增内存（4→~26GB、6→OOM）；可用 env `WATER_HYDRO_TILE_WORKERS` 调参。
const HYDRO_TILE_WORKERS: usize = 3;

/// 实际瓦片线程数（env 覆盖 + 不超过作业数）。
fn tile_workers(n_jobs: usize) -> usize {
    let cap = std::env::var("WATER_HYDRO_TILE_WORKERS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(HYDRO_TILE_WORKERS);
    cap.max(1).min(n_jobs.max(1))
}

/// 分阶段耗时累加器（纳秒；供瓦片路径 profile，env `WATER_HYDRO_PROFILE=1` 打印）。
/// 瓦片内各阶段跨 4 线程累加，故其和 > 墙钟；用于看**相对占比**。
#[derive(Default)]
struct PhaseAcc {
    read_ns: AtomicU64,    // 读 padded 源 DEM 窗口
    warp_ns: AtomicU64,    // warp 正/反投影（3 次/瓦片）
    solve_ns: AtomicU64,   // compute_water_surface（栅格化 + Laplace 解）
    compose_ns: AtomicU64, // 掩膜/组合数组构造
}
impl PhaseAcc {
    #[inline]
    fn add(field: &AtomicU64, t: Instant) {
        field.fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
    }
}
/// profile 开关（env `WATER_HYDRO_PROFILE=1`）。
fn profile_on() -> bool {
    std::env::var("WATER_HYDRO_PROFILE").map(|v| v == "1").unwrap_or(false)
}

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

/// 逆仿射（rasterio Affine 序 `[a,b,c,d,e,f]`）：地理 (x,y) → 像素 (col,row)，返回同序。
#[cfg(feature = "gpu")]
fn invert_affine(t: &[f64; 6]) -> Option<[f64; 6]> {
    let det = t[0] * t[4] - t[1] * t[3];
    if det.abs() < 1e-300 {
        return None;
    }
    Some([
        t[4] / det,
        -t[1] / det,
        (t[1] * t[5] - t[4] * t[2]) / det,
        -t[3] / det,
        t[0] / det,
        (t[3] * t[2] - t[0] * t[5]) / det,
    ])
}

/// GPU warp 是否适用（`--features gpu` 且一侧局地 UTM、另一侧 WGS84(4326)）。
/// 适用时正向 warp 走 GPU 全算，可**跳过 CPU 掩膜构建**（binary_dilation 膨胀）。
/// env `WATER_HYDRO_GPU_WARP=0` 可关闭 GPU warp（保留 GPU Laplace，用于对拍/回退）。
fn gpu_warp_applicable(_a: u16, _b: u16) -> bool {
    #[cfg(feature = "gpu")]
    {
        if !crate::gpu_enabled() {
            return false;
        }
        if std::env::var("WATER_HYDRO_GPU_WARP").map(|v| v == "0").unwrap_or(false) {
            return false;
        }
        (water_gpu::UtmParams::from_epsg(_a).is_some() && _b == 4326)
            || (water_gpu::UtmParams::from_epsg(_b).is_some() && _a == 4326)
    }
    #[cfg(not(feature = "gpu"))]
    {
        false
    }
}

/// warp 分派：`--features gpu` 且为 WGS84(4326)↔局地 UTM 时走 **GPU 逐像元精确变换**
/// （免 CPU 近似器逐行 PROJ 细分）；否则回退 CPU `reproject_masked`。
///
/// GPU 路径**计算整幅** dst（忽略 `mask`）——掩膜本是 CPU 提速手段，GPU 算力足够全算，
/// 且非水像元不被下游消费；坐标变换用 GPU 浮点，与 proj4rs 非逐位一致（~mm 级差，已获准）。
#[allow(clippy::too_many_arguments)]
fn warp_dispatch(
    src: &Array2<f32>,
    src_transform: [f64; 6],
    src_epsg: u16,
    src_nodata: Option<f64>,
    dst_transform: [f64; 6],
    dst_w: usize,
    dst_h: usize,
    dst_epsg: u16,
    resampling: Resampling,
    mask: Option<&Array2<bool>>,
) -> Result<Array2<f32>> {
    #[cfg(feature = "gpu")]
    {
        if let Some(res) = try_warp_gpu(
            src, src_transform, src_epsg, src_nodata, dst_transform, dst_w, dst_h, dst_epsg,
            resampling,
        ) {
            return res;
        }
    }
    reproject_masked(
        src, src_transform, src_epsg, src_nodata, dst_transform, dst_w, dst_h, dst_epsg,
        resampling, GDAL_WARP_MAX_ERROR, mask,
    )
    .map_err(|e| WaterError::Other(e))
}

/// GPU warp 尝试：仅当一侧为局地 UTM、另一侧为 WGS84(4326) 时适用，否则 `None`（回退 CPU）。
#[cfg(feature = "gpu")]
#[allow(clippy::too_many_arguments)]
fn try_warp_gpu(
    src: &Array2<f32>,
    src_transform: [f64; 6],
    src_epsg: u16,
    src_nodata: Option<f64>,
    dst_transform: [f64; 6],
    dst_w: usize,
    dst_h: usize,
    dst_epsg: u16,
    resampling: Resampling,
) -> Option<Result<Array2<f32>>> {
    use water_gpu::{GpuResampling, UtmParams};

    if !gpu_warp_applicable(src_epsg, dst_epsg) {
        return None;
    }
    // 判定 UTM 侧与方向；另一侧须为 WGS84 地理(4326)。
    let (dst_is_utm, utm_epsg, geo_epsg) = if UtmParams::from_epsg(dst_epsg).is_some() {
        (true, dst_epsg, src_epsg)
    } else if UtmParams::from_epsg(src_epsg).is_some() {
        (false, src_epsg, dst_epsg)
    } else {
        return None;
    };
    if geo_epsg != 4326 {
        return None;
    }
    let utm = UtmParams::from_epsg(utm_epsg)?;
    let src_inv = invert_affine(&src_transform)?;
    let (sh, sw) = src.dim();
    let src_std = src.as_standard_layout();
    let src_slice = src_std.as_slice()?;
    let resamp = match resampling {
        Resampling::Nearest => GpuResampling::Nearest,
        Resampling::Bilinear => GpuResampling::Bilinear,
    };
    match water_gpu::warp_reproject(
        src_slice, sh, sw, src_nodata.map(|v| v as f32), dst_transform, src_inv, dst_w, dst_h,
        dst_is_utm, &utm, resamp,
    ) {
        Ok((v, _timing)) => Some(
            Array2::from_shape_vec((dst_h, dst_w), v)
                .map_err(|e| WaterError::Other(anyhow::anyhow!("GPU warp 结果整形失败: {e}"))),
        ),
        Err(e) => Some(Err(WaterError::Other(anyhow::anyhow!("GPU warp 失败: {e}")))),
    }
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
    acc: Option<&PhaseAcc>,
) -> Result<Array2<f32>> {
    let (pad_col0, pad_row0, pad_w, pad_h) = pad;
    let (core_col0, core_row0, core_w, core_h) = core;

    // 读 padded 原始 DEM（源网格，不重采样）。
    let t = Instant::now();
    let (dem_src, src_win_t) = dem.read_window_f32(pad_col0, pad_row0, pad_w, pad_h)?;
    if let Some(a) = acc {
        PhaseAcc::add(&a.read_ns, t);
    }

    // warp 到工作网格。
    let t = Instant::now();
    let warp = suggested_warp_output(src_epsg, target_epsg, src_win_t, pad_w as usize, pad_h as usize)?;
    let work_pixels = warp.width as u64 * warp.height as u64;
    if work_pixels > HYDRO_MAX_FULL_RASTER_PIXELS {
        return Err(WaterError::InvalidInput(format!(
            "瓦片工作网格 {work_pixels} px 超出预算（瓦片边长应更小）",
        )));
    }
    // 正向 warp「只算需要部分」（仅 CPU 路径需要）：构造工作网格水掩膜（栅格化各多边形
    // + 膨胀覆盖 solve 边界 halo 与裙边读取带 2N），只重投影掩膜内 DEM 像元。**不改变输出
    // 网格/变换器拟合**，故 compute_water_surface 消费的像元逐位一致。GPU warp 全算整幅
    // （算力足够、掩膜外不被消费），**跳过掩膜构建**（binary_dilation 是 CPU warp 相的主开销）。
    let (wh, ww) = (warp.height as usize, warp.width as usize);
    let work_mask = if gpu_warp_applicable(src_epsg, target_epsg) {
        None
    } else {
        let mut wm = Array2::<bool>::from_elem((wh, ww), false);
        for polygon in polys_target {
            if polygon.exterior().0.is_empty() {
                continue;
            }
            if let Some((r0, c0, lh, lw, win_t)) =
                window_from_geometry_bounds(polygon, &warp.transform, wh, ww, 2)
            {
                let pm = water_io::raster::rasterize_polygon_mask(polygon, &win_t, lw as u32, lh as u32, all_touched);
                for r in 0..lh {
                    for c in 0..lw {
                        if pm[(r, c)] {
                            wm[(r0 + r, c0 + c)] = true;
                        }
                    }
                }
            }
        }
        // 膨胀覆盖读取带（4-连通菱形膨胀，半径 = DILATE ≥ 裙边 ramp 2N + halo）。
        Some(water_core::raster_ops::binary_dilation(&wm, HYDRO_WARP_MASK_DILATE))
    };
    let dem_work = warp_dispatch(
        &dem_src, src_win_t, src_epsg, m.nodata,
        warp.transform, warp.width as usize, warp.height as usize, target_epsg,
        Resampling::Bilinear, work_mask.as_ref(),
    )?
    .mapv(|v| v as f64);
    if let Some(a) = acc {
        PhaseAcc::add(&a.warp_ns, t);
    }

    // 工作网格水面 + 写入掩膜（含裙边）。
    let t = Instant::now();
    let (surface_work, mask_work) = compute_water_surface(
        &warp.transform, &dem_work, polys_target, fclass,
        all_touched, HYDRO_WATER_SKIRT_PIXELS,
        |_idx, n| (0..n).collect::<Vec<usize>>(),
    );
    if let Some(a) = acc {
        PhaseAcc::add(&a.solve_ns, t);
    }

    // 只把水面 + 掩膜投回 padded 源窗口（方案 B）。
    let t = Instant::now();
    let masked_surface = Array2::from_shape_fn(surface_work.dim(), |(r, c)| {
        if mask_work[(r, c)] { surface_work[(r, c)] } else { f32::NAN }
    });
    if let Some(a) = acc {
        PhaseAcc::add(&a.compose_ns, t);
    }
    let t = Instant::now();
    // 先算 mask（nearest，全窗）——compose 只消费 mask_src>=0.5 的像元，故它天然是
    // surf_src 的「需要计算」掩膜。
    let mask_f32 = mask_work.mapv(|b| if b { 1.0f32 } else { 0.0f32 });
    let mask_src = warp_dispatch(
        &mask_f32, warp.transform, target_epsg, None,
        src_win_t, pad_w as usize, pad_h as usize, src_epsg,
        Resampling::Nearest, None,
    )?;
    // 反向 warp 水面。CPU 路径用 mask_src>=0.5 掩膜化（只算被 compose 消费的像元，逐位一致）；
    // GPU 路径整幅精确变换（源 masked_surface 在非水处为 NaN，采样自然得 NaN）。
    let surf_dst_mask = mask_src.mapv(|v| v >= 0.5);
    let surf_src = warp_dispatch(
        &masked_surface, warp.transform, target_epsg, None,
        src_win_t, pad_w as usize, pad_h as usize, src_epsg,
        Resampling::Bilinear, Some(&surf_dst_mask),
    )?;
    if let Some(a) = acc {
        PhaseAcc::add(&a.warp_ns, t);
    }

    // 提取 core：与精确源 DEM 组合。
    let t = Instant::now();
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
    if let Some(a) = acc {
        PhaseAcc::add(&a.compose_ns, t);
    }
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
        None,
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
    let prof = profile_on();

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
                //
                // 注：曾试「把 core 收紧到水 bbox 只 warp 水区」（~2.6× warp、总 ~11%），但**改变 warp
                // 输出网格范围 → GDAL 近似变换器(0.125px)拟合变化**，在林芝极端地形（峡谷陡崖，单像素
                // 高差可达数百米）下，亚像素位移被放大成**最大 437m 的粗差**（0.5% 像元变动、~230 个岸边
                // 像元 >30m），与 ~11% 加速**不成正比**，已放弃。正解是「保持整窗 warp 变换、只**计算**水
                // 像元」——同一变换器 → 水像元值逐位一致、跳过 97% 非水像元——需在 eci-gdal 的 reproject
                // 加输出掩膜支持（子模块改动）。见 pipeline-exam/迭代日志.md。
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

    // ── 含水瓦片并行处理（全分辨率不变）──
    // 两级并行：块内 mem_tiles 个瓦片并发（限峰值内存），线程池取满核，瓦片**内层**
    // （per-polygon / EDT / 高斯 / 膨胀已 rayon 化）借空闲线程铺满 CPU。
    let n_water = jobs.len() as u32;
    let mem_tiles = tile_workers(jobs.len());
    let n_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(mem_tiles)
        .max(mem_tiles);
    tracing::info!(
        "[hydro] 并行处理 {n_water} 个含水瓦片（并发瓦片 {mem_tiles}，线程 {n_threads}，全分辨率）…"
    );
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(n_threads)
        .build()
        .map_err(|err| WaterError::Other(anyhow::anyhow!("rayon 线程池构建失败: {err}")))?;
    let acc = PhaseAcc::default();
    let is_geo = proj_from_epsg(src_epsg)?.is_latlong();

    // 瓦片处理助手：把给定作业按 mem_tiles 分块并行处理，产出各 core 子块。
    let process_jobs = |these: &[(Win, Win)], done: &std::sync::atomic::AtomicU32|
     -> Result<Vec<(Win, Array2<f32>)>> {
        pool.install(|| -> Result<Vec<(Win, Array2<f32>)>> {
            let mut all: Vec<(Win, Array2<f32>)> = Vec::with_capacity(these.len());
            for chunk in these.chunks(mem_tiles) {
                let mut cr: Vec<(Win, Array2<f32>)> = chunk
                    .par_iter()
                    .map(|&(pad_win, core_win)| {
                        let core = process_window(
                            dem, m, src_epsg, target_epsg, polys_target, fclass,
                            job.all_touched, job.output_mode, pad_win, core_win,
                            if prof { Some(&acc) } else { None },
                        )?;
                        let k = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                        tracing::info!("[hydro] 含水瓦片 {k}/{n_water} 完成");
                        Ok((core_win, core))
                    })
                    .collect::<Result<Vec<_>>>()?;
                all.append(&mut cr);
            }
            Ok(all)
        })
    };

    // 预算分派：整幅输出可放心装内存 → 全幅铺底 + 单条带写（与既有文件级逐位一致）；
    // 否则 → 流式逐条带写，峰值内存仅一个瓦片行带（避免超大图 OOM）。
    // env `WATER_HYDRO_FORCE_STREAM=1` 可强制走流式（用于测试/低内存环境）。
    let force_stream = std::env::var("WATER_HYDRO_FORCE_STREAM").map(|v| v == "1").unwrap_or(false);
    let stream = force_stream || (fw as u64 * fh as u64) > HYDRO_FULLFRAME_OUT_MAX_PIXELS;

    if !stream {
        // ── 全幅路径（小图，保持既有文件级一致）──
        let t_base = Instant::now();
        let mut out: Array2<f32> = if with_dem {
            let (mut full_dem, _t) = dem.read_window_f32(0, 0, fw, fh)?;
            full_dem.mapv_inplace(|v| if v.is_finite() { v } else { OUTPUT_NODATA as f32 });
            full_dem
        } else {
            Array2::from_elem((fh as usize, fw as usize), OUTPUT_NODATA as f32)
        };
        if prof {
            tracing::info!("[profile] 全幅铺底读入 {:.2}s（{fw}×{fh}）", t_base.elapsed().as_secs_f64());
        }
        let done = std::sync::atomic::AtomicU32::new(0);
        let t_tiles = Instant::now();
        let results = process_jobs(&jobs, &done)?;
        if prof {
            tracing::info!(
                "[profile] 瓦片处理 {:.2}s（墙钟）| 分项累加(跨{HYDRO_TILE_WORKERS}线程): 读={:.2}s warp={:.2}s 解算={:.2}s 组合={:.2}s",
                t_tiles.elapsed().as_secs_f64(),
                acc.read_ns.load(Ordering::Relaxed) as f64 / 1e9,
                acc.warp_ns.load(Ordering::Relaxed) as f64 / 1e9,
                acc.solve_ns.load(Ordering::Relaxed) as f64 / 1e9,
                acc.compose_ns.load(Ordering::Relaxed) as f64 / 1e9,
            );
        }
        let t_asm = Instant::now();
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
        if prof {
            tracing::info!("[profile] 写回全幅 {:.2}s", t_asm.elapsed().as_secs_f64());
        }
        let t_write = Instant::now();
        write_geotiff_f32(&job.output_path, &out, full_t, src_epsg, is_geo, Some(OUTPUT_NODATA))?;
        if prof {
            tracing::info!("[profile] 输出写盘 {:.2}s", t_write.elapsed().as_secs_f64());
        }
    } else {
        // ── 流式逐条带路径（超大图）：按瓦片行带自顶向下产出并直接写盘 ──
        // 各含水瓦片核 core 行起点 trow0 恒为 tile 的整数倍，故按 trow0/tile 归入行带；
        // 条带 rows_per_strip = tile，与瓦片行对齐，末带自动取剩余行。
        use std::collections::HashMap;
        let mut by_band: HashMap<u32, Vec<(Win, Win)>> = HashMap::new();
        for &j in &jobs {
            by_band.entry(j.1 .1 / tile).or_default().push(j);
        }
        tracing::info!(
            "[hydro] 流式逐条带写（整幅 {fw}×{fh} 超内存预算，条带 {tile} 行，含水 {n_water} 无水 {n_dry}）…"
        );
        let done = std::sync::atomic::AtomicU32::new(0);
        let t_stream = Instant::now();
        write_geotiff_f32_banded(
            &job.output_path,
            fw as usize,
            fh as usize,
            full_t,
            src_epsg,
            is_geo,
            Some(OUTPUT_NODATA),
            tile,
            |row_start, band_h| -> anyhow::Result<Vec<f32>> {
                // 铺底：with_dem 读本行带精确源 DEM；only 用 nodata。
                let mut band: Array2<f32> = if with_dem {
                    let (mut d, _t) = dem.read_window_f32(0, row_start as u32, fw, band_h as u32)?;
                    d.mapv_inplace(|v| if v.is_finite() { v } else { OUTPUT_NODATA as f32 });
                    d
                } else {
                    Array2::from_elem((band_h, fw as usize), OUTPUT_NODATA as f32)
                };
                // 处理本行带含水瓦片，盖印到带缓冲（core 非水像元即精确源 DEM，与铺底一致）。
                let band_idx = row_start as u32 / tile;
                if let Some(bj) = by_band.get(&band_idx) {
                    let results = process_jobs(bj, &done)?;
                    for ((tcol0, trow0, tw, th), core) in results {
                        let dr = trow0 as usize - row_start; // 带对齐瓦片行，dr=0
                        for r in 0..th as usize {
                            for c in 0..tw as usize {
                                band[(dr + r, tcol0 as usize + c)] = core[(r, c)];
                            }
                        }
                    }
                }
                Ok(band.into_raw_vec_and_offset().0)
            },
        )?;
        tracing::info!(
            "[hydro] 全部 {total_tiles} 个瓦片处理完成（含水 {n_water}，无水 {n_dry}），输出 {fh}×{fw}"
        );
        if prof {
            tracing::info!(
                "[profile] 流式处理+写盘 {:.2}s | 分项累加(跨{HYDRO_TILE_WORKERS}线程): 读={:.2}s warp={:.2}s 解算={:.2}s 组合={:.2}s",
                t_stream.elapsed().as_secs_f64(),
                acc.read_ns.load(Ordering::Relaxed) as f64 / 1e9,
                acc.warp_ns.load(Ordering::Relaxed) as f64 / 1e9,
                acc.solve_ns.load(Ordering::Relaxed) as f64 / 1e9,
                acc.compose_ns.load(Ordering::Relaxed) as f64 / 1e9,
            );
        }
    }
    Ok(())
}

