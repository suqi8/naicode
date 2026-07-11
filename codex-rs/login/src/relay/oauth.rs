//! naicode 浏览器 OAuth 登录（Authorization Code + PKCE S256 + 本地回环）。
//!
//! naicode 起本地回环 → 生成 state + code_verifier/challenge → 打开酸奶中转站
//! consent 页面 → 用户同意 → 短期授权码回调 → CLI 用 code + verifier 换取
//! 15 分钟 access token 与可轮换 refresh token → 以 RelayOAuthTokens 模式持久化。
//! 整个流程不创建、不返回、不保存长期 sk-；手动 API key 仍是独立备用入口。
//!
//! 全同步实现（tiny_http + reqwest blocking + webbrowser），不依赖 tokio，
//! 便于在 CLI 主流程里直接调用。

use crate::auth::AuthDotJson;
use crate::auth::AuthKeyringBackendKind;
use crate::auth::RelayOAuthTokens;
use crate::auth::save_auth;
use crate::relay::api::RELAY_BASE_URL;
use crate::relay::state::RelayState;
use base64::Engine;
use chrono::Utc;
use codex_config::types::AuthCredentialsStoreMode;
use codex_protocol::auth::AuthMode;
use serde::Deserialize;
use sha2::Digest;
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

/// 登录结果摘要（不含明文 token）。
#[derive(Debug, Clone)]
pub struct RelayOAuthSummary {
    pub group: String,
    pub model: String,
}

#[derive(Debug, Deserialize)]
struct RelayOAuthTokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: i64,
    session_id: String,
    #[serde(default)]
    account: Option<RelayOAuthAccount>,
}

#[derive(Debug, Deserialize)]
struct RelayOAuthAccount {
    id: serde_json::Value,
    #[serde(default)]
    name: Option<String>,
}

fn random_state() -> String {
    use rand::RngCore;
    let mut buf = [0u8; 32]; // 256 bit state
    rand::rng().fill_bytes(&mut buf);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

fn generate_pkce() -> (String, String) {
    use rand::RngCore;
    let mut buf = [0u8; 32];
    rand::rng().fill_bytes(&mut buf);
    let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf);
    let digest = sha2::Sha256::digest(verifier.as_bytes());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
    (verifier, challenge)
}

/// 从回调请求的查询串里取参数。
fn query_param(url_raw: &str, key: &str) -> Option<String> {
    let parsed = url::Url::parse(&format!("http://127.0.0.1{url_raw}")).ok()?;
    parsed
        .query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

/// 用一次性授权码 + PKCE verifier 向中转站换取 OAuth token pair。
fn exchange_code(
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<RelayOAuthTokenResponse, RelayOAuthError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| RelayOAuthError::Exchange(e.to_string()))?;
    let resp = client
        .post(format!("{RELAY_BASE_URL}/api/cli/oauth/token"))
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", "naicode-cli"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("code_verifier", code_verifier),
        ])
        .send()
        .map_err(|e| RelayOAuthError::Exchange(e.to_string()))?;
    let status = resp.status();
    let bytes = resp
        .bytes()
        .map_err(|e| RelayOAuthError::Exchange(e.to_string()))?;
    if !status.is_success() {
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
        let msg = body
            .get("error_description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("换取 OAuth 凭据失败");
        return Err(RelayOAuthError::Exchange(msg.to_string()));
    }
    serde_json::from_slice::<RelayOAuthTokenResponse>(&bytes)
        .map_err(|e| RelayOAuthError::Exchange(format!("OAuth 响应无效: {e}")))
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
    let server =
        tiny_http::Server::http("127.0.0.1:0").map_err(|e| RelayOAuthError::Bind(e.to_string()))?;
    let port = match server.server_addr() {
        tiny_http::ListenAddr::IP(addr) => addr.port(),
        #[allow(unreachable_patterns)]
        _ => return Err(RelayOAuthError::Bind("无法确定回环端口".to_string())),
    };
    let state = random_state();
    let (code_verifier, code_challenge) = generate_pkce();
    let callback = format!("http://127.0.0.1:{port}/callback");
    let device_name = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "naicode CLI".to_string());

    // 2) 拼 OAuth Authorization Code + PKCE 授权页 URL 并打开浏览器。
    let auth_url = format!(
        "{RELAY_BASE_URL}/cli-auth?response_type=code&client_id=naicode-cli&redirect_uri={}&state={}&code_challenge={}&code_challenge_method=S256&device_name={}",
        urlencoding::encode(&callback),
        urlencoding::encode(&state),
        urlencoding::encode(&code_challenge),
        urlencoding::encode(&device_name),
    );
    on_url(&auth_url);
    if let Err(e) = webbrowser::open(&auth_url) {
        // 打开失败不致命：用户可手动复制 URL。
        tracing::warn!("自动打开浏览器失败: {e}");
    }

    // 3) 等回调（带超时）。
    let (code, group_hint, model_hint) = wait_for_callback(&server, &state)?;

    // 4) 用授权码 + PKCE verifier 换短期 access / rotating refresh token。
    let token = exchange_code(&code, &callback, &code_verifier)?;
    let account_id = token.account.as_ref().map(|a| a.id.to_string());
    let account_name = token.account.as_ref().and_then(|a| a.name.clone());

    // 5) 以真实 RelayOAuthTokens 模式原子持久化，不再调用 login_with_api_key。
    let auth = AuthDotJson {
        auth_mode: Some(AuthMode::RelayOAuthTokens),
        openai_api_key: None,
        tokens: None,
        last_refresh: None,
        agent_identity: None,
        personal_access_token: None,
        bedrock_api_key: None,
        relay_oauth: Some(RelayOAuthTokens {
            access_token: token.access_token,
            refresh_token: token.refresh_token,
            expires_at: Utc::now().timestamp() + token.expires_in,
            session_id: token.session_id,
            account_id,
            account_name,
            device_name: Some(device_name),
        }),
    };
    save_auth(codex_home, &auth, store_mode, keyring)
        .map_err(|e| RelayOAuthError::Persist(e.to_string()))?;

    // 6) relay.json 只记非敏感 group/model 缓存，不存 token 或 browser cookie。
    let group = group_hint;
    let model = model_hint;
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
            let _ = request
                .respond(tiny_http::Response::from_string("Not Found").with_status_code(404));
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

/// 向浏览器返回与酸奶中转站授权页一致的简洁结果页。
/// 保持白底、细边框、柔和暖色强调，不使用夸张插画/渐变或 AI 风格文案。
fn respond_html(request: tiny_http::Request, title: &str, body: &str) {
    let success = title == "登录成功";
    let icon = if success { "✓" } else { "!" };
    let icon_class = if success { "success" } else { "error" };
    let html = format!(
        r#"<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title} · naicode</title>
<style>
:root{{--bg:#fafafa;--card:#fff;--text:#18181b;--muted:#71717a;--border:#e4e4e7;--accent:#f59e0b;--success:#16a34a;--error:#dc2626}}
*{{box-sizing:border-box}}
body{{margin:0;min-height:100vh;background:var(--bg);color:var(--text);font-family:ui-sans-serif,system-ui,-apple-system,"Segoe UI","Microsoft YaHei",sans-serif;display:grid;place-items:center;padding:24px}}
.card{{width:min(100%,420px);background:var(--card);border:1px solid var(--border);border-radius:14px;padding:32px;box-shadow:0 1px 2px rgba(0,0,0,.04)}}
.brand{{display:flex;align-items:center;gap:10px;margin-bottom:28px;font-size:14px;font-weight:600}}
.mark{{display:grid;place-items:center;width:28px;height:28px;border-radius:8px;background:#fef3c7;color:#92400e;font-weight:700}}
.status{{display:grid;place-items:center;width:44px;height:44px;border-radius:50%;font-size:22px;font-weight:600;margin-bottom:18px;background:#f4f4f5}}
.status.success{{color:var(--success);background:#f0fdf4}}
.status.error{{color:var(--error);background:#fef2f2}}
h1{{font-size:21px;line-height:1.3;margin:0 0 10px;font-weight:650;letter-spacing:-.01em}}
p{{font-size:14px;line-height:1.65;color:var(--muted);margin:0}}
.hint{{margin-top:24px;padding-top:20px;border-top:1px solid var(--border);font-size:12px;color:#a1a1aa}}
</style>
</head>
<body>
<main class="card">
  <div class="brand"><span class="mark">N</span><span>naicode</span></div>
  <div class="status {icon_class}" aria-hidden="true">{icon}</div>
  <h1>{title}</h1>
  <p>{body}</p>
  <div class="hint">现在可以关闭此页面并返回终端。</div>
</main>
</body>
</html>"#
    );
    let mut resp = tiny_http::Response::from_string(html);
    if let Ok(h) =
        tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..])
    {
        resp.add_header(h);
    }
    let _ = request.respond(resp);
}
