//! Water2Rust REST API（Axum）。
//!
//! 形态仿照 `GeoAI_Toolkit/workshop` 的 FastAPI 服务：
//! - `POST /api/v1/tasks/{hydro|fclass|edge-depth}` 提交任务，返回 task_id
//! - `GET  /api/v1/tasks/{task_id}` 轮询任务状态（权威终态）
//! - `GET  /api/v1/events/status/{task_id}` SSE 实时进度
//! - `GET  /api/v1/reports/{task_id}` 任务结果报告
//! - `GET  /health` 健康检查

mod models;
mod routes;
mod state;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::routing::get;
use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let state = Arc::new(AppState::new());

    let app = Router::new()
        .route("/health", get(routes::health))
        .nest("/api/v1", routes::api_router())
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 8000));
    tracing::info!("Water2Rust API 监听于 http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
