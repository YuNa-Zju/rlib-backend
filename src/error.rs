use axum::{http::StatusCode, response::{IntoResponse, Response}, Json};
use serde_json::json;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("网络请求失败: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("数据库持久化错误: {0}")]
    Database(#[from] sqlx::Error),
    #[error("未授权的访问或 Token 过期")]
    Unauthorized,
    #[error("业务处理异常: {0}")]
    Business(String),
    #[error("内部系统错误: {0}")]
    Internal(#[from] anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, msg) = match &self {
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, self.to_string()),
            AppError::Business(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        (status, Json(json!({ "code": 0, "msg": msg }))).into_response()
    }
}
