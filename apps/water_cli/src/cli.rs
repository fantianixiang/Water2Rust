//! Water2Rust CLI 参数定义（clap）。
//!
//! 子命令对应原 Python `waters/__main__.py` 的处理阶段：
//! - `hydro`      → 生成水面 DEM（`generate_hydro_water_dem`）
//! - `fclass`     → 水体 fclass 分类（`--fclass-only`）
//! - `edge-depth` → 水边深度导出（`--edge-depth-only`）

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

/// Water2Rust —— 纯 Rust 水体处理工具链。
#[derive(Debug, Parser)]
#[command(name = "water2rust", version, about = "纯 Rust 水体处理工具链（水面 DEM / fclass / 水边深度）")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// 生成水面 DEM。
    Hydro(HydroArgs),
    /// 水体 fclass 语义分类。
    Fclass(FclassArgs),
    /// 水边深度导出。
    EdgeDepth(EdgeDepthArgs),
}

/// 输出组合模式（对应 `--output-mode`）。
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OutputMode {
    /// 仅写求解的水体像素。
    WaterSurfaceOnly,
    /// 非水像素与未求解空洞用 DEM 回填。
    WaterSurfaceWithDem,
}

#[derive(Debug, Parser)]
pub struct HydroArgs {
    /// 输入 DEM/DTM GeoTIFF。
    #[arg(long)]
    pub dem: PathBuf,
    /// 水体多边形（shapefile / GeoJSON / GeoPackage）。
    #[arg(long)]
    pub water: PathBuf,
    /// 输出 water_dem.tif。
    #[arg(long)]
    pub output: PathBuf,
    /// 输出组合模式。
    #[arg(long, value_enum, default_value_t = OutputMode::WaterSurfaceWithDem)]
    pub output_mode: OutputMode,
    /// 栅格化时 all_touched。
    #[arg(long, default_value_t = true)]
    pub all_touched: bool,
    /// 开启调试诊断。
    #[arg(long, default_value_t = false)]
    pub debug: bool,
}

#[derive(Debug, Parser)]
pub struct FclassArgs {
    /// 水体多边形输入。
    #[arg(long)]
    pub water: PathBuf,
    /// 输出矢量路径。
    #[arg(long)]
    pub output: PathBuf,
    /// 准备好的水体参考 GeoPackage。
    #[arg(long)]
    pub reference_path: PathBuf,
    /// 仅检测语义过渡位置。
    #[arg(long, default_value_t = false)]
    pub transition_only: bool,
}

#[derive(Debug, Parser)]
pub struct EdgeDepthArgs {
    /// 水体多边形输入。
    #[arg(long)]
    pub water: PathBuf,
    /// 输出 shapefile 路径。
    #[arg(long)]
    pub output: PathBuf,
    /// 栅格化时 all_touched。
    #[arg(long, default_value_t = true)]
    pub all_touched: bool,
}
