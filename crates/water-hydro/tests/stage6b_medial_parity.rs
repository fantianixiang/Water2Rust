//! hydro 阶段 6b medial_axis 对拍：与 skimage 同随机种子下逐像素一致。
//!
//! 夹具由 `scripts/parity/gen_stage6b_medial_fixtures.py` 用固定种子生成，
//! 并 dump skimage 内部的 `tiebreaker`；本测试注入同一 tiebreaker，比对骨架逐像素相等。

use std::path::PathBuf;

use ndarray::Array2;
use serde_json::Value;
use water_core::raster_ops::medial_axis;

fn load() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/stage6b_medial_cases.json");
    serde_json::from_str(&std::fs::read_to_string(&path).expect("读取夹具")).expect("解析夹具")
}

#[test]
fn medial_axis_matches_skimage() {
    let v = load();
    for case in v["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let h = case["h"].as_u64().unwrap() as usize;
        let w = case["w"].as_u64().unwrap() as usize;

        let mut mask = Array2::<bool>::from_elem((h, w), false);
        let mflat = case["mask"].as_array().unwrap();
        for i in 0..h * w {
            mask[(i / w, i % w)] = mflat[i].as_u64().unwrap() != 0;
        }
        let tiebreaker: Vec<usize> = case["tiebreaker"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_u64().unwrap() as usize)
            .collect();

        let got = medial_axis(&mask, &tiebreaker);

        let skel = case["skel"].as_array().unwrap();
        let mut mismatch = 0usize;
        for i in 0..h * w {
            let (r, c) = (i / w, i % w);
            if got[(r, c)] != (skel[i].as_u64().unwrap() != 0) {
                mismatch += 1;
            }
        }
        println!("medial_axis {name}: 不一致像素 {mismatch}");
        assert_eq!(mismatch, 0, "{name} 与 skimage 骨架不一致 {mismatch} 像素");
    }
}
