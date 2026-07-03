//! 一次性工具：从 linzhi 全量数据裁出含「大 / 中 / 小」三个水体的小范围测试集。
//!
//! 目的：全量 DEM 1.8GB + 大水体导致 hydro 无法在合理时间跑完；裁出紧凑子场景，
//! 使 fclass→edge→hydro 全链路可在秒级/分钟级完成，用于验证 Laplace 求解器换 faer 后
//! 的正确性与耗时。
//!
//! 用法：cargo run --release -p water-io --example clip_testdata
//! 产物：data/linzhi_clip/{dem.tif, waters.shp/.shx/.dbf/.prj}

use std::path::PathBuf;

use geo::{Area, BoundingRect};
use geo_types::{Geometry, MultiPolygon};
use water_io::geotiff_write::write_geotiff_f32;
use water_io::raster::Dem;
use water_io::vector::{
    json_to_shp_value, read_shapefile_fields, read_vector, write_polygons_shapefile, ShpValue,
};

/// 单个水体要素的度量。
struct Feat {
    idx: usize,
    area: f64,       // deg²（同一小区域内用于相对排序足够）
    gb: [f64; 4],    // 地理 bbox [min_x,min_y,max_x,max_y]
    fclass: String,
}

/// “大”水体像素跨度上限（每边），避免选到会把求解器算爆的超大水体。
const LARGE_MAX_SPAN_PX: f64 = 2000.0;
/// 裁剪窗口每边像素上限，控制 DEM 产物规模与解码耗时。
const MAX_WIN_PX: i64 = 3600;
/// 窗口在三水体并集外的填充比例。
const PAD_FRAC: f64 = 0.15;

fn to_multipolygon(g: &Geometry<f64>) -> Option<MultiPolygon<f64>> {
    match g {
        Geometry::Polygon(p) => Some(MultiPolygon(vec![p.clone()])),
        Geometry::MultiPolygon(mp) => Some(mp.clone()),
        _ => None,
    }
}

fn center(gb: &[f64; 4]) -> (f64, f64) {
    ((gb[0] + gb[2]) / 2.0, (gb[1] + gb[3]) / 2.0)
}

fn span_px(gb: &[f64; 4], px: f64, py: f64) -> (f64, f64) {
    (((gb[2] - gb[0]) / px).abs(), ((gb[3] - gb[1]) / py).abs())
}

/// 选大/中/小三个水体：大=跨度受限下面积最大；中/小=就近于“大”且面积分档。
fn pick_three(feats: &[Feat], px: f64, py: f64) -> (usize, usize, usize) {
    // feats 已按 area 降序。
    let large_pos = feats
        .iter()
        .position(|f| {
            let (w, h) = span_px(&f.gb, px, py);
            w <= LARGE_MAX_SPAN_PX && h <= LARGE_MAX_SPAN_PX
        })
        .unwrap_or(0);
    let large = &feats[large_pos];
    let (lx, ly) = center(&large.gb);
    let dist = |gb: &[f64; 4]| {
        let (cx, cy) = center(gb);
        ((cx - lx).powi(2) + (cy - ly).powi(2)).sqrt()
    };

    // 中：面积在 [0.08,0.6]×大，就近；小：面积 < 0.08×大 且 >0，就近。
    let nearest = |lo: f64, hi: f64| -> Option<usize> {
        feats
            .iter()
            .enumerate()
            .filter(|(i, f)| *i != large_pos && f.area > lo * large.area && f.area <= hi * large.area)
            .min_by(|(_, a), (_, b)| dist(&a.gb).partial_cmp(&dist(&b.gb)).unwrap())
            .map(|(i, _)| i)
    };
    let med = nearest(0.08, 0.6).unwrap_or_else(|| (large_pos + 1).min(feats.len() - 1));
    let small = nearest(0.0, 0.08)
        .filter(|&i| i != med)
        .unwrap_or_else(|| feats.len() - 1);
    (large_pos, med, small)
}

fn main() -> anyhow::Result<()> {
    let base = PathBuf::from(r"E:\Projects\Water2Rust\data\linzhi");
    let out = PathBuf::from(r"E:\Projects\Water2Rust\data\linzhi_clip");
    std::fs::create_dir_all(&out)?;
    let waters_shp = base.join("waters.shp");

    // ── DEM 元数据（惰性） ──
    let dem = Dem::open(&base.join("dem.tif"))?;
    let m = dem.meta();
    let (px, py) = (m.pixel_size_x, m.pixel_size_y);
    let (ox, oy) = (m.min_x, m.max_y);
    println!(
        "DEM {}x{} epsg={:?} px=({:.3e},{:.3e}) x[{:.5},{:.5}] y[{:.5},{:.5}] nodata={:?}",
        m.width, m.height, m.crs_epsg, px, py, m.min_x, m.max_x, m.min_y, m.max_y, m.nodata
    );

    // ── 读水体，计算面积/bbox ──
    let fc = read_vector(&waters_shp)?;
    let mut feats: Vec<Feat> = fc
        .features
        .iter()
        .enumerate()
        .filter_map(|(i, f)| {
            let mp = to_multipolygon(&f.geometry)?;
            let r = mp.bounding_rect()?;
            let fclass = f
                .properties
                .get("fclass")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string();
            Some(Feat {
                idx: i,
                area: mp.unsigned_area(),
                gb: [r.min().x, r.min().y, r.max().x, r.max().y],
                fclass,
            })
        })
        .collect();
    feats.sort_by(|a, b| b.area.partial_cmp(&a.area).unwrap());
    println!("features={} crs={:?}", feats.len(), fc.crs_epsg);
    for f in &feats {
        let (w, h) = span_px(&f.gb, px, py);
        let (cx, cy) = center(&f.gb);
        println!(
            "  #{:>2} {:<6} area={:.3e}  pxbbox={:>6.0}x{:<6.0}  center=({:.4},{:.4})",
            f.idx, f.fclass, f.area, w, h, cx, cy
        );
    }

    // ── 选三，计算并集窗口 ──
    let (li, mi, si) = pick_three(&feats, px, py);
    let chosen = [&feats[li], &feats[mi], &feats[si]];
    println!(
        "选定 大=#{}({}) 中=#{}({}) 小=#{}({})",
        chosen[0].idx, chosen[0].fclass, chosen[1].idx, chosen[1].fclass, chosen[2].idx, chosen[2].fclass
    );
    let mut u = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for c in &chosen {
        u[0] = u[0].min(c.gb[0]);
        u[1] = u[1].min(c.gb[1]);
        u[2] = u[2].max(c.gb[2]);
        u[3] = u[3].max(c.gb[3]);
    }
    let (pw, ph) = ((u[2] - u[0]) * PAD_FRAC, (u[3] - u[1]) * PAD_FRAC);
    let gb = [u[0] - pw, u[1] - ph, u[2] + pw, u[3] + ph];

    // ── 地理 bbox → DEM 像素窗口，clamp 到 DEM 与最大窗口 ──
    let col0 = (((gb[0] - ox) / px).floor() as i64).max(0);
    let row0 = (((oy - gb[3]) / py).floor() as i64).max(0);
    let mut col1 = (((gb[2] - ox) / px).ceil() as i64).min(m.width as i64);
    let mut row1 = (((oy - gb[1]) / py).ceil() as i64).min(m.height as i64);
    if col1 - col0 > MAX_WIN_PX {
        col1 = col0 + MAX_WIN_PX;
    }
    if row1 - row0 > MAX_WIN_PX {
        row1 = row0 + MAX_WIN_PX;
    }
    let (w, h) = ((col1 - col0) as u32, (row1 - row0) as u32);
    // 窗口实际地理范围（用于矢量筛选，保证只留完全落入窗口的要素）。
    let win_gb = [
        ox + col0 as f64 * px,
        oy - row1 as f64 * py,
        ox + col1 as f64 * px,
        oy - row0 as f64 * py,
    ];
    println!(
        "窗口 px=({},{}) {}x{}  geo x[{:.5},{:.5}] y[{:.5},{:.5}]",
        col0, row0, w, h, win_gb[0], win_gb[2], win_gb[1], win_gb[3]
    );

    // ── 裁 DEM 窗口并写出（nodata=NaN，与源保真） ──
    let (win, tr) = dem.read_window_f32(col0 as u32, row0 as u32, w, h)?;
    let epsg = m.crs_epsg.unwrap_or(4326) as u16;
    let is_geo = matches!(m.crs_epsg, Some(4326) | Some(4490));
    write_geotiff_f32(&out.join("dem.tif"), &win, tr, epsg, is_geo, Some(f64::NAN))?;
    println!("写出 dem.tif {}x{}", w, h);

    // ── 裁矢量：完全落入窗口的要素 ──
    let fields = read_shapefile_fields(&waters_shp)?;
    let prj = std::fs::read_to_string(base.join("waters.prj")).ok();
    let mut polys: Vec<MultiPolygon<f64>> = Vec::new();
    let mut records: Vec<Vec<ShpValue>> = Vec::new();
    let mut kept = 0usize;
    for (i, f) in fc.features.iter().enumerate() {
        let Some(mp) = to_multipolygon(&f.geometry) else { continue };
        let Some(r) = mp.bounding_rect() else { continue };
        let inside = r.min().x >= win_gb[0]
            && r.min().y >= win_gb[1]
            && r.max().x <= win_gb[2]
            && r.max().y <= win_gb[3];
        if !inside {
            continue;
        }
        let rec = fields
            .iter()
            .map(|fd| json_to_shp_value(f.properties.get(&fd.name), &fd.ty))
            .collect();
        let _ = i;
        polys.push(mp);
        records.push(rec);
        kept += 1;
    }
    write_polygons_shapefile(&out.join("waters.shp"), &polys, &fields, &records, prj.as_deref())?;
    println!("写出 waters.shp 要素={}（窗口内完全包含）", kept);
    Ok(())
}
