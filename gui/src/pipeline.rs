//! 后台流水线：直接链接业务 crate 执行 fclass / edge / hydro。
//!
//! 语义参照 MyProject water 页：**fclass 是 edge / hydro 的前置条件，自动注入**；
//! 各任务产物按 `<输出名>_fclass.shp` / `_edge.shp` / `_hydro.tif` 落盘。

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use water_edge_depth::EdgeDepthOptions;
use water_fclass::FclassOptions;
use water_hydro::OutputMode;

use crate::log::GuiEvent;

/// 一次运行的全部参数（由 UI 收集）。
#[derive(Clone)]
pub struct PipelineParams {
    pub water_path: PathBuf,
    pub output_path: PathBuf,
    pub dem_path: Option<PathBuf>,
    pub reference_path: PathBuf,
    pub do_fclass: bool,
    pub do_edge: bool,
    pub do_hydro: bool,
    pub hydro_with_dem: bool,
}

/// 依输出基名派生某任务的产物路径：`<dir>/<stem>_<suffix>.<ext>`。
fn derive_path(base: &Path, suffix: &str, ext: &str) -> PathBuf {
    let dir = base.parent().unwrap_or_else(|| Path::new("."));
    let stem = base
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "water".to_string());
    dir.join(format!("{stem}_{suffix}.{ext}"))
}

/// 在后台线程中执行流水线；进度经 `tracing` 汇入 GUI 日志，终态经 `tx` 回传。
pub fn run(params: PipelineParams, tx: Sender<GuiEvent>) {
    match execute(&params, &tx) {
        Ok(outputs) => {
            let _ = tx.send(GuiEvent::Done(outputs));
        }
        Err(err) => {
            tracing::error!("流水线失败：{err:#}");
            let _ = tx.send(GuiEvent::Failed(format!("{err:#}")));
        }
    }
}

fn execute(p: &PipelineParams, tx: &Sender<GuiEvent>) -> anyhow::Result<Vec<(String, PathBuf)>> {
    let mut outputs: Vec<(String, PathBuf)> = Vec::new();

    // fclass 为前置：任一任务选中都需先产出已分类水体。
    let need_fclass = p.do_fclass || p.do_edge || p.do_hydro;

    let classified = if need_fclass {
        let _ = tx.send(GuiEvent::TaskStart("fclass".to_string()));
        let out = derive_path(&p.output_path, "fclass", "shp");
        tracing::info!("[1] fclass 分类：{} → {}", p.water_path.display(), out.display());
        let opts = FclassOptions {
            reference_path: p.reference_path.clone(),
            transition_only: false,
        };
        water_fclass::run_fclass(&p.water_path, &out, &opts)?;
        let shp = out.with_extension("shp");
        if p.do_fclass {
            outputs.push(("fclass".to_string(), shp.clone()));
        }
        let _ = tx.send(GuiEvent::TaskDone("fclass".to_string()));
        shp
    } else {
        p.water_path.clone()
    };

    if p.do_edge {
        let _ = tx.send(GuiEvent::TaskStart("edge".to_string()));
        let out = derive_path(&p.output_path, "edge", "shp");
        tracing::info!("[2] edge 水边深度：{} → {}", classified.display(), out.display());
        let opts = EdgeDepthOptions::default();
        water_edge_depth::export_water_edge_depth(&classified, &out, &opts)?;
        outputs.push(("edge".to_string(), out.with_extension("shp")));
        let _ = tx.send(GuiEvent::TaskDone("edge".to_string()));
    }

    if p.do_hydro {
        let _ = tx.send(GuiEvent::TaskStart("hydro".to_string()));
        let dem = p
            .dem_path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("hydro 需要 DEM 影像，请填写 DEM 路径"))?;
        let out = derive_path(&p.output_path, "hydro", "tif");
        let mode = if p.hydro_with_dem {
            OutputMode::WaterSurfaceWithDem
        } else {
            OutputMode::WaterSurfaceOnly
        };
        tracing::info!("[3] hydro 水面 DEM：{} + {} → {}", dem.display(), classified.display(), out.display());
        water_hydro::run(dem, &classified, &out, mode)?;
        outputs.push(("hydro".to_string(), out));
        let _ = tx.send(GuiEvent::TaskDone("hydro".to_string()));
    }

    tracing::info!("流水线完成，产物 {} 项", outputs.len());
    Ok(outputs)
}
