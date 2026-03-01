mod api;
mod core;
mod error;
mod models;
mod worker_engine;

use axum::{routing::{delete, get, post}, Router};
use dashmap::DashMap;
use sqlx::{sqlite::SqlitePoolOptions, FromRow, SqlitePool};
use std::sync::Arc;
use tokio::task::AbortHandle;
use tower_http::services::{ServeDir, ServeFile}; // 补全托管

pub struct AppState {
    pub db_pool: SqlitePool,
    pub sessions: DashMap<String, core::ZjuClient>,
    // 状态机中也要存入 area_name 和 start_time
    pub task_handles: DashMap<String, (AbortHandle, String, String, String, String, u64)>,
}

#[derive(FromRow)]
struct TaskRow {
    id: String, username: String, password: String,
    target_date: String, area_id: String, area_name: String, 
    start_time: String, interval_sec: i64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let pool = SqlitePoolOptions::new()
        .connect("sqlite://booking.db?mode=rwc")
        .await?;

    let state = Arc::new(AppState {
        db_pool: pool.clone(),
        sessions: DashMap::new(),
        task_handles: DashMap::new(),
    });

    // 恢复任务
    let saved_tasks = sqlx::query_as::<_, TaskRow>("SELECT * FROM tasks").fetch_all(&pool).await?;
    for row in saved_tasks {
        worker_engine::start_worker(
            row.id, row.username, row.password, row.target_date,
            row.area_id, row.area_name, row.start_time, row.interval_sec as u64, state.clone(),
        ).await;
    }

    let frontend_service = ServeDir::new("dist").not_found_service(ServeFile::new("dist/index.html"));

    let app = Router::new()
        .route("/api/auth/login", post(api::login))
        .route("/api/auth/logout", post(api::logout))
        .route("/api/auth/me", get(api::get_user_info))
        .route("/api/library/areas", get(api::get_all_areas))
        .route("/api/library/areas/{area_id}/segments", get(api::get_segments))
        .route("/api/library/areas/{area_id}/seats", get(api::get_free_seats))
        .route("/api/library/reservations", post(api::reserve_seat).get(api::get_reservations))
        .route("/api/library/reservations/{id}", delete(api::cancel_reservation))
        .route("/api/tasks", post(api::create_task).get(api::list_tasks))
        .route("/api/tasks/{id}", delete(api::delete_task))
        .with_state(state)
        .fallback_service(frontend_service); // 托管网页

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}
