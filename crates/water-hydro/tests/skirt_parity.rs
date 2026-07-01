//! water_surface_skirt 对拍 scipy：合成用例逐像素核对裙边水面与外扩掩膜。

use ndarray::Array2;
use water_hydro::skirt::apply_water_surface_skirt;

fn parse_grid<'a>(lines: &mut impl Iterator<Item = &'a str>, h: usize) -> Vec<Vec<String>> {
    (0..h)
        .map(|_| {
            lines
                .next()
                .unwrap()
                .split_whitespace()
                .map(|s| s.to_string())
                .collect()
        })
        .collect()
}

#[test]
fn skirt_matches_scipy() {
    let fx = std::fs::read_to_string("tests/fixtures/skirt_ref.txt").expect("fixture");
    let mut lines = fx.lines();
    let hdr: Vec<i64> = lines
        .next()
        .unwrap()
        .split_whitespace()
        .map(|s| s.parse().unwrap())
        .collect();
    let (h, w, n) = (hdr[0] as usize, hdr[1] as usize, hdr[2] as usize);
    let (flat_ref, ramp_ref) = (hdr[3] as usize, hdr[4] as usize);

    let dem_s = parse_grid(&mut lines, h);
    let surf_in_s = parse_grid(&mut lines, h);
    let mask_in_s = parse_grid(&mut lines, h);
    let surf_out_s = parse_grid(&mut lines, h);
    let mask_out_s = parse_grid(&mut lines, h);

    let mut dem = Array2::<f32>::zeros((h, w));
    let mut surface = Array2::<f32>::from_elem((h, w), f32::NAN);
    let mut mask = Array2::<bool>::from_elem((h, w), false);
    for r in 0..h {
        for c in 0..w {
            dem[(r, c)] = dem_s[r][c].parse().unwrap();
            if surf_in_s[r][c] != "nan" {
                surface[(r, c)] = surf_in_s[r][c].parse().unwrap();
            }
            mask[(r, c)] = mask_in_s[r][c] == "1";
        }
    }

    let (flat_added, ramp_added) = apply_water_surface_skirt(&mut surface, &mut mask, &dem, n);
    assert_eq!(flat_added, flat_ref, "flat_added");
    assert_eq!(ramp_added, ramp_ref, "ramp_added");

    let mut worst = 0.0f32;
    let mut mask_mismatch = 0usize;
    for r in 0..h {
        for c in 0..w {
            let want_mask = mask_out_s[r][c] == "1";
            if mask[(r, c)] != want_mask {
                mask_mismatch += 1;
            }
            let got = surface[(r, c)];
            let want_nan = surf_out_s[r][c] == "nan";
            if want_nan {
                assert!(got.is_nan(), "want nan at ({r},{c}) got {got}");
            } else {
                let want: f32 = surf_out_s[r][c].parse().unwrap();
                worst = worst.max((got - want).abs());
            }
        }
    }
    println!("skirt 对拍: 最大误差={worst:.3e} mask_mismatch={mask_mismatch}");
    assert_eq!(mask_mismatch, 0, "输出掩膜不一致");
    assert!(worst < 1e-4, "水面误差 {worst:.3e}");
}
