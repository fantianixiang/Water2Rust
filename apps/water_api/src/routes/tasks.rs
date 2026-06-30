//! 任务提交与查询路由。

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use crate::models::{
    EdgeDepthRequest, FclassRequest, HydroRequest, TaskSnapshot, TaskState,
};
use crate::state::AppState;

/// 提交水面 DEM 任务。
pub async fn submit_hydro(
    State(state): State<Arc<AppState>>,
    Json(req): Json<HydroRequest>,
) -> (StatusCode, Json<TaskSnapshot>) {
    let task_id = state.create_task();
    spawn_stub(state.clone(), task_id.clone(), format!("hydro: {} + {} -> {}", req.dem, req.water, req.output));
    let snap = state.snapshot(&task_id).expect("just created");
    (StatusCode::ACCEPTED, Json(snap))
}

/// 提交 fclass 分类任务。
pub async fn submit_fclass(
    State(state): State<Arc<AppState>>,
    Json(req): Json<FclassRequest>,
) -> (StatusCode, Json<TaskSnapshot>) {
    let task_id = state.create_task();
    spawn_stub(state.clone(), task_id.clone(), format!("fclass: {} -> {}", req.water, req.output));
    let snap = state.snapshot(&task_id).expect("just created");
    (StatusCode::ACCEPTED, Json(snap))
}

/// 提交水边深度任务。
pub async fn submit_edge_depth(
    State(state): State<Arc<AppState>>,
    Json(req): Json<EdgeDepthRequest>,
) -> (StatusCode, Json<TaskSnapshot>) {
    let task_id = state.create_task();
    spawn_stub(state.clone(), task_id.clone(), format!("edge-depth: {} -> {}", req.water, req.output));
    let snap = state.snapshot(&task_id).expect("just created");
    (StatusCode::ACCEPTED, Json(snap))
}

/// 轮询任务状态（权威终态）。
pub async fn get_task(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
) -> Result<Json<TaskSnapshot>, StatusCode> {
    state.snapshot(&task_id).map(Json).ok_or(StatusCode::NOT_FOUND)
}

/// 骨架占位：标记任务为运行中，并记录入参。真实执行接入业务 crate 后替换。
fn spawn_stub(state: Arc<AppState>, task_id: String, desc: String) {
    tokio::spawn(async move {
        state.update(&task_id, |rec| {
            rec.state = TaskState::Running;
            rec.progress = 0;
            rec.logs.push(format!("已受理任务：{desc}"));
            rec.logs.push("业务执行尚未接入（骨架阶段）".to_string());
            rec.state = TaskState::Failed;
            rec.error = Some("NotImplemented: 业务 crate 待接入 eci-gdal".to_string());
        });
    });
}
