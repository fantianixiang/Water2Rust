//! hydro 阶段 6c 秩滤波对拍：与 scipy.ndimage percentile_filter / median_filter 一致。
//!
//! 夹具由 `scripts/parity/gen_stage6c_rankfilter_fixtures.py` 生成；
//! JSON 中 +inf/-inf 以字符串 "inf"/"-inf" 表示，NaN 以 null 表示。

use std::path::PathBuf;

use ndarray::Array2;
use serde_json::Value;
use water_core::rank_filter::{median_filter_1d, percentile_filter_2d};

fn load() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/stage6c_rankfilter_cases.json");
    serde_json::from_str(&std::fs::read_to_string(&path).expect("读取夹具")).expect("解析夹具")
}

fn num(v: &Value) -> f64 {
    match v {
        Value::Null => f64::NAN,
        Value::String(s) => match s.as_str() {
            "inf" => f64::INFINITY,
            "-inf" => f64::NEG_INFINITY,
            _ => panic!("未知字符串数值 {s}"),
        },
        _ => v.as_f64().unwrap(),
    }
}

fn to_vec(arr: &[Value]) -> Vec<f64> {
    arr.iter().map(num).collect()
}

/// 逐元素比较，±inf / NaN 需状态一致，有限值比绝对误差。
fn diff(got: f64, want: f64) -> f64 {
    match (got.is_finite(), want.is_finite()) {
        (true, true) => (got - want).abs(),
        _ => {
            assert!(
                (got.is_nan() && want.is_nan())
                    || (got == want),
                "非有限值不一致: got={got} want={want}"
            );
            0.0
        }
    }
}

#[test]
fn rank_filter_matches_scipy() {
    let v = load();
    let mut worst = 0.0f64;
    for case in v["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let kind = case["kind"].as_str().unwrap();
        let data = to_vec(case["data"].as_array().unwrap());
        let want = to_vec(case["out"].as_array().unwrap());

        let got: Vec<f64> = match kind {
            "pct2d" => {
                let h = case["h"].as_u64().unwrap() as usize;
                let w = case["w"].as_u64().unwrap() as usize;
                let pct = case["percentile"].as_f64().unwrap();
                let size = case["size"].as_u64().unwrap() as usize;
                let mut arr = Array2::<f64>::zeros((h, w));
                for i in 0..h * w {
                    arr[(i / w, i % w)] = data[i];
                }
                let out = percentile_filter_2d(&arr, pct, size);
                out.iter().copied().collect()
            }
            "med1d" => {
                let size = case["size"].as_u64().unwrap() as usize;
                median_filter_1d(&data, size)
            }
            other => panic!("未知 kind {other}"),
        };

        assert_eq!(got.len(), want.len(), "{name} 长度不一致");
        let mut max_diff = 0.0f64;
        for (g, w) in got.iter().zip(want.iter()) {
            max_diff = max_diff.max(diff(*g, *w));
        }
        println!("rankfilter {kind} {name}: 最大绝对误差 = {max_diff:.3e}");
        assert!(max_diff < 1e-12, "{name} 误差 {max_diff:.3e} 超限");
        worst = worst.max(max_diff);
    }
    println!("rankfilter 总体最大误差 = {worst:.3e}");
}
