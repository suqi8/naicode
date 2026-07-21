//! 酸奶中转站账号编排层：把 HTTP 调用（[`api`]）、会话态旁路存储
//! （[`state`]）与 codex 的 `auth.json` 写入（`login_with_api_key`）串起来。
//!
//! 对外两个入口：
//! - [`relay_login`]：登录 → 只建/复用一个名为 `naicode` 的 key → 取明文拼
//!   `sk-` → 写 auth.json → 存会话态。
//! - [`relay_switch_group`]：读会话态，`PUT /api/token/` 换那个 key 的分组，
//!   **不新建 key、不重写 auth.json**。

pub mod api;
pub mod notice;
pub mod oauth;
pub mod pricing;
pub mod state;

use crate::auth::AuthKeyringBackendKind;
use crate::auth::AuthManager;
use crate::auth::RelayRequestError;
use crate::auth::login_with_api_key;
use api::NAICODE_TOKEN_NAME;
use api::RelayClient;
use api::RelayError;
use api::RelaySession;
use api::TokenInfo;
use codex_config::types::AuthCredentialsStoreMode;
use state::RelayState;
use std::path::Path;

/// 编排层错误。
#[derive(Debug, thiserror::Error)]
pub enum RelayLoginError {
    #[error(transparent)]
    Relay(#[from] RelayError),
    #[error("IO 错误: {0}")]
    Io(String),
    #[error("尚未登录，请先运行 `naicode login`")]
    NotLoggedIn,
    #[error("找不到 naicode 令牌，请重新登录")]
    TokenMissing,
    #[error(transparent)]
    OAuthRequest(#[from] RelayRequestError),
}

impl From<std::io::Error> for RelayLoginError {
    fn from(err: std::io::Error) -> Self {
        RelayLoginError::Io(err.to_string())
    }
}

/// 登录成功后的摘要（供 CLI 展示，不含明文 key）。
#[derive(Debug, Clone)]
pub struct RelayLoginSummary {
    pub username: String,
    pub group: String,
    pub token_id: i64,
}

/// 在指定 session 下确保存在唯一的 naicode key，返回其 TokenInfo。
///
/// 已存在则复用（**不重复建**），不存在才 `create_token`。
async fn ensure_naicode_token(
    client: &RelayClient,
    session: &RelaySession,
    group: &str,
) -> Result<TokenInfo, RelayLoginError> {
    if let Some(tok) = find_naicode_token(client, session).await? {
        return Ok(tok);
    }
    client
        .create_token(session, NAICODE_TOKEN_NAME, group)
        .await?;
    find_naicode_token(client, session)
        .await?
        .ok_or(RelayLoginError::TokenMissing)
}

/// 翻页查找名为 naicode 的 token（最多扫若干页，避免无限翻）。
async fn find_naicode_token(
    client: &RelayClient,
    session: &RelaySession,
) -> Result<Option<TokenInfo>, RelayLoginError> {
    const PAGE_SIZE: i64 = 100;
    const MAX_PAGES: i64 = 20;
    for page in 1..=MAX_PAGES {
        let tokens = client.list_tokens(session, page, PAGE_SIZE).await?;
        let empty = tokens.is_empty();
        if let Some(tok) = tokens.into_iter().find(|t| t.name == NAICODE_TOKEN_NAME) {
            return Ok(Some(tok));
        }
        if empty {
            break;
        }
    }
    Ok(None)
}

/// 用已拿到的 session 完成建 key + 写 auth.json + 存会话态。
///
/// 这是 OAuth 与账密两条登录路径的公共收尾：拿到 session 后都走这里。
/// `group` 为首建 key 时的分组；`store_mode`/`keyring` 决定 auth.json 落盘方式。
pub async fn finalize_relay_session(
    codex_home: &Path,
    session: RelaySession,
    username: Option<String>,
    group: &str,
    store_mode: AuthCredentialsStoreMode,
    keyring: AuthKeyringBackendKind,
) -> Result<RelayLoginSummary, RelayLoginError> {
    let client = RelayClient::new()?;
    let token = ensure_naicode_token(&client, &session, group).await?;
    let plaintext = client.get_token_key(&session, token.id).await?;
    let sk = format!("sk-{plaintext}");

    // 写 auth.json（复用 codex 唯一的高层接口，不自己写文件）。
    login_with_api_key(codex_home, &sk, store_mode, keyring)?;
    // 明文 key 用完即弃，不驻留、不入 state。
    drop(plaintext);

    let effective_group = if token.group.is_empty() {
        group.to_string()
    } else {
        token.group.clone()
    };

    let mut st = RelayState::load(codex_home)?;
    st.set_session(username.clone(), &session);
    st.token_id = Some(token.id);
    st.group = Some(effective_group.clone());
    st.save(codex_home)?;

    Ok(RelayLoginSummary {
        username: username.unwrap_or_default(),
        group: effective_group,
        token_id: token.id,
    })
}

/// 账密登录入口：登录 → finalize。2FA 场景返回 `RelayError::Require2FA`，
/// 调用方应改用 [`relay_login_2fa`]。
pub async fn relay_login(
    codex_home: &Path,
    username: &str,
    password: &str,
    group: &str,
    store_mode: AuthCredentialsStoreMode,
    keyring: AuthKeyringBackendKind,
) -> Result<RelayLoginSummary, RelayLoginError> {
    let client = RelayClient::new()?;
    let session = client.login(username, password).await?;
    finalize_relay_session(
        codex_home,
        session,
        Some(username.to_string()),
        group,
        store_mode,
        keyring,
    )
    .await
}

pub async fn relay_switch_group_with_manager(
    auth_manager: std::sync::Arc<AuthManager>,
    new_group: &str,
) -> Result<(), RelayLoginError> {
    let client = reqwest::Client::new();
    let url = format!("{}/api/cli/oauth/group", api::RELAY_BASE_URL);
    let payload = serde_json::json!({ "group": new_group });
    let response = auth_manager
        .execute_relay_request(|token| client.put(&url).bearer_auth(token).json(&payload))
        .await?;
    if !response.status().is_success() {
        return Err(RelayLoginError::Io(format!(
            "[group_switch_http] OAuth 换组失败: {}",
            response.status()
        )));
    }
    Ok(())
}

/// 换分组：OAuth 模式使用 AuthManager 的统一授权执行器；旧账号/session 模式保留
/// 原 PUT /api/token/。兼容入口在远端成功后提交 relay.json 缓存。
pub async fn relay_switch_group(codex_home: &Path, new_group: &str) -> Result<(), RelayLoginError> {
    let auth_manager = AuthManager::shared(
        codex_home.to_path_buf(),
        false,
        AuthCredentialsStoreMode::Auto,
        None,
        None,
        AuthKeyringBackendKind::default(),
        None,
    )
    .await;
    if auth_manager.get_api_auth_mode() == Some(codex_protocol::auth::AuthMode::RelayOAuthTokens) {
        relay_switch_group_with_manager(auth_manager, new_group).await?;
        let mut state = RelayState::load(codex_home)?;
        state.group = Some(new_group.to_string());
        state.save(codex_home)?;
        return Ok(());
    }

    // 兼容旧 session/key 登录。
    let mut st = RelayState::load(codex_home)?;
    let session = st.session().ok_or(RelayLoginError::NotLoggedIn)?;
    let client = RelayClient::new()?;

    // 优先用 state 里的 token_id 定位；缺失则回退按 name 查找。
    let token = match st.token_id {
        Some(id) => {
            // 拉一份最新 TokenInfo（update 需要带完整字段）。
            find_naicode_token(&client, &session)
                .await?
                .filter(|t| t.id == id)
                .or_else(|| {
                    Some(TokenInfo {
                        id,
                        name: NAICODE_TOKEN_NAME.to_string(),
                        ..Default::default()
                    })
                })
                .ok_or(RelayLoginError::TokenMissing)?
        }
        None => find_naicode_token(&client, &session)
            .await?
            .ok_or(RelayLoginError::TokenMissing)?,
    };

    client
        .update_token_group(&session, &token, new_group)
        .await?;
    st.group = Some(new_group.to_string());
    st.save(codex_home)?;
    Ok(())
}

/// 仅执行远端换组 PUT，不写 relay.json 缓存。
///
/// OAuth 模式直接委托给 [`relay_switch_group_with_manager`]；旧账密模式执行
/// `PUT /api/token/` 换组但不持久化本地状态，由调用方在合适时机提交缓存。
/// `codex_home` 仅在旧账密模式下使用（加载 RelayState 获取 session/token_id）。
pub async fn relay_switch_group_remote_only(
    auth_manager: std::sync::Arc<AuthManager>,
    codex_home: &Path,
    new_group: &str,
) -> Result<(), RelayLoginError> {
    if auth_manager.get_api_auth_mode() == Some(codex_protocol::auth::AuthMode::RelayOAuthTokens) {
        return relay_switch_group_with_manager(auth_manager, new_group).await;
    }

    // 兼容旧 session/key 登录：仅远端换组，不写本地缓存。
    let st = RelayState::load(codex_home)?;
    let session = st.session().ok_or(RelayLoginError::NotLoggedIn)?;
    let client = RelayClient::new()?;

    let token = match st.token_id {
        Some(id) => find_naicode_token(&client, &session)
            .await?
            .filter(|t| t.id == id)
            .or_else(|| {
                Some(TokenInfo {
                    id,
                    name: NAICODE_TOKEN_NAME.to_string(),
                    ..Default::default()
                })
            })
            .ok_or(RelayLoginError::TokenMissing)?,
        None => find_naicode_token(&client, &session)
            .await?
            .ok_or(RelayLoginError::TokenMissing)?,
    };

    client
        .update_token_group(&session, &token, new_group)
        .await?;
    // 不调用 st.save() —— 调用方负责在合适时机提交缓存。
    Ok(())
}

/// 仅写 relay.json 本地缓存，不做任何远端操作。
///
/// 在原子切换事务中作为"提交本地缓存"步骤使用，配合
/// [`relay_switch_group_remote_only`] 分离远端/本地两个阶段。
pub async fn relay_commit_group_cache(
    codex_home: &Path,
    new_group: &str,
) -> Result<(), RelayLoginError> {
    let mut state = RelayState::load(codex_home)?;
    state.group = Some(new_group.to_string());
    state.save(codex_home)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_temp_codex_home() -> TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        // relay_commit_group_cache 内部会 RelayState::load + save，
        // load 对不存在的文件应返回默认空状态（而不是 Err），
        // 因此只需要确保目录存在即可。
        dir
    }

    // relay_commit_group_cache 写入 relay.json 后，
    // 重新加载能读到正确的 group 值。
    #[tokio::test]
    async fn commit_group_cache_persists_group() {
        let dir = make_temp_codex_home();
        let codex_home = dir.path();

        relay_commit_group_cache(codex_home, "vip")
            .await
            .expect("commit_group_cache");

        let state = RelayState::load(codex_home).expect("load after commit");
        assert_eq!(
            state.group.as_deref(),
            Some("vip"),
            "保存后 relay.json 应记录 group=vip"
        );
    }

    // 连续两次 commit 以不同 group，最终应保存最后一次的值。
    #[tokio::test]
    async fn commit_group_cache_overwrites_previous() {
        let dir = make_temp_codex_home();
        let codex_home = dir.path();

        relay_commit_group_cache(codex_home, "default")
            .await
            .expect("first commit");
        relay_commit_group_cache(codex_home, "premium")
            .await
            .expect("second commit");

        let state = RelayState::load(codex_home).expect("load");
        assert_eq!(
            state.group.as_deref(),
            Some("premium"),
            "第二次 commit 应覆盖第一次"
        );
    }
}
