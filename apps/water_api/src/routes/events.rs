//! SSE 实时进度推送，对应 workshop 的 `events/status/{task_id}`。

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream::Stream;

use crate::models::TaskState;
use crate::state::AppState;

/// 以 SSE 周期性推送任务状态快照，直到任务终态。
pub async fn status_stream(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = async_stream::stream! {
        let mut ticker = tokio::time::interval(Duration::from_millis(500));
        loop {
            ticker.tick().await;
            let Some(snap) = state.snapshot(&task_id) else {
                yield Ok(Event::default().event("error").data("task not found"));
                break;
            };
            let payload = serde_json::to_string(&snap).unwrap_or_default();
            yield Ok(Event::default().event("status").data(payload));
            if matches!(snap.state, TaskState::Completed | TaskState::Failed) {
                break;
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}
