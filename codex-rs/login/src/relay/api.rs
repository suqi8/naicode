//! 酸奶中转站（new-api）HTTP 账号层。
//!
//! 从姊妹项目 cc-switch 的 `newapi_auth.rs` 翻译而来，去掉桌面端的
//! Tauri/RwLock 状态机，改为 CLI 用的无状态 `RelayClient`：调用方持有
//! session cookie + user_id，每次请求手动注入 `Cookie: session=X` +
//! `New-Api-User: <uid>` 双头（后者缺失会 401）。
//!
//! ## 中转站 API 契约
//! - 登录：`POST /api/user/login` → Set-Cookie `session=X` + body `data.id`
//! - 信封统一：`{ success: bool, message: string, data: any }`
//! - 建 key 三步：`POST /api/token/` → `GET /api/token/?p=..` 找 id →
//!   `POST /api/token/<id>/key` 取 48 位明文，拼 `sk-<key>`
//! - 换分组：`PUT /api/token/` 更新该 token 的 `group` 字段（不新建 key）
//! - 金额按人民币，不做美元汇率换算。

use serde::Deserialize;
use serde::Serialize;

/// 中转站根地址（写死）。注意：provider base_url 另带 `/v1`，这里是控制台 API 根。
pub const RELAY_BASE_URL: &str = "https://closedai.kylenqaq.com";

/// naicode 专用 key 的固定名字。首登只建这一个，换分组只 PUT 它。
pub const NAICODE_TOKEN_NAME: &str = "naicode";

const USER_AGENT: &str = "naicode-cli";

/// 中转站账号层错误。
#[derive(Debug, thiserror::Error)]
pub enum RelayError {
    #[error("未登录或会话已过期")]
    Unauthenticated,
    #[error("登录失败: {0}")]
    LoginFailed(String),
    #[error("需要两步验证")]
    Require2FA,
    #[error("网络错误: {0}")]
    Network(String),
    #[error("解析错误: {0}")]
    Parse(String),
    #[error("接口返回错误: {0}")]
    Api(String),
}

impl From<reqwest::Error> for RelayError {
    fn from(err: reqwest::Error) -> Self {
        RelayError::Network(err.to_string())
    }
}

/// 一次登录拿到的凭据（会话式）。
#[derive(Debug, Clone)]
pub struct RelaySession {
    /// session cookie 的 VALUE（不含 "session=" 前缀）。
    pub session_cookie: String,
    /// new-api user id，用于 `New-Api-User` 头。
    pub user_id: i64,
}

/// 令牌信息（列表项）。字段与 new-api 对齐，多余字段忽略。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenInfo {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub status: i64,
    #[serde(default)]
    pub unlimited_quota: bool,
    #[serde(default)]
    pub remain_quota: i64,
    #[serde(default)]
    pub used_quota: i64,
}

/// 用户信息（/api/user/self 的 data，仅取常用字段）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserSelf {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub quota: i64,
    #[serde(default)]
    pub used_quota: i64,
}

/// 中转站客户端：持有一个带 cookie jar 的 reqwest client。
///
/// cookie jar 让登录拿到的 session 在同一 client 的后续请求里自动带上，
/// 换分组不必重登。`New-Api-User` 头仍需每次手动注入（authed 里做）。
pub struct RelayClient {
    base_url: String,
    http: reqwest::Client,
}

impl RelayClient {
    /// 用写死的中转站地址创建。
    pub fn new() -> Result<Self, RelayError> {
        Self::with_base_url(RELAY_BASE_URL)
    }

    pub fn with_base_url(base_url: &str) -> Result<Self, RelayError> {
        // 不用 reqwest 的 cookie_store（未启用 `cookies` feature）；session 由
        // RelaySession 显式持有，每个请求在 authed() 里手动注入 Cookie 头，
        // 与 cc-switch 桌面端一致。
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .map_err(RelayError::from)?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    /// 构造带认证头的 RequestBuilder（注入 New-Api-User；session 由 cookie jar 带）。
    fn authed(
        &self,
        method: reqwest::Method,
        path: &str,
        session: &RelaySession,
    ) -> reqwest::RequestBuilder {
        self.http
            .request(method, self.url(path))
            .header("New-Api-User", session.user_id.to_string())
            .header("Cookie", format!("session={}", session.session_cookie))
    }

    /// 账号密码登录。返回正式 session；若需 2FA 返回 `RelayError::Require2FA`。
    pub async fn login(&self, username: &str, password: &str) -> Result<RelaySession, RelayError> {
        let resp = self
            .http
            .post(self.url("/api/user/login"))
            .json(&serde_json::json!({ "username": username, "password": password }))
            .send()
            .await?;
        let cookie = extract_session_cookie(&resp);
        let status = resp.status();
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| RelayError::Parse(e.to_string()))?;
        if !envelope_ok(&body) {
            let msg = envelope_message(&body, "登录失败");
            return Err(RelayError::LoginFailed(format!("{status}: {msg}")));
        }
        let data = body.get("data").cloned().unwrap_or(serde_json::Value::Null);
        if data
            .get("require_2fa")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            return Err(RelayError::Require2FA);
        }
        let user_id = data
            .get("id")
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| RelayError::LoginFailed("登录响应缺少用户 id".to_string()))?;
        let session_cookie = cookie
            .ok_or_else(|| RelayError::LoginFailed("登录响应未返回 session cookie".to_string()))?;
        Ok(RelaySession {
            session_cookie,
            user_id,
        })
    }

    /// 两步验证补验（带上登录时的 pending session cookie）。
    pub async fn login_2fa(
        &self,
        pending: &RelaySession,
        code: &str,
    ) -> Result<RelaySession, RelayError> {
        let resp = self
            .authed(reqwest::Method::POST, "/api/user/login/2fa", pending)
            .json(&serde_json::json!({ "code": code }))
            .send()
            .await?;
        let cookie = extract_session_cookie(&resp)
            .unwrap_or_else(|| pending.session_cookie.clone());
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| RelayError::Parse(e.to_string()))?;
        if !envelope_ok(&body) {
            return Err(RelayError::LoginFailed(envelope_message(&body, "两步验证失败")));
        }
        let data = body.get("data").cloned().unwrap_or(serde_json::Value::Null);
        let user_id = data
            .get("id")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(pending.user_id);
        Ok(RelaySession {
            session_cookie: cookie,
            user_id,
        })
    }

    /// 建 key：`POST /api/token/`（只返成功，不返明文）。
    pub async fn create_token(
        &self,
        session: &RelaySession,
        name: &str,
        group: &str,
    ) -> Result<(), RelayError> {
        let resp = self
            .authed(reqwest::Method::POST, "/api/token/", session)
            .json(&serde_json::json!({
                "name": name,
                "group": group,
                "expired_time": -1,
                "remain_quota": 0,
                "unlimited_quota": true,
                "model_limits_enabled": false,
                "model_limits": "",
                "allow_ips": "",
            }))
            .send()
            .await?;
        parse_envelope(resp).await.map(|_| ())
    }

    /// 列 token，分页。返回列表用于按 name 找 id。
    pub async fn list_tokens(
        &self,
        session: &RelaySession,
        page: i64,
        size: i64,
    ) -> Result<Vec<TokenInfo>, RelayError> {
        let path = format!("/api/token/?p={page}&page_size={size}");
        let resp = self
            .authed(reqwest::Method::GET, &path, session)
            .send()
            .await?;
        let data = parse_envelope(resp).await?;
        // data 可能是数组，或 {items:[...]} 分页对象。
        let items = if data.is_array() {
            data
        } else {
            data.get("items")
                .cloned()
                .unwrap_or(serde_json::Value::Array(vec![]))
        };
        serde_json::from_value(items).map_err(|e| RelayError::Parse(e.to_string()))
    }

    /// 取某 token 的 48 位明文 key。
    pub async fn get_token_key(
        &self,
        session: &RelaySession,
        id: i64,
    ) -> Result<String, RelayError> {
        let resp = self
            .authed(reqwest::Method::POST, &format!("/api/token/{id}/key"), session)
            .send()
            .await?;
        let data = parse_envelope(resp).await?;
        data.get("key")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| RelayError::Parse("响应缺少 key 字段".to_string()))
    }

    /// 换分组：`PUT /api/token/` 更新该 token 的 group（不新建 key）。
    pub async fn update_token_group(
        &self,
        session: &RelaySession,
        token: &TokenInfo,
        new_group: &str,
    ) -> Result<(), RelayError> {
        let resp = self
            .authed(reqwest::Method::PUT, "/api/token/", session)
            .json(&serde_json::json!({
                "id": token.id,
                "name": token.name,
                "group": new_group,
                "expired_time": -1,
                "remain_quota": token.remain_quota,
                "unlimited_quota": token.unlimited_quota,
                "model_limits_enabled": false,
                "model_limits": "",
                "allow_ips": "",
                "status": token.status,
            }))
            .send()
            .await?;
        parse_envelope(resp).await.map(|_| ())
    }

    /// 当前用户信息（余额/分组）。
    pub async fn get_self(&self, session: &RelaySession) -> Result<UserSelf, RelayError> {
        let resp = self
            .authed(reqwest::Method::GET, "/api/user/self", session)
            .send()
            .await?;
        let data = parse_envelope(resp).await?;
        serde_json::from_value(data).map_err(|e| RelayError::Parse(e.to_string()))
    }

    /// 可用分组列表（`GET /api/user/self/groups`）。返回原始 JSON，
    /// 通常是 `{ 分组名: { desc, ratio, ... } }` 映射。
    pub async fn get_groups(&self, session: &RelaySession) -> Result<serde_json::Value, RelayError> {
        let resp = self
            .authed(reqwest::Method::GET, "/api/user/self/groups", session)
            .send()
            .await?;
        parse_envelope(resp).await
    }

    /// 可用模型列表（`GET /api/user/models`）。
    pub async fn get_models(&self, session: &RelaySession) -> Result<Vec<String>, RelayError> {
        let resp = self
            .authed(reqwest::Method::GET, "/api/user/models", session)
            .send()
            .await?;
        let data = parse_envelope(resp).await?;
        serde_json::from_value(data).map_err(|e| RelayError::Parse(e.to_string()))
    }

    /// 定价（`GET /api/pricing`）。注意 group_ratio / usable_group 是 data
    /// 的**同级字段**，返回完整 body（不只取 data）。
    pub async fn get_pricing(&self, session: &RelaySession) -> Result<serde_json::Value, RelayError> {
        let resp = self
            .authed(reqwest::Method::GET, "/api/pricing", session)
            .send()
            .await?;
        parse_envelope_full(resp).await
    }
}

// ============================================================================
// 辅助函数
// ============================================================================

/// 从响应头提取 session cookie 的 VALUE。
fn extract_session_cookie(resp: &reqwest::Response) -> Option<String> {
    for hv in resp.headers().get_all(reqwest::header::SET_COOKIE) {
        if let Ok(s) = hv.to_str()
            && let Some(rest) = s.strip_prefix("session=")
        {
            let val = rest.split(';').next().unwrap_or("").to_string();
            if !val.is_empty() {
                return Some(val);
            }
        }
    }
    None
}

fn envelope_ok(body: &serde_json::Value) -> bool {
    body.get("success")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn envelope_message(body: &serde_json::Value, fallback: &str) -> String {
    body.get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(fallback)
        .to_string()
}

/// 解析 new-api 信封 `{success,message,data}` → 返回 data。
async fn parse_envelope(resp: reqwest::Response) -> Result<serde_json::Value, RelayError> {
    if resp.status().as_u16() == 401 {
        return Err(RelayError::Unauthenticated);
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| RelayError::Parse(e.to_string()))?;
    if !envelope_ok(&body) {
        return Err(RelayError::Api(envelope_message(&body, "接口返回失败")));
    }
    Ok(body.get("data").cloned().unwrap_or(serde_json::Value::Null))
}

/// 同上但返回**完整 body**（保留 data 同级字段，用于 /api/pricing）。
async fn parse_envelope_full(resp: reqwest::Response) -> Result<serde_json::Value, RelayError> {
    if resp.status().as_u16() == 401 {
        return Err(RelayError::Unauthenticated);
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| RelayError::Parse(e.to_string()))?;
    if !envelope_ok(&body) {
        return Err(RelayError::Api(envelope_message(&body, "接口返回失败")));
    }
    Ok(body)
}
