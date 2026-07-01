//! 湖泊常数水位计算与压平（把常数水位盖回求解面）。
//!
//! 忠实复刻 Python `hydro/hydro_lake_flatten.py` 的：
//! - `_is_lake_fclass`
//! - `_compute_lake_constant_z_for_polygon`（无网络情形：岸线环中位数 → 内部中位数）
//! - `_compute_lake_constant_z_for_component`（汇集各多边形环样本 → 截尾中位数；回退汇集内部中位数）
//! - `_flatten_lake_polygons_on_surface`
//!
//! 说明：原 Python 分层还含 tier 1/2（求解节点 z / 剖面样本 z），在当前**无河网**流水线中
//! 这两层的输入恒为空，故此处实现 DEM 观测的 tier 0/3（与无网络运行等价）。

use std::collections::HashMap;

use geo_types::Polygon;
use ndarray::Array2;

use crate::lake::{
    boundary_ring_dem_values, interior_dem_values, iterative_trimmed_median,
    polygon_window_mask, sample_polygon_boundary_ring_dem_median,
    sample_polygon_interior_dem_median,
};

/// 常数水位来源（对应 Python 的 source_tag）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LakeZSource {
    BoundaryRingDem,
    InteriorDem,
    None,
}

/// 湖泊 fclass 集合。对应 `HYDRO_LAKE_FCLASS_VALUES`。
pub fn is_lake_fclass(label: Option<&str>) -> bool {
    match label {
        None => false,
        Some(s) => matches!(
            s.trim().to_ascii_lowercase().as_str(),
            "water"
                | "lake"
                | "reservoir"
                | "glacier"
                | "sea"
                | "dock"
                | "pond"
                | "basin"
                | "lagoon"
                | "wetland"
        ),
    }
}

/// 单湖泊多边形常数水位（无网络：岸线环中位数优先，回退内部中位数）。
///
/// 忠实复刻 `_compute_lake_constant_z_for_polygon` 的 DEM 分层（tier 0 → tier 3）。
pub fn compute_lake_constant_z_for_polygon(
    polygon: &Polygon<f64>,
    dem: &Array2<f32>,
    transform: &[f64; 6],
) -> (f64, LakeZSource) {
    let (boundary, _bc) = sample_polygon_boundary_ring_dem_median(polygon, dem, transform);
    if boundary.is_finite() {
        return (boundary, LakeZSource::BoundaryRingDem);
    }
    let (interior, _ic) = sample_polygon_interior_dem_median(polygon, dem, transform);
    if interior.is_finite() {
        return (interior, LakeZSource::InteriorDem);
    }
    (f64::NAN, LakeZSource::None)
}

/// numpy 中位数（升序，偶数取中间两者均值）。`vals` 不含 NaN。
fn median(vals: &mut [f64]) -> f64 {
    let n = vals.len();
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if n % 2 == 1 {
        vals[n / 2]
    } else {
        0.5 * (vals[n / 2 - 1] + vals[n / 2])
    }
}

/// 多湖泊多边形共享常数水位（汇集各多边形岸线环样本 → 截尾中位数；回退汇集内部中位数）。
///
/// 忠实复刻 `_compute_lake_constant_z_for_component` 的 DEM 分层。
pub fn compute_lake_constant_z_for_component(
    polygons: &[&Polygon<f64>],
    dem: &Array2<f32>,
    transform: &[f64; 6],
) -> (f64, LakeZSource) {
    let mut ring_union: Vec<f64> = Vec::new();
    for p in polygons {
        ring_union.extend(boundary_ring_dem_values(p, dem, transform));
    }
    let boundary_z = iterative_trimmed_median(&ring_union, 5);
    if boundary_z.is_finite() {
        return (boundary_z, LakeZSource::BoundaryRingDem);
    }

    let mut interior_union: Vec<f64> = Vec::new();
    for p in polygons {
        interior_union.extend(interior_dem_values(p, dem, transform));
    }
    if !interior_union.is_empty() {
        return (median(&mut interior_union), LakeZSource::InteriorDem);
    }
    (f64::NAN, LakeZSource::None)
}

/// 压平结果摘要（对应 Python 返回的 summary 关键计数）。
#[derive(Debug, Clone, Default)]
pub struct FlattenSummary {
    pub lake_polygon_count: usize,
    pub filled_polygon_count: usize,
    pub skipped_no_constant: usize,
    pub filled_pixel_count: usize,
    pub polygon_constant_z: HashMap<usize, f64>,
}

/// 将湖泊 fclass 多边形内部的求解面覆盖为常数水位。就地修改 `surface`。
///
/// 忠实复刻 `_flatten_lake_polygons_on_surface`：按 `component_of_polygon` 分组（无 component 的
/// 多边形各自成孤立组），多多边形组共享一个常数水位；栅格化用 `all_touched`（默认 true），
/// `surface[mask] = constant_z`（f32）。
pub fn flatten_lake_polygons_on_surface(
    surface: &mut Array2<f32>,
    transform: &[f64; 6],
    water_polygons: &[Polygon<f64>],
    water_fclass: &[Option<String>],
    dem: &Array2<f32>,
    all_touched: bool,
    component_of_polygon: Option<&HashMap<usize, i64>>,
) -> FlattenSummary {
    let (height, width) = surface.dim();
    let mut summary = FlattenSummary::default();

    // 按 component 分组；无 component 的多边形各自成孤立组（负键哨兵，保序）。
    let mut groups: Vec<(i64, Vec<usize>)> = Vec::new();
    let mut group_index: HashMap<i64, usize> = HashMap::new();
    let mut next_orphan: i64 = -1;
    for (idx, polygon) in water_polygons.iter().enumerate() {
        if polygon.exterior().0.is_empty() {
            continue;
        }
        let label = water_fclass.get(idx).and_then(|o| o.as_deref());
        if !is_lake_fclass(label) {
            continue;
        }
        let key = match component_of_polygon.and_then(|m| m.get(&idx)) {
            Some(&c) => c,
            None => {
                let k = next_orphan;
                next_orphan -= 1;
                k
            }
        };
        match group_index.get(&key) {
            Some(&gi) => groups[gi].1.push(idx),
            None => {
                group_index.insert(key, groups.len());
                groups.push((key, vec![idx]));
            }
        }
    }

    for (_key, indices) in &groups {
        summary.lake_polygon_count += indices.len();
        let group_polys: Vec<&Polygon<f64>> = indices.iter().map(|&i| &water_polygons[i]).collect();
        let (constant_z, _src) = if group_polys.len() > 1 {
            compute_lake_constant_z_for_component(&group_polys, dem, transform)
        } else {
            compute_lake_constant_z_for_polygon(group_polys[0], dem, transform)
        };

        if !constant_z.is_finite() {
            summary.skipped_no_constant += indices.len();
            continue;
        }

        for &idx in indices {
            let polygon = &water_polygons[idx];
            let Some((col_off, row_off, mask)) =
                polygon_window_mask(polygon, height, width, transform, all_touched)
            else {
                continue;
            };
            let z = constant_z as f32;
            let (lh, lw) = mask.dim();
            for lr in 0..lh {
                for lc in 0..lw {
                    if mask[(lr, lc)] {
                        surface[(row_off + lr, col_off + lc)] = z;
                    }
                }
            }
            summary.polygon_constant_z.insert(idx, constant_z);
            summary.filled_polygon_count += 1;
            summary.filled_pixel_count += mask.iter().filter(|&&b| b).count();
        }
    }

    summary
}
