//! read_window_f32 对拍 rasterio：真实林芝 DEM 窗口原始值 + 窗口变换一致。
//!
//! 参照 fixture 由 rasterio 生成（见 fixtures/linzhi_window.txt）：首行 `c0 r0 w h`，
//! 次行窗口 Affine 6 元，其后 h 行 × w 列原始高程。
//! 需本机存在 data/linzhi/dem.tif（gitignored，1.9GB）；缺失则跳过。

use std::path::Path;

use water_io::raster::Dem;

#[test]
fn read_window_matches_rasterio() {
    let dem_path = Path::new("../../data/linzhi/dem.tif");
    if !dem_path.exists() {
        eprintln!("跳过：未找到 {dem_path:?}");
        return;
    }
    let fx = std::fs::read_to_string("tests/fixtures/linzhi_window.txt").expect("fixture");
    let mut lines = fx.lines();
    let hdr: Vec<u32> = lines
        .next()
        .unwrap()
        .split_whitespace()
        .map(|s| s.parse().unwrap())
        .collect();
    let (c0, r0, w, h) = (hdr[0], hdr[1], hdr[2], hdr[3]);
    let t_ref: Vec<f64> = lines
        .next()
        .unwrap()
        .split_whitespace()
        .map(|s| s.parse().unwrap())
        .collect();
    let vals_ref: Vec<Vec<f32>> = lines
        .map(|l| l.split_whitespace().map(|s| s.parse().unwrap()).collect())
        .collect();

    let dem = Dem::open(dem_path).expect("open dem");
    let (win, transform) = dem.read_window_f32(c0, r0, w, h);

    // 窗口变换一致（Affine 序）。
    for k in 0..6 {
        assert!(
            (transform[k] - t_ref[k]).abs() < 1e-9,
            "transform[{k}] {} vs {}",
            transform[k],
            t_ref[k]
        );
    }

    // 原始像素值逐点一致（float32 存储，应精确）。
    let mut worst = 0.0f32;
    for j in 0..h as usize {
        for i in 0..w as usize {
            let a = win[(j, i)];
            let b = vals_ref[j][i];
            let d = if a.is_nan() && b == 0.0 { 0.0 } else { (a - b).abs() };
            worst = worst.max(d);
        }
    }
    println!("read_window 林芝 12x16: 最大误差={worst:.3e}");
    assert!(worst < 1e-3, "像素值误差 {worst:.3e}");
}
