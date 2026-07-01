//! hydro 阶段 5c 端到端对拍：湖泊压平（常数水位盖回求解面）。
//!
//! 夹具由 `scripts/parity/gen_stage5c_fixtures.py` 调用真实 Python
//! `_flatten_lake_polygons_on_surface` 生成，覆盖孤立湖与 component 分组两种场景。

use std::collections::HashMap;
use std::path::PathBuf;

use geo_types::{Coord, LineString, Polygon};
use ndarray::Array2;
use serde_json::Value;
use water_hydro::lake_flatten::flatten_lake_polygons_on_surface;

fn load() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stage5c_cases.json");
    serde_json::from_str(&std::fs::read_to_string(&path).expect("读取夹具")).expect("解析夹具")
}

fn ring_from(coords: &[Value]) -> LineString<f64> {
    LineString(
        coords
            .iter()
            .map(|p| {
                let a = p.as_array().unwrap();
                Coord { x: a[0].as_f64().unwrap(), y: a[1].as_f64().unwrap() }
            })
            .collect(),
    )
}

fn poly_from(v: &Value) -> Polygon<f64> {
    let exterior = ring_from(v["exterior"].as_array().unwrap());
    let interiors = v["interiors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| ring_from(r.as_array().unwrap()))
        .collect();
    Polygon::new(exterior, interiors)
}

fn f32_grid(arr: &[Value], h: usize, w: usize) -> Array2<f32> {
    let mut g = Array2::<f32>::from_elem((h, w), f32::NAN);
    for i in 0..h * w {
        if let Some(v) = arr[i].as_f64() {
            g[(i / w, i % w)] = v as f32;
        }
    }
    g
}

#[test]
fn flatten_lake_matches_python() {
    let v = load();
    let h = v["h"].as_u64().unwrap() as usize;
    let w = v["w"].as_u64().unwrap() as usize;
    let t: Vec<f64> = v["transform"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    let transform: [f64; 6] = [t[0], t[1], t[2], t[3], t[4], t[5]];
    let dem = f32_grid(v["dem"].as_array().unwrap(), h, w);

    for case in v["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let polygons: Vec<Polygon<f64>> =
            case["polygons"].as_array().unwrap().iter().map(poly_from).collect();
        let fclass: Vec<Option<String>> = case["fclass"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().map(|s| s.to_string()))
            .collect();
        let component_map: Option<HashMap<usize, i64>> = case["component_map"].as_object().map(|m| {
            m.iter()
                .map(|(k, val)| (k.parse::<usize>().unwrap(), val.as_i64().unwrap()))
                .collect()
        });

        let mut surface = Array2::<f32>::from_elem((h, w), f32::NAN);
        let summary = flatten_lake_polygons_on_surface(
            &mut surface,
            &transform,
            &polygons,
            &fclass,
            &dem,
            true,
            component_map.as_ref(),
        );

        // 1) 压平后 surface 逐像素一致
        let exp_surface = case["surface_after"].as_array().unwrap();
        for i in 0..h * w {
            let (r, c) = (i / w, i % w);
            let g = surface[(r, c)];
            match exp_surface[i].as_f64() {
                Some(e) => assert!(
                    g.is_finite() && (g - e as f32).abs() == 0.0,
                    "{name} 像素({r},{c}) 期望 {e} 得 {g}"
                ),
                None => assert!(g.is_nan(), "{name} 像素({r},{c}) 期望 NaN 得 {g}"),
            }
        }

        // 2) summary 计数一致
        let s = &case["summary"];
        assert_eq!(summary.lake_polygon_count, s["lake_polygon_count"].as_u64().unwrap() as usize, "{name} lake_polygon_count");
        assert_eq!(summary.filled_polygon_count, s["filled_polygon_count"].as_u64().unwrap() as usize, "{name} filled_polygon_count");
        assert_eq!(summary.skipped_no_constant, s["skipped_no_constant"].as_u64().unwrap() as usize, "{name} skipped_no_constant");
        assert_eq!(summary.filled_pixel_count, s["filled_pixel_count"].as_u64().unwrap() as usize, "{name} filled_pixel_count");

        // 3) 逐多边形常数水位一致
        let exp_z = case["polygon_constant_z"].as_object().unwrap();
        assert_eq!(summary.polygon_constant_z.len(), exp_z.len(), "{name} polygon_constant_z 数量");
        for (k, val) in exp_z {
            let idx = k.parse::<usize>().unwrap();
            let got = summary.polygon_constant_z.get(&idx).unwrap_or_else(|| panic!("{name} 缺 polygon {idx}"));
            let diff = (got - val.as_f64().unwrap()).abs();
            assert!(diff < 1e-9, "{name} polygon {idx} z 误差 {diff:.3e}");
        }
        println!("flatten {name}: filled={} pixels={} ✓", summary.filled_polygon_count, summary.filled_pixel_count);
    }
}
