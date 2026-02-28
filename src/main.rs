mod core;
mod api;

use axum::Router;
use dashmap::DashMap;
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
use std::sync::Arc;
use tokio::task::AbortHandle;
use tokio::time::{sleep, Duration};
use chrono::Local;

pub struct AppState {
    pub db_pool: SqlitePool,
    // Task_ID -> (AbortHandle, TargetDate, AreaID)
    pub task_handles: DashMap<String, (AbortHandle, String, String)>,
}

#[tokio::main]
async fn main() {
    // 1. 初始化 SQLite
    let pool = SqlitePoolOptions::new().connect("sqlite://booking.db?mode=rwc").await.unwrap();
    
    sqlx::query("
        CREATE TABLE IF NOT EXISTS tasks (
            id TEXT PRIMARY KEY, username TEXT, password TEXT,
            target_date TEXT, area_id TEXT, interval_sec INTEGER
        )
    ").execute(&pool).await.unwrap();

    let state = Arc::new(AppState { db_pool: pool.clone(), task_handles: DashMap::new() });

    // 2. 故障恢复：重启时从数据库加载所有未完成的任务
    let saved_tasks = sqlx::query!("SELECT * FROM tasks").fetch_all(&pool).await.unwrap();
    for row in saved_tasks {
        println!("🔄 恢复持久化任务: {}", row.id);
        start_worker(
            row.id, row.username.unwrap(), row.password.unwrap(),
            row.target_date.unwrap(), row.area_id.unwrap(), row.interval_sec.unwrap() as u64,
            state.clone()
        ).await;
    }

    // 3. 挂载 API 路由
    let app = Router::new()
        .route("/api/tasks", axum::routing::post(api::create_task).get(api::list_tasks))
        .route("/api/tasks/:id", axum::routing::delete(api::delete_task))
        .with_state(state);

    println!("🚀 极光图书馆调度中间件启动 | http://127.0.0.1:3000");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

/// 派发后台扫描与抢座守护线程
pub async fn start_worker(
    task_id: String, username: String, password: String,
    target_date: String, area_id: String, interval: u64, state: Arc<AppState>
) {
    let tid = task_id.clone();
    let handle = tokio::spawn(async move {
        println!("[Worker {}] 进入轮询队列...", tid);
        loop {
            // 这里可以加入日期判断，如果是未来的日期，可以计算差值并 sleep 大段时间
            // if target_date > today { sleep(long_time); continue; }

            // 每次轮询前重新登录，保证 Token 永远最新
            match core::login_zju(&username, &password).await {
                Ok(client) => {
                    if let Ok((seg_id, start, end)) = client.fetch_segment(&area_id, &target_date).await {
                        if let Ok(seats) = client.fetch_free_seats(&area_id, &seg_id, &target_date, &start, &end).await {
                            if let Some(best_seat) = seats.first() {
                                let seat_id = best_seat["id"].as_str().unwrap();
                                let no = best_seat["no"].as_str().unwrap();
                                
                                println!("[Worker {}] 🎯 发现空位 {}，发起绝杀!", tid, no);
                                if let Ok(msg) = client.confirm_booking(seat_id, &seg_id).await {
                                    println!("[Worker {}] ✅ 抢座成功: {}", tid, msg);
                                    // 抢到了，清理数据库记录和内存态
                                    let _ = sqlx::query!("DELETE FROM tasks WHERE id = ?", tid).execute(&state.db_pool).await;
                                    state.task_handles.remove(&tid);
                                    break; // 结束线程
                                }
                            }
                        }
                    }
                }
                Err(e) => println!("[Worker {}] 登录失败或票据过期: {:?}", tid, e),
            }
            sleep(Duration::from_secs(interval)).await;
        }
    });

    state.task_handles.insert(task_id, (handle.abort_handle(), target_date, area_id));
}
