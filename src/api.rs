use crate::{
    core::{self, ZjuClient},
    error::AppError,
    models::*,
    worker_engine, AppState,
};
use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    Json,
};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

/// 从请求头中提取 Token，获取学号，再从全局 client_pool 中取出长效会话
fn get_client(headers: &HeaderMap, state: &Arc<AppState>) -> Result<ZjuClient, AppError> {
    let auth = headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let token = auth.strip_prefix("Bearer ").ok_or(AppError::Unauthorized)?;

    // 1. 验证 Web Token 是否有效，取出学号
    let username = state
        .sessions
        .get(token)
        .map(|s| s.clone())
        .ok_or(AppError::Unauthorized)?;
    // 2. 取出全局复用的 Client
    let client = state
        .client_pool
        .get(&username)
        .map(|c| c.clone())
        .ok_or(AppError::Unauthorized)?;

    Ok(client)
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LoginReq>,
) -> Result<Json<serde_json::Value>, AppError> {
    // 核心复用逻辑：如果池子里有这个学号的存活 Client，直接复用；否则发起真实的浙大登录请求
    let client = if let Some(c) = state.client_pool.get(&req.username) {
        c.clone()
    } else {
        let new_client = core::login_zju(&req.username, &req.password).await?;
        state
            .client_pool
            .insert(req.username.clone(), new_client.clone());
        new_client
    };

    let our_jwt = Uuid::new_v4().to_string();
    let uid = client.uid.clone();
    let name = client.name.clone();

    // 仅建立 Web Token 到 学号 的映射
    state.sessions.insert(our_jwt.clone(), req.username.clone());

    Ok(Json(
        json!({ "code": 1, "msg": "登录成功", "token": our_jwt, "uid": uid, "name": name }),
    ))
}

pub async fn logout(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let auth = headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    if let Some(token) = auth.strip_prefix("Bearer ") {
        // 仅销毁 Web 会话，绝不影响 Worker 的长效 Client
        state.sessions.remove(token);
    }
    Ok(Json(json!({ "code": 1, "msg": "已退出登录" })))
}

pub async fn get_user_info(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let client = get_client(&headers, &state)?;
    Ok(Json(
        json!({ "code": 1, "data": UserInfo { uid: client.uid, name: client.name } }),
    ))
}

pub async fn get_all_areas(
    headers: HeaderMap,
    Query(q): Query<DateQuery>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let client = get_client(&headers, &state)?;
    let data = client.fetch_quick_select(&q.date).await?;
    Ok(Json(json!({ "code": 1, "data": data })))
}

pub async fn get_segments(
    headers: HeaderMap,
    Path(area_id): Path<String>,
    Query(q): Query<DateQuery>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let client = get_client(&headers, &state)?;
    let data = client.fetch_segment(&area_id, &q.date).await?;
    Ok(Json(json!({ "code": 1, "data": data })))
}

pub async fn get_free_seats(
    headers: HeaderMap,
    Path(area_id): Path<String>,
    Query(q): Query<SeatQuery>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let client = get_client(&headers, &state)?;
    let data = client
        .fetch_free_seats(&area_id, &q.segment, &q.date, &q.start, &q.end)
        .await?;
    Ok(Json(json!({ "code": 1, "data": data })))
}

pub async fn reserve_seat(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(req): Json<ReserveReq>,
) -> Result<Json<serde_json::Value>, AppError> {
    let client = get_client(&headers, &state)?;
    let msg = client
        .confirm_booking(&req.seat_id, &req.segment_id)
        .await?;
    Ok(Json(json!({ "code": 1, "msg": msg })))
}

pub async fn get_reservations(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let client = get_client(&headers, &state)?;
    let data = client.fetch_subscriptions().await?;
    Ok(Json(json!({ "code": 1, "data": data })))
}

pub async fn cancel_reservation(
    headers: HeaderMap,
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let client = get_client(&headers, &state)?;
    let msg = client.cancel_booking(&id).await?;
    Ok(Json(json!({ "code": 1, "msg": msg })))
}

pub async fn update_dingtalk_config(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateDingTalkReq>,
) -> Result<Json<serde_json::Value>, AppError> {
    // 写入数据库 (id写死为1，保证全局唯一配置)
    sqlx::query(
        "INSERT INTO dingtalk_config (id, webhook, secret) VALUES (1, $1, $2) 
         ON CONFLICT(id) DO UPDATE SET webhook=excluded.webhook, secret=excluded.secret",
    )
    .bind(&req.webhook)
    .bind(&req.secret)
    .execute(&state.db_pool)
    .await
    .map_err(|e| AppError::Business(format!("数据库保存失败: {}", e)))?;

    // 更新内存缓存
    let mut config = state.dingtalk_config.write().await;
    *config = Some(DingTalkConfig {
        webhook: req.webhook,
        secret: req.secret,
    });

    Ok(Json(
        json!({ "code": 1, "msg": "钉钉机器人配置已更新并持久化" }),
    ))
}

pub async fn create_task(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateTaskReq>,
) -> Result<Json<serde_json::Value>, AppError> {
    let task_id = Uuid::new_v4().to_string();
    let interval_i64 = req.interval_sec as i64;
    let actual_target_date = core::calculate_target_date(&req.target_date);

    sqlx::query!(
        "INSERT INTO tasks (id, username, password, target_date, area_id, area_name, start_time, interval_sec) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        task_id, req.username, req.password, actual_target_date, req.area_id, req.area_name, req.start_time, interval_i64
    ).execute(&state.db_pool).await?;

    worker_engine::start_worker(
        task_id.clone(),
        req.username,
        req.password,
        actual_target_date,
        req.area_id,
        req.area_name,
        req.start_time,
        req.interval_sec,
        state.clone(),
    )
    .await;

    Ok(Json(
        json!({ "code": 1, "msg": "任务排队成功", "task_id": task_id }),
    ))
}

pub async fn list_tasks(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mut tasks = vec![];
    for entry in state.task_handles.iter() {
        let (_abort_handle, target_date, area_id, area_name, start_time, interval_sec) =
            entry.value();
        tasks.push(TaskInfo {
            id: entry.key().clone(),
            target_date: target_date.clone(),
            start_time: start_time.clone(),
            area_id: area_id.clone(),
            area_name: area_name.clone(),
            interval_sec: *interval_sec,
        });
    }
    Ok(Json(json!({ "code": 1, "data": tasks })))
}

pub async fn delete_task(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    if let Some((_, (abort_handle, _, _, _, _, _))) = state.task_handles.remove(&id) {
        abort_handle.abort();
        sqlx::query!("DELETE FROM tasks WHERE id = ?", id)
            .execute(&state.db_pool)
            .await?;
        Ok(Json(json!({ "code": 1, "msg": "任务已终止" })))
    } else {
        Err(AppError::Business("找不到指定任务".to_string()))
    }
}
