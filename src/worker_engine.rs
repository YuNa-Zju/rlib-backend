use std::sync::Arc;
use tokio::time::{sleep, Duration};
use chrono::Local;
use crate::{core, models::FlexNum, AppState};

pub async fn start_worker(
    task_id: String, username: String, password: String,
    target_date: String, area_id: String, area_name: String, start_time: String, 
    interval: u64, state: Arc<AppState>
) {
    let tid = task_id.clone();
    let target_date_worker = target_date.clone();
    let area_id_worker = area_id.clone();
    let area_name_worker = area_name.clone();
    let start_time_worker = start_time.clone();
    let state_worker = state.clone();

    let handle = tokio::spawn(async move {
        println!("[Worker {}] 挂机启动! 目标时间: {} 区域: {} ({})", tid, start_time_worker, area_name_worker, area_id_worker);
        
        loop {
            let now_str = Local::now().format("%Y-%m-%dT%H:%M").to_string();
            
            if start_time_worker > now_str {
                println!("[Worker {}] 未到设定的扫描时间 {} (当前: {})，等待中...", tid, start_time_worker, now_str);
                sleep(Duration::from_secs(15)).await;
                continue;
            }

            println!("[Worker {}] 扫描时间已到，开始高频捡漏...", tid);
            match core::login_zju(&username, &password).await {
                Ok(client) => {
                    if let Ok(day_data) = client.fetch_segment(&area_id_worker, &target_date_worker).await {
                        if let Some(first_time) = day_data.times.first() {
                            let seg_id = &first_time.id;
                            let start = &first_time.start;
                            let end = &first_time.end;

                            if let Ok(mut free_seats) = client.fetch_free_seats(&area_id_worker, seg_id, &target_date_worker, start, end).await {
                                free_seats.retain(|s| {
                                    match &s.status {
                                        FlexNum::Num(n) => *n == 1,
                                        FlexNum::Str(st) => st == "1",
                                    }
                                });
                                
                                free_seats.sort_by_key(|s| s.no.clone());
                                
                                if let Some(best_seat) = free_seats.first() {
                                    println!("[Worker {}] 🎯 发现空位 {}，立刻提交!", tid, best_seat.no);
                                    if let Ok(msg) = client.confirm_booking(&best_seat.id, seg_id).await {
                                        println!("[Worker {}] ✅ 绝杀成功: {}", tid, msg);
                                        let _ = sqlx::query!("DELETE FROM tasks WHERE id = ?", tid).execute(&state_worker.db_pool).await;
                                        state_worker.task_handles.remove(&tid);
                                        break; 
                                    }
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

    state.task_handles.insert(task_id, (handle.abort_handle(), target_date, area_id, area_name, start_time, interval));
}
