//! 栅格重投影 warp（纯 Rust，长期驻留 water-io，**不上移 eci-gdal**）。
//!
//! 参考蓝图：GDAL `GDALSuggestedWarpOutput2`（`alg/gdaltransformer.cpp`）+ warp 重采样核，
//! 以及 crates.io `rwarp`（纯 Rust GDAL warp 移植）的结构。CRS 变换经 **eci-gdal-proj**
//! （proj4rs，已验证与 pyproj 一致）。**不引入** GDAL/PROJ 的 C 绑定。
//!
//! 当前实现：`suggested_warp_output`（= `calculate_default_transform`，计算目标网格）。
//! 后续：`reproject`（逐目标像素反投影 + 双线性/最近邻重采样）。

use anyhow::{bail, Result};
use eci_gdal_core::RasterCrs;
use eci_gdal_proj::transform::transform_point;

/// 建议 warp 输出网格：目标 GeoTransform + 宽高（对应 `calculate_default_transform`）。
#[derive(Debug, Clone, PartialEq)]
pub struct WarpOutput {
    /// rasterio Affine 序 `[a, b, c, d, e, f]`：`[pixelSizeX, 0, minX, 0, -pixelSizeY, maxY]`（north-up，方形像素）。
    pub transform: [f64; 6],
    pub width: usize,
    pub height: usize,
}

/// GenImgProj 前向变换：源像素 (P, L) → 源地理 → 重投影到目标 CRS 地理坐标。
///
/// `st` 为 rasterio Affine 序 `[a, b, c, d, e, f]`：x=a·col+b·row+c，y=d·col+e·row+f。
fn forward(
    src: &eci_gdal_proj::Proj,
    dst: &eci_gdal_proj::Proj,
    st: &[f64; 6],
    p: f64,
    l: f64,
) -> Option<(f64, f64)> {
    let x = st[0] * p + st[1] * l + st[2];
    let y = st[3] * p + st[4] * l + st[5];
    transform_point(src, dst, x, y).ok()
}

/// 计算建议 warp 输出（复刻 `GDALSuggestedWarpOutput2` 核心，默认 round-nearest 取整）。
///
/// 仅处理常规仿射源网格 + 非反经线场景（边界微调分支在正常场景为 no-op，故略）。
pub fn suggested_warp_output(
    src_epsg: u16,
    dst_epsg: u16,
    src_transform: [f64; 6],
    src_w: usize,
    src_h: usize,
) -> Result<WarpOutput> {
    if src_w == 0 || src_h == 0 {
        bail!("源网格尺寸为 0");
    }
    let src = RasterCrs::Epsg(src_epsg).proj()?;
    let dst = RasterCrs::Epsg(dst_epsg).proj()?;

    // nSteps = clamp(round(min(W,H)/50), 20, 100)（GDAL N_PIXELSTEP=50）。
    let mut nsteps = ((src_w.min(src_h) as f64) / 50.0 + 0.5) as i64;
    nsteps = nsteps.clamp(20, 100);
    let n = nsteps as usize;
    let nsp1 = n + 1;
    let step = 1.0 / nsteps as f64;

    // 沿四边采样源像素坐标，顺序与 GDAL 一致（上/下/左/右），
    // 使 pts[0]=(0,0)、pts[last]=(W,H)（对角两端）。
    let (sw, sh) = (src_w as f64, src_h as f64);
    let mut src_px = vec![(0.0f64, 0.0f64); 4 * nsp1];
    for istep in 0..=n {
        let ratio = if istep == n { 1.0 } else { istep as f64 * step };
        src_px[istep] = (ratio * sw, 0.0); // 上
        src_px[istep + nsp1] = (ratio * sw, sh); // 下
        src_px[istep + 2 * nsp1] = (0.0, ratio * sh); // 左
        src_px[istep + 3 * nsp1] = (sw, ratio * sh); // 右
    }

    let transformed: Vec<Option<(f64, f64)>> = src_px
        .iter()
        .map(|&(p, l)| forward(&src, &dst, &src_transform, p, l))
        .collect();

    // 收集包围盒（忽略失败点）。
    let (mut min_x, mut min_y) = (f64::INFINITY, f64::INFINITY);
    let (mut max_x, mut max_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut got = false;
    let mut failed = 0usize;
    for t in &transformed {
        match t {
            Some((x, y)) => {
                min_x = min_x.min(*x);
                min_y = min_y.min(*y);
                max_x = max_x.max(*x);
                max_y = max_y.max(*y);
                got = true;
            }
            None => failed += 1,
        }
    }
    if !got || failed > transformed.len().saturating_sub(10) {
        bail!("过多采样点变换失败，无法估计输出网格");
    }

    // 像素尺寸：源对角两端变换后连线长度 / 源像素对角长度（方形像素）。
    let first = transformed[0];
    let last = transformed[4 * nsp1 - 1];
    let (mut ddx, mut ddy) = match (first, last) {
        (Some((x0, y0)), Some((xn, yn))) => (xn - x0, yn - y0),
        _ => (0.0, 0.0),
    };
    if ddx == 0.0 || ddy == 0.0 {
        ddx = max_x - min_x;
        ddy = max_y - min_y;
    }
    let diag = (ddx * ddx + ddy * ddy).sqrt();
    let pixel_size = diag / ((sw * sw + sh * sh).sqrt());
    if !(pixel_size > 0.0) {
        bail!("估计的像素尺寸非正");
    }

    // 宽高（默认 round-nearest：int(x + 0.5)）。
    let d_pixels = (max_x - min_x) / pixel_size;
    let d_lines = (max_y - min_y) / pixel_size;
    let width = (d_pixels + 0.5) as usize;
    let height = (d_lines + 0.5) as usize;

    // 重算上边界使返回值一致；north-up。输出采 rasterio Affine 序 [a,b,c,d,e,f]。
    let transform = [pixel_size, 0.0, min_x, 0.0, -pixel_size, max_y];
    Ok(WarpOutput {
        transform,
        width,
        height,
    })
}
