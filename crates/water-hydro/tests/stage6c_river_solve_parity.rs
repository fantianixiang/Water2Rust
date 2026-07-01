//! hydro 阶段 6c 河流水面数值核对拍：与 Python `solve_laplace_per_polygon` 内层块一致。
//!
//! 夹具由 `scripts/parity/gen_stage6c_river_solve_fixtures.py` 复刻内层块生成，
//! medial_axis 用固定种子并 dump tiebreaker（Rust 注入同序列）。

use std::path::PathBuf;

use ndarray::Array2;
use serde_json::Value;
use water_hydro::river_solve::solve_river_polygon_surface;

fn load() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/stage6c_river_solve_cases.json");
    serde_json::from_str(&std::fs::read_to_string(&path).expect("读取夹具")).expect("解析夹具")
}

#[test]
fn river_solve_matches_python() {
    let v = load();
    let mut worst = 0.0f64;
    for case in v["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let h = case["h"].as_u64().unwrap() as usize;
        let w = case["w"].as_u64().unwrap() as usize;
        let pixel_m = case["pixel_m"].as_f64().unwrap();

        let pm = case["poly_mask"].as_array().unwrap();
        let dm = case["dem"].as_array().unwrap();
        let mut poly_mask = Array2::<bool>::default((h, w));
        let mut dem = Array2::<f64>::zeros((h, w));
        for i in 0..h * w {
            poly_mask[(i / w, i % w)] = pm[i].as_i64().unwrap() != 0;
            dem[(i / w, i % w)] = dm[i].as_f64().unwrap();
        }

        let tiebreaker: Vec<usize> = case["tiebreaker"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e.as_u64().unwrap() as usize)
            .collect();
        let want: Vec<Option<f64>> = case["z_local"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| if e.is_null() { None } else { Some(e.as_f64().unwrap()) })
            .collect();

        let got = solve_river_polygon_surface(&poly_mask, &dem, &tiebreaker, pixel_m);

        let mut max_diff = 0.0f64;
        for i in 0..h * w {
            let g = got[(i / w, i % w)];
            match (g.is_nan(), &want[i]) {
                (true, None) => {}
                (false, Some(wv)) => max_diff = max_diff.max((g - wv).abs()),
                _ => panic!("{name} 像素 {i} NaN 状态不一致: got={g} want={:?}", want[i]),
            }
        }
        println!("river_solve {name}: z_local 最大误差={max_diff:.3e}");
        assert!(max_diff < 1e-9, "{name} z_local 误差 {max_diff:.3e} 超限");
        worst = worst.max(max_diff);
    }
    println!("river_solve 总体最大误差 = {worst:.3e}");
}
