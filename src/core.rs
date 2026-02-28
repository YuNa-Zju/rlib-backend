use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
use anyhow::{anyhow, Result};
use base64::Engine;
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
    pub fn get_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/json, text/plain, */*"),
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            ORIGIN,
            HeaderValue::from_static("https://booking.lib.zju.edu.cn"),
        );
        headers.insert(
            REFERER,
            HeaderValue::from_static("https://booking.lib.zju.edu.cn/h5/index.html"),
        );
        headers.insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/145.0.0.0 Safari/537.36 Edg/145.0.0.0"));
        headers.insert(
            "X-Requested-With",
            HeaderValue::from_static("XMLHttpRequest"),
        );
        headers.insert("lang", HeaderValue::from_static("zh"));

        // JWT 是 base64 字符，安全 unwrap
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

    pub async fn fetch_segment(
        &self,
        area_id: &str,
        target_date: &str,
    ) -> Result<(String, String, String)> {
        let url = "https://booking.lib.zju.edu.cn/api/Seat/date";
        let payload = serde_json::json!({ "build_id": area_id, "authorization": format!("bearer{}", self.jwt) });

        let res: Value = self
            .client
            .post(url)
            .headers(self.get_headers())
            .json(&payload)
            .send()
            .await?
            .json()
            .await?;
        if res["code"] != 1 {
            return Err(anyhow!("获取时间段失败: {}", res["msg"]));
        }

        let days = res["data"]
            .as_array()
            .ok_or_else(|| anyhow!("解析时间段失败：数据格式非数组"))?;
        let target = days
            .iter()
            .find(|d| d["day"].as_str() == Some(target_date))
            .ok_or_else(|| anyhow!("未找到指定日期 ({}) 的开放时间段", target_date))?;

        let times = target["times"]
            .as_array()
            .ok_or_else(|| anyhow!("该日期时间段列表为空"))?;
        let first_time = times
            .first()
            .ok_or_else(|| anyhow!("获取首个时间段信息失败"))?;

        let seg_id = first_time["id"]
            .as_str()
            .ok_or_else(|| anyhow!("id 字段解析失败"))?
            .to_string();
        let start = first_time["start"]
            .as_str()
            .ok_or_else(|| anyhow!("start 字段解析失败"))?
            .to_string();
        let end = first_time["end"]
            .as_str()
            .ok_or_else(|| anyhow!("end 字段解析失败"))?
            .to_string();

        Ok((seg_id, start, end))
    }

    pub async fn fetch_free_seats(
        &self,
        area_id: &str,
        segment_id: &str,
        date: &str,
        start: &str,
        end: &str,
    ) -> Result<Vec<Value>> {
        let url = "https://booking.lib.zju.edu.cn/api/Seat/seat";
        let payload = serde_json::json!({
            "area": area_id, "segment": segment_id, "day": date,
            "startTime": start, "endTime": end, "authorization": format!("bearer{}", self.jwt)
        });

        let res: Value = self
            .client
            .post(url)
            .headers(self.get_headers())
            .json(&payload)
            .send()
            .await?
            .json()
            .await?;
        if res["code"] != 1 {
            return Err(anyhow!("获取具体座位失败: {}", res["msg"]));
        }

        let mut free_seats: Vec<Value> = res["data"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter(|s| s["status"].as_str() == Some("1"))
            .cloned()
            .collect();

        free_seats.sort_by_key(|s| s["no"].as_str().unwrap_or("").to_string());
        Ok(free_seats)
    }

    pub async fn confirm_booking(&self, seat_id: &str, segment_id: &str) -> Result<String> {
        let url = "https://booking.lib.zju.edu.cn/api/Seat/confirm";
        let raw_payload = serde_json::json!({ "seat_id": seat_id, "segment": segment_id });
        let aesjson = self.aes_encrypt(&raw_payload)?;

        let final_payload = serde_json::json!({
            "aesjson": aesjson,
            "authorization": format!("bearer{}", self.jwt)
        });

        let res: Value = self
            .client
            .post(url)
            .headers(self.get_headers())
            .json(&final_payload)
            .send()
            .await?
            .json()
            .await?;
        if res["code"] == 1 {
            Ok(res["msg"].as_str().unwrap_or("成功").to_string())
        } else {
            Err(anyhow!("预约失败: {}", res["msg"]))
        }
    }

    pub async fn fetch_subscriptions(&self) -> Result<Vec<Value>> {
        let url = "https://booking.lib.zju.edu.cn/api/index/subscribe";
        let payload = serde_json::json!({ "authorization": format!("bearer{}", self.jwt) });

        let res: Value = self
            .client
            .post(url)
            .headers(self.get_headers())
            .json(&payload)
            .send()
            .await?
            .json()
            .await?;
        if res["code"] == 1 {
            Ok(res["data"].as_array().cloned().unwrap_or_default())
        } else {
            Err(anyhow!("获取预约列表失败: {}", res["msg"]))
        }
    }

    pub async fn cancel_booking(&self, record_id: &str) -> Result<String> {
        let url = "https://booking.lib.zju.edu.cn/api/Space/cancel";
        let payload = serde_json::json!({
            "id": record_id,
            "authorization": format!("bearer{}", self.jwt)
        });

        let res: Value = self
            .client
            .post(url)
            .headers(self.get_headers())
            .json(&payload)
            .send()
            .await?
            .json()
            .await?;
        if res["code"] == 1 {
            Ok("取消预约成功".to_string())
        } else {
            Err(anyhow!("取消预约失败: {}", res["msg"]))
        }
    }
}

pub async fn login_zju(username: &str, password: &str) -> Result<ZjuClient> {
    // 【核心修复】全局只使用一个 Client！
    // 开启 Cookie 维持，并全局禁止重定向 (完美复刻 Python 的 allow_redirects=False)
    let client = reqwest::Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()?;

    let ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/145.0.0.0 Safari/537.36 Edg/145.0.0.0";
    let login_url = "https://zjuam.zju.edu.cn/cas/login?service=https%3A%2F%2Fbooking.lib.zju.edu.cn%2Fapi%2Fcas%2Fcas";

    // 1. 获取 execution (此时 client 自动保存了初始的 JSESSIONID)
    let html = client
        .get(login_url)
        .header(USER_AGENT, ua)
        .send()
        .await?
        .text()
        .await?;
    let execution = html
        .split("name=\"execution\" value=\"")
        .nth(1)
        .and_then(|s| s.split("\"").next())
        .ok_or_else(|| {
            anyhow!("无法从登录页面提取 execution 参数，页面结构可能已更改或网络受限")
        })?;

    // 2. 获取 RSA 公钥
    let pub_res: Value = client
        .get("https://zjuam.zju.edu.cn/cas/v2/getPubKey")
        .header(USER_AGENT, ua)
        .send()
        .await?
        .json()
        .await?;
    let modulus = pub_res["modulus"]
        .as_str()
        .ok_or_else(|| anyhow!("获取公钥失败：缺少 modulus"))?;
    let exponent = pub_res["exponent"]
        .as_str()
        .ok_or_else(|| anyhow!("获取公钥失败：缺少 exponent"))?;

    // 3. RSA 加密
    let m = BigUint::from_bytes_be(password.as_bytes());
    let e = BigUint::parse_bytes(exponent.as_bytes(), 16)
        .ok_or_else(|| anyhow!("解析 exponent 失败"))?;
    let n =
        BigUint::parse_bytes(modulus.as_bytes(), 16).ok_or_else(|| anyhow!("解析 modulus 失败"))?;
    let c = m.modpow(&e, &n);
    let enc_pwd = format!("{:0>128x}", c);

    // 4. 提交登录表单 (带着初始的 JSESSIONID 一起提交，这次绝对合法)
    let form = [
        ("username", username),
        ("password", &enc_pwd),
        ("authcode", ""),
        ("execution", execution),
        ("_eventId", "submit"),
    ];
    let submit_res = client
        .post(login_url)
        .header(USER_AGENT, ua)
        .form(&form)
        .send()
        .await?;

    let location = submit_res
        .headers()
        .get("location")
        .ok_or_else(|| anyhow!("CAS 登录未发生重定向，请检查账号密码"))?
        .to_str()
        .map_err(|_| anyhow!("Location 头部格式异常"))?;

    // 增加一层防护：确保我们是被重定向到了图书馆，而不是重定向回了带 error 参数的登录页
    if location.contains("login") {
        return Err(anyhow!("CAS 身份认证被拒，账号或密码错误。"));
    }

    // 5. 追寻重定向链路并提取 CAS Token (安全提取)
    let ticket_res = client.get(location).header(USER_AGENT, ua).send().await?;

    let mut cas_token = String::new();
    if let Some(h5_loc) = ticket_res
        .headers()
        .get("location")
        .and_then(|l| l.to_str().ok())
    {
        if let Some(t) = h5_loc
            .split("cas=")
            .nth(1)
            .and_then(|s| s.split('&').next())
        {
            cas_token = t.to_string();
        }
    }

    // 补偿机制
    if cas_token.is_empty() {
        let res2 = client
            .get("https://booking.lib.zju.edu.cn/api/cas/cas")
            .header(USER_AGENT, ua)
            .send()
            .await?;
        if let Some(loc2) = res2.headers().get("location").and_then(|l| l.to_str().ok()) {
            if let Some(t) = loc2.split("cas=").nth(1).and_then(|s| s.split('&').next()) {
                cas_token = t.to_string();
            }
        }
    }

    if cas_token.is_empty() {
        return Err(anyhow!(
            "重定向认证链条断裂：无法提取到临时的 cas token。系统可能进行了安全升级。"
        ));
    }

    // 6. 兑换业务 JWT
    let jwt_res: Value = client
        .post("https://booking.lib.zju.edu.cn/api/cas/user")
        .header(USER_AGENT, ua)
        .json(&serde_json::json!({ "cas": cas_token }))
        .send()
        .await?
        .json()
        .await?;

    if jwt_res["code"] == 1 {
        let jwt = jwt_res["member"]["token"]
            .as_str()
            .ok_or_else(|| anyhow!("响应报文中缺失 token 字段"))?
            .to_string();
        Ok(ZjuClient { client, jwt })
    } else {
        Err(anyhow!("业务层认证被拒: {}", jwt_res["msg"]))
    }
}
