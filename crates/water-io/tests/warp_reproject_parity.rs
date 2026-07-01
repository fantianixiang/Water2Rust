//! warp `reproject` 对拍：与 rasterio `reproject`（bilinear/nearest + nodata）一致。
//!
//! 夹具由 `scripts/parity/gen_warp_reproject_fixtures.py` 用 rasterio/GDAL 生成。
//! 目标网格 transform/宽高直接采用 rasterio 的 `calculate_default_transform` 结果，
//! 以隔离本测试专测 reproject 重采样核（网格计算已由 warp_suggest 对拍覆盖）。

use std::path::PathBuf;

use ndarray::Array2;
use serde_json::Value;
use water_io::warp::{reproject, Resampling};

fn load() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/warp_reproject_cases.json");
    serde_json::from_str(&std::fs::read_to_string(&path).expect("读取夹具")).expect("解析夹具")
}

fn arr6(v: &Value) -> [f64; 6] {
    let a = v.as_array().unwrap();
    let mut t = [0.0f64; 6];
    for i in 0..6 {
        t[i] = a[i].as_f64().unwrap();
    }
    t
}

#[test]
fn reproject_matches_gdal() {
    let v = load();
    let mut worst = 0.0f64;
    for c in v["cases"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        let method = c["method"].as_str().unwrap();
        let src_epsg = c["src_epsg"].as_u64().unwrap() as u16;
        let dst_epsg = c["dst_epsg"].as_u64().unwrap() as u16;
        let src_transform = arr6(&c["src_transform"]);
        let dst_transform = arr6(&c["dst_transform"]);
        let sw = c["src_w"].as_u64().unwrap() as usize;
        let sht = c["src_h"].as_u64().unwrap() as usize;
        let dw = c["dst_w"].as_u64().unwrap() as usize;
        let dh = c["dst_h"].as_u64().unwrap() as usize;
        let nodata = c["nodata"].as_f64();

        let sflat = c["src"].as_array().unwrap();
        let mut src = Array2::<f32>::zeros((sht, sw));
        for k in 0..sw * sht {
            src[(k / sw, k % sw)] = if sflat[k].is_null() { f32::NAN } else { sflat[k].as_f64().unwrap() as f32 };
        }

        let want: Vec<Option<f64>> = c["dst"].as_array().unwrap().iter()
            .map(|e| if e.is_null() { None } else { Some(e.as_f64().unwrap()) }).collect();

        let rs = if method == "bilinear" { Resampling::Bilinear } else { Resampling::Nearest };
        let got = reproject(&src, src_transform, src_epsg, nodata, dst_transform, dw, dh, dst_epsg, rs)
            .expect("reproject 失败");

        let mut max_diff = 0.0f64;
        let mut nan_mismatch = 0usize;
        for k in 0..dw * dh {
            let g = got[(k / dw, k % dw)];
            match (g.is_nan(), &want[k]) {
                (true, None) => {}
                (false, Some(wv)) => max_diff = max_diff.max((g as f64 - wv).abs()),
                _ => nan_mismatch += 1,
            }
        }
        println!("reproject {name}({method}): 最大误差={max_diff:.3e} NaN失配={nan_mismatch}");
        assert_eq!(nan_mismatch, 0, "{name} NaN 掩膜失配 {nan_mismatch} 处");
        assert!(max_diff < 1e-3, "{name} 值误差 {max_diff:.3e} 超限");
        worst = worst.max(max_diff);
    }
    println!("reproject 总体最大误差 = {worst:.3e}");
}
