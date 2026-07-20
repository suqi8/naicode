#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StatusAccountDisplay {
    ChatGpt {
        email: Option<String>,
        plan: Option<String>,
    },
    /// 酸奶中转站一等 OAuth 登录（真实 RelayOAuthTokens，不是 API key 映射）。
    RelayOAuth {
        account_name: Option<String>,
    },
    ApiKey,
}
