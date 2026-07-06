//! 全量林芝 GPU Laplace 计算（供 Nsight Systems 抓取）：把 `WATER_LAPLACE_DUMP_DIR`
//! 里转储的**所有真实林芝 Laplace 系统** `.bin` 逐个用 GPU 聚合多重网格 PCG 求解一遍。
//!
//! 用法（需 `--features gpu`）：
//!   `cargo run --release --features gpu -p water-hydro --example gpu_linzhi_full -- [dir]`
//! 或在 nsys 下：`nsys profile -o out.nsys-rep <上述二进制> [dir]`（dir 默认 /tmp/laplace_full）。
//!
//! `.bin` 格式见 `bench_laplace_real`（i64 h,w + u8 poly + u8 dmask + f64 dz，小端）。

use std::time::Instant;

use ndarray::Array2;

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
    #[cfg(not(feature = "gpu"))]
    {
        eprintln!("需以 --features gpu 构建运行");
    }
    #[cfg(feature = "gpu")]
    {
        use water_hydro::laplace::build_pcg_compact;

        let dir = std::env::args().nth(1).unwrap_or_else(|| "/tmp/laplace_full".to_string());
        let mut files: Vec<String> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("读取目录 {dir} 失败: {e}"))
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().map(|x| x == "bin").unwrap_or(false))
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        files.sort();
        assert!(!files.is_empty(), "目录 {dir} 内无 .bin 系统");
        println!("全量林芝 GPU Laplace：{} 个真实系统，目录 {dir}", files.len());

        // GPU 预热（context/分配器初始化不计入统计代表值；nsys 仍会记录）。
        {
            let (poly, dmask, dz) = load(&files[0]);
            let (diag, nbr, b, int_rc) = build_pcg_compact(&poly, &dmask, &dz);
            let rows: Vec<i32> = int_rc.iter().map(|&(r, _)| r as i32).collect();
            let cols: Vec<i32> = int_rc.iter().map(|&(_, c)| c as i32).collect();
            let _ = water_gpu::mg::laplace_pcg_mg(&diag, &nbr, &b, &rows, &cols, 1e-13, 5000, 2, 2, 40, 0.8);
        }

        let t_all = Instant::now();
        let mut total_iters = 0i64;
        let mut n_conv = 0usize;
        for (k, f) in files.iter().enumerate() {
            let (poly, dmask, dz) = load(f);
            let (diag, nbr, b, int_rc) = build_pcg_compact(&poly, &dmask, &dz);
            let n = int_rc.len();
            if n == 0 {
                continue;
            }
            let rows: Vec<i32> = int_rc.iter().map(|&(r, _)| r as i32).collect();
            let cols: Vec<i32> = int_rc.iter().map(|&(_, c)| c as i32).collect();
            let t = Instant::now();
            let res = water_gpu::mg::laplace_pcg_mg(&diag, &nbr, &b, &rows, &cols, 1e-13, 5000, 2, 2, 40, 0.8)
                .expect("GPU MG-PCG 失败");
            let wall = t.elapsed().as_secs_f64() * 1e3;
            let name = std::path::Path::new(f).file_name().unwrap().to_string_lossy();
            let conv = res.residual.is_finite() && res.residual <= 1e-13;
            if conv {
                n_conv += 1;
            }
            total_iters += res.iters as i64;
            println!(
                "[{:>2}/{}] {name}: n={n} iters={} res={:.1e} kernel={:.1}ms wall={wall:.1}ms conv={conv}",
                k + 1,
                files.len(),
                res.iters,
                res.residual,
                res.timing.kernel_ms
            );
        }
        println!(
            "完成：{} 系统，收敛 {n_conv}，总迭代 {total_iters}，总墙钟 {:.2}s",
            files.len(),
            t_all.elapsed().as_secs_f64()
        );
    }
}
