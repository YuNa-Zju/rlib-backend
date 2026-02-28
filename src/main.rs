mod api;
mod core;
mod error;
mod models;
mod worker_engine;

use axum::{
    routing::{delete, get, post},
    Router,
};
use dashmap::DashMap;
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
use std::sync::Arc;
use tokio::task::AbortHandle;

pub struct AppState {
    pub db_pool: SqlitePool,
    pub sessions: DashMap<String, core::ZjuClient>,
    pub task_handles: DashMap<String, (AbortHandle, String, String, u64)>,
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

    let saved_tasks = sqlx::query!("SELECT * FROM tasks").fetch_all(&pool).await?;
    for row in saved_tasks {
        // 【修复 E0308】SQLite 取出来的值是 Option，通过 unwrap 解包
        let t_id = row.id.unwrap();
        println!("🔄 从 SQLite 恢复持久化任务: {}", t_id);

        worker_engine::start_worker(
            t_id,
            row.username,
            row.password,
            row.target_date,
            row.area_id,
            row.interval_sec as u64,
            state.clone(),
        )
        .await;
    }

    // 修复 Axum 0.7 的路由捕获语法：将 :var 替换为 {var}
    let app = Router::new()
        .route("/api/auth/login", post(api::login))
        .route("/api/auth/logout", post(api::logout))
        // 注意这里：:area_id 改成了 {area_id}
        .route(
            "/api/library/areas/{area_id}/segments",
            get(api::get_segments),
        )
        .route(
            "/api/library/areas/{area_id}/seats",
            get(api::get_free_seats),
        )
        .route(
            "/api/library/reservations",
            post(api::reserve_seat).get(api::get_reservations),
        )
        // 注意这里：:id 改成了 {id}
        .route(
            "/api/library/reservations/{id}",
            delete(api::cancel_reservation),
        )
        .route("/api/tasks", post(api::create_task).get(api::list_tasks))
        // 注意这里：:id 改成了 {id}
        .route("/api/tasks/{id}", delete(api::delete_task))
        .with_state(state);

    println!("🚀 Polaris 预约中间件启动完成 | http://127.0.0.1:3000");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    axum::serve(listener, app).await?;

    Ok(())
}
