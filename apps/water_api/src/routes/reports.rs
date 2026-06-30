//! 任务结果报告，对应 workshop 的 `reports/{report_id}`。

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;

use crate::state::AppState;

/// 返回任务的结果报告（result 字段）。
pub async fn get_report(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let snap = state.snapshot(&task_id).ok_or(StatusCode::NOT_FOUND)?;
    match snap.result {
        Some(result) => Ok(Json(result)),
        None => Err(StatusCode::NOT_FOUND),
    }
}
