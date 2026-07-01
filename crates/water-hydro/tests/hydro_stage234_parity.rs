//! hydro 阶段 2/3/4 数值对拍：湖泊常数水位内核、河床抬升、输出组合。
//!
//! 夹具 `tests/fixtures/hydro_stage234_cases.json` 由
//! `scripts/parity/gen_hydro_stage234_fixtures.py` 调用真实 Python 函数生成。

use std::path::PathBuf;

use ndarray::Array2;
use serde_json::Value;
use water_hydro::lake::iterative_trimmed_median;
use water_hydro::output::compose_water_output_array;
use water_hydro::postprocess::apply_river_dem_floor_lift;
use water_hydro::OutputMode;

fn load() -> Value {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hydro_stage234_cases.json");
    serde_json::from_str(&std::fs::read_to_string(&path).expect("读取夹具")).expect("解析夹具")
}

/// (f64|null) 数组 → Array2<f32>，null → NaN。
fn to_f32_grid(arr: &[Value], h: usize, w: usize) -> Array2<f32> {
    let mut g = Array2::<f32>::from_elem((h, w), f32::NAN);
    for i in 0..h * w {
        if let Some(v) = arr[i].as_f64() {
            g[(i / w, i % w)] = v as f32;
        }
    }
    g
}

fn to_bool_grid(arr: &[Value], h: usize, w: usize) -> Array2<bool> {
    let mut g = Array2::<bool>::from_elem((h, w), false);
    for i in 0..h * w {
        g[(i / w, i % w)] = arr[i].as_u64().unwrap() != 0;
    }
    g
}

/// f32 网格逐像素比对：有限值须相等，NaN 须对齐。
fn assert_grid_eq(name: &str, got: &Array2<f32>, exp: &[Value], h: usize, w: usize) {
    for i in 0..h * w {
        let (r, c) = (i / w, i % w);
        let g = got[(r, c)];
        match exp[i].as_f64() {
            Some(e) => assert!(
                g.is_finite() && (g - e as f32).abs() == 0.0,
                "{name} 像素({r},{c}) 期望 {e} 得 {g}"
            ),
            None => assert!(g.is_nan(), "{name} 像素({r},{c}) 期望 NaN 得 {g}"),
        }
    }
}

#[test]
fn trimmed_median_matches_python() {
    let v = load();
    let mut worst = 0.0f64;
    for case in v["trimmed_median"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let values: Vec<f64> = case["values"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_f64().unwrap_or(f64::NAN))
            .collect();
        let max_iter = case["max_iter"].as_u64().unwrap() as usize;
        let got = iterative_trimmed_median(&values, max_iter);
        match case["expected"].as_f64() {
            Some(expected) => {
                let diff = (got - expected).abs();
                println!("trimmed_median {name}: got={got} exp={expected} diff={diff:.3e}");
                assert!(diff < 1e-9, "{name} 误差 {diff:.3e} 超限");
                worst = worst.max(diff);
            }
            None => {
                println!("trimmed_median {name}: expected NaN, got {got}");
                assert!(got.is_nan(), "{name} 期望 NaN 得 {got}");
            }
        }
    }
    println!("trimmed_median 总体最大误差 = {worst:.3e}");
}

#[test]
fn floor_lift_matches_python() {
    let v = load();
    for case in v["floor_lift"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let h = case["h"].as_u64().unwrap() as usize;
        let w = case["w"].as_u64().unwrap() as usize;
        let dem = to_f32_grid(case["dem"].as_array().unwrap(), h, w);
        let mut surface = to_f32_grid(case["surface"].as_array().unwrap(), h, w);
        let write = to_bool_grid(case["write"].as_array().unwrap(), h, w);
        let river = to_bool_grid(case["river"].as_array().unwrap(), h, w);
        let lake = to_bool_grid(case["lake"].as_array().unwrap(), h, w);

        let (n_lifted, n_lake_excluded, n_overbank) =
            apply_river_dem_floor_lift(&mut surface, &write, &dem, &river, &lake);

        let counts = case["counts"].as_array().unwrap();
        assert_eq!(n_lifted, counts[0].as_u64().unwrap() as usize, "{name} n_lifted");
        assert_eq!(
            n_lake_excluded,
            counts[1].as_u64().unwrap() as usize,
            "{name} n_lake_excluded"
        );
        assert_eq!(n_overbank, counts[2].as_u64().unwrap() as usize, "{name} n_overbank");
        assert_grid_eq(name, &surface, case["after"].as_array().unwrap(), h, w);
        println!("floor_lift {name}: counts=({n_lifted},{n_lake_excluded},{n_overbank}) ✓");
    }
}

#[test]
fn compose_matches_python() {
    let v = load();
    for case in v["compose"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let h = case["h"].as_u64().unwrap() as usize;
        let w = case["w"].as_u64().unwrap() as usize;
        let dem = to_f32_grid(case["dem"].as_array().unwrap(), h, w);
        let surface = to_f32_grid(case["surface"].as_array().unwrap(), h, w);
        let mask = to_bool_grid(case["mask"].as_array().unwrap(), h, w);
        let mode = match case["mode"].as_str().unwrap() {
            "water_surface_with_dem" => OutputMode::WaterSurfaceWithDem,
            "water_surface_only" => OutputMode::WaterSurfaceOnly,
            other => panic!("未知模式 {other}"),
        };

        let (output, metrics) = compose_water_output_array(&dem, &surface, &mask, mode);
        assert_grid_eq(name, &output, case["output"].as_array().unwrap(), h, w);

        let m = &case["metrics"];
        assert_eq!(
            metrics.surface_write_pixels,
            m["surface_write_pixels"].as_u64().unwrap() as usize,
            "{name} surface_write_pixels"
        );
        assert_eq!(
            metrics.water_dem_fill_pixels,
            m["water_dem_fill_pixels"].as_u64().unwrap() as usize,
            "{name} water_dem_fill_pixels"
        );
        assert_eq!(
            metrics.background_dem_pixels,
            m["background_dem_pixels"].as_u64().unwrap() as usize,
            "{name} background_dem_pixels"
        );
        assert_eq!(
            metrics.water_remaining_nodata_pixels,
            m["water_remaining_nodata_pixels"].as_u64().unwrap() as usize,
            "{name} water_remaining_nodata_pixels"
        );
        assert_eq!(
            metrics.remaining_nodata_pixels,
            m["remaining_nodata_pixels"].as_u64().unwrap() as usize,
            "{name} remaining_nodata_pixels"
        );
        println!("compose {name}: ✓");
    }
}
