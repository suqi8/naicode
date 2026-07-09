//! naicode 浏览器 OAuth 登录（本地回环）。
//!
//! 纯客户端做不到"借用浏览器已登录中转站的 session"（跨域），因此授权页
//! 挂在中转站域名下：naicode 起本地回环 → 打开浏览器到中转站授权页 →
//! 用户（已登录）选分组/模型并同意 → 中转站后端在其账号下找/建唯一的
//! `naicode` key、生成一次性授权码、重定向回本地回环 → naicode 用该码
//! 向后端换取 `sk-<key>`（后端→CLI，明文不经浏览器地址栏）→ 写 auth.json。
//!
//! 全同步实现（tiny_http + reqwest blocking + webbrowser），不依赖 tokio，
//! 便于在 CLI 主流程里直接调用。

use crate::auth::AuthKeyringBackendKind;
use crate::auth::login_with_api_key;
use crate::relay::api::RELAY_BASE_URL;
use crate::relay::state::RelayState;
use codex_config::types::AuthCredentialsStoreMode;
use std::path::Path;
use std::time::Duration;

/// 等待用户在浏览器完成授权的最长时间。
const OAUTH_TIMEOUT: Duration = Duration::from_secs(300);

/// OAuth 登录错误。
#[derive(Debug, thiserror::Error)]
pub enum RelayOAuthError {
    #[error("启动本地回环失败: {0}")]
    Bind(String),
    #[error("打开浏览器失败: {0}")]
    Browser(String),
    #[error("授权超时或被取消")]
    Timeout,
    #[error("state 校验失败（可能遭遇 CSRF）")]
    StateMismatch,
    #[error("授权失败: {0}")]
    Denied(String),
    #[error("换取凭据失败: {0}")]
    Exchange(String),
    #[error("写入本地凭据失败: {0}")]
    Persist(String),
}

/// 登录结果摘要（不含明文 key）。
#[derive(Debug, Clone)]
pub struct RelayOAuthSummary {
    pub group: String,
    pub model: String,
}

fn random_state() -> String {
    use rand::RngCore;
    let mut buf = [0u8; 16];
    rand::rng().fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

/// 从回调请求的查询串里取参数。
fn query_param(url_raw: &str, key: &str) -> Option<String> {
    let parsed = url::Url::parse(&format!("http://127.0.0.1{url_raw}")).ok()?;
    parsed
        .query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

/// 用一次性授权码向中转站换取凭据（后端→CLI，同步 blocking）。
///
/// 返回 (sk_key, group, model)。
fn exchange_code(code: &str) -> Result<(String, String, String), RelayOAuthError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| RelayOAuthError::Exchange(e.to_string()))?;
    let resp = client
        .post(format!("{RELAY_BASE_URL}/api/cli/token"))
        .json(&serde_json::json!({ "code": code }))
        .send()
        .map_err(|e| RelayOAuthError::Exchange(e.to_string()))?;
    let body: serde_json::Value = resp
        .json()
        .map_err(|e| RelayOAuthError::Exchange(e.to_string()))?;
    if !body
        .get("success")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        let msg = body
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("换码失败")
            .to_string();
        return Err(RelayOAuthError::Exchange(msg));
    }
    let data = body.get("data").cloned().unwrap_or(serde_json::Value::Null);
    let key = data
        .get("key")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RelayOAuthError::Exchange("响应缺少 key".to_string()))?
        .to_string();
    let group = data
        .get("group")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();
    let model = data
        .get("model")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();
    Ok((key, group, model))
}

/// 运行一次浏览器 OAuth 登录，阻塞直到完成或超时。
///
/// `on_url` 回调用于向用户展示授权 URL（例如打印到终端）。
pub fn run_relay_oauth_login(
    codex_home: &Path,
    store_mode: AuthCredentialsStoreMode,
    keyring: AuthKeyringBackendKind,
    mut on_url: impl FnMut(&str),
) -> Result<RelayOAuthSummary, RelayOAuthError> {
    // 1) 起本地回环（随机端口）。
    let server = tiny_http::Server::http("127.0.0.1:0")
        .map_err(|e| RelayOAuthError::Bind(e.to_string()))?;
    let port = match server.server_addr() {
        tiny_http::ListenAddr::IP(addr) => addr.port(),
        #[allow(unreachable_patterns)]
        _ => return Err(RelayOAuthError::Bind("无法确定回环端口".to_string())),
    };
    let state = random_state();
    let callback = format!("http://127.0.0.1:{port}/callback");

    // 2) 拼授权页 URL 并打开浏览器。
    let auth_url = format!(
        "{RELAY_BASE_URL}/cli-auth?callback={}&state={}",
        urlencoding::encode(&callback),
        urlencoding::encode(&state),
    );
    on_url(&auth_url);
    if let Err(e) = webbrowser::open(&auth_url) {
        // 打开失败不致命：用户可手动复制 URL。
        tracing::warn!("自动打开浏览器失败: {e}");
    }

    // 3) 等回调（带超时）。
    let (code, group_hint, model_hint) = wait_for_callback(&server, &state)?;

    // 4) 换码取 sk（后端→CLI）。
    let (sk, group, model) = exchange_code(&code)?;
    let group = if group.is_empty() { group_hint } else { group };
    let model = if model.is_empty() { model_hint } else { model };

    // 5) 写 auth.json（复用 codex 高层接口）。
    login_with_api_key(codex_home, &sk, store_mode, keyring)
        .map_err(|e| RelayOAuthError::Persist(e.to_string()))?;

    // 6) 存 relay 会话态（OAuth 模式无 session cookie，只记 group/model）。
    let mut st = RelayState::load(codex_home).unwrap_or_default();
    st.version = 1;
    if !group.is_empty() {
        st.group = Some(group.clone());
    }
    if !model.is_empty() {
        st.model = Some(model.clone());
    }
    let _ = st.save(codex_home);

    Ok(RelayOAuthSummary { group, model })
}

/// 阻塞等待回环的 `/callback` 请求，校验 state，返回 (code, group, model)。
///
/// group/model 是授权页可能顺带回传的提示值（后端换码结果为准，这里作兜底）。
fn wait_for_callback(
    server: &tiny_http::Server,
    expected_state: &str,
) -> Result<(String, String, String), RelayOAuthError> {
    loop {
        let request = match server.recv_timeout(OAUTH_TIMEOUT) {
            Ok(Some(req)) => req,
            Ok(None) => return Err(RelayOAuthError::Timeout),
            Err(e) => return Err(RelayOAuthError::Bind(e.to_string())),
        };
        let url_raw = request.url().to_string();

        // 只认 /callback；其余路径回 404 继续等。
        if !url_raw.starts_with("/callback") {
            let _ = request.respond(
                tiny_http::Response::from_string("Not Found").with_status_code(404),
            );
            continue;
        }

        // 授权被拒绝。
        if let Some(err) = query_param(&url_raw, "error") {
            respond_html(request, "授权失败", "你可以关闭本页并回到终端。");
            return Err(RelayOAuthError::Denied(err));
        }

        let state = query_param(&url_raw, "state").unwrap_or_default();
        if state != expected_state {
            respond_html(request, "校验失败", "state 不匹配，请重试。");
            return Err(RelayOAuthError::StateMismatch);
        }

        let code = match query_param(&url_raw, "code") {
            Some(c) if !c.is_empty() => c,
            _ => {
                respond_html(request, "缺少授权码", "请回到终端重试。");
                return Err(RelayOAuthError::Denied("缺少授权码".to_string()));
            }
        };
        let group = query_param(&url_raw, "group").unwrap_or_default();
        let model = query_param(&url_raw, "model").unwrap_or_default();

        respond_html(
            request,
            "登录成功",
            "naicode 已连接到酸奶中转站，你可以关闭本页回到终端。",
        );
        return Ok((code, group, model));
    }
}

/// 向浏览器回一个简单的中文 HTML 结果页。
fn respond_html(request: tiny_http::Request, title: &str, body: &str) {
    let html = format!(
        "<!doctype html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\">\
<title>{title}</title><style>body{{font-family:system-ui,sans-serif;\
background:#faf8f3;color:#333;display:flex;align-items:center;\
justify-content:center;height:100vh;margin:0}}.card{{text-align:center;\
padding:2rem 3rem;background:#fff;border-radius:16px;\
box-shadow:0 4px 24px rgba(0,0,0,.08)}}h1{{color:#e8a04b;margin:0 0 .5rem}}\
</style></head><body><div class=\"card\"><h1>{title}</h1><p>{body}</p>\
</div></body></html>"
    );
    let mut resp = tiny_http::Response::from_string(html);
    if let Ok(h) = tiny_http::Header::from_bytes(
        &b"Content-Type"[..],
        &b"text/html; charset=utf-8"[..],
    ) {
        resp.add_header(h);
    }
    let _ = request.respond(resp);
}
