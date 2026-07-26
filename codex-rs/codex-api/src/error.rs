use crate::rate_limits::RateLimitError;
use codex_client::TransportError;
use http::StatusCode;
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error(transparent)]
    Transport(#[from] TransportError),
    #[error("接口返回错误 {status}：{message}")]
    Api { status: StatusCode, message: String },
    #[error("{0}")]
    Stream(String),
    #[error("上下文窗口已超出上限")]
    ContextWindowExceeded,
    #[error("额度已用尽")]
    QuotaExceeded,
    #[error("当前套餐不含此用量")]
    UsageNotIncluded,
    #[error("可重试错误：{message}")]
    Retryable {
        message: String,
        delay: Option<Duration>,
    },
    #[error("触发限流：{0}")]
    RateLimit(String),
    #[error("请求无效：{message}")]
    InvalidRequest { message: String },
    #[error("内容安全策略拦截：{message}")]
    CyberPolicy { message: String },
    #[error("服务端已过载")]
    ServerOverloaded,
}

impl From<RateLimitError> for ApiError {
    fn from(err: RateLimitError) -> Self {
        Self::RateLimit(err.to_string())
    }
}
