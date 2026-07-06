//! GPU warp 微基准：单个 ~8256² 瓦片的 WGS84→UTM warp，报告 H2D/kernel/D2H 分段耗时。
use water_gpu::{warp_reproject, GpuResampling, UtmParams};

fn main() {
    let (sh, sw) = (8256usize, 8256usize);
    let src: Vec<f32> = (0..sh * sw).map(|i| (i % 4000) as f32).collect();
    let src_t = [1e-4, 0.0, 94.0, 0.0, -1e-4, 30.0];
    let dst_t = [10.0, 0.0, 300_000.0, 0.0, -10.0, 3_320_000.0];
    let utm = UtmParams::from_epsg(32646).unwrap();
    let det = src_t[0] * src_t[4] - src_t[1] * src_t[3];
    let src_inv = [
        src_t[4] / det, -src_t[1] / det, (src_t[1] * src_t[5] - src_t[4] * src_t[2]) / det,
        -src_t[3] / det, src_t[0] / det, (src_t[3] * src_t[2] - src_t[0] * src_t[5]) / det,
    ];
    let _ = warp_reproject(&src, sh, sw, Some(-9999.0), dst_t, src_inv, sw, sh, true, &utm, GpuResampling::Bilinear).unwrap();
    let (mut h2d, mut ker, mut d2h) = (0.0, 0.0, 0.0);
    let reps = 5;
    for _ in 0..reps {
        let (_v, t) = warp_reproject(&src, sh, sw, Some(-9999.0), dst_t, src_inv, sw, sh, true, &utm, GpuResampling::Bilinear).unwrap();
        h2d += t.h2d_ms; ker += t.kernel_ms; d2h += t.d2h_ms;
    }
    println!("GPU warp {sh}x{sw} 均值({reps}次): H2D={:.1}ms kernel={:.1}ms D2H={:.1}ms 合计={:.1}ms",
        h2d/reps as f64, ker/reps as f64, d2h/reps as f64, (h2d+ker+d2h)/reps as f64);
}
