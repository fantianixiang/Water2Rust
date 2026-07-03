//! GPU matrix-free PCG 的**真实矩阵**验证（feature `gpu`）。
//!
//! 两层验证：
//! 1. `pcg_matches_python_fixtures`：用与 `laplace_parity` 同一份夹具
//!    （由 MyProject 真实 Python `solve_laplace_dirichlet` 生成的不规则域），
//!    GPU PCG 解 vs Python 期望，逐像素 < 1e-6。
//! 2. `pcg_matches_faer_large_irregular`：大不规则域（圆盘，内部变量 > GPU 阈值），
//!    GPU PCG（含分派路径）vs CPU faer 直接解 < 1e-6，并验证收敛稳定性。
#![cfg(feature = "gpu")]

use std::path::PathBuf;

use ndarray::Array2;
use serde_json::Value;
use water_hydro::laplace::{
    solve_laplace_dirichlet, solve_laplace_dirichlet_cpu, solve_laplace_dirichlet_gpu,
};

fn load_fixtures() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/laplace_cases.json");
    let text = std::fs::read_to_string(&path).expect("读取夹具文件");
    serde_json::from_str(&text).expect("解析夹具 JSON")
}

/// GPU PCG 在真实不规则域夹具上与 Python 期望一致。
#[test]
fn pcg_matches_python_fixtures() {
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

        // 直接走 GPU PCG 路径（夹具规模在阈值以下，故绕过分派显式调用）。
        let got = solve_laplace_dirichlet_gpu(&poly, &dmask, &dz)
            .unwrap_or_else(|| panic!("用例 {name}: GPU PCG 未收敛/失败"));

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
        eprintln!("[PCG fixture] {name}: max_abs(vs Python) = {max_diff:.3e}");
        assert!(max_diff < 1e-6, "用例 {name} PCG vs Python 超差: {max_diff:.3e}");
        overall = overall.max(max_diff);
    }
    eprintln!("[PCG fixture] 总体最大绝对误差 = {overall:.3e}（判据 < 1e-6）");
}

/// 大不规则域（圆盘）：GPU PCG（含分派）与 CPU faer 直接解一致，且收敛稳定。
#[test]
fn pcg_matches_faer_large_irregular() {
    let (h, w) = (720usize, 720usize);
    let (cx, cy, rad) = (360.0f64, 360.0f64, 340.0f64);
    let r2 = rad * rad;

    let mut poly = Array2::<bool>::from_elem((h, w), false);
    for r in 0..h {
        for c in 0..w {
            let dx = c as f64 - cx;
            let dy = r as f64 - cy;
            poly[(r, c)] = dx * dx + dy * dy <= r2;
        }
    }
    // Dirichlet = 圆盘边界像素（至少一个非 poly 邻居）；dz 用一条斜坡定值。
    let mut dmask = Array2::<bool>::from_elem((h, w), false);
    let mut dz = Array2::<f64>::zeros((h, w));
    for r in 0..h {
        for c in 0..w {
            if !poly[(r, c)] {
                continue;
            }
            let boundary = (r == 0 || !poly[(r - 1, c)])
                || (r + 1 >= h || !poly[(r + 1, c)])
                || (c == 0 || !poly[(r, c - 1)])
                || (c + 1 >= w || !poly[(r, c + 1)]);
            if boundary {
                dmask[(r, c)] = true;
                dz[(r, c)] = 800.0 + c as f64 * 0.5 + r as f64 * 0.3; // 类高程斜坡
            }
        }
    }
    let n_int: usize = (0..h * w)
        .filter(|&i| poly[(i / w, i % w)] && !dmask[(i / w, i % w)])
        .count();
    eprintln!("[PCG large] 圆盘域内部变量 n_int = {n_int}");
    assert!(n_int > 200_000, "构造的内部变量应超过 GPU 阈值，实为 {n_int}");

    // CPU faer 参考。
    let cpu = solve_laplace_dirichlet_cpu(&poly, &dmask, &dz);
    // 直接 GPU PCG。
    let gpu = solve_laplace_dirichlet_gpu(&poly, &dmask, &dz).expect("GPU PCG 应收敛");
    // 分派路径（n_int > 阈值 → 应走 GPU）。
    let dispatch = solve_laplace_dirichlet(&poly, &dmask, &dz);

    let mut max_gpu = 0.0f64;
    let mut max_disp = 0.0f64;
    for i in 0..h * w {
        let (r, c) = (i / w, i % w);
        let cv = cpu[(r, c)];
        if cv.is_finite() {
            assert!(gpu[(r, c)].is_finite(), "GPU 结果在内部/边界应有限");
            max_gpu = max_gpu.max((gpu[(r, c)] - cv).abs());
            max_disp = max_disp.max((dispatch[(r, c)] - cv).abs());
        } else {
            assert!(gpu[(r, c)].is_nan(), "GPU 结果在域外应为 NaN");
        }
    }
    eprintln!(
        "[PCG large] n_int={n_int} | max_abs(GPU vs faer)={max_gpu:.3e} \
         max_abs(分派 vs faer)={max_disp:.3e}（判据 < 1e-6）"
    );
    assert!(max_gpu < 1e-6, "大域 GPU vs faer 超差: {max_gpu:.3e}");
    assert!(max_disp < 1e-6, "大域 分派 vs faer 超差: {max_disp:.3e}");
}
