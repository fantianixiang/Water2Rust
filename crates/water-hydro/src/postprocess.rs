//! 栅格阶段后处理：河床抬升与漫滩分类。
//!
//! 忠实复刻 Python `hydro/hydro_raster_postprocess.py::apply_river_dem_floor_lift`
//! 的**数组逻辑**。原函数内部先把河流/湖泊多边形栅格化为掩膜；栅格化本身留待阶段 5，
//! 此处以已栅格化的 `river_mask` / `lake_mask` 作为输入，专注复刻数组判定与抬升逻辑。
//!
//! 河流多边形内两类物理像素：
//! - **漫滩**（`dem > water_z`）：地形高于水面，排除抬升，保留 Laplace 上游水面。
//! - **河道内**（`dem <= water_z`）：河床在水下，若解低于 DEM 则夹回 DEM。
//!
//! 湖泊/水库遵循不同物理（坝设定的固定水位可低于岸壁 DEM），既不漫滩也不抬升。

use ndarray::Array2;

/// 就地修改 `output_surface`：河道内像素抬升到 ≥ 河床 DEM。
///
/// 返回 `(n_lifted, n_lake_excluded, n_overbank)`。
pub fn apply_river_dem_floor_lift(
    output_surface: &mut Array2<f32>,
    write_mask: &Array2<bool>,
    dem: &Array2<f32>,
    river_mask: &Array2<bool>,
    lake_mask: &Array2<bool>,
) -> (usize, usize, usize) {
    let (h, w) = output_surface.dim();
    let mut n_overbank = 0usize;
    let mut n_lifted = 0usize;
    let mut n_lake_excluded = 0usize;

    for r in 0..h {
        for c in 0..w {
            let d = dem[(r, c)];
            let s = output_surface[(r, c)];
            let finite_dem = d.is_finite();
            let finite_surface = s.is_finite();

            // 漫滩：河流多边形 ∩ 有限 DEM ∩ 有限水面 ∩ DEM > 水面
            let overbank = river_mask[(r, c)] && finite_dem && finite_surface && d > s;
            if overbank && write_mask[(r, c)] {
                n_overbank += 1;
            }

            // 河道内抬升：write ∩ 有限 DEM ∩ 水面 < DEM ∩ ~湖泊 ∩ ~漫滩
            let lift = write_mask[(r, c)]
                && finite_dem
                && s < d
                && !lake_mask[(r, c)]
                && !overbank;
            if lift {
                n_lifted += 1;
                output_surface[(r, c)] = d;
            }

            if lake_mask[(r, c)] && write_mask[(r, c)] {
                n_lake_excluded += 1;
            }
        }
    }

    (n_lifted, n_lake_excluded, n_overbank)
}
