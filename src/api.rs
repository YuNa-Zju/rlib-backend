use axum::{extract::{Path, Query, State}, http::HeaderMap, Json};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;
use crate::{core::{self, ZjuClient}, error::AppError, models::*, worker_engine, AppState};

fn get_client(headers: &HeaderMap, state: &Arc<AppState>) -> Result<ZjuClient, AppError> {
    let auth = headers.get("Authorization").and_then(|h| h.to_str().ok()).unwrap_or("");
    if !auth.starts_with("Bearer ") { return Err(AppError::Unauthorized); }
    let token = &auth[7..];
    state.sessions.get(token).map(|c| c.clone()).ok_or(AppError::Unauthorized)
}

pub async fn login(State(state): State<Arc<AppState>>, Json(req): Json<LoginReq>) -> Result<Json<serde_json::Value>, AppError> {
    let client = core::login_zju(&req.username, &req.password).await?;
    let our_jwt = Uuid::new_v4().to_string(); 
    state.sessions.insert(our_jwt.clone(), client);
    Ok(Json(json!({ "code": 1, "msg": "登录成功", "token": our_jwt })))
}

pub async fn logout(headers: HeaderMap, State(state): State<Arc<AppState>>) -> Result<Json<serde_json::Value>, AppError> {
    let auth = headers.get("Authorization").and_then(|h| h.to_str().ok()).unwrap_or("");
    if auth.starts_with("Bearer ") { state.sessions.remove(&auth[7..]); }
    Ok(Json(json!({ "code": 1, "msg": "已退出登录" })))
}

pub async fn get_segments(headers: HeaderMap, Path(area_id): Path<String>, Query(q): Query<DateQuery>, State(state): State<Arc<AppState>>) -> Result<Json<serde_json::Value>, AppError> {
    let client = get_client(&headers, &state)?;
    let (id, start, end) = client.fetch_segment(&area_id, &q.date).await?;
    Ok(Json(json!({ "code": 1, "data": { "segment_id": id, "start": start, "end": end } })))
}

pub async fn get_free_seats(headers: HeaderMap, Path(area_id): Path<String>, Query(q): Query<SeatQuery>, State(state): State<Arc<AppState>>) -> Result<Json<serde_json::Value>, AppError> {
    let client = get_client(&headers, &state)?;
    let seats = client.fetch_free_seats(&area_id, &q.segment, &q.date, &q.start, &q.end).await?;
    Ok(Json(json!({ "code": 1, "data": seats })))
}

pub async fn reserve_seat(headers: HeaderMap, State(state): State<Arc<AppState>>, Json(req): Json<ReserveReq>) -> Result<Json<serde_json::Value>, AppError> {
    let client = get_client(&headers, &state)?;
    let msg = client.confirm_booking(&req.seat_id, &req.segment_id).await?;
    Ok(Json(json!({ "code": 1, "msg": msg })))
}

pub async fn get_reservations(headers: HeaderMap, State(state): State<Arc<AppState>>) -> Result<Json<serde_json::Value>, AppError> {
    let client = get_client(&headers, &state)?;
    let data = client.fetch_subscriptions().await?;
    Ok(Json(json!({ "code": 1, "data": data })))
}

pub async fn cancel_reservation(headers: HeaderMap, Path(id): Path<String>, State(state): State<Arc<AppState>>) -> Result<Json<serde_json::Value>, AppError> {
    let client = get_client(&headers, &state)?;
    let msg = client.cancel_booking(&id).await?;
    Ok(Json(json!({ "code": 1, "msg": msg })))
}

pub async fn create_task(State(state): State<Arc<AppState>>, Json(req): Json<CreateTaskReq>) -> Result<Json<serde_json::Value>, AppError> {
    let task_id = Uuid::new_v4().to_string();
    
    // 【修复 E0716】提前绑定 i64 变量，防止宏展开时生命周期过短
    let interval_i64 = req.interval_sec as i64;
    sqlx::query!(
        "INSERT INTO tasks (id, username, password, target_date, area_id, interval_sec) VALUES (?, ?, ?, ?, ?, ?)",
        task_id, req.username, req.password, req.target_date, req.area_id, interval_i64
    ).execute(&state.db_pool).await?;

    worker_engine::start_worker(task_id.clone(), req.username, req.password, req.target_date, req.area_id, req.interval_sec, state.clone()).await;
    Ok(Json(json!({ "code": 1, "msg": "任务排队成功", "task_id": task_id })))
}

pub async fn list_tasks(State(state): State<Arc<AppState>>) -> Result<Json<serde_json::Value>, AppError> {
    let mut tasks = vec![];
    for entry in state.task_handles.iter() {
        tasks.push(TaskInfo { id: entry.key().clone(), target_date: entry.value().1.clone(), area_id: entry.value().2.clone(), interval_sec: entry.value().3 });
    }
    Ok(Json(json!({ "code": 1, "data": tasks })))
}

pub async fn delete_task(Path(id): Path<String>, State(state): State<Arc<AppState>>) -> Result<Json<serde_json::Value>, AppError> {
    if let Some((_, (abort_handle, _, _, _))) = state.task_handles.remove(&id) {
        abort_handle.abort();
        sqlx::query!("DELETE FROM tasks WHERE id = ?", id).execute(&state.db_pool).await?;
        Ok(Json(json!({ "code": 1, "msg": "任务已终止" })))
    } else { Err(AppError::Business("找不到指定任务".to_string())) }
}
