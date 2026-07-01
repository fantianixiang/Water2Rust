//! hydro 阶段 6c 河流外层循环对拍：与 Python `solve_laplace_per_polygon` 一致。
//!
//! 夹具由 `scripts/parity/gen_stage6c_river_pipeline_fixtures.py` 生成：合成 transform +
//! DEM + 河流多边形，逐多边形窗口/光栅化/求解/缝合，dump 每多边形 tiebreaker + 最终 surface。

use std::path::PathBuf;

use geo_types::{LineString, Polygon};
use ndarray::Array2;
use serde_json::Value;
use water_hydro::river_pipeline::solve_laplace_per_polygon;

fn load() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/stage6c_river_pipeline_cases.json");
    serde_json::from_str(&std::fs::read_to_string(&path).expect("读取夹具")).expect("解析夹具")
}

#[test]
fn river_pipeline_matches_python() {
    let v = load();
    let case = &v["cases"][0];
    let h = case["h"].as_u64().unwrap() as usize;
    let w = case["w"].as_u64().unwrap() as usize;

    let t = case["transform"].as_array().unwrap();
    let mut transform = [0.0f64; 6];
    for i in 0..6 {
        transform[i] = t[i].as_f64().unwrap();
    }

    let dflat = case["dem"].as_array().unwrap();
    let mut dem = Array2::<f64>::zeros((h, w));
    for i in 0..h * w {
        dem[(i / w, i % w)] = dflat[i].as_f64().unwrap();
    }

    let polygons: Vec<Polygon<f64>> = case["polygons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|ring| {
            let coords: Vec<(f64, f64)> = ring
                .as_array()
                .unwrap()
                .iter()
                .map(|p| {
                    let a = p.as_array().unwrap();
                    (a[0].as_f64().unwrap(), a[1].as_f64().unwrap())
                })
                .collect();
            Polygon::new(LineString::from(coords), vec![])
        })
        .collect();

    let fclass: Vec<Option<String>> = case["fclass"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e.as_str().map(|s| s.to_string()))
        .collect();

    let tiebreakers: Vec<Vec<usize>> = case["tiebreakers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tb| {
            tb.as_array()
                .unwrap()
                .iter()
                .map(|e| e.as_u64().unwrap() as usize)
                .collect()
        })
        .collect();

    let want: Vec<Option<f64>> = case["surface"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| if e.is_null() { None } else { Some(e.as_f64().unwrap()) })
        .collect();

    let surface = solve_laplace_per_polygon(
        &transform,
        h,
        w,
        &polygons,
        &fclass,
        &dem,
        true,
        |idx, _n| tiebreakers[idx].clone(),
    );

    let mut max_diff = 0.0f64;
    for i in 0..h * w {
        let g = surface[(i / w, i % w)];
        match (g.is_nan(), &want[i]) {
            (true, None) => {}
            (false, Some(wv)) => max_diff = max_diff.max((g as f64 - wv).abs()),
            _ => panic!("像素 {i} NaN 状态不一致: got={g} want={:?}", want[i]),
        }
    }
    println!("river_pipeline: surface 最大误差={max_diff:.3e}");
    assert!(max_diff < 1e-5, "surface 误差 {max_diff:.3e} 超限");
}
