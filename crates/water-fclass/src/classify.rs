//! 单个水体多边形的 fclass 分类（优先级判定）。
//!
//! 对应 Python `fclass/partition.py::_classify_single_geometry` 与
//! `fclass/classify.py` 的 `_has_line_clip_match` / `_any_polygon_match`。

use geo::{Area, BooleanOps, BoundingRect, Centroid, Contains, EuclideanLength};
use geo_types::{MultiLineString, MultiPolygon, Point};

use crate::reference::{LineLayer, PolygonLayer, ReferenceLayers};

/// 线相交裁剪的最小长度（米）。对应 Python `OSM_LINE_MIN_LENGTH_M`。
const OSM_LINE_MIN_LENGTH_M: f64 = 20.0;
/// 多边形重叠面积占比阈值。对应 Python `POLYGON_OVERLAP_THRESHOLD`。
const POLYGON_OVERLAP_THRESHOLD: f64 = 0.2;

/// 分类优先级。对应 Python `FCLASS_PRIORITY`。
const FCLASS_PRIORITY: [&str; 7] =
    ["sea", "lake", "glacier", "reservoir", "dock", "river", "stream"];

/// 优先级项 → `_fclass_reason`。对应 Python `_PRIORITY_REASONS`。
fn priority_reason(fclass: &str) -> &'static str {
    match fclass {
        "sea" => "sea_line",
        "lake" => "lake_polygon",
        "glacier" => "glacier_polygon",
        "river" => "river_line",
        "reservoir" => "reservoir_polygon",
        "dock" => "dock_polygon",
        "stream" => "stream_line",
        _ => "fallback",
    }
}

/// 是否有参照线与水体多边形的裁剪长度 ≥ `OSM_LINE_MIN_LENGTH_M`。
///
/// 对应 Python `_has_line_clip_match`：仅判断“是否有参照线落入该多边形”。
fn has_line_clip_match(water: &MultiPolygon<f64>, layer: &LineLayer) -> bool {
    let Some(bbox) = water.bounding_rect() else {
        return false;
    };
    for idx in layer.candidates(&bbox) {
        let line = &layer.lines[idx];
        let ml = MultiLineString(vec![line.clone()]);
        let clipped = water.clip(&ml, false);
        if clipped.euclidean_length() >= OSM_LINE_MIN_LENGTH_M {
            return true;
        }
    }
    false
}

/// 是否有参照多边形满足质心命中或重叠占比 ≥ 阈值。
///
/// 对应 Python `_any_polygon_match`：`contains(centroid)` 或
/// `intersection_area / water_area ≥ POLYGON_OVERLAP_THRESHOLD`。
fn any_polygon_match(
    water: &MultiPolygon<f64>,
    centroid: &Point<f64>,
    area: f64,
    layer: &PolygonLayer,
) -> bool {
    let Some(bbox) = water.bounding_rect() else {
        return false;
    };
    for idx in layer.candidates(&bbox) {
        let poly = &layer.polys[idx];
        if poly.contains(centroid) {
            return true;
        }
        let inter = water.intersection(&MultiPolygon(vec![poly.clone()]));
        if inter.unsigned_area() / area >= POLYGON_OVERLAP_THRESHOLD {
            return true;
        }
    }
    false
}

/// 对单个水体多边形分类，返回 `(fclass, reason)`。
///
/// 对应 Python `_classify_single_geometry`。无匹配回退 `("water", "fallback")`。
pub fn classify_geometry(water: &MultiPolygon<f64>, refs: &ReferenceLayers) -> (&'static str, &'static str) {
    let Some(centroid) = water.centroid() else {
        return ("water", "fallback");
    };
    let area = water.unsigned_area();
    if area <= 0.0 {
        return ("water", "fallback");
    }

    for &fclass in &FCLASS_PRIORITY {
        let matched = match fclass {
            "sea" | "river" | "stream" => refs
                .lines
                .get(fclass)
                .map(|l| has_line_clip_match(water, l))
                .unwrap_or(false),
            "lake" | "glacier" | "reservoir" | "dock" => refs
                .polygons
                .get(fclass)
                .map(|p| any_polygon_match(water, &centroid, area, p))
                .unwrap_or(false),
            _ => false,
        };
        if matched {
            return (leak_fclass(fclass), priority_reason(fclass));
        }
    }
    ("water", "fallback")
}

/// 将优先级名映射到 `'static` 字符串（编译期固定集合）。
fn leak_fclass(fclass: &str) -> &'static str {
    match fclass {
        "sea" => "sea",
        "lake" => "lake",
        "glacier" => "glacier",
        "reservoir" => "reservoir",
        "dock" => "dock",
        "river" => "river",
        "stream" => "stream",
        _ => "water",
    }
}
