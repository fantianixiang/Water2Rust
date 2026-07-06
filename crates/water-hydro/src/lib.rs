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
pub mod pipeline;

/// 运行时 GPU 开关（仅 `--features gpu` 编译时有效）。默认开；可由 GUI/CLI 运行时切换。
/// 关闭则 GPU Laplace 与 GPU warp 全走 CPU。env `WATER_HYDRO_USE_GPU=0` 为硬性关闭。
#[cfg(feature = "gpu")]
static GPU_ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// 设置运行时是否使用 GPU（无 gpu 特性编译时为空操作，恒为 CPU）。
pub fn set_gpu_enabled(_on: bool) {
    #[cfg(feature = "gpu")]
    GPU_ENABLED.store(_on, std::sync::atomic::Ordering::Relaxed);
}

/// 运行时 GPU 是否启用：需 `--features gpu` 编译 + 运行时开关开 + 未被 env 硬关。
pub fn gpu_enabled() -> bool {
    #[cfg(feature = "gpu")]
    {
        if std::env::var("WATER_HYDRO_USE_GPU").map(|v| v == "0").unwrap_or(false) {
            return false;
        }
        GPU_ENABLED.load(std::sync::atomic::Ordering::Relaxed)
    }
    #[cfg(not(feature = "gpu"))]
    {
        false
    }
}

/// 是否编译进了 GPU 支持（`--features gpu`）。GUI 据此决定是否显示 GPU 选项。
pub fn gpu_compiled() -> bool {
    cfg!(feature = "gpu")
}

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

/// 生成水面 DEM。对应 `generate_hydro_water_dem`（非瓦片 / 无网络主路径）。
pub fn generate_hydro_water_dem(job: &HydroJob) -> Result<()> {
    // 输入齐全性检查：强制 DEM + 已分类(fclass)水体
    validate_hydro_inputs(&job.dem_path, &job.water_path)?;
    pipeline::run_hydro_pipeline(job)
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
