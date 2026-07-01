//! hydro 阶段 6c gaussian_filter 对拍：与 scipy.ndimage.gaussian_filter 一致。
//!
//! 夹具由 `scripts/parity/gen_stage6c_gaussian_fixtures.py` 用 scipy 默认参数生成。

use std::path::PathBuf;

use ndarray::Array2;
use serde_json::Value;
use water_core::raster_ops::gaussian_smooth;

fn load() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/stage6c_gaussian_cases.json");
    serde_json::from_str(&std::fs::read_to_string(&path).expect("读取夹具")).expect("解析夹具")
}

#[test]
fn gaussian_matches_scipy() {
    let v = load();
    let mut worst = 0.0f64;
    for case in v["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let h = case["h"].as_u64().unwrap() as usize;
        let w = case["w"].as_u64().unwrap() as usize;
        let sigma = case["sigma"].as_f64().unwrap();
        let dflat = case["data"].as_array().unwrap();
        let oflat = case["out"].as_array().unwrap();

        let mut data = Array2::<f64>::zeros((h, w));
        for i in 0..h * w {
            data[(i / w, i % w)] = dflat[i].as_f64().unwrap();
        }

        let got = gaussian_smooth(&data, sigma);
        let mut max_diff = 0.0f64;
        for i in 0..h * w {
            let (r, c) = (i / w, i % w);
            max_diff = max_diff.max((got[(r, c)] - oflat[i].as_f64().unwrap()).abs());
        }
        println!("gaussian {name}: 最大绝对误差 = {max_diff:.3e}");
        assert!(max_diff < 1e-9, "{name} 误差 {max_diff:.3e} 超限");
        worst = worst.max(max_diff);
    }
    println!("gaussian 总体最大误差 = {worst:.3e}");
}
