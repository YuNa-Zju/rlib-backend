use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct LoginReq { pub username: String, pub password: String }

#[derive(Deserialize)]
pub struct DateQuery { pub date: String }

#[derive(Deserialize)]
pub struct SeatQuery { pub date: String, pub segment: String, pub start: String, pub end: String }

#[derive(Deserialize)]
pub struct ReserveReq { pub seat_id: String, pub segment_id: String }

#[derive(Deserialize)]
pub struct CreateTaskReq {
    pub username: String,
    pub password: String,
    pub target_date: String,
    pub area_id: String,
    pub interval_sec: u64,
}

#[derive(Serialize)]
pub struct TaskInfo { pub id: String, pub target_date: String, pub area_id: String, pub interval_sec: u64 }
