//! hydro 阶段 6c 等渗回归（PAVA）对拍：与 Python `_isotonic_non_increasing` 一致。
//!
//! 夹具由 `scripts/parity/gen_stage6c_isotonic_fixtures.py` 直接调用原 Python 函数生成。
//! JSON 中 NaN 以 null 表示（Python json 写 NaN 非法）。

use std::path::PathBuf;

use serde_json::Value;
use water_hydro::skeleton_zloc::{isotonic_non_decreasing, isotonic_non_increasing};

fn load() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/stage6c_isotonic_cases.json");
    serde_json::from_str(&std::fs::read_to_string(&path).expect("读取夹具")).expect("解析夹具")
}

fn to_vec(arr: &[Value]) -> Vec<f64> {
    arr.iter()
        .map(|v| if v.is_null() { f64::NAN } else { v.as_f64().unwrap() })
        .collect()
}

/// 逐元素比较，NaN 位置须双方一致。
fn cmp(name: &str, kind: &str, got: &[f64], want: &[f64]) -> f64 {
    assert_eq!(got.len(), want.len(), "{name}/{kind} 长度不一致");
    let mut max_diff = 0.0f64;
    for (i, (&g, &w)) in got.iter().zip(want.iter()).enumerate() {
        match (g.is_nan(), w.is_nan()) {
            (true, true) => {}
            (false, false) => max_diff = max_diff.max((g - w).abs()),
            _ => panic!("{name}/{kind} 位置 {i} NaN 状态不一致: got={g} want={w}"),
        }
    }
    max_diff
}

#[test]
fn isotonic_matches_python() {
    let v = load();
    let mut worst = 0.0f64;
    for case in v["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let values = to_vec(case["values"].as_array().unwrap());
        let want_inc = to_vec(case["non_increasing"].as_array().unwrap());
        let want_dec = to_vec(case["non_decreasing"].as_array().unwrap());

        let got_inc = isotonic_non_increasing(&values);
        let got_dec = isotonic_non_decreasing(&values);

        let d1 = cmp(name, "non_inc", &got_inc, &want_inc);
        let d2 = cmp(name, "non_dec", &got_dec, &want_dec);
        let d = d1.max(d2);
        println!("isotonic {name}: 最大绝对误差 = {d:.3e}");
        assert!(d < 1e-12, "{name} 误差 {d:.3e} 超限");
        worst = worst.max(d);
    }
    println!("isotonic 总体最大误差 = {worst:.3e}");
}
