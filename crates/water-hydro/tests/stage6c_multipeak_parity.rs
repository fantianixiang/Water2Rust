//! hydro 阶段 6c 多峰等渗回归对拍：与 Python `_isotonic_multi_peak` 一致。
//!
//! 夹具由 `scripts/parity/gen_stage6c_multipeak_fixtures.py` 直接调用原 Python 生成；
//! NaN 以 null 表示。串联了 median_filter → find_peaks → 分段 PAVA 全链路。

use std::path::PathBuf;

use serde_json::Value;
use water_hydro::skeleton_zloc::isotonic_multi_peak;

fn load() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/stage6c_multipeak_cases.json");
    serde_json::from_str(&std::fs::read_to_string(&path).expect("读取夹具")).expect("解析夹具")
}

fn to_vec(arr: &[Value]) -> Vec<f64> {
    arr.iter()
        .map(|v| if v.is_null() { f64::NAN } else { v.as_f64().unwrap() })
        .collect()
}

#[test]
fn multipeak_matches_python() {
    let v = load();
    let mut worst = 0.0f64;
    for case in v["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let z = to_vec(case["z"].as_array().unwrap());
        let want = to_vec(case["out"].as_array().unwrap());

        let got = isotonic_multi_peak(&z);
        assert_eq!(got.len(), want.len(), "{name} 长度不一致");
        let mut max_diff = 0.0f64;
        for (i, (&g, &w)) in got.iter().zip(want.iter()).enumerate() {
            match (g.is_nan(), w.is_nan()) {
                (true, true) => {}
                (false, false) => max_diff = max_diff.max((g - w).abs()),
                _ => panic!("{name} 位置 {i} NaN 状态不一致: got={g} want={w}"),
            }
        }
        println!("multipeak {name}: 最大绝对误差 = {max_diff:.3e}");
        assert!(max_diff < 1e-9, "{name} 误差 {max_diff:.3e} 超限");
        worst = worst.max(max_diff);
    }
    println!("multipeak 总体最大误差 = {worst:.3e}");
}
