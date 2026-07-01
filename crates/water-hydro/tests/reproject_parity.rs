//! hydro 几何重投影对拍：逐顶点 src→dst 与 pyproj（geopandas to_crs）一致。
//!
//! 夹具由 `scripts/parity/gen_reproject_fixtures.py` 生成。proj4rs 与 pyproj 的投影实现
//! 略有差异，容差取 UTM/Mercator 尺度下 ≤1e-2 米（实测远优于此）。

use std::path::PathBuf;

use geo_types::{LineString, Polygon};
use serde_json::Value;
use water_hydro::crs::{proj_from_epsg, reproject_polygon};

fn load() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/reproject_cases.json");
    serde_json::from_str(&std::fs::read_to_string(&path).expect("读取夹具")).expect("解析夹具")
}

fn ring(v: &Value) -> LineString<f64> {
    let coords: Vec<(f64, f64)> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            let a = p.as_array().unwrap();
            (a[0].as_f64().unwrap(), a[1].as_f64().unwrap())
        })
        .collect();
    LineString::from(coords)
}

#[test]
fn reproject_matches_pyproj() {
    let v = load();
    let mut worst = 0.0f64;
    for case in v["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let src_epsg = case["src_epsg"].as_u64().unwrap() as u16;
        let dst_epsg = case["dst_epsg"].as_u64().unwrap() as u16;

        let exterior = ring(&case["src_exterior"]);
        let interiors: Vec<LineString<f64>> =
            case["src_interiors"].as_array().unwrap().iter().map(ring).collect();
        let polygon = Polygon::new(exterior, interiors);

        let src = proj_from_epsg(src_epsg).expect("src proj");
        let dst = proj_from_epsg(dst_epsg).expect("dst proj");
        let got = reproject_polygon(&polygon, &src, &dst).expect("重投影失败");

        let want_ext = ring(&case["dst_exterior"]);
        let want_ints: Vec<LineString<f64>> =
            case["dst_interiors"].as_array().unwrap().iter().map(ring).collect();

        let mut max_diff = 0.0f64;
        let mut cmp_ring = |g: &LineString<f64>, w: &LineString<f64>| {
            assert_eq!(g.0.len(), w.0.len(), "{name} 环顶点数不一致");
            for (gc, wc) in g.0.iter().zip(w.0.iter()) {
                max_diff = max_diff.max((gc.x - wc.x).abs()).max((gc.y - wc.y).abs());
            }
        };
        cmp_ring(got.exterior(), &want_ext);
        for (gi, wi) in got.interiors().iter().zip(want_ints.iter()) {
            cmp_ring(gi, wi);
        }

        println!("reproject {name}: {src_epsg}->{dst_epsg} 最大坐标误差={max_diff:.3e}");
        assert!(max_diff < 1e-2, "{name} 误差 {max_diff:.3e} 超限");
        worst = worst.max(max_diff);
    }
    println!("reproject 总体最大坐标误差 = {worst:.3e}");
}
