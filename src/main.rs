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
use sqlx::{sqlite::SqlitePoolOptions, FromRow, Row, SqlitePool};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::AbortHandle;
use tower_http::services::{ServeDir, ServeFile};

pub struct AppState {
    pub db_pool: SqlitePool,
    // Web Token 映射到 学号 (仅用于维持网页端会话)
    pub sessions: DashMap<String, String>,
    // 学号 映射到 底层长效 ZjuClient (供 Worker 和 Web 共同复用)
    pub client_pool: DashMap<String, core::ZjuClient>,
    pub task_handles: DashMap<String, (AbortHandle, String, String, String, String, u64)>,
    // 钉钉配置在内存中的缓存，启动时从数据库加载
    pub dingtalk_config: Arc<RwLock<Option<models::DingTalkConfig>>>,
}

#[derive(FromRow)]
struct TaskRow {
    id: String,
    username: String,
    password: String,
    target_date: String,
    area_id: String,
    area_name: String,
    start_time: String,
    interval_sec: i64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. 初始化数据库连接
    let pool = SqlitePoolOptions::new()
        .connect("sqlite://booking.db?mode=rwc")
        .await?;

    // 2. 自动创建钉钉配置表 (如果不存在)
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS dingtalk_config (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            webhook TEXT NOT NULL,
            secret TEXT
        )",
    )
    .execute(&pool)
    .await?;

    // 3. 尝试从数据库加载钉钉配置
    let dt_row = sqlx::query("SELECT webhook, secret FROM dingtalk_config WHERE id = 1")
        .fetch_optional(&pool)
        .await?;

    let initial_dt_config = dt_row.map(|row| models::DingTalkConfig {
        webhook: row.get("webhook"),
        secret: row.get("secret"),
    });

    // 4. 构建全局状态
    let state = Arc::new(AppState {
        db_pool: pool.clone(),
        sessions: DashMap::new(),
        client_pool: DashMap::new(),
        task_handles: DashMap::new(),
        dingtalk_config: Arc::new(RwLock::new(initial_dt_config)),
    });

    // 5. 恢复未完成的抢座任务
    let saved_tasks = sqlx::query_as::<_, TaskRow>("SELECT * FROM tasks")
        .fetch_all(&pool)
        .await?;
    for row in saved_tasks {
        worker_engine::start_worker(
            row.id,
            row.username,
            row.password,
            row.target_date,
            row.area_id,
            row.area_name,
            row.start_time,
            row.interval_sec as u64,
            state.clone(),
        )
        .await;
    }

    let frontend_service =
        ServeDir::new("dist").not_found_service(ServeFile::new("dist/index.html"));

    let app = Router::new()
        .route("/api/auth/login", post(api::login))
        .route("/api/auth/logout", post(api::logout))
        .route("/api/auth/me", get(api::get_user_info))
        .route("/api/library/areas", get(api::get_all_areas))
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
        .route(
            "/api/library/reservations/{id}",
            delete(api::cancel_reservation),
        )
        .route("/api/tasks", post(api::create_task).get(api::list_tasks))
        .route("/api/tasks/{id}", delete(api::delete_task))
        .route("/api/config/dingtalk", post(api::update_dingtalk_config))
        .with_state(state)
        .fallback_service(frontend_service);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    println!("Server running on http://0.0.0.0:3000");
    axum::serve(listener, app).await?;
    Ok(())
}
