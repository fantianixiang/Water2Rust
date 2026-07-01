//! warp `suggested_warp_output` 对拍：与 rasterio `calculate_default_transform` 一致。
//!
//! 夹具由 `scripts/parity/gen_warp_suggest_fixtures.py` 用 rasterio/GDAL 生成。

use std::path::PathBuf;

use serde_json::Value;
use water_io::warp::suggested_warp_output;

fn load() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/warp_suggest_cases.json");
    serde_json::from_str(&std::fs::read_to_string(&path).expect("读取夹具")).expect("解析夹具")
}

#[test]
fn suggested_output_matches_gdal() {
    let v = load();
    let mut worst_px = 0.0f64;
    for c in v["cases"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        let src_epsg = c["src_epsg"].as_u64().unwrap() as u16;
        let dst_epsg = c["dst_epsg"].as_u64().unwrap() as u16;
        let st = c["src_transform"].as_array().unwrap();
        let mut src_transform = [0.0f64; 6];
        for i in 0..6 {
            src_transform[i] = st[i].as_f64().unwrap();
        }
        let src_w = c["src_w"].as_u64().unwrap() as usize;
        let src_h = c["src_h"].as_u64().unwrap() as usize;

        let want_t: Vec<f64> = c["dst_transform"].as_array().unwrap().iter().map(|e| e.as_f64().unwrap()).collect();
        let want_w = c["dst_w"].as_u64().unwrap() as usize;
        let want_h = c["dst_h"].as_u64().unwrap() as usize;

        let got = suggested_warp_output(src_epsg, dst_epsg, src_transform, src_w, src_h).expect("warp 估计失败");

        println!(
            "warp {name}: got {}x{} px={:.6} ox={:.4} | want {}x{} px={:.6} ox={:.4}",
            got.width, got.height, got.transform[0], got.transform[2],
            want_w, want_h, want_t[0], want_t[2]
        );
        assert_eq!(got.width, want_w, "{name} 宽不一致");
        assert_eq!(got.height, want_h, "{name} 高不一致");
        // transform：像素尺寸相对误差、原点绝对误差（Affine 序：px=t[0]、originX=t[2]、originY=t[5]）。
        let px_rel = ((got.transform[0] - want_t[0]) / want_t[0]).abs();
        let originx = (got.transform[2] - want_t[2]).abs();
        let originy = (got.transform[5] - want_t[5]).abs();
        // 原点单位：投影为米、地理为度；用相对像素尺寸衡量。
        let ox_px = originx / want_t[0].abs();
        let oy_px = originy / want_t[0].abs();
        println!(
            "warp {name}: {}x{} px_rel={px_rel:.2e} origin_px=({ox_px:.2e},{oy_px:.2e})",
            got.width, got.height
        );
        assert!(px_rel < 1e-6, "{name} 像素尺寸相对误差 {px_rel:.2e} 超限");
        assert!(ox_px < 1e-4 && oy_px < 1e-4, "{name} 原点误差过大");
        worst_px = worst_px.max(px_rel);
    }
    println!("warp suggested 像素尺寸相对误差最大 = {worst_px:.2e}");
}
