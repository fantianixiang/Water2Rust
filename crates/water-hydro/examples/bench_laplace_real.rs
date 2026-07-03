//! 真实地形 Laplace 系统三方对比之 Rust 侧：加载转储的真实系统 `.bin`，
//! 端到端计时 CPU faer（`solve_laplace_dirichlet_cpu`）与 GPU PCG
//! （`solve_laplace_dirichlet_gpu`，需 `--features gpu`）。
//!
//! 用法：`cargo run --release [--features gpu] -p water-hydro --example bench_laplace_real -- <sys.bin>`
//! 二进制格式见 `water_hydro::laplace` 的转储钩子（i64 h,w + u8 poly + u8 dmask + f64 dz，小端）。

use std::time::Instant;

use ndarray::Array2;
use water_hydro::laplace::solve_laplace_dirichlet_cpu;

fn load(path: &str) -> (Array2<bool>, Array2<bool>, Array2<f64>) {
    let raw = std::fs::read(path).expect("读取 .bin");
    let mut off = 0usize;
    let take_i64 = |raw: &[u8], off: &mut usize| -> i64 {
        let v = i64::from_le_bytes(raw[*off..*off + 8].try_into().unwrap());
        *off += 8;
        v
    };
    let h = take_i64(&raw, &mut off) as usize;
    let w = take_i64(&raw, &mut off) as usize;
    let n = h * w;
    let mut poly = Array2::<bool>::from_elem((h, w), false);
    let mut dmask = Array2::<bool>::from_elem((h, w), false);
    let mut dz = Array2::<f64>::zeros((h, w));
    for i in 0..n {
        poly[(i / w, i % w)] = raw[off + i] != 0;
    }
    off += n;
    for i in 0..n {
        dmask[(i / w, i % w)] = raw[off + i] != 0;
    }
    off += n;
    for i in 0..n {
        let b = raw[off + i * 8..off + i * 8 + 8].try_into().unwrap();
        dz[(i / w, i % w)] = f64::from_le_bytes(b);
    }
    (poly, dmask, dz)
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("用法: bench_laplace_real <sys.bin>");
    let (poly, dmask, dz) = load(&path);
    let (h, w) = poly.dim();
    let n_int = (0..h * w)
        .filter(|&i| poly[(i / w, i % w)] && !dmask[(i / w, i % w)])
        .count();
    println!("系统 {path}: {h}x{w}, n_int={n_int}");

    // CPU faer（端到端：装配 + 直接解），best-of-3。
    let _ = solve_laplace_dirichlet_cpu(&poly, &dmask, &dz); // warmup
    let mut faer_ms = f64::INFINITY;
    for _ in 0..3 {
        let t = Instant::now();
        let _ = solve_laplace_dirichlet_cpu(&poly, &dmask, &dz);
        faer_ms = faer_ms.min(t.elapsed().as_secs_f64() * 1e3);
    }
    println!("faer_ms={faer_ms:.2}");

    #[cfg(feature = "gpu")]
    {
        use water_hydro::laplace::solve_laplace_dirichlet_gpu;
        let _ = solve_laplace_dirichlet_gpu(&poly, &dmask, &dz); // warmup
        let mut gpu_ms = f64::INFINITY;
        let mut converged = false;
        for _ in 0..3 {
            let t = Instant::now();
            let r = solve_laplace_dirichlet_gpu(&poly, &dmask, &dz);
            gpu_ms = gpu_ms.min(t.elapsed().as_secs_f64() * 1e3);
            converged = r.is_some();
        }
        println!("gpu_pcg_ms={gpu_ms:.2} converged={converged}");
    }
}
