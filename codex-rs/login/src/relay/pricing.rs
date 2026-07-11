//! 酸奶中转站模型与价格目录。
//!
//! Relay OAuth 模式只调用授权 catalog，并复用 [`AuthManager`] 的 reload/refresh/401
//! 重试语义；不会回退匿名价格。旧 API key/session 用户仍可使用公开 `/api/pricing`。

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

use crate::auth::AuthKeyringBackendKind;
use crate::auth::AuthManager;
use crate::auth::RelayRequestError;
use crate::relay::api::RELAY_BASE_URL;
use codex_config::types::AuthCredentialsStoreMode;
use codex_protocol::auth::AuthMode;

const USER_AGENT: &str = "naicode-cli";

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct PricingDisplay {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub currency_code: Option<String>,
    #[serde(default)]
    pub currency_symbol: String,
    #[serde(default)]
    pub token_unit: String,
    #[serde(default)]
    pub quota_display_type: String,
    #[serde(default)]
    pub pricing_mode: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct EffectivePrice {
    #[serde(default)]
    pub group_ratio: Option<f64>,
    #[serde(default)]
    pub basis: String,
    #[serde(default)]
    pub currency_code: Option<String>,
    #[serde(default)]
    pub currency_symbol: String,
    #[serde(default)]
    pub input: Option<f64>,
    #[serde(default)]
    pub output: Option<f64>,
    #[serde(default)]
    pub cache_read: Option<f64>,
    #[serde(default)]
    pub cache_create_5m: Option<f64>,
    #[serde(default)]
    pub cache_create_1h: Option<f64>,
    #[serde(default)]
    pub image_input: Option<f64>,
    #[serde(default)]
    pub audio_input: Option<f64>,
    #[serde(default)]
    pub audio_output: Option<f64>,
    #[serde(default)]
    pub request: Option<f64>,
    #[serde(default)]
    pub preview: Option<serde_json::Value>,
}

/// 单个模型的目录条目。原始倍率字段仅为旧页面/诊断兼容；客户端展示只读取
/// `effective_prices`，不再自行计算倍率、货币或计价单位。
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct PricingModel {
    #[serde(default)]
    pub model_name: String,
    #[serde(default)]
    pub model_ratio: f64,
    #[serde(default)]
    pub completion_ratio: f64,
    #[serde(default)]
    pub enable_groups: Vec<String>,
    #[serde(default)]
    pub quota_type: i64,
    #[serde(default)]
    pub model_price: f64,
    #[serde(default)]
    pub billing_mode: String,
    #[serde(default)]
    pub billing_expr: Option<String>,
    #[serde(default)]
    pub pricing_version: String,
    #[serde(default)]
    pub effective_prices: HashMap<String, EffectivePrice>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupInfo {
    pub name: String,
    pub desc: String,
    pub ratio: f64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RelayPricing {
    pub models: Vec<PricingModel>,
    pub group_ratio: HashMap<String, f64>,
    pub usable_group: HashMap<String, String>,
    pub selected_group: Option<String>,
    pub display: PricingDisplay,
    pub version: Option<String>,
}

impl RelayPricing {
    pub fn groups(&self) -> Vec<GroupInfo> {
        let mut names: Vec<String> = self.group_ratio.keys().cloned().collect();
        for model in &self.models {
            for group in model.effective_prices.keys() {
                if !names.contains(group) {
                    names.push(group.clone());
                }
            }
        }
        names.sort_by(|a, b| {
            let a_ratio = self.group_ratio.get(a).copied().unwrap_or(f64::INFINITY);
            let b_ratio = self.group_ratio.get(b).copied().unwrap_or(f64::INFINITY);
            a_ratio
                .partial_cmp(&b_ratio)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.cmp(b))
        });
        names
            .into_iter()
            .map(|name| GroupInfo {
                ratio: self.group_ratio.get(&name).copied().unwrap_or_default(),
                desc: self.usable_group.get(&name).cloned().unwrap_or_default(),
                name,
            })
            .collect()
    }

    pub fn models_in_group(&self, group: &str) -> Vec<&PricingModel> {
        self.models
            .iter()
            .filter(|model| {
                model.effective_prices.contains_key(group)
                    || model.enable_groups.iter().any(|enabled| enabled == group)
            })
            .collect()
    }

    pub fn effective_price<'a>(
        &'a self,
        model: &'a PricingModel,
        group: &str,
    ) -> Option<&'a EffectivePrice> {
        model.effective_prices.get(group)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RelayCatalogError {
    #[error(transparent)]
    Auth(#[from] RelayRequestError),
    #[error("获取分组与价格失败（HTTP {0}）")]
    Http(reqwest::StatusCode),
    #[error("分组与价格响应无效: {0}")]
    InvalidResponse(String),
}

impl RelayCatalogError {
    pub fn category(&self) -> &'static str {
        match self {
            Self::Auth(error) => error.category(),
            Self::Http(_) => "catalog_http",
            Self::InvalidResponse(_) => "catalog_invalid_response",
        }
    }
}

pub async fn fetch_pricing_with_manager(
    auth_manager: Arc<AuthManager>,
) -> Result<RelayPricing, RelayCatalogError> {
    if auth_manager.get_api_auth_mode() != Some(AuthMode::RelayOAuthTokens) {
        return Err(RelayRequestError::NotLoggedIn.into());
    }
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(|error| RelayRequestError::Network(error.to_string()))?;
    let url = format!("{RELAY_BASE_URL}/api/cli/oauth/catalog");
    let response = auth_manager
        .execute_relay_request(|token| client.get(&url).bearer_auth(token))
        .await?;
    if !response.status().is_success() {
        return Err(RelayCatalogError::Http(response.status()));
    }
    let body = response
        .json()
        .await
        .map_err(|error| RelayCatalogError::InvalidResponse(error.to_string()))?;
    parse_pricing_body(body).map_err(RelayCatalogError::InvalidResponse)
}

/// 兼容现有调用方的授权 catalog 入口。实际凭据读取由 AuthManager 完成，因此同时
/// 支持 file/keyring/auto；Relay OAuth 模式绝不匿名降级。
pub async fn fetch_pricing_with_auth(
    codex_home: &std::path::Path,
    store_mode: AuthCredentialsStoreMode,
    keyring: AuthKeyringBackendKind,
) -> Result<RelayPricing, String> {
    let manager = AuthManager::shared(
        codex_home.to_path_buf(),
        false,
        store_mode,
        None,
        None,
        keyring,
        None,
    )
    .await;
    fetch_pricing_with_manager(manager)
        .await
        .map_err(|error| format!("[{}] {error}", error.category()))
}

/// 公开 pricing 只保留给明确的旧 API-key/session 兼容流程。
pub async fn fetch_pricing() -> Result<RelayPricing, String> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .get(format!("{RELAY_BASE_URL}/api/pricing"))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let body = response.json().await.map_err(|error| error.to_string())?;
    parse_pricing_body(body)
}

fn parse_pricing_body(body: serde_json::Value) -> Result<RelayPricing, String> {
    let models = serde_json::from_value(body.get("data").cloned().unwrap_or_default())
        .map_err(|error| error.to_string())?;
    let group_ratio = serde_json::from_value(
        body.get("group_ratio")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
    )
    .map_err(|error| error.to_string())?;
    let usable_group = serde_json::from_value(
        body.get("usable_group")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
    )
    .map_err(|error| error.to_string())?;
    let display = serde_json::from_value(
        body.get("display")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
    )
    .map_err(|error| error.to_string())?;
    Ok(RelayPricing {
        models,
        group_ratio,
        usable_group,
        selected_group: body
            .get("selected_group")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        display,
        version: body
            .get("pricing_version")
            .or_else(|| body.get("version"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
    })
}

/// 按服务端返回的数值做纯显示格式化：不改变币种、不应用倍率。
pub fn format_price_value(value: Option<f64>) -> String {
    let Some(value) = value.filter(|value| value.is_finite() && *value >= 0.0) else {
        return "—".to_string();
    };
    let precision = if value.abs() >= 1.0 { 4 } else { 6 };
    let formatted = format!("{value:.precision$}");
    formatted
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_structured_catalog_and_all_price_channels() {
        let catalog = parse_pricing_body(serde_json::json!({
            "pricing_version": "catalog-v2",
            "selected_group": "vip",
            "display": {
                "kind": "currency", "currency_code": "CNY", "currency_symbol": "¥",
                "token_unit": "1M tokens", "quota_display_type": "CNY", "pricing_mode": "billing"
            },
            "group_ratio": {"vip": 0.5},
            "usable_group": {"vip": "会员"},
            "data": [{
                "model_name": "gpt-test", "enable_groups": ["vip"],
                "billing_mode": "ratio", "pricing_version": "model-v3",
                "effective_prices": {"vip": {
                    "group_ratio": 0.5, "basis": "per_million_tokens",
                    "currency_code": "CNY", "currency_symbol": "¥",
                    "input": 0.2, "output": 1.6, "cache_read": 0.02,
                    "cache_create_5m": 0.25, "cache_create_1h": 0.4,
                    "image_input": 0.03, "audio_input": 0.04,
                    "audio_output": 0.08, "request": null
                }}
            }]
        }))
        .expect("catalog");
        assert_eq!(catalog.selected_group.as_deref(), Some("vip"));
        assert_eq!(catalog.version.as_deref(), Some("catalog-v2"));
        assert_eq!(catalog.display.currency_symbol, "¥");
        let price = catalog.effective_price(&catalog.models[0], "vip").unwrap();
        assert_eq!(price.cache_create_1h, Some(0.4));
        assert_eq!(price.audio_output, Some(0.08));
        assert_eq!(price.request, None);
    }

    #[test]
    fn formatting_is_currency_agnostic_and_never_scientific() {
        assert_eq!(format_price_value(Some(12.34000)), "12.34");
        assert_eq!(format_price_value(Some(0.0001234)), "0.000123");
        assert_eq!(format_price_value(Some(0.0)), "0");
        assert_eq!(format_price_value(None), "—");
        assert_eq!(format_price_value(Some(f64::NAN)), "—");
    }

    #[test]
    fn legacy_version_field_still_parses() {
        let catalog = parse_pricing_body(serde_json::json!({
            "version": "legacy-v1",
            "data": []
        }))
        .expect("catalog");
        assert_eq!(catalog.version.as_deref(), Some("legacy-v1"));
    }

    #[test]
    fn groups_can_come_from_effective_prices() {
        let pricing = RelayPricing {
            models: vec![PricingModel {
                effective_prices: HashMap::from([(
                    "oauth-only".to_string(),
                    EffectivePrice::default(),
                )]),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(pricing.groups()[0].name, "oauth-only");
    }
}
