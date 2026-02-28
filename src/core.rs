use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
use anyhow::{anyhow, Result};
use chrono::Local;
use num_bigint::BigUint;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, CONTENT_TYPE, ORIGIN, REFERER, USER_AGENT};
use serde_json::Value;

type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;

#[derive(Clone)]
pub struct ZjuClient {
    client: reqwest::Client,
    pub jwt: String,
}

impl ZjuClient {
    /// 构造严格对齐的通用 Header
    fn get_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json, text/plain, */*"));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(ORIGIN, HeaderValue::from_static("https://booking.lib.zju.edu.cn"));
        headers.insert(REFERER, HeaderValue::from_static("https://booking.lib.zju.edu.cn/h5/index.html"));
        headers.insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36"));
        headers.insert("X-Requested-With", HeaderValue::from_static("XMLHttpRequest"));
        headers.insert("lang", HeaderValue::from_static("zh"));
        headers.insert("authorization", HeaderValue::from_str(&format!("bearer{}", self.jwt)).unwrap());
        headers
    }

    /// 执行 AES-128-CBC 加密 (复刻前端逻辑)
    fn aes_encrypt(&self, payload: &Value) -> Result<String> {
        let today = Local::now().format("%Y%m%d").to_string();
        let key_str = format!("{}{}", today, today.chars().rev().collect::<String>());
        let iv_str = "ZZWBKJ_ZHIHUAWEI";

        let pt = serde_json::to_string(payload)?;
        let mut buf = vec![0u8; pt.len() + 16]; // 预留 Padding 空间
        let ct_len = Aes128CbcEnc::new(key_str.as_bytes().into(), iv_str.as_bytes().into())
            .encrypt_padded_mut::<Pkcs7>(&mut buf, pt.as_bytes(), pt.len())
            .map_err(|e| anyhow!("AES 加密失败: {:?}", e))?
            .len();

        Ok(base64::engine::general_purpose::STANDARD.encode(&buf[..ct_len]))
    }

    /// 获取某区域当天的可用时间段 (Segment)
    pub async fn fetch_segment(&self, area_id: &str, target_date: &str) -> Result<(String, String, String)> {
        let url = "https://booking.lib.zju.edu.cn/api/Seat/date";
        let payload = serde_json::json!({ "build_id": area_id, "authorization": format!("bearer{}", self.jwt) });
        
        let res: Value = self.client.post(url).headers(self.get_headers()).json(&payload).send().await?.json().await?;
        if res["code"] != 1 { return Err(anyhow!("获取时间段失败")); }

        let days = res["data"].as_array().ok_or(anyhow!("数据格式错误"))?;
        let target = days.iter().find(|d| d["day"].as_str() == Some(target_date))
            .ok_or(anyhow!("找不到该日期的开放时间段"))?;
        
        let times = target["times"].as_array().ok_or(anyhow!("时间段为空"))?;
        let first_time = &times[0];
        
        Ok((
            first_time["id"].as_str().unwrap().to_string(),
            first_time["start"].as_str().unwrap().to_string(),
            first_time["end"].as_str().unwrap().to_string(),
        ))
    }

    /// 拉取具体座位并过滤空闲位置
    pub async fn fetch_free_seats(&self, area_id: &str, segment_id: &str, date: &str, start: &str, end: &str) -> Result<Vec<Value>> {
        let url = "https://booking.lib.zju.edu.cn/api/Seat/seat";
        let payload = serde_json::json!({
            "area": area_id, "segment": segment_id, "day": date,
            "startTime": start, "endTime": end, "authorization": format!("bearer{}", self.jwt)
        });

        let res: Value = self.client.post(url).headers(self.get_headers()).json(&payload).send().await?.json().await?;
        if res["code"] != 1 { return Err(anyhow!("获取座位失败")); }

        let mut free_seats: Vec<Value> = res["data"].as_array().unwrap_or(&vec![]).iter()
            .filter(|s| s["status"].as_str() == Some("1"))
            .cloned().collect();
        
        // 按座位号从小到大排序 (例如 Z3F001 优先)
        free_seats.sort_by_key(|s| s["no"].as_str().unwrap_or("").to_string());
        Ok(free_seats)
    }

    /// 确认抢座
    pub async fn confirm_booking(&self, seat_id: &str, segment_id: &str) -> Result<String> {
        let url = "https://booking.lib.zju.edu.cn/api/Seat/confirm";
        let raw_payload = serde_json::json!({ "seat_id": seat_id, "segment": segment_id });
        let aesjson = self.aes_encrypt(&raw_payload)?;

        let final_payload = serde_json::json!({
            "aesjson": aesjson,
            "authorization": format!("bearer{}", self.jwt)
        });

        let res: Value = self.client.post(url).headers(self.get_headers()).json(&final_payload).send().await?.json().await?;
        if res["code"] == 1 {
            Ok(res["msg"].as_str().unwrap_or("成功").to_string())
        } else {
            Err(anyhow!("预约失败: {}", res["msg"]))
        }
    }
}

/// 登录流程入口，返回包含会话状态的客户端
pub async fn login_zju(username: &str, password: &str) -> Result<ZjuClient> {
    let client = reqwest::Client::builder().cookie_store(true).build()?;
    let ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";
    let login_url = "https://zjuam.zju.edu.cn/cas/login?service=https%3A%2F%2Fbooking.lib.zju.edu.cn%2Fapi%2Fcas%2Fcas";
    
    // 1. 获取 execution 和公钥
    let html = client.get(login_url).header(USER_AGENT, ua).send().await?.text().await?;
    let execution = html.split("name=\"execution\" value=\"").nth(1)
        .and_then(|s| s.split("\"").next())
        .ok_or(anyhow!("找不到 execution"))?;

    let pub_res: Value = client.get("https://zjuam.zju.edu.cn/cas/v2/getPubKey").header(USER_AGENT, ua).send().await?.json().await?;
    let (modulus, exponent) = (pub_res["modulus"].as_str().unwrap(), pub_res["exponent"].as_str().unwrap());

    // 2. RSA 纯数学加密
    let m = BigUint::from_bytes_be(password.as_bytes());
    let e = BigUint::parse_bytes(exponent.as_bytes(), 16).unwrap();
    let n = BigUint::parse_bytes(modulus.as_bytes(), 16).unwrap();
    let c = m.modpow(&e, &n);
    let enc_pwd = format!("{:0>128x}", c);

    // 3. 提交登录表单 (禁止重定向截获 Ticket)
    let no_redirect_client = reqwest::Client::builder().cookie_store(true).redirect(reqwest::redirect::Policy::none()).build()?;
    let form = [("username", username), ("password", &enc_pwd), ("authcode", ""), ("execution", execution), ("_eventId", "submit")];
    let submit_res = no_redirect_client.post(login_url).header(USER_AGENT, ua).form(&form).send().await?;
    
    let location = submit_res.headers().get("location").ok_or(anyhow!("CAS 登录失败，密码可能错误"))?.to_str()?;

    // 4. 兑换业务 JWT
    let ticket_res = no_redirect_client.get(location).header(USER_AGENT, ua).send().await?;
    let h5_loc = ticket_res.headers().get("location").ok_or(anyhow!("重定向链断裂"))?.to_str()?;
    let cas_token = h5_loc.split("cas=").nth(1).and_then(|s| s.split('&').next()).unwrap();

    let jwt_res: Value = client.post("https://booking.lib.zju.edu.cn/api/cas/user")
        .header(USER_AGENT, ua).json(&serde_json::json!({"cas": cas_token})).send().await?.json().await?;
    
    if jwt_res["code"] == 1 {
        let jwt = jwt_res["member"]["token"].as_str().unwrap().to_string();
        Ok(ZjuClient { client, jwt })
    } else {
        Err(anyhow!("换取 JWT 失败"))
    }
}
