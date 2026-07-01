//! hydro 阶段 6c 横断面水位采样对拍：与 Python `_cross_section_z_at_skeleton_pixels` 一致。
//!
//! 夹具由 `scripts/parity/gen_stage6c_xsec_fixtures.py` 直接调用原 Python 函数生成。

use std::path::PathBuf;

use ndarray::Array2;
use serde_json::Value;
use water_hydro::cross_section::{cross_section_z_at_skeleton_pixels, CrossSectionParams};
use water_hydro::skeleton_zloc::Pixel;

fn load() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/stage6c_xsec_cases.json");
    serde_json::from_str(&std::fs::read_to_string(&path).expect("读取夹具")).expect("解析夹具")
}

fn opt_hit(v: &Value) -> Option<Pixel> {
    if v.is_null() {
        None
    } else {
        let a = v.as_array().unwrap();
        Some((a[0].as_i64().unwrap(), a[1].as_i64().unwrap()))
    }
}

#[test]
fn xsec_matches_python() {
    let v = load();
    let mut worst = 0.0f64;
    for case in v["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let h = case["h"].as_u64().unwrap() as usize;
        let w = case["w"].as_u64().unwrap() as usize;

        let ordered: Vec<Pixel> = case["ordered"].as_array().unwrap().iter().map(|p| {
            let a = p.as_array().unwrap();
            (a[0].as_i64().unwrap(), a[1].as_i64().unwrap())
        }).collect();
        let tangents: Vec<[f64; 2]> = case["tangents"].as_array().unwrap().iter().map(|t| {
            let a = t.as_array().unwrap();
            [a[0].as_f64().unwrap(), a[1].as_f64().unwrap()]
        }).collect();

        let bflat = case["boundary"].as_array().unwrap();
        let dflat = case["dem"].as_array().unwrap();
        let mut boundary = Array2::<bool>::default((h, w));
        let mut dem = Array2::<f64>::zeros((h, w));
        for i in 0..h * w {
            boundary[(i / w, i % w)] = bflat[i].as_i64().unwrap() != 0;
            dem[(i / w, i % w)] = if dflat[i].is_null() { f64::NAN } else { dflat[i].as_f64().unwrap() };
        }

        let edt: Option<Vec<f64>> = case["edt"].as_array().map(|a| {
            a.iter().map(|e| e.as_f64().unwrap()).collect()
        });
        let z_ref = case["z_ref"].as_f64();
        let epsilon = case["epsilon"].as_f64();
        let max_ray = case["max_ray"].as_u64().unwrap() as usize;

        let params = CrossSectionParams {
            max_ray_steps: max_ray,
            z_ref,
            epsilon,
            edt_half_widths: edt.as_deref(),
        };
        let res = cross_section_z_at_skeleton_pixels(&ordered, &tangents, &boundary, &dem, &params);

        let want_z: Vec<Option<f64>> = case["z_cross"].as_array().unwrap().iter()
            .map(|e| if e.is_null() { None } else { Some(e.as_f64().unwrap()) }).collect();
        let want_lh: Vec<Option<Pixel>> = case["left_hits"].as_array().unwrap().iter().map(opt_hit).collect();
        let want_rh: Vec<Option<Pixel>> = case["right_hits"].as_array().unwrap().iter().map(opt_hit).collect();

        let mut max_diff = 0.0f64;
        for (i, want) in want_z.iter().enumerate() {
            match (res.z_cross[i].is_nan(), want) {
                (true, None) => {}
                (false, Some(wv)) => max_diff = max_diff.max((res.z_cross[i] - wv).abs()),
                _ => panic!("{name} 站 {i} z_cross NaN 状态不一致: got={} want={want:?}", res.z_cross[i]),
            }
        }
        assert_eq!(res.left_hits, want_lh, "{name} 左命中不一致");
        assert_eq!(res.right_hits, want_rh, "{name} 右命中不一致");
        println!("xsec {name}: z_cross 最大误差={max_diff:.3e} n={}", ordered.len());
        assert!(max_diff < 1e-12, "{name} z_cross 误差 {max_diff:.3e} 超限");
        worst = worst.max(max_diff);
    }
    println!("xsec 总体最大误差 = {worst:.3e}");
}
