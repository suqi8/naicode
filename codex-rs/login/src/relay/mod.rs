//! 酸奶中转站账号编排层：把 HTTP 调用（[`api`]）、会话态旁路存储
//! （[`state`]）与 codex 的 `auth.json` 写入（`login_with_api_key`）串起来。
//!
//! 对外两个入口：
//! - [`relay_login`]：登录 → 只建/复用一个名为 `naicode` 的 key → 取明文拼
//!   `sk-` → 写 auth.json → 存会话态。
//! - [`relay_switch_group`]：读会话态，`PUT /api/token/` 换那个 key 的分组，
//!   **不新建 key、不重写 auth.json**。

pub mod api;
pub mod oauth;
pub mod state;

use crate::auth::AuthKeyringBackendKind;
use crate::auth::login_with_api_key;
use api::RelayClient;
use api::RelayError;
use api::RelaySession;
use api::TokenInfo;
use api::NAICODE_TOKEN_NAME;
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
    client.create_token(session, NAICODE_TOKEN_NAME, group).await?;
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

/// 换分组：读会话态 → `PUT /api/token/` 更新那个 key 的 group（不新建 key）。
pub async fn relay_switch_group(
    codex_home: &Path,
    new_group: &str,
) -> Result<(), RelayLoginError> {
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

    client.update_token_group(&session, &token, new_group).await?;
    st.group = Some(new_group.to_string());
    st.save(codex_home)?;
    Ok(())
}
