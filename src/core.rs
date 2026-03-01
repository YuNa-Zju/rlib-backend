use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
use anyhow::{anyhow, Result};
use base64::Engine;
use chrono::Local;
use num_bigint::BigUint;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, CONTENT_TYPE, ORIGIN, REFERER, USER_AGENT};
use serde_json::Value;
use crate::models::{QuickSelectData, SeatInfo, SegmentDay, Subscription};

type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;

#[derive(Clone)]
pub struct ZjuClient {
    pub client: reqwest::Client,
    pub jwt: String,
    pub uid: String,
    pub name: String,
}

impl ZjuClient {
    pub fn get_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json, text/plain, */*"));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(ORIGIN, HeaderValue::from_static("https://booking.lib.zju.edu.cn"));
        headers.insert(REFERER, HeaderValue::from_static("https://booking.lib.zju.edu.cn/h5/index.html"));
        headers.insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/145.0.0.0 Safari/537.36 Edg/145.0.0.0"));
        headers.insert("X-Requested-With", HeaderValue::from_static("XMLHttpRequest"));
        headers.insert("lang", HeaderValue::from_static("zh"));

        let auth_val = HeaderValue::from_str(&format!("bearer{}", self.jwt))
            .unwrap_or(HeaderValue::from_static("bearerINVALID"));
        headers.insert("authorization", auth_val);

        headers
    }

    fn aes_encrypt(&self, payload: &Value) -> Result<String> {
        let today = Local::now().format("%Y%m%d").to_string();
        let key_str = format!("{}{}", today, today.chars().rev().collect::<String>());
        let iv_str = "ZZWBKJ_ZHIHUAWEI";

        let pt = serde_json::to_string(payload)?;
        let pt_len = pt.len();

        let mut buf = vec![0u8; pt_len + 16];
        buf[..pt_len].copy_from_slice(pt.as_bytes());

        let ct = Aes128CbcEnc::new(key_str.as_bytes().into(), iv_str.as_bytes().into())
            .encrypt_padded_mut::<Pkcs7>(&mut buf, pt_len)
            .map_err(|e| anyhow!("AES 加密操作失败: {:?}", e))?;

        Ok(base64::engine::general_purpose::STANDARD.encode(ct))
    }

    pub async fn fetch_quick_select(&self, date: &str) -> Result<QuickSelectData> {
        let url = "https://booking.lib.zju.edu.cn/reserve/index/quickSelect";
        let payload = serde_json::json!({
            "id": "1", "date": date, "categoryIds": ["1"], "members": 0,
            "authorization": format!("bearer{}", self.jwt)
        });

        let res: Value = self.client.post(url).headers(self.get_headers()).json(&payload).send().await?.json().await?;
        if res["code"] == 0 {
            let data: QuickSelectData = serde_json::from_value(res["data"].clone())?;
            Ok(data)
        } else {
            Err(anyhow!("获取区域总览失败: {}", res["msg"]))
        }
    }

    pub async fn fetch_segment(&self, area_id: &str, target_date: &str) -> Result<SegmentDay> {
        let url = "https://booking.lib.zju.edu.cn/api/Seat/date";
        let payload = serde_json::json!({ "build_id": area_id, "authorization": format!("bearer{}", self.jwt) });

        let res: Value = self.client.post(url).headers(self.get_headers()).json(&payload).send().await?.json().await?;
        if res["code"] != 1 {
            return Err(anyhow!("获取时间段失败: {}", res["msg"]));
        }

        let days: Vec<SegmentDay> = serde_json::from_value(res["data"].clone())?;
        let target = days.into_iter().find(|d| d.day == target_date)
            .ok_or_else(|| anyhow!("未找到指定日期 ({}) 的开放时间段", target_date))?;

        Ok(target)
    }

    pub async fn fetch_free_seats(&self, area_id: &str, segment_id: &str, date: &str, start: &str, end: &str) -> Result<Vec<SeatInfo>> {
        let url = "https://booking.lib.zju.edu.cn/api/Seat/seat";
        let payload = serde_json::json!({
            "area": area_id, "segment": segment_id, "day": date,
            "startTime": start, "endTime": end, "authorization": format!("bearer{}", self.jwt)
        });

        let res: Value = self.client.post(url).headers(self.get_headers()).json(&payload).send().await?.json().await?;
        if res["code"] != 1 {
            return Err(anyhow!("获取具体座位失败: {}", res["msg"]));
        }

        let seats: Vec<SeatInfo> = serde_json::from_value(res["data"].clone())?;
        Ok(seats)
    }

    pub async fn confirm_booking(&self, seat_id: &str, segment_id: &str) -> Result<String> {
        let url = "https://booking.lib.zju.edu.cn/api/Seat/confirm";
        let raw_payload = serde_json::json!({ "seat_id": seat_id, "segment": segment_id });
        let aesjson = self.aes_encrypt(&raw_payload)?;

        let final_payload = serde_json::json!({ "aesjson": aesjson, "authorization": format!("bearer{}", self.jwt) });
        let res: Value = self.client.post(url).headers(self.get_headers()).json(&final_payload).send().await?.json().await?;
        if res["code"] == 1 { Ok(res["msg"].as_str().unwrap_or("成功").to_string()) } 
        else { Err(anyhow!("预约失败: {}", res["msg"])) }
    }

    pub async fn fetch_subscriptions(&self) -> Result<Vec<Subscription>> {
        let url = "https://booking.lib.zju.edu.cn/api/index/subscribe";
        let payload = serde_json::json!({ "authorization": format!("bearer{}", self.jwt) });

        let res: Value = self.client.post(url).headers(self.get_headers()).json(&payload).send().await?.json().await?;
        if res["code"] == 1 {
            let subs: Vec<Subscription> = serde_json::from_value(res["data"].clone())?;
            Ok(subs)
        } else {
            Err(anyhow!("获取预约列表失败: {}", res["msg"]))
        }
    }

    pub async fn cancel_booking(&self, record_id: &str) -> Result<String> {
        let url = "https://booking.lib.zju.edu.cn/api/Space/cancel";
        let payload = serde_json::json!({ "id": record_id, "authorization": format!("bearer{}", self.jwt) });

        let res: Value = self.client.post(url).headers(self.get_headers()).json(&payload).send().await?.json().await?;
        if res["code"] == 1 { Ok("取消预约成功".to_string()) } 
        else { Err(anyhow!("取消预约失败: {}", res["msg"])) }
    }
}

pub async fn login_zju(username: &str, password: &str) -> Result<ZjuClient> {
    let client = reqwest::Client::builder().cookie_store(true).redirect(reqwest::redirect::Policy::none()).build()?;
    let ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/145.0.0.0 Safari/537.36 Edg/145.0.0.0";
    let login_url = "https://zjuam.zju.edu.cn/cas/login?service=https%3A%2F%2Fbooking.lib.zju.edu.cn%2Fapi%2Fcas%2Fcas";

    let html = client.get(login_url).header(USER_AGENT, ua).send().await?.text().await?;
    let execution = html.split("name=\"execution\" value=\"").nth(1).and_then(|s| s.split("\"").next()).ok_or_else(|| anyhow!("无法提取 execution"))?;
    
    let pub_res: Value = client.get("https://zjuam.zju.edu.cn/cas/v2/getPubKey").header(USER_AGENT, ua).send().await?.json().await?;
    let modulus = pub_res["modulus"].as_str().ok_or_else(|| anyhow!("无 modulus"))?;
    let exponent = pub_res["exponent"].as_str().ok_or_else(|| anyhow!("无 exponent"))?;

    let m = BigUint::from_bytes_be(password.as_bytes());
    let e = BigUint::parse_bytes(exponent.as_bytes(), 16).unwrap();
    let n = BigUint::parse_bytes(modulus.as_bytes(), 16).unwrap();
    let c = m.modpow(&e, &n);
    let enc_pwd = format!("{:0>128x}", c);

    let form = [("username", username), ("password", &enc_pwd), ("authcode", ""), ("execution", execution), ("_eventId", "submit")];
    let submit_res = client.post(login_url).header(USER_AGENT, ua).form(&form).send().await?;
    let location = submit_res.headers().get("location").ok_or_else(|| anyhow!("未重定向"))?.to_str().unwrap();

    if location.contains("login") { return Err(anyhow!("账号或密码错误。")); }

    let ticket_res = client.get(location).header(USER_AGENT, ua).send().await?;
    
    // 优化：利用链式调用折叠冗长的嵌套 if let 
    let mut cas_token = String::new();
    if let Some(t) = ticket_res.headers().get("location")
        .and_then(|l| l.to_str().ok())
        .and_then(|h5_loc| h5_loc.split("cas=").nth(1))
        .and_then(|s| s.split('&').next()) 
    {
        cas_token = t.to_string();
    }
    
    if cas_token.is_empty() {
        let res2 = client.get("https://booking.lib.zju.edu.cn/api/cas/cas").header(USER_AGENT, ua).send().await?;
        if let Some(t) = res2.headers().get("location")
            .and_then(|l| l.to_str().ok())
            .and_then(|loc2| loc2.split("cas=").nth(1))
            .and_then(|s| s.split('&').next())
        { 
            cas_token = t.to_string(); 
        }
    }

    if cas_token.is_empty() { return Err(anyhow!("无法提取 CAS Token")); }

    let jwt_res: Value = client.post("https://booking.lib.zju.edu.cn/api/cas/user")
        .header(USER_AGENT, ua)
        .json(&serde_json::json!({ "cas": cas_token }))
        .send().await?.json().await?;

    if jwt_res["code"] == 1 {
        let jwt = jwt_res["member"]["token"].as_str().unwrap().to_string();
        let uid = jwt_res["member"]["id"].as_str().unwrap_or("").to_string();
        let name = jwt_res["member"]["name"].as_str().unwrap_or("").to_string();
        
        Ok(ZjuClient { client, jwt, uid, name })
    } else {
        Err(anyhow!("业务层认证被拒: {}", jwt_res["msg"]))
    }
}
