//! Laplace 求解器数值对拍测试。
//!
//! 夹具 `tests/fixtures/laplace_cases.json` 由 `scripts/parity/gen_laplace_fixtures.py`
//! 调用 MyProject 中**真实**的 Python `solve_laplace_dirichlet` 生成。
//! 本测试用 Rust 实现求解同样输入，逐像素比对，最大绝对误差须 < 1e-6。

use std::path::PathBuf;

use ndarray::Array2;
use serde_json::Value;
use water_hydro::laplace::solve_laplace_dirichlet;

fn load_fixtures() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/laplace_cases.json");
    let text = std::fs::read_to_string(&path).expect("读取夹具文件");
    serde_json::from_str(&text).expect("解析夹具 JSON")
}

#[test]
fn laplace_matches_python() {
    let v = load_fixtures();
    let cases = v["cases"].as_array().expect("cases 数组");
    let mut overall = 0.0f64;

    for case in cases {
        let name = case["name"].as_str().unwrap();
        let h = case["h"].as_u64().unwrap() as usize;
        let w = case["w"].as_u64().unwrap() as usize;
        let poly_flat = case["poly"].as_array().unwrap();
        let dmask_flat = case["dmask"].as_array().unwrap();
        let dz_flat = case["dz"].as_array().unwrap();
        let expected = case["expected"].as_array().unwrap();

        let mut poly = Array2::<bool>::from_elem((h, w), false);
        let mut dmask = Array2::<bool>::from_elem((h, w), false);
        let mut dz = Array2::<f64>::zeros((h, w));
        for i in 0..h * w {
            let (r, c) = (i / w, i % w);
            poly[(r, c)] = poly_flat[i].as_u64().unwrap() != 0;
            dmask[(r, c)] = dmask_flat[i].as_u64().unwrap() != 0;
            dz[(r, c)] = dz_flat[i].as_f64().unwrap();
        }

        let got = solve_laplace_dirichlet(&poly, &dmask, &dz);

        let mut max_diff = 0.0f64;
        for i in 0..h * w {
            let (r, c) = (i / w, i % w);
            match expected[i].as_f64() {
                Some(exp) => {
                    let g = got[(r, c)];
                    assert!(
                        g.is_finite(),
                        "用例 {name} 像素({r},{c}) 期望有限值 {exp} 但得 NaN"
                    );
                    max_diff = max_diff.max((g - exp).abs());
                }
                None => {
                    assert!(
                        got[(r, c)].is_nan(),
                        "用例 {name} 像素({r},{c}) 期望 NaN 但得 {}",
                        got[(r, c)]
                    );
                }
            }
        }
        println!("用例 {name}: 最大绝对误差 = {max_diff:.3e}");
        assert!(max_diff < 1e-6, "用例 {name} 误差 {max_diff:.3e} 超过 1e-6");
        overall = overall.max(max_diff);
    }
    println!("总体最大绝对误差 = {overall:.3e}");
}
