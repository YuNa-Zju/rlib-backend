use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum FlexNum {
    Num(i64),
    Str(String),
}

impl Default for FlexNum {
    fn default() -> Self {
        FlexNum::Num(0)
    }
}

#[derive(Deserialize)]
pub struct LoginReq {
    pub username: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct DateQuery {
    pub date: String,
}

#[derive(Deserialize)]
pub struct SeatQuery {
    pub date: String,
    pub segment: String,
    pub start: String,
    pub end: String,
}

#[derive(Deserialize)]
pub struct ReserveReq {
    pub seat_id: String,
    pub segment_id: String,
}

#[derive(Deserialize)]
pub struct CreateTaskReq {
    pub username: String,
    pub password: String,
    pub target_date: String,
    pub area_id: String,
    pub area_name: String,
    pub start_time: String,
    pub interval_sec: u64,
}

#[derive(Serialize)]
pub struct TaskInfo {
    pub id: String,
    pub target_date: String,
    pub start_time: String,
    pub area_id: String,
    pub area_name: String,
    pub interval_sec: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UserInfo {
    pub uid: String,
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LocationNode {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub enname: String,
    #[serde(default, rename = "nameMerge")]
    pub name_merge: String,
    #[serde(default, rename = "parentId")]
    pub parent_id: FlexNum,
    #[serde(default, rename = "topId")]
    pub top_id: FlexNum,
    #[serde(default)]
    pub total_num: Option<FlexNum>,
    #[serde(default)]
    pub free_num: Option<FlexNum>,
    #[serde(default)]
    pub sort: Option<FlexNum>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct QuickSelectData {
    pub date: Vec<String>,
    pub premises: Vec<LocationNode>,
    pub storey: Vec<LocationNode>,
    pub area: Vec<LocationNode>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SegmentTime {
    pub id: String,
    pub start: String,
    pub end: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SegmentDay {
    pub day: String,
    pub times: Vec<SegmentTime>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SeatInfo {
    pub id: String,
    pub no: String,
    #[serde(default)]
    pub name: String,
    pub status: FlexNum,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Subscription {
    pub id: String,
    #[serde(default, rename = "areaName")]
    pub area_name: String,
    pub no: String,
    #[serde(default, rename = "beginTime")]
    pub begin_time: String,
    #[serde(default, rename = "endTime")]
    pub end_time: String,
    #[serde(default, rename = "statusName")]
    pub status_name: String,
}

#[derive(Deserialize)]
pub struct UpdateDingTalkReq {
    pub webhook: String,
    pub secret: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DingTalkConfig {
    pub webhook: String,
    pub secret: Option<String>,
}
