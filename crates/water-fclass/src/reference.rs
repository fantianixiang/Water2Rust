//! fclass 分类参照层的读取、重投影、几何族过滤与空间索引构建。
//!
//! 对应 Python `fclass/io.py::_read_prepared_reference_layers`：
//! - reservoir / lake / dock / glacier 按 **多边形族** 读取（`MultiPolygon` 拆成 `Polygon`）；
//! - river / stream / sea 按 **线族** 读取（`MultiLineString` 拆成 `LineString`）；
//! - 均重投影到工作 CRS（EPSG:3857），并以水体外扩包围盒预过滤。

use std::collections::HashMap;
use std::path::Path;

use geo::BoundingRect;
use geo_types::{Geometry, LineString, Polygon, Rect};
use rstar::primitives::{GeomWithData, Rectangle};
use rstar::RTree;
use water_core::error::Result;
use water_io::vector::{list_gpkg_layers, load_gpkg_layer, reproject_geometry};

/// 工作 CRS（与 Python `WORKING_CRS = EPSG:3857` 一致）。
pub const WORKING_EPSG: u16 = 3857;

/// rtree 元素：几何包围盒 + 在层内的下标。
type IndexEntry = GeomWithData<Rectangle<[f64; 2]>, usize>;

/// 索引后的多边形参照层。
pub struct PolygonLayer {
    pub polys: Vec<Polygon<f64>>,
    tree: RTree<IndexEntry>,
}

/// 索引后的线参照层。
pub struct LineLayer {
    pub lines: Vec<LineString<f64>>,
    tree: RTree<IndexEntry>,
}

impl PolygonLayer {
    /// 返回包围盒与查询盒相交的候选多边形下标。
    pub fn candidates(&self, bbox: &Rect<f64>) -> Vec<usize> {
        query_indices(&self.tree, bbox)
    }
}

impl LineLayer {
    /// 返回包围盒与查询盒相交的候选线下标。
    pub fn candidates(&self, bbox: &Rect<f64>) -> Vec<usize> {
        query_indices(&self.tree, bbox)
    }
}

fn query_indices(tree: &RTree<IndexEntry>, bbox: &Rect<f64>) -> Vec<usize> {
    let envelope = rstar::AABB::from_corners(
        [bbox.min().x, bbox.min().y],
        [bbox.max().x, bbox.max().y],
    );
    tree.locate_in_envelope_intersecting(&envelope)
        .map(|e| e.data)
        .collect()
}

fn build_tree<F>(count: usize, bbox_of: F) -> RTree<IndexEntry>
where
    F: Fn(usize) -> Option<Rect<f64>>,
{
    let mut entries = Vec::with_capacity(count);
    for i in 0..count {
        if let Some(r) = bbox_of(i) {
            let rect = Rectangle::from_corners([r.min().x, r.min().y], [r.max().x, r.max().y]);
            entries.push(GeomWithData::new(rect, i));
        }
    }
    RTree::bulk_load(entries)
}

/// 两包围盒是否相交（闭区间）。
fn rects_intersect(a: &Rect<f64>, b: &Rect<f64>) -> bool {
    a.min().x <= b.max().x
        && a.max().x >= b.min().x
        && a.min().y <= b.max().y
        && a.max().y >= b.min().y
}

/// 全部分类参照层（多边形层 + 线层）。
pub struct ReferenceLayers {
    pub polygons: HashMap<&'static str, PolygonLayer>,
    pub lines: HashMap<&'static str, LineLayer>,
}

/// 多边形族参照层名。
const POLYGON_LAYERS: [&str; 4] = ["reservoir", "lake", "dock", "glacier"];
/// 线族参照层名（sea 亦按线族读取，对应 Python `sea_lines`）。
const LINE_LAYERS: [&str; 3] = ["river", "stream", "sea"];

impl ReferenceLayers {
    /// 从准备好的 GeoPackage 读取全部参照层，过滤到 `aoi`（工作 CRS 包围盒）内。
    pub fn load(path: &Path, aoi: &Rect<f64>) -> Result<Self> {
        let available: Vec<String> = list_gpkg_layers(path)?;
        let has = |name: &str| available.iter().any(|l| l == name);

        let mut polygons = HashMap::new();
        for &name in &POLYGON_LAYERS {
            let layer = if has(name) {
                load_polygon_layer(path, name, aoi)?
            } else {
                PolygonLayer { polys: Vec::new(), tree: RTree::new() }
            };
            polygons.insert(name, layer);
        }

        let mut lines = HashMap::new();
        for &name in &LINE_LAYERS {
            let layer = if has(name) {
                load_line_layer(path, name, aoi)?
            } else {
                LineLayer { lines: Vec::new(), tree: RTree::new() }
            };
            lines.insert(name, layer);
        }

        Ok(Self { polygons, lines })
    }
}

/// 将几何重投影到工作 CRS（若源已是工作 CRS 则原样返回）。
fn to_working(geom: &Geometry<f64>, src_epsg: u16) -> Result<Geometry<f64>> {
    reproject_geometry(geom, src_epsg, WORKING_EPSG)
}

/// 读取一个多边形族参照层：重投影 → 拆 `MultiPolygon` → aoi 过滤 → 建索引。
fn load_polygon_layer(path: &Path, layer: &str, aoi: &Rect<f64>) -> Result<PolygonLayer> {
    let (geoms, epsg) = load_gpkg_layer(path, layer)?;
    let src = epsg.unwrap_or(WORKING_EPSG);
    let mut polys: Vec<Polygon<f64>> = Vec::new();
    for g in &geoms {
        let g = to_working(g, src)?;
        for p in extract_polygons(&g) {
            if p.bounding_rect().map(|r| rects_intersect(&r, aoi)).unwrap_or(false) {
                polys.push(p);
            }
        }
    }
    let tree = build_tree(polys.len(), |i| polys[i].bounding_rect());
    Ok(PolygonLayer { polys, tree })
}

/// 读取一个线族参照层：重投影 → 拆 `MultiLineString` → aoi 过滤 → 建索引。
fn load_line_layer(path: &Path, layer: &str, aoi: &Rect<f64>) -> Result<LineLayer> {
    let (geoms, epsg) = load_gpkg_layer(path, layer)?;
    let src = epsg.unwrap_or(WORKING_EPSG);
    let mut lines: Vec<LineString<f64>> = Vec::new();
    for g in &geoms {
        let g = to_working(g, src)?;
        for l in extract_lines(&g) {
            if l.bounding_rect().map(|r| rects_intersect(&r, aoi)).unwrap_or(false) {
                lines.push(l);
            }
        }
    }
    let tree = build_tree(lines.len(), |i| lines[i].bounding_rect());
    Ok(LineLayer { lines, tree })
}

/// 提取几何中的全部多边形（对应 Python `_extract_polygon_geometries`）。
fn extract_polygons(g: &Geometry<f64>) -> Vec<Polygon<f64>> {
    match g {
        Geometry::Polygon(p) => vec![p.clone()],
        Geometry::MultiPolygon(mp) => mp.0.clone(),
        Geometry::GeometryCollection(gc) => gc.0.iter().flat_map(extract_polygons).collect(),
        _ => Vec::new(),
    }
}

/// 提取几何中的全部线（对应 Python `_extract_line_geometries`）。
fn extract_lines(g: &Geometry<f64>) -> Vec<LineString<f64>> {
    match g {
        Geometry::LineString(l) => vec![l.clone()],
        Geometry::MultiLineString(ml) => ml.0.clone(),
        Geometry::GeometryCollection(gc) => gc.0.iter().flat_map(extract_lines).collect(),
        _ => Vec::new(),
    }
}
