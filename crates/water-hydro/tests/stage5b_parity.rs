//! hydro 阶段 5b 端到端对拍：多边形内部/岸线环 DEM 中位数。
//!
//! 夹具 `tests/fixtures/stage5b_cases.json` 由 `scripts/parity/gen_stage5b_fixtures.py`
//! 调用真实 Python 函数生成。多边形为整数像素对齐（栅格化精确一致），故中位数与像素数应精确匹配。

use std::path::PathBuf;

use geo_types::{Coord, LineString, Polygon};
use ndarray::Array2;
use serde_json::Value;
use water_hydro::lake::{
    sample_polygon_boundary_ring_dem_median, sample_polygon_interior_dem_median,
};

fn load() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stage5b_cases.json");
    serde_json::from_str(&std::fs::read_to_string(&path).expect("读取夹具")).expect("解析夹具")
}

fn ring_from(coords: &[Value]) -> LineString<f64> {
    LineString(
        coords
            .iter()
            .map(|p| {
                let a = p.as_array().unwrap();
                Coord { x: a[0].as_f64().unwrap(), y: a[1].as_f64().unwrap() }
            })
            .collect(),
    )
}

fn check(name: &str, kind: &str, got: (f64, usize), exp_med: &Value, exp_cnt: u64) {
    assert_eq!(got.1 as u64, exp_cnt, "{name} {kind} 像素数不符");
    match exp_med.as_f64() {
        Some(m) => {
            let diff = (got.0 - m).abs();
            println!("{name} {kind}: got=({},{}) exp=({m},{exp_cnt}) diff={diff:.3e}", got.0, got.1);
            assert!(diff < 1e-9, "{name} {kind} 中位数误差 {diff:.3e} 超限");
        }
        None => assert!(got.0.is_nan(), "{name} {kind} 期望 NaN 得 {}", got.0),
    }
}

#[test]
fn ring_and_interior_median_match_python() {
    let v = load();
    let h = v["h"].as_u64().unwrap() as usize;
    let w = v["w"].as_u64().unwrap() as usize;
    let dem_flat = v["dem"].as_array().unwrap();
    let mut dem = Array2::<f32>::from_elem((h, w), f32::NAN);
    for i in 0..h * w {
        if let Some(val) = dem_flat[i].as_f64() {
            dem[(i / w, i % w)] = val as f32;
        }
    }

    for case in v["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let t: Vec<f64> = case["transform"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
        let transform: [f64; 6] = [t[0], t[1], t[2], t[3], t[4], t[5]];
        let exterior = ring_from(case["exterior"].as_array().unwrap());
        let interiors: Vec<LineString<f64>> = case["interiors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| ring_from(r.as_array().unwrap()))
            .collect();
        let polygon = Polygon::new(exterior, interiors);

        let interior = sample_polygon_interior_dem_median(&polygon, &dem, &transform);
        check(name, "interior", interior, &case["interior_median"], case["interior_count"].as_u64().unwrap());

        let ring = sample_polygon_boundary_ring_dem_median(&polygon, &dem, &transform);
        check(name, "ring", ring, &case["ring_median"], case["ring_count"].as_u64().unwrap());
    }
}
