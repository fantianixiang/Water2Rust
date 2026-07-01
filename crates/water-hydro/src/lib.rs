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
pub mod lake;
pub mod lake_flatten;
pub mod output;
pub mod postprocess;
pub mod cross_section;
pub mod crs;
pub mod skeleton_graph;
pub mod skeleton_zloc;
pub mod river_zsmooth;
pub mod river_solve;
pub mod river_pipeline;
pub mod skirt;

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

/// 校验 hydro 输入齐全性：**强制要求 DEM 与已分类(fclass)水体同时具备**。
///
/// hydro 需要的是**已完成 fclass 分类**的水体（河流/湖泊等语义决定不同物理处理），
/// 因此拒绝缺少 `fclass` 字段的原始水体输入——应先运行 fclass 流程。
pub fn validate_hydro_inputs(dem: &Path, water: &Path) -> Result<()> {
    if !dem.exists() {
        return Err(WaterError::InvalidInput(format!(
            "DEM 输入不存在: {}",
            dem.display()
        )));
    }
    if !water.exists() {
        return Err(WaterError::InvalidInput(format!(
            "水体输入不存在: {}",
            water.display()
        )));
    }
    let fc = water_io::vector::read_vector(water)?;
    if fc.features.is_empty() {
        return Err(WaterError::InvalidInput(format!(
            "水体输入无任何要素: {}",
            water.display()
        )));
    }
    let has_fclass = fc
        .features
        .iter()
        .any(|f| f.properties.keys().any(|k| k.eq_ignore_ascii_case("fclass")));
    if !has_fclass {
        return Err(WaterError::InvalidInput(format!(
            "水体输入缺少 fclass 字段: {}。hydro 要求已分类的水体，请先运行 fclass 流程为水体赋予 fclass。",
            water.display()
        )));
    }
    Ok(())
}

/// 生成水面 DEM（占位）。对应 `generate_hydro_water_dem`。
pub fn generate_hydro_water_dem(job: &HydroJob) -> Result<()> {
    // 输入齐全性检查：强制 DEM + 已分类(fclass)水体
    validate_hydro_inputs(&job.dem_path, &job.water_path)?;
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
