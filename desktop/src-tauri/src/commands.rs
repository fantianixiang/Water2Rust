//! 前端调用的辅助命令：edge/depth 调参规格、自检。

use serde::Serialize;
use water_core::edge_depth::edge_depth_guidance;

/// 单个 fclass 的 edge/depth 调参规格（默认值 + 推荐范围）。
///
/// 范围上界可能为无穷（sea）；JSON 无法表示 Infinity，故用 `Option<f64>`（`null` = ∞）。
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../src/lib/bindings/", rename_all = "camelCase")]
pub struct EdgeGuidanceItem {
    pub fclass: String,
    pub edge: f64,
    pub depth: f64,
    pub edge_min: f64,
    pub edge_max: Option<f64>,
    pub depth_min: f64,
    pub depth_max: Option<f64>,
}

fn finite_or_none(v: f64) -> Option<f64> {
    if v.is_finite() { Some(v) } else { None }
}

/// 返回 8 个 fclass 的 edge/depth 默认值与推荐范围，供前端齿轮子窗口构建。
#[tauri::command]
pub fn get_edge_guidance() -> Vec<EdgeGuidanceItem> {
    edge_depth_guidance()
        .into_iter()
        .map(|(fclass, g)| EdgeGuidanceItem {
            fclass: fclass.to_string(),
            edge: g.edge_expand,
            depth: g.depth,
            edge_min: g.edge_expand_range.0,
            edge_max: finite_or_none(g.edge_expand_range.1),
            depth_min: g.depth_range.0,
            depth_max: finite_or_none(g.depth_range.1),
        })
        .collect()
}

/// 后端自检命令。
#[tauri::command]
pub fn greet(name: &str) -> String {
    format!("你好，{name}！Water2Rust 纯 Rust 后端已就绪。")
}
