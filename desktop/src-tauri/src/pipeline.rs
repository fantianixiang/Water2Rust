//! water 业务流水线的 Tauri 封装：fclass / edge / hydro。
//!
//! 语义与 egui GUI 一致：**fclass 是 edge/hydro 的前置，自动注入**；各任务产物按
//! `<输出名>_fclass.shp` / `_edge.shp` / `_hydro.tif` 落盘。进度经 Tauri 事件回传前端。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use water_core::edge_depth::EdgeDepthConfig;
use water_edge_depth::EdgeDepthOptions;
use water_fclass::FclassOptions;
use water_hydro::OutputMode;

/// 单个 fclass 的 edge/depth 覆盖值（来自前端齿轮子窗口）。
#[derive(Debug, Clone, Deserialize)]
pub struct EdgeOverride {
    pub fclass: String,
    pub edge: f64,
    pub depth: f64,
}

/// 一次运行的全部参数（前端 camelCase → serde rename）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineParams {
    pub water_path: String,
    pub output_path: String,
    pub dem_path: Option<String>,
    pub reference_path: String,
    pub do_fclass: bool,
    pub do_edge: bool,
    pub do_hydro: bool,
    pub hydro_with_dem: bool,
    pub edge_overrides: Vec<EdgeOverride>,
}

/// 任务产物（task, 路径）。
#[derive(Debug, Clone, Serialize)]
pub struct TaskOutput {
    pub task: String,
    pub path: String,
}

/// 依输出基名派生产物路径：`<dir>/<stem>_<suffix>.<ext>`。
fn derive_path(base: &Path, suffix: &str, ext: &str) -> PathBuf {
    let dir = base.parent().unwrap_or_else(|| Path::new("."));
    let stem = base
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "water".to_string());
    dir.join(format!("{stem}_{suffix}.{ext}"))
}

/// 由覆盖值构建 EdgeDepthConfig（默认起，逐项 set）。
fn build_edge_config(overrides: &[EdgeOverride]) -> EdgeDepthConfig {
    let mut cfg = EdgeDepthConfig::default();
    for o in overrides {
        let _ = cfg.set(&o.fclass, o.edge, o.depth);
    }
    cfg
}

/// 运行流水线（异步命令：后台线程执行，事件回传进度/终态）。
#[tauri::command]
pub fn run_pipeline(app: AppHandle, params: PipelineParams) {
    std::thread::spawn(move || match execute(&app, &params) {
        Ok(outputs) => {
            let _ = app.emit("pipeline://done", outputs);
        }
        Err(err) => {
            tracing::error!("流水线失败：{err:#}");
            let _ = app.emit("pipeline://failed", format!("{err:#}"));
        }
    });
}

fn execute(app: &AppHandle, p: &PipelineParams) -> anyhow::Result<Vec<TaskOutput>> {
    if p.reference_path.trim().is_empty() {
        anyhow::bail!("请填写分类参考库路径");
    }
    let water = PathBuf::from(p.water_path.trim());
    let output = PathBuf::from(p.output_path.trim());
    let mut outputs: Vec<TaskOutput> = Vec::new();

    let start = |t: &str| { let _ = app.emit("pipeline://task-start", t.to_string()); };
    let done = |t: &str| { let _ = app.emit("pipeline://task-done", t.to_string()); };

    // fclass 为前置：任一任务选中都需先产出已分类水体。
    let need_fclass = p.do_fclass || p.do_edge || p.do_hydro;
    let classified = if need_fclass {
        start("fclass");
        let out = derive_path(&output, "fclass", "shp");
        tracing::info!("[1] fclass 分类：{} → {}", water.display(), out.display());
        let opts = FclassOptions {
            reference_path: PathBuf::from(p.reference_path.trim()),
            transition_only: false,
        };
        water_fclass::run_fclass(&water, &out, &opts)?;
        let shp = out.with_extension("shp");
        if p.do_fclass {
            outputs.push(TaskOutput { task: "fclass".into(), path: shp.display().to_string() });
        }
        done("fclass");
        shp
    } else {
        water.clone()
    };

    if p.do_edge {
        start("edge");
        let out = derive_path(&output, "edge", "shp");
        tracing::info!("[2] edge 水边深度：{} → {}", classified.display(), out.display());
        let opts = EdgeDepthOptions { config: build_edge_config(&p.edge_overrides) };
        water_edge_depth::export_water_edge_depth(&classified, &out, &opts)?;
        outputs.push(TaskOutput {
            task: "edge".into(),
            path: out.with_extension("shp").display().to_string(),
        });
        done("edge");
    }

    if p.do_hydro {
        start("hydro");
        let dem = p
            .dem_path
            .as_ref()
            .map(|s| PathBuf::from(s.trim()))
            .filter(|s| !s.as_os_str().is_empty())
            .ok_or_else(|| anyhow::anyhow!("hydro 需要 DEM 影像，请填写 DEM 路径"))?;
        let out = derive_path(&output, "hydro", "tif");
        let mode = if p.hydro_with_dem {
            OutputMode::WaterSurfaceWithDem
        } else {
            OutputMode::WaterSurfaceOnly
        };
        tracing::info!("[3] hydro 水面 DEM：{} + {} → {}", dem.display(), classified.display(), out.display());
        water_hydro::run(&dem, &classified, &out, mode)?;
        outputs.push(TaskOutput { task: "hydro".into(), path: out.display().to_string() });
        done("hydro");
    }

    tracing::info!("流水线完成，产物 {} 项", outputs.len());
    Ok(outputs)
}
