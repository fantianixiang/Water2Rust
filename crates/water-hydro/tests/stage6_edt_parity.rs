//! hydro 阶段 6 EDT 数值对拍：`distance_transform_edt` 与 scipy 一致。
//!
//! 夹具由 `scripts/parity/gen_stage6_edt_fixtures.py` 用 scipy 生成。
//! - 距离场：与 scipy 精确一致（容差 1e-9）。
//! - 最近特征索引：并列平手时 scipy 与本实现可能选不同像素，故校验
//!   「本实现所选特征确为背景像素、且其欧氏距离等于 scipy 距离」，即为正确最近特征。

use std::path::PathBuf;

use ndarray::Array2;
use serde_json::Value;
use water_core::raster_ops::distance_transform_edt;

fn load() -> Value {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stage6_edt_cases.json");
    serde_json::from_str(&std::fs::read_to_string(&path).expect("读取夹具")).expect("解析夹具")
}

#[test]
fn edt_matches_scipy() {
    let v = load();
    let mut worst = 0.0f64;
    for case in v["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let h = case["h"].as_u64().unwrap() as usize;
        let w = case["w"].as_u64().unwrap() as usize;
        let mask_flat = case["mask"].as_array().unwrap();
        let dist_flat = case["dist"].as_array().unwrap();
        let ir_flat = case["ir"].as_array().unwrap();
        let ic_flat = case["ic"].as_array().unwrap();

        let mut mask = Array2::<bool>::from_elem((h, w), false);
        for i in 0..h * w {
            mask[(i / w, i % w)] = mask_flat[i].as_u64().unwrap() != 0;
        }

        let edt = distance_transform_edt(&mask);

        let mut max_diff = 0.0f64;
        for i in 0..h * w {
            let (r, c) = (i / w, i % w);
            // 距离精确一致
            let exp = dist_flat[i].as_f64().unwrap();
            let got = edt.distances[(r, c)];
            max_diff = max_diff.max((got - exp).abs());

            // 特征索引校验：本实现所选特征须为背景像素，且距离等于 scipy 距离
            let (fr, fc) = (edt.index_row[(r, c)] as usize, edt.index_col[(r, c)] as usize);
            assert!(
                !mask[(fr, fc)],
                "{name} 像素({r},{c}) 特征({fr},{fc}) 不是背景像素"
            );
            let dr = r as f64 - fr as f64;
            let dc = c as f64 - fc as f64;
            let feat_dist = (dr * dr + dc * dc).sqrt();
            assert!(
                (feat_dist - exp).abs() < 1e-9,
                "{name} 像素({r},{c}) 特征距离 {feat_dist} != scipy {exp}"
            );

            // 若 scipy 的特征与本实现一致（无平手），顺带确认（非强制）
            let _ = (ir_flat[i].as_i64(), ic_flat[i].as_i64());
        }
        println!("edt {name}: 距离最大误差 = {max_diff:.3e}");
        assert!(max_diff < 1e-9, "{name} 距离误差 {max_diff:.3e} 超限");
        worst = worst.max(max_diff);
    }
    println!("edt 总体距离最大误差 = {worst:.3e}");
}
