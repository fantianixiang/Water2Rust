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
        use rayon::prelude::*;
        use water_hydro::laplace::{build_pcg_compact, solve_laplace_dirichlet_gpu};

        // ── GPU MG-PCG 分步计时（真实系统）：装配 / 层次构建 / H2D / kernel / D2H / 写回 ──
        // warmup
        let (diag, nbr, b, int_rc) = build_pcg_compact(&poly, &dmask, &dz);
        let rows: Vec<i32> = int_rc.iter().map(|&(r, _)| r as i32).collect();
        let cols: Vec<i32> = int_rc.iter().map(|&(_, c)| c as i32).collect();
        let _ = water_gpu::mg::laplace_pcg_mg(&diag, &nbr, &b, &rows, &cols, 1e-13, 5000, 2, 2, 40, 0.8);

        let mut best = (f64::MAX, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0i32); // total,asm,hier,h2d,ker,d2h,wb,iters
        for _ in 0..3 {
            let t = Instant::now();
            let (diag, nbr, b, int_rc) = build_pcg_compact(&poly, &dmask, &dz);
            let asm = t.elapsed().as_secs_f64() * 1e3;
            let rows: Vec<i32> = int_rc.iter().map(|&(r, _)| r as i32).collect();
            let cols: Vec<i32> = int_rc.iter().map(|&(_, c)| c as i32).collect();

            let t = Instant::now();
            let mg = water_gpu::mg::laplace_pcg_mg(&diag, &nbr, &b, &rows, &cols, 1e-13, 5000, 2, 2, 40, 0.8)
                .expect("MG-PCG");
            let mg_wall = t.elapsed().as_secs_f64() * 1e3;
            let hier = (mg_wall - mg.timing.h2d_ms - mg.timing.kernel_ms - mg.timing.d2h_ms).max(0.0);

            // 写回（与 solve_laplace_dirichlet_gpu 同实现）计时。
            let t = Instant::now();
            let mut flat = vec![0.0f64; h * w];
            flat.par_chunks_mut(w).enumerate().for_each(|(r, row)| {
                for (c, cell) in row.iter_mut().enumerate() {
                    *cell = if dmask[(r, c)] { dz[(r, c)] } else { f64::NAN };
                }
            });
            for (i, &(r, c)) in int_rc.iter().enumerate() {
                flat[r * w + c] = mg.z[i];
            }
            let wb = t.elapsed().as_secs_f64() * 1e3;

            let total = asm + mg_wall + wb;
            if total < best.0 {
                best = (total, asm, hier, mg.timing.h2d_ms, mg.timing.kernel_ms, mg.timing.d2h_ms, wb, mg.iters);
            }
        }
        let (total, asm, hier, h2d, ker, d2h, wb, iters) = best;

        // ── 理想紧凑矩阵（同 n 的满填充方形网格，bbox 填充率 100%）作对照 ──
        let m = (n_int as f64).sqrt().round() as usize;
        let (id, inb, ib, ir, ic) = build_synth_grid(m);
        let _ = water_gpu::mg::laplace_pcg_mg(&id, &inb, &ib, &ir, &ic, 1e-13, 5000, 2, 2, 40, 0.8);
        let mut ideal = (f64::MAX, 0.0, 0i32); // wall,kernel,iters
        for _ in 0..3 {
            let t = Instant::now();
            let r = water_gpu::mg::laplace_pcg_mg(&id, &inb, &ib, &ir, &ic, 1e-13, 5000, 2, 2, 40, 0.8)
                .expect("ideal MG");
            let wall = t.elapsed().as_secs_f64() * 1e3;
            if wall < ideal.0 {
                ideal = (wall, r.timing.kernel_ms, r.iters);
            }
        }

        // 验证收敛。
        let _ = solve_laplace_dirichlet_gpu(&poly, &dmask, &dz).expect("GPU 应收敛");

        println!(
            "  [MG 分步/ms] 装配={asm:.1} 层次={hier:.1} H2D={h2d:.1} kernel={ker:.1} D2H={d2h:.1} 写回={wb:.1} | 合计={total:.1} 迭代={iters}"
        );
        println!(
            "  [总加速] faer/GPU = {:.2}×（仅 kernel {:.2}×）",
            faer_ms / total,
            faer_ms / ker
        );
        println!(
            "  [理想方网 n≈{}²] kernel={:.1}ms 迭代={} e2e={:.1}ms | 真实/理想 kernel={:.2}× 迭代={:.2}×",
            m, ideal.1, ideal.2, ideal.0, ker / ideal.1, iters as f64 / ideal.2 as f64
        );
        println!(
            "RESULT n={n_int} faer={faer_ms:.2} asm={asm:.2} hier={hier:.2} h2d={h2d:.2} ker={ker:.2} d2h={d2h:.2} wb={wb:.2} total={total:.2} iters={iters} ideal_ker={:.2} ideal_iters={} ideal_total={:.2}",
            ideal.1, ideal.2, ideal.0
        );
    }
}

/// 理想紧凑对照：m×m 满填充方形网格（bbox 填充率 100%），5 点 Poisson-Dirichlet 紧凑系统。
#[cfg(feature = "gpu")]
fn build_synth_grid(m: usize) -> (Vec<f64>, Vec<i32>, Vec<f64>, Vec<i32>, Vec<i32>) {
    let n = m * m;
    let diag = vec![4.0f64; n];
    let b: Vec<f64> = (0..n).map(|i| ((i * 7 + 3) % 13) as f64 - 6.0).collect();
    let mut nbr = vec![-1i32; n * 4];
    let mut rows = vec![0i32; n];
    let mut cols = vec![0i32; n];
    for r in 0..m {
        for c in 0..m {
            let i = r * m + c;
            rows[i] = r as i32;
            cols[i] = c as i32;
            if r > 0 {
                nbr[i * 4] = ((r - 1) * m + c) as i32;
            }
            if r + 1 < m {
                nbr[i * 4 + 1] = ((r + 1) * m + c) as i32;
            }
            if c > 0 {
                nbr[i * 4 + 2] = (r * m + c - 1) as i32;
            }
            if c + 1 < m {
                nbr[i * 4 + 3] = (r * m + c + 1) as i32;
            }
        }
    }
    (diag, nbr, b, rows, cols)
}
