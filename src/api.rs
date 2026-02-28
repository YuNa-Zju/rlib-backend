use axum::{extract::{Path, State}, http::StatusCode, routing::{delete, get, post}, Json, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;
use crate::AppState;

#[derive(Deserialize)]
pub struct CreateTaskReq {
    pub username: String,
    pub password: String, // 前端传过来，后端存入 SQLite 用于守护进程
    pub target_date: String,
    pub area_id: String,
    pub interval_sec: u64,
}

#[derive(Serialize)]
pub struct TaskInfo { pub id: String, pub target_date: String, pub area_id: String }

/// POST /api/tasks (创建并持久化任务)
pub async fn create_task(State(state): State<Arc<AppState>>, Json(req): Json<CreateTaskReq>) -> Result<Json<serde_json::Value>, StatusCode> {
    let task_id = Uuid::new_v4().to_string();
    
    // 存入 SQLite 数据库，保证重启不丢失
    let _ = sqlx::query!("INSERT INTO tasks (id, username, password, target_date, area_id, interval_sec) VALUES (?, ?, ?, ?, ?, ?)",
        task_id, req.username, req.password, req.target_date, req.area_id, req.interval_sec as i64
    ).execute(&state.db_pool).await;

    // 启动后台扫描引擎
    crate::start_worker(task_id.clone(), req.username, req.password, req.target_date, req.area_id, req.interval_sec, state.clone()).await;

    Ok(Json(serde_json::json!({"msg": "任务启动成功", "task_id": task_id})))
}

/// GET /api/tasks
pub async fn list_tasks(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let mut tasks = vec![];
    for entry in state.task_handles.iter() {
        tasks.push(TaskInfo {
            id: entry.key().clone(),
            target_date: entry.value().1.clone(),
            area_id: entry.value().2.clone(),
        });
    }
    Json(serde_json::json!({"active_tasks": tasks}))
}

/// DELETE /api/tasks/:id
pub async fn delete_task(Path(id): Path<String>, State(state): State<Arc<AppState>>) -> Result<Json<serde_json::Value>, StatusCode> {
    if let Some((_, (abort_handle, _, _))) = state.task_handles.remove(&id) {
        abort_handle.abort();
        let _ = sqlx::query!("DELETE FROM tasks WHERE id = ?", id).execute(&state.db_pool).await;
        Ok(Json(serde_json::json!({"msg": "已终止并删除任务"})))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}
