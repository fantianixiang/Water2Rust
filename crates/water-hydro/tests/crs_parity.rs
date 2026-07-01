//! hydro CRS 解析对拍：局地 UTM 估计与原 Python 一致。
//!
//! 夹具由 `scripts/parity/gen_crs_fixtures.py` 生成（estimate 用真实 pyproj）。

use std::path::PathBuf;

use serde_json::Value;
use water_hydro::crs::{estimate_local_utm_epsg, utm_epsg_from_center};

fn load() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/crs_cases.json");
    serde_json::from_str(&std::fs::read_to_string(&path).expect("读取夹具")).expect("解析夹具")
}

#[test]
fn utm_center_matches_python() {
    let v = load();
    for c in v["centers"].as_array().unwrap() {
        let lon = c["lon"].as_f64().unwrap();
        let lat = c["lat"].as_f64().unwrap();
        let want = c["epsg"].as_u64().unwrap() as u16;
        let got = utm_epsg_from_center(lon, lat).expect("utm 估计失败");
        assert_eq!(got, want, "lon={lon} lat={lat} EPSG 不一致");
    }
    println!("utm_epsg_from_center: {} 例全部一致", v["centers"].as_array().unwrap().len());
}

#[test]
fn estimate_local_utm_matches_python() {
    let v = load();
    let cases = v["estimates"].as_array().unwrap();
    for c in cases {
        let b = c["bounds"].as_array().unwrap();
        let bounds = [
            b[0].as_f64().unwrap(),
            b[1].as_f64().unwrap(),
            b[2].as_f64().unwrap(),
            b[3].as_f64().unwrap(),
        ];
        let source_epsg = c["source_epsg"].as_u64().unwrap() as u16;
        let want = c["epsg"].as_u64().unwrap() as u16;
        let got = estimate_local_utm_epsg(bounds, source_epsg).expect("estimate 失败");
        assert_eq!(got, want, "bounds={bounds:?} src={source_epsg} EPSG 不一致");
        println!("estimate src={source_epsg} → EPSG:{got}（期望 {want}）");
    }
    println!("estimate_local_utm: {} 例一致", cases.len());
}
