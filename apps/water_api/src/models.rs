//! API 请求 / 响应数据结构。

use serde::{Deserialize, Serialize};

/// 任务状态枚举。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TaskState {
    Pending,
    Running,
    Completed,
    Failed,
}

/// 水面 DEM 任务请求。
#[derive(Debug, Clone, Deserialize)]
pub struct HydroRequest {
    pub dem: String,
    pub water: String,
    pub output: String,
    #[serde(default)]
    pub output_mode: Option<String>,
}

/// fclass 分类任务请求。
#[derive(Debug, Clone, Deserialize)]
pub struct FclassRequest {
    pub water: String,
    pub output: String,
    pub reference_path: String,
    #[serde(default)]
    pub transition_only: bool,
}

/// 水边深度任务请求。
#[derive(Debug, Clone, Deserialize)]
pub struct EdgeDepthRequest {
    pub water: String,
    pub output: String,
    /// 每 fclass 的 edge/depth 覆盖（GUI 调参）；未列出的 fclass 用默认 `WATER_FCLASS_EDGE_DEPTH`。
    #[serde(default)]
    pub overrides: Vec<EdgeDepthOverride>,
}

/// 单个 fclass 的 edge/depth 覆盖值（GUI 滑块结果）。范围提示见 `water_core::edge_depth::edge_depth_guidance`。
#[derive(Debug, Clone, Deserialize)]
pub struct EdgeDepthOverride {
    pub fclass: String,
    pub edgeexpand: f64,
    pub depth: f64,
}

/// 任务状态快照（轮询 / SSE 返回）。
#[derive(Debug, Clone, Serialize)]
pub struct TaskSnapshot {
    pub task_id: String,
    pub state: TaskState,
    pub progress: u8,
    pub logs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
