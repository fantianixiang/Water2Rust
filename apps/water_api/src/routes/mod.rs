//! API 路由汇总。

mod events;
mod reports;
mod tasks;

use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;

use crate::state::AppState;

/// `/health` 健康检查。
pub async fn health() -> &'static str {
    "ok"
}

/// `/api/v1` 子路由。
pub fn api_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/tasks/hydro", post(tasks::submit_hydro))
        .route("/tasks/fclass", post(tasks::submit_fclass))
        .route("/tasks/edge-depth", post(tasks::submit_edge_depth))
        .route("/tasks/{task_id}", get(tasks::get_task))
        .route("/events/status/{task_id}", get(events::status_stream))
        .route("/reports/{task_id}", get(reports::get_report))
}
