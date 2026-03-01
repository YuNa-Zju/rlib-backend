use crate::{core, models::FlexNum, AppState};
use chrono::Local;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

pub async fn start_worker(
    task_id: String,
    username: String,
    password: String,
    target_date: String,
    area_id: String,
    area_name: String,
    start_time: String,
    interval: u64,
    state: Arc<AppState>,
) {
    let tid = task_id.clone();
    let target_date_worker = target_date.clone();
    let area_id_worker = area_id.clone();
    let area_name_worker = area_name.clone();
    let start_time_worker = start_time.clone();
    let state_worker = state.clone();

    // 保存账号密码用于失效重登
    let un_worker = username.clone();
    let pw_worker = password.clone();

    let handle = tokio::spawn(async move {
        println!(
            "[Worker {}] 挂机启动! 目标时间: {} 区域: {} 日期: {}",
            tid, start_time_worker, area_name_worker, target_date_worker
        );

        loop {
            let now_str = Local::now().format("%Y-%m-%dT%H:%M").to_string();

            if start_time_worker > now_str {
                sleep(Duration::from_secs(15)).await;
                continue;
            }

            // 获取复用 Client 或自动重登
            let client = {
                if let Some(c) = state_worker.client_pool.get(&un_worker) {
                    c.clone()
                } else {
                    println!("[Worker {}] 客户端池无有效会话，正在重登...", tid);
                    match core::login_zju(&un_worker, &pw_worker).await {
                        Ok(new_c) => {
                            state_worker
                                .client_pool
                                .insert(un_worker.clone(), new_c.clone());
                            new_c
                        }
                        Err(e) => {
                            println!("[Worker {}] 登录异常跳过本次扫描: {:?}", tid, e);
                            sleep(Duration::from_secs(interval)).await;
                            continue;
                        }
                    }
                }
            };

            // 业务执行
            match client
                .fetch_segment(&area_id_worker, &target_date_worker)
                .await
            {
                Ok(day_data) => {
                    if let Some(first_time) = day_data.times.first() {
                        let seg_id = &first_time.id;
                        if let Ok(mut free_seats) = client
                            .fetch_free_seats(
                                &area_id_worker,
                                seg_id,
                                &target_date_worker,
                                &first_time.start,
                                &first_time.end,
                            )
                            .await
                        {
                            free_seats.retain(|s| match &s.status {
                                FlexNum::Num(n) => *n == 1,
                                FlexNum::Str(st) => st == "1",
                            });
                            free_seats.sort_by_key(|s| s.no.clone());

                            if let Some(best_seat) = free_seats.first() {
                                println!(
                                    "[Worker {}] 🎯 发现空位 {}，提交中...",
                                    tid, best_seat.no
                                );
                                
                                // 🌟 核心修改 1：使用 match 捕获并打印所有的失败原因
                                match client.confirm_booking(&best_seat.id, seg_id).await {
                                    Ok(msg) => {
                                        println!("[Worker {}] ✅ 绝杀成功: {}", tid, msg);

                                        // 按需读取钉钉配置发送通知
                                        let config_guard = state_worker.dingtalk_config.read().await;
                                        if let Some(config) = config_guard.as_ref() {
                                            let notify_msg = format!(
                                                "【通知】🎉 座位抢占成功！\n区域: {}\n座位号: {}\n日期: {}",
                                                area_name_worker, best_seat.no, target_date_worker
                                            );
                                            let _ =
                                                core::send_dingtalk_notification(config, &notify_msg)
                                                    .await;
                                        } else {
                                            println!(
                                                "[Worker {}] 提示: 未配置钉钉机器人，跳过通知。",
                                                tid
                                            );
                                        }

                                        let _ = sqlx::query!("DELETE FROM tasks WHERE id = ?", tid)
                                            .execute(&state_worker.db_pool)
                                            .await;
                                        state_worker.task_handles.remove(&tid);
                                        break;
                                    },
                                    Err(e) => {
                                        // 打印出浙大系统拦截预约的具体原因！
                                        println!("[Worker {}] ❌ 提交失败，原因: {}", tid, e);
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    // 🌟 核心修改 2：防掉线逻辑，执行无缝重登而不是直接 remove
                    let err_msg = e.to_string();
                    if err_msg.contains("认证")
                        || err_msg.contains("登录")
                        || err_msg.contains("失效")
                    {
                        println!(
                            "[Worker {}] 检测到会话过期，执行无缝重登保护网页端...",
                            tid
                        );
                        // 原地登录覆盖旧会话，避免产生连接池真空期
                        if let Ok(new_c) = core::login_zju(&un_worker, &pw_worker).await {
                            state_worker.client_pool.insert(un_worker.clone(), new_c);
                            println!("[Worker {}] 🔄 无缝重登完毕!", tid);
                        }
                    } else {
                        // 打印其他网络或接口错误
                        println!("[Worker {}] 接口请求报错: {}", tid, err_msg);
                    }
                }
            }
            sleep(Duration::from_secs(interval)).await;
        }
    });

    state.task_handles.insert(
        task_id,
        (
            handle.abort_handle(),
            target_date,
            area_id,
            area_name,
            start_time,
            interval,
        ),
    );
}
