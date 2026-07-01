//! hydro 阶段 6c 骨架几何对拍：切线 + 汇流站点，与 Python 一致。
//!
//! 夹具由 `scripts/parity/gen_stage6c_skeleton_geom_fixtures.py` 直接调用原 Python 函数生成。

use std::path::PathBuf;

use serde_json::Value;
use water_hydro::skeleton_zloc::{compute_skeleton_tangents, detect_junction_stations, Pixel};

fn load() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/stage6c_skeleton_geom_cases.json");
    serde_json::from_str(&std::fs::read_to_string(&path).expect("读取夹具")).expect("解析夹具")
}

#[test]
fn skeleton_geom_matches_python() {
    let v = load();
    let mut worst_tan = 0.0f64;
    for case in v["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let context = case["context"].as_u64().unwrap() as usize;
        let radius = case["radius"].as_f64().unwrap();
        let thr = case["thr"].as_f64().unwrap();

        let ordered: Vec<Pixel> = case["ordered"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                let a = p.as_array().unwrap();
                (a[0].as_i64().unwrap(), a[1].as_i64().unwrap())
            })
            .collect();
        let want_tan: Vec<[f64; 2]> = case["tangents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| {
                let a = t.as_array().unwrap();
                [a[0].as_f64().unwrap(), a[1].as_f64().unwrap()]
            })
            .collect();
        let want_junc: Vec<bool> = case["junction"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b.as_bool().unwrap())
            .collect();

        let got_tan = compute_skeleton_tangents(&ordered, context);
        let mut max_tan_diff = 0.0f64;
        assert_eq!(got_tan.len(), want_tan.len(), "{name} 切线长度不一致");
        for (g, w) in got_tan.iter().zip(want_tan.iter()) {
            max_tan_diff = max_tan_diff.max((g[0] - w[0]).abs()).max((g[1] - w[1]).abs());
        }

        // 用 Python 的切线驱动汇流检测，确保逻辑独立可对拍。
        let got_junc = detect_junction_stations(&ordered, &want_tan, radius, thr);
        assert_eq!(got_junc, want_junc, "{name} 汇流站点掩膜不一致");

        // 再用 Rust 自算切线驱动一遍，确保端到端一致。
        let got_junc2 = detect_junction_stations(&ordered, &got_tan, radius, thr);
        assert_eq!(got_junc2, want_junc, "{name} 汇流站点(自算切线)不一致");

        println!("skeleton_geom {name}: 切线最大误差={max_tan_diff:.3e} 汇流站点={}", want_junc.iter().filter(|b| **b).count());
        assert!(max_tan_diff < 1e-12, "{name} 切线误差 {max_tan_diff:.3e} 超限");
        worst_tan = worst_tan.max(max_tan_diff);
    }
    println!("skeleton_geom 切线总体最大误差 = {worst_tan:.3e}");
}
