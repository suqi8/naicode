//! 中转站会话态旁路存储 `<CODEX_HOME>/relay.json`。
//!
//! codex 的 `auth.json` 字段固定（只放 OPENAI_API_KEY / tokens 等），放不下
//! 中转站特有的会话信息。因此 naicode 把 session cookie、user_id、那个唯一
//! naicode key 的 token_id、当前分组单独存这里，供**换分组不重登**使用：
//! 换组时读 token_id + session，直接 `PUT /api/token/`，不重建 key、
//! 不重写 auth.json。
//!
//! 明文 sk key 不存这里（它在 auth.json）。session cookie 属敏感凭据，
//! 落盘用 0600（与 FileAuthStorage 一致）。

use crate::relay::api::RelaySession;
use serde::Deserialize;
use serde::Serialize;
use std::fs;
use std::io::Write as _;
use std::path::Path;
use std::path::PathBuf;

const RELAY_STATE_FILE: &str = "relay.json";

/// relay.json 的持久化结构。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RelayState {
    #[serde(default)]
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// session cookie VALUE（不含前缀）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_cookie: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<i64>,
    /// 那个唯一的 naicode key 的 token id（换分组用）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_id: Option<i64>,
    /// 当前分组。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// 当前选中的模型（TUI 选择器写入，供零配置默认）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// cookie 写入时间戳（毫秒），用于过期本地预警。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cookie_saved_at_ms: Option<i64>,
}

impl RelayState {
    fn path(codex_home: &Path) -> PathBuf {
        codex_home.join(RELAY_STATE_FILE)
    }

    /// 从磁盘加载；文件不存在返回默认（未登录）。
    pub fn load(codex_home: &Path) -> std::io::Result<Self> {
        let path = Self::path(codex_home);
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(&path)?;
        serde_json::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))
    }

    /// 原子写入（0600）。
    pub fn save(&self, codex_home: &Path) -> std::io::Result<()> {
        fs::create_dir_all(codex_home)?;
        let path = Self::path(codex_home);
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let tmp = codex_home.join(format!("{RELAY_STATE_FILE}.tmp.{ts}"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            use std::os::unix::fs::PermissionsExt;
            let mut f = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&tmp)?;
            f.write_all(content.as_bytes())?;
            f.flush()?;
            fs::rename(&tmp, &path)?;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }

        #[cfg(windows)]
        {
            let mut f = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&tmp)?;
            f.write_all(content.as_bytes())?;
            f.flush()?;
            if path.exists() {
                let _ = fs::remove_file(&path);
            }
            fs::rename(&tmp, &path)?;
        }

        Ok(())
    }

    /// 是否已有可用会话。
    pub fn is_logged_in(&self) -> bool {
        self.session_cookie.as_ref().is_some_and(|c| !c.is_empty()) && self.user_id.is_some()
    }

    /// 取出会话凭据（缺任一返回 None）。
    pub fn session(&self) -> Option<RelaySession> {
        match (&self.session_cookie, self.user_id) {
            (Some(c), Some(uid)) if !c.is_empty() => Some(RelaySession {
                session_cookie: c.clone(),
                user_id: uid,
            }),
            _ => None,
        }
    }

    /// 写入一次成功登录的会话。
    pub fn set_session(&mut self, username: Option<String>, session: &RelaySession) {
        self.version = 1;
        self.username = username;
        self.session_cookie = Some(session.session_cookie.clone());
        self.user_id = Some(session.user_id);
        self.cookie_saved_at_ms = Some(chrono::Utc::now().timestamp_millis());
    }
}
