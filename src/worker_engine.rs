use std::sync::Arc;
use tokio::time::{sleep, Duration};
use chrono::Local;
use crate::{core, AppState};

pub async fn start_worker(
    task_id: String, username: String, password: String,
    target_date: String, area_id: String, interval: u64, state: Arc<AppState>
) {
    // 【修复 E0382】提前 Clone 一份数据给 Tokio 闭包内部使用
    let tid = task_id.clone();
    let target_date_worker = target_date.clone();
    let area_id_worker = area_id.clone();
    let state_worker = state.clone();

    let handle = tokio::spawn(async move {
        println!("[Worker {}] 挂机启动! 目标: {} 区域: {}", tid, target_date_worker, area_id_worker);
        
        loop {
            let today = Local::now().format("%Y-%m-%d").to_string();
            if target_date_worker > today {
                println!("[Worker {}] 远期任务未到执行日，等待中...", tid);
                sleep(Duration::from_secs(300)).await;
                continue;
            }

            println!("[Worker {}] 执行日已到，开始高频扫描...", tid);
            match core::login_zju(&username, &password).await {
                Ok(client) => {
                    if let Ok((seg_id, start, end)) = client.fetch_segment(&area_id_worker, &target_date_worker).await {
                        if let Ok(seats) = client.fetch_free_seats(&area_id_worker, &seg_id, &target_date_worker, &start, &end).await {
                            if let Some(best_seat) = seats.first() {
                                let seat_id = best_seat["id"].as_str().unwrap();
                                let no = best_seat["no"].as_str().unwrap();
                                
                                println!("[Worker {}] 🎯 发现空位 {}，立刻提交!", tid, no);
                                if let Ok(msg) = client.confirm_booking(seat_id, &seg_id).await {
                                    println!("[Worker {}] ✅ 绝杀成功: {}", tid, msg);
                                    let _ = sqlx::query!("DELETE FROM tasks WHERE id = ?", tid).execute(&state_worker.db_pool).await;
                                    state_worker.task_handles.remove(&tid);
                                    break; 
                                }
                            }
                        }
                    }
                }
                Err(e) => println!("[Worker {}] 会话获取失败: {:?}", tid, e),
            }
            sleep(Duration::from_secs(interval)).await;
        }
    });

    // 此时使用的是没被转移走的原始变量
    state.task_handles.insert(task_id, (handle.abort_handle(), target_date, area_id, interval));
}
