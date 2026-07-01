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
use ndarray::Array2;

/// 应用仿射（rasterio Affine 序 `[a,b,c,d,e,f]`）：像素 (col,row) → 地理 (x,y)。
///
/// `x = a·col + b·row + c`；`y = d·col + e·row + f`。
#[inline]
fn apply_affine(t: &[f64; 6], col: f64, row: f64) -> (f64, f64) {
    (t[0] * col + t[1] * row + t[2], t[3] * col + t[4] * row + t[5])
}

/// 逆仿射（rasterio Affine 序）：地理 (x,y) → 像素 (col,row)。返回同为 `[a,b,c,d,e,f]` 序。
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

/// 重采样方式（对应 rasterio `Resampling`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resampling {
    Nearest,
    Bilinear,
}


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

/// 栅格重投影重采样（对应 rasterio `reproject`）。
///
/// 逐目标像素：目标像素中心 → 目标地理 → 反投影到源 CRS → 源像素坐标 → 采样。
/// 源无效值（`src_nodata`）与越界均记为 NaN；bilinear 对有效邻居加权平均后归一化。
///
/// 采用**精确逐像素** PROJ 变换（`max_error = 0`）。参数均严格采用 rasterio Affine 序。
#[allow(clippy::too_many_arguments)]
pub fn reproject(
    src: &Array2<f32>,
    src_transform: [f64; 6],
    src_epsg: u16,
    src_nodata: Option<f64>,
    dst_transform: [f64; 6],
    dst_w: usize,
    dst_h: usize,
    dst_epsg: u16,
    resampling: Resampling,
) -> Result<Array2<f32>> {
    reproject_with_max_error(
        src, src_transform, src_epsg, src_nodata, dst_transform, dst_w, dst_h, dst_epsg,
        resampling, 0.0,
    )
}

/// 栅格重投影重采样，可指定近似变换误差阈值 `max_error`（像素，曼哈顿）。
///
/// - `max_error = 0`：逐像素精确 PROJ 变换（数学精确）。
/// - `max_error > 0`（GDAL 默认 0.125）：逐目标行走 `GDALApproxTransform` 近似变换器
///   （`warp_approx`），与 GDAL/rasterio warp 默认行为 **bit 级一致**且更快。
///
/// 采样阶段（nearest / bilinear）与源无效值门控不受 `max_error` 影响，只改变
/// 目标像素→源像素坐标的求解方式。
#[allow(clippy::too_many_arguments)]
pub fn reproject_with_max_error(
    src: &Array2<f32>,
    src_transform: [f64; 6],
    src_epsg: u16,
    src_nodata: Option<f64>,
    dst_transform: [f64; 6],
    dst_w: usize,
    dst_h: usize,
    dst_epsg: u16,
    resampling: Resampling,
    max_error: f64,
) -> Result<Array2<f32>> {
    let (sh, sw) = src.dim();
    let src_proj = RasterCrs::Epsg(src_epsg).proj()?;
    let dst_proj = RasterCrs::Epsg(dst_epsg).proj()?;
    let src_inv = invert_affine(&src_transform).ok_or_else(|| anyhow::anyhow!("源仿射不可逆"))?;
    let nodata = src_nodata.map(|v| v as f32);

    // 判断源像素有效（在界内且非 nodata），返回 Some(值) 或 None。
    let sample_at = |ic: i64, ir: i64| -> Option<f32> {
        if ir < 0 || ir >= sh as i64 || ic < 0 || ic >= sw as i64 {
            return None;
        }
        let v = src[(ir as usize, ic as usize)];
        if !v.is_finite() {
            return None;
        }
        if let Some(nd) = nodata {
            if v == nd {
                return None;
            }
        }
        Some(v)
    };

    // 精确基变换：目标像素 `(col, row)` → 源像素 `(scol, srow)`。
    let base = |px: f64, py: f64| -> Option<(f64, f64)> {
        let (dx, dy) = apply_affine(&dst_transform, px, py);
        let (sx, sy) = transform_point(&dst_proj, &src_proj, dx, dy).ok()?;
        Some(apply_affine(&src_inv, sx, sy))
    };

    let mut dst = Array2::<f32>::from_elem((dst_h, dst_w), f32::NAN);
    for i in 0..dst_h {
        // 逐目标行求源像素坐标（近似或精确，取决于 max_error）。
        let approx = crate::warp_approx::RowApprox {
            py: i as f64 + 0.5,
            max_error,
            base: &base,
        };
        let (sxs, sys, oks) = approx.transform_row(dst_w);

        for j in 0..dst_w {
            if !oks[j] {
                continue;
            }
            let (scol, srow) = (sxs[j], sys[j]);
            // GDAL 门控：源点须落在源栅格 [0,W)×[0,H) 内，否则记为 NaN。
            if scol < 0.0 || scol >= sw as f64 || srow < 0.0 || srow >= sh as f64 {
                continue;
            }

            match resampling {
                Resampling::Nearest => {
                    let ic = scol.floor() as i64;
                    let ir = srow.floor() as i64;
                    if let Some(v) = sample_at(ic, ir) {
                        dst[(i, j)] = v;
                    }
                }
                Resampling::Bilinear => {
                    // GDAL 规则：中心所在（containing）源像素为 nodata/越界 → NaN。
                    if sample_at(scol.floor() as i64, srow.floor() as i64).is_none() {
                        continue;
                    }
                    // 转中心约定（整数=像素中心），取左上邻居 + 比例，对有效邻居加权平均。
                    let cx = scol - 0.5;
                    let cy = srow - 0.5;
                    let i0 = cx.floor();
                    let j0 = cy.floor();
                    let rx = cx - i0;
                    let ry = cy - j0;
                    let (ci, cj) = (i0 as i64, j0 as i64);
                    let neigh = [
                        (cj, ci, (1.0 - rx) * (1.0 - ry)),
                        (cj, ci + 1, rx * (1.0 - ry)),
                        (cj + 1, ci, (1.0 - rx) * ry),
                        (cj + 1, ci + 1, rx * ry),
                    ];
                    let mut acc = 0.0f64;
                    let mut wsum = 0.0f64;
                    for &(nr, nc, w) in &neigh {
                        if w == 0.0 {
                            continue;
                        }
                        if let Some(v) = sample_at(nc, nr) {
                            acc += w * v as f64;
                            wsum += w;
                        }
                    }
                    if wsum > 0.0 {
                        dst[(i, j)] = (acc / wsum) as f32;
                    }
                }
            }
        }
    }
    Ok(dst)
}
