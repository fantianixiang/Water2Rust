//! water-hydro — 水面 DEM 生成（最大模块）。
//!
//! 对应原 Python `waters/hydro/`（`generate_hydro_water_dem`）。
//! **替换的 Python 库**：rasterio、scipy（ndimage / spatial）、skimage、numpy。
//!
//! 核心流程（与原 Python 对齐，逐步改造）：
//! 1. 读取 DEM 与水体域、构建掩膜（`hydro_io`）
//! 2. 骨架提取与分支拆分（`processing/skeleton`，替代 skimage.skeletonize）
//! 3. 锚点采样与图距离（`processing/graph`，替代 scipy.spatial.cKDTree → rstar）
//! 4. 纵剖面拟合与单调约束（`processing/profile`）
//! 5. 逐多边形 Laplace 求解水面（`hydro_laplace`）
//! 6. 湖泊压平、河床抬升、平滑（`hydro_lake_flatten` / `hydro_raster_postprocess`）
//! 7. 重投影回源网格并写出（`hydro_raster_io`）
//!
//! 当前为骨架占位。

use std::path::Path;
use water_core::{error::WaterError, settings::HydroSettings, Result};

pub mod laplace;

/// 水面 DEM 输出组合模式。对应 `WATER_OUTPUT_MODES`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// 仅写求解的水体像素，其余为 nodata。
    WaterSurfaceOnly,
    /// 非水像素与未求解空洞用 DEM 回填。
    WaterSurfaceWithDem,
}

/// 水面 DEM 生成参数。
#[derive(Debug, Clone)]
pub struct HydroJob {
    pub dem_path: std::path::PathBuf,
    pub water_path: std::path::PathBuf,
    pub output_path: std::path::PathBuf,
    pub output_mode: OutputMode,
    pub all_touched: bool,
    pub settings: HydroSettings,
    pub debug: bool,
}

/// 生成水面 DEM（占位）。对应 `generate_hydro_water_dem`。
pub fn generate_hydro_water_dem(_job: &HydroJob) -> Result<()> {
    Err(WaterError::NotImplemented("water_hydro::generate_hydro_water_dem"))
}

/// 便捷入口：用默认设置生成。
pub fn run(dem: &Path, water: &Path, output: &Path, mode: OutputMode) -> Result<()> {
    let job = HydroJob {
        dem_path: dem.to_path_buf(),
        water_path: water.to_path_buf(),
        output_path: output.to_path_buf(),
        output_mode: mode,
        all_touched: true,
        settings: HydroSettings::default(),
        debug: false,
    };
    generate_hydro_water_dem(&job)
}
