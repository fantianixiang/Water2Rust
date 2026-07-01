//! hydro 阶段 6c find_peaks 对拍：与 scipy.signal.find_peaks(prominence=thr) 一致。
//!
//! 夹具由 `scripts/parity/gen_stage6c_findpeaks_fixtures.py` 生成；x 中 NaN 以 null 表示。

use std::path::PathBuf;

use serde_json::Value;
use water_core::find_peaks::find_peaks_prominence;

fn load() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/stage6c_findpeaks_cases.json");
    serde_json::from_str(&std::fs::read_to_string(&path).expect("读取夹具")).expect("解析夹具")
}

fn to_vec(arr: &[Value]) -> Vec<f64> {
    arr.iter()
        .map(|v| if v.is_null() { f64::NAN } else { v.as_f64().unwrap() })
        .collect()
}

#[test]
fn find_peaks_matches_scipy() {
    let v = load();
    let mut worst = 0.0f64;
    for case in v["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let x = to_vec(case["x"].as_array().unwrap());
        let prom = case["prom"].as_f64().unwrap();
        let want_peaks: Vec<usize> = case["peaks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as usize)
            .collect();
        let want_proms = to_vec(case["prominences"].as_array().unwrap());

        let (peaks, proms) = find_peaks_prominence(&x, prom);
        assert_eq!(peaks, want_peaks, "{name} 峰下标不一致");

        assert_eq!(proms.len(), want_proms.len(), "{name} 显著度数量不一致");
        let mut max_diff = 0.0f64;
        for (g, w) in proms.iter().zip(want_proms.iter()) {
            max_diff = max_diff.max((g - w).abs());
        }
        println!("find_peaks {name}: 峰数={} 显著度最大误差={max_diff:.3e}", peaks.len());
        assert!(max_diff < 1e-12, "{name} 显著度误差 {max_diff:.3e} 超限");
        worst = worst.max(max_diff);
    }
    println!("find_peaks 总体显著度最大误差 = {worst:.3e}");
}
