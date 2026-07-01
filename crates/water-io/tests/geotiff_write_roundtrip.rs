//! GeoTIFF 写入器圆环验证：Rust 写出 → eci-gdal 读回，核对几何/CRS/像素值。

use std::path::PathBuf;

use ndarray::Array2;
use water_io::geotiff_write::write_geotiff_f32;
use water_io::raster::Dem;

fn tmp(name: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("w2r_geotiff_{name}.tif"));
    p
}

fn roundtrip(name: &str, transform: [f64; 6], epsg: u16, is_geographic: bool) {
    let (h, w) = (6usize, 8usize);
    let mut data = Array2::<f32>::zeros((h, w));
    for r in 0..h {
        for c in 0..w {
            data[(r, c)] = 100.0 + c as f32 * 1.5 + r as f32 * 0.7;
        }
    }
    let path = tmp(name);
    write_geotiff_f32(&path, &data, transform, epsg, is_geographic, Some(f64::NAN)).expect("写出");

    let dem = Dem::open(&path).expect("读回");
    let m = dem.meta();
    assert_eq!(m.width, w as u32, "{name} 宽");
    assert_eq!(m.height, h as u32, "{name} 高");
    assert_eq!(m.crs_epsg, Some(epsg as u32), "{name} EPSG");

    // bounds（角约定）。
    let (ox, oy) = (transform[2], transform[5]);
    let (a, e) = (transform[0], transform[4]);
    assert!((m.min_x - ox).abs() < 1e-6, "{name} min_x");
    assert!((m.max_x - (ox + w as f64 * a)).abs() < 1e-6, "{name} max_x");
    assert!((m.max_y - oy).abs() < 1e-6, "{name} max_y");
    assert!((m.min_y - (oy + h as f64 * e)).abs() < 1e-6, "{name} min_y");

    // 值保真验证：data 为线性斜坡，线性场的双线性插值恒等于解析值。
    // 在栅格内部（避开边界节点的浮点越界）采样，核对读端几何映射 + 值。
    // 读端约定：整数像素索引置于像素左上角，故 geo→索引 fx=(x-originX)/a、fy=(y-originY)/e。
    let mut worst = 0.0f32;
    for r in 0..(h - 1) {
        for c in 0..(w - 1) {
            let fx = c as f64 + 0.5;
            let fy = r as f64 + 0.5;
            let x = a * fx + transform[2];
            let y = e * fy + transform[5];
            let v = dem.sample_model_bilinear(x, y).expect("采样");
            let expected = 100.0 + fx as f32 * 1.5 + fy as f32 * 0.7;
            worst = worst.max((v - expected).abs());
        }
    }
    println!("geotiff {name}: EPSG {epsg} 像素最大误差={worst:.3e}");
    assert!(worst < 1e-2, "{name} 像素值误差 {worst:.3e}");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn write_read_utm() {
    roundtrip("utm49", [30.0, 0.0, 500000.0, 0.0, -30.0, 2500000.0], 32649, false);
}

#[test]
fn write_read_wgs84() {
    roundtrip("wgs84", [0.001, 0.0, 113.9, 0.0, -0.001, 22.6], 4326, true);
}
