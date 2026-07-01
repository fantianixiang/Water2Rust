//! 输出阶段水面裙边（对应 `generate_hydro_water_dem` 的 `water_surface_skirt` 段）。
//!
//! 纯后处理，不回馈求解：把已解水面掩膜按 `N` 像素膨胀出**两条带**——
//! - 内 `N` 像素（flat 带）：直接盖印最近种子的水位（下游业务要求的平坦水面外扩）；
//! - 外 `N` 像素（ramp 带）：从水位线性过渡到本地 DEM，避免裙边处出现垂直墙。
//!
//! 忠实复刻 Python：种子 = 输出掩膜 ∩ 有限水面；EDT 求最近种子（`~seed` 上做距离变换）；
//! ramp 深度 `d = clip(dist - N, 0, N)`、`t = d/(N+1)`、`blended = wz·(1-t)+dem·t`，
//! DEM 有限则 `max(blended, wz)`、否则平铺 `wz`。裙边完成后输出掩膜扩为外带。

use ndarray::Array2;
use water_core::edt::distance_transform_edt;
use water_core::raster_ops::binary_dilation;

/// 就地在 `surface` 上生成裙边，并把 `output_mask` 扩展为外带（`ramp_outer`）。
///
/// - `surface`：工作网格水面（非水像素为 `NaN`）；
/// - `output_mask`：已解水面写入掩膜（就地更新为 2N 膨胀掩膜）；
/// - `dem`：工作网格 DEM（`f32`，`NaN` 为 nodata）；
/// - `skirt_pixels`：单侧带宽 `N`（默认 5）；`0` 时直接返回。
///
/// 返回 `(flat_added, ramp_added)` 像素计数。
pub fn apply_water_surface_skirt(
    surface: &mut Array2<f32>,
    output_mask: &mut Array2<bool>,
    dem: &Array2<f32>,
    skirt_pixels: usize,
) -> (usize, usize) {
    if skirt_pixels == 0 {
        return (0, 0);
    }
    let (h, w) = surface.dim();
    let n = skirt_pixels;

    // 种子：输出掩膜 ∩ 有限水面。
    let mut seed = Array2::<bool>::from_elem((h, w), false);
    let mut any_seed = false;
    for r in 0..h {
        for c in 0..w {
            if output_mask[(r, c)] && surface[(r, c)].is_finite() {
                seed[(r, c)] = true;
                any_seed = true;
            }
        }
    }
    if !any_seed {
        return (0, 0);
    }

    let flat_mask = binary_dilation(output_mask, n);
    let ramp_outer = binary_dilation(output_mask, 2 * n);

    // 是否存在待填像素。
    let mut has_fill = false;
    for r in 0..h {
        for c in 0..w {
            let in_mask = output_mask[(r, c)];
            if (flat_mask[(r, c)] && !in_mask) || (ramp_outer[(r, c)] && !flat_mask[(r, c)]) {
                has_fill = true;
                break;
            }
        }
        if has_fill {
            break;
        }
    }
    if !has_fill {
        *output_mask = ramp_outer;
        return (0, 0);
    }

    // EDT 在 ~seed 上：每像素到最近种子的距离 + 最近种子行列。
    let inv_seed = seed.mapv(|b| !b);
    let edt = distance_transform_edt(&inv_seed);

    let nf = n as f32;
    let mut flat_added = 0usize;
    let mut ramp_added = 0usize;
    for r in 0..h {
        for c in 0..w {
            let in_mask = output_mask[(r, c)];
            if flat_mask[(r, c)] && !in_mask {
                let sr = edt.index_row[(r, c)] as usize;
                let sc = edt.index_col[(r, c)] as usize;
                surface[(r, c)] = surface[(sr, sc)];
                flat_added += 1;
            } else if ramp_outer[(r, c)] && !flat_mask[(r, c)] {
                let sr = edt.index_row[(r, c)] as usize;
                let sc = edt.index_col[(r, c)] as usize;
                let water_z = surface[(sr, sc)];
                let dem_at = dem[(r, c)];
                let d_ramp = (edt.distances[(r, c)] as f32 - nf).clamp(0.0, nf);
                let t = d_ramp / (nf + 1.0);
                let mut blended = water_z * (1.0 - t) + dem_at * t;
                blended = if dem_at.is_finite() {
                    blended.max(water_z)
                } else {
                    water_z
                };
                surface[(r, c)] = blended;
                ramp_added += 1;
            }
        }
    }

    *output_mask = ramp_outer;
    (flat_added, ramp_added)
}
