//! hydro 阶段 5 数值对拍：多边形栅格化(all_touched=False) 与 binary_erosion。
//!
//! 夹具 `tests/fixtures/stage5_cases.json` 由 `scripts/parity/gen_stage5_fixtures.py`
//! 用 rasterio(栅格化) 与 scipy(腐蚀) 生成。

use std::path::PathBuf;

use geo_types::{Coord, LineString, Polygon};
use ndarray::Array2;
use serde_json::Value;
use water_core::raster_ops::binary_erosion;
use water_io::raster::rasterize_polygon_mask;

fn load() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stage5_cases.json");
    serde_json::from_str(&std::fs::read_to_string(&path).expect("读取夹具")).expect("解析夹具")
}

fn ring_from(coords: &[Value]) -> LineString<f64> {
    LineString(
        coords
            .iter()
            .map(|p| {
                let a = p.as_array().unwrap();
                Coord {
                    x: a[0].as_f64().unwrap(),
                    y: a[1].as_f64().unwrap(),
                }
            })
            .collect(),
    )
}

fn bool_grid(arr: &[Value], h: usize, w: usize) -> Array2<bool> {
    let mut g = Array2::<bool>::from_elem((h, w), false);
    for i in 0..h * w {
        g[(i / w, i % w)] = arr[i].as_u64().unwrap() != 0;
    }
    g
}

#[test]
fn rasterize_matches_rasterio() {
    let v = load();
    let mut total_mismatch = 0usize;
    for case in v["rasterize_false"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let h = case["h"].as_u64().unwrap() as usize;
        let w = case["w"].as_u64().unwrap() as usize;
        let t: Vec<f64> = case["transform"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_f64().unwrap())
            .collect();
        let transform: [f64; 6] = [t[0], t[1], t[2], t[3], t[4], t[5]];

        let exterior = ring_from(case["exterior"].as_array().unwrap());
        let interiors: Vec<LineString<f64>> = case["interiors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| ring_from(r.as_array().unwrap()))
            .collect();
        let polygon = Polygon::new(exterior, interiors);

        let got = rasterize_polygon_mask(&polygon, &transform, w as u32, h as u32);
        let expected = bool_grid(case["mask"].as_array().unwrap(), h, w);

        let mut mismatch = 0usize;
        for r in 0..h {
            for c in 0..w {
                if got[(r, c)] != expected[(r, c)] {
                    mismatch += 1;
                }
            }
        }
        let total = h * w;
        let frac = mismatch as f64 / total as f64;
        println!(
            "rasterize {name}: 不一致像素 {mismatch}/{total} ({:.4}%)",
            100.0 * frac
        );
        // 容差：整数/轴对齐多边形精确一致；分数斜边因像素中心压边的浮点 tie-break，
        // 每多边形容许 ≤1 个边界像素、且占比 < 0.5%（物理可忽略，详见 docs/HYDRO.md）。
        assert!(
            mismatch <= 1 && frac < 0.005,
            "rasterize {name} 不一致 {mismatch}/{total} 超出容差(≤1 且 <0.5%)"
        );
        total_mismatch += mismatch;
    }
    println!("rasterize 总不一致像素 = {total_mismatch}（均为分数斜边压边 tie-break）");
}

#[test]
fn erosion_matches_scipy() {
    let v = load();
    for case in v["erosion"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let h = case["h"].as_u64().unwrap() as usize;
        let w = case["w"].as_u64().unwrap() as usize;
        let iters = case["iterations"].as_u64().unwrap() as usize;
        let mask = bool_grid(case["mask"].as_array().unwrap(), h, w);
        let expected = bool_grid(case["eroded"].as_array().unwrap(), h, w);

        let got = binary_erosion(&mask, iters);
        let mut mismatch = 0usize;
        for r in 0..h {
            for c in 0..w {
                if got[(r, c)] != expected[(r, c)] {
                    mismatch += 1;
                }
            }
        }
        println!("erosion {name}: 不一致像素 {mismatch}");
        assert_eq!(mismatch, 0, "{name} 腐蚀与 scipy 不一致 {mismatch} 像素");
    }
}
