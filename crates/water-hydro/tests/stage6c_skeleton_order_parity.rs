//! hydro 阶段 6c 骨架沿流排序对拍：与 Python `_order_skeleton_pixels_along_flow` 一致。
//!
//! 夹具由 `scripts/parity/gen_stage6c_skeleton_order_fixtures.py` 直接调用原 Python 函数生成。

use std::path::PathBuf;

use ndarray::Array2;
use serde_json::Value;
use water_hydro::skeleton_graph::order_skeleton_pixels_along_flow;

fn load() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/stage6c_skeleton_order_cases.json");
    serde_json::from_str(&std::fs::read_to_string(&path).expect("读取夹具")).expect("解析夹具")
}

#[test]
fn skeleton_order_matches_python() {
    let v = load();
    for case in v["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let h = case["h"].as_u64().unwrap() as usize;
        let w = case["w"].as_u64().unwrap() as usize;
        let skel_flat = case["skel"].as_array().unwrap();
        let dem_flat = case["dem"].as_array().unwrap();

        let mut skel = Array2::<bool>::default((h, w));
        let mut dem = Array2::<f64>::zeros((h, w));
        for i in 0..h * w {
            skel[(i / w, i % w)] = skel_flat[i].as_i64().unwrap() != 0;
            dem[(i / w, i % w)] = dem_flat[i].as_f64().unwrap();
        }

        let want: Vec<(i64, i64)> = case["ordered"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                let a = p.as_array().unwrap();
                (a[0].as_i64().unwrap(), a[1].as_i64().unwrap())
            })
            .collect();

        let got = order_skeleton_pixels_along_flow(&skel, &dem);
        assert_eq!(got, want, "{name} 排序不一致\n实得={got:?}\n期望={want:?}");
        println!("skeleton_order {name}: n={} 一致", got.len());
    }
}
