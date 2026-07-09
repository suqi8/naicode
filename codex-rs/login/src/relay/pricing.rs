//! 酸奶中转站定价数据（公开端点，无需登录）。
//!
//! `/api/pricing` 是公开的：匿名 GET 即可拿到全部模型的倍率、每个分组的
//! 倍率(`group_ratio`)、以及分组说明(`usable_group`)。naicode 的 TUI
//! 「模型 + 分组 + 价格」三合一选择器据此渲染，不必带 session/sk。
//!
//! 价格换算（人民币，不做美元汇率换算）：
//!   每百万 token 单价 ¥/1M = model_ratio × group_ratio × 2
//! （2 = new-api 约定的 $/1M 基准系数；本站 1 额度 = 1 元，故直接当人民币）。

use serde::Deserialize;
use std::collections::HashMap;

use crate::relay::api::RELAY_BASE_URL;

const USER_AGENT: &str = "naicode-cli";

/// 单个模型的定价条目（取 /api/pricing data[] 需要的字段）。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PricingModel {
    #[serde(default)]
    pub model_name: String,
    #[serde(default)]
    pub model_ratio: f64,
    #[serde(default)]
    pub completion_ratio: f64,
    /// 该模型可用的分组名列表。
    #[serde(default)]
    pub enable_groups: Vec<String>,
    /// 0=按量计费，1=按次计费（按次时用 model_price 而非 ratio）。
    #[serde(default)]
    pub quota_type: i64,
    #[serde(default)]
    pub model_price: f64,
}

/// 一个分组的展示信息。
#[derive(Debug, Clone)]
pub struct GroupInfo {
    pub name: String,
    /// 分组说明（usable_group 的值，可能为空）。
    pub desc: String,
    /// 分组倍率（group_ratio）。
    pub ratio: f64,
}

/// /api/pricing 的完整解析结果。
#[derive(Debug, Clone, Default)]
pub struct RelayPricing {
    pub models: Vec<PricingModel>,
    /// 分组名 → 倍率。
    pub group_ratio: HashMap<String, f64>,
    /// 分组名 → 说明。
    pub usable_group: HashMap<String, String>,
}

impl RelayPricing {
    /// 列出所有可用分组（按倍率升序，便宜的在前）。
    pub fn groups(&self) -> Vec<GroupInfo> {
        let mut list: Vec<GroupInfo> = self
            .group_ratio
            .iter()
            .map(|(name, ratio)| GroupInfo {
                name: name.clone(),
                desc: self.usable_group.get(name).cloned().unwrap_or_default(),
                ratio: *ratio,
            })
            .collect();
        list.sort_by(|a, b| a.ratio.partial_cmp(&b.ratio).unwrap_or(std::cmp::Ordering::Equal));
        list
    }

    /// 某分组下可用的模型（enable_groups 含该分组）。
    pub fn models_in_group(&self, group: &str) -> Vec<&PricingModel> {
        self.models
            .iter()
            .filter(|m| m.enable_groups.iter().any(|g| g == group))
            .collect()
    }

    /// 计算某模型在某分组下的输入价格 ¥/1M。None 表示按次计费或数据缺失。
    pub fn input_price_per_m(&self, model: &PricingModel, group: &str) -> Option<f64> {
        if model.quota_type == 1 {
            return None; // 按次计费，不按 token
        }
        let gr = self.group_ratio.get(group).copied()?;
        Some(model.model_ratio * gr * 2.0)
    }

    /// 输出价格 ¥/1M（= 输入价 × completion_ratio）。
    pub fn output_price_per_m(&self, model: &PricingModel, group: &str) -> Option<f64> {
        let input = self.input_price_per_m(model, group)?;
        let cr = if model.completion_ratio > 0.0 {
            model.completion_ratio
        } else {
            1.0
        };
        Some(input * cr)
    }
}

/// 匿名拉取并解析 /api/pricing（异步）。
pub async fn fetch_pricing() -> Result<RelayPricing, String> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(format!("{RELAY_BASE_URL}/api/pricing"))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

    let models: Vec<PricingModel> = body
        .get("data")
        .and_then(|d| serde_json::from_value(d.clone()).ok())
        .unwrap_or_default();
    let group_ratio: HashMap<String, f64> = body
        .get("group_ratio")
        .and_then(|g| serde_json::from_value(g.clone()).ok())
        .unwrap_or_default();
    let usable_group: HashMap<String, String> = body
        .get("usable_group")
        .and_then(|u| serde_json::from_value(u.clone()).ok())
        .unwrap_or_default();

    Ok(RelayPricing {
        models,
        group_ratio,
        usable_group,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> RelayPricing {
        RelayPricing {
            models: vec![
                PricingModel {
                    model_name: "claude-sonnet-4".to_string(),
                    model_ratio: 1.5,
                    completion_ratio: 5.0,
                    enable_groups: vec!["claude5".to_string(), "claude1".to_string()],
                    quota_type: 0,
                    model_price: 0.0,
                },
                PricingModel {
                    model_name: "seedance".to_string(),
                    model_ratio: 0.0,
                    completion_ratio: 0.0,
                    enable_groups: vec!["seedance".to_string()],
                    quota_type: 1,
                    model_price: 1.8,
                },
            ],
            group_ratio: HashMap::from([
                ("claude5".to_string(), 0.12),
                ("claude1".to_string(), 0.23),
            ]),
            usable_group: HashMap::from([("claude5".to_string(), "五折分组".to_string())]),
        }
    }

    #[test]
    fn groups_sorted_by_ratio_ascending() {
        let p = sample();
        let groups = p.groups();
        assert_eq!(groups[0].name, "claude5");
        assert_eq!(groups[0].desc, "五折分组");
        assert_eq!(groups[1].name, "claude1");
    }

    #[test]
    fn models_in_group_filters_by_enable_groups() {
        let p = sample();
        let in_claude5 = p.models_in_group("claude5");
        assert_eq!(in_claude5.len(), 1);
        assert_eq!(in_claude5[0].model_name, "claude-sonnet-4");
        assert!(p.models_in_group("seedance").len() == 1);
    }

    #[test]
    fn price_uses_model_ratio_times_group_ratio_times_two() {
        let p = sample();
        let m = &p.models[0];
        // 1.5 × 0.12 × 2 = 0.36
        let input = p.input_price_per_m(m, "claude5").expect("priced");
        assert!((input - 0.36).abs() < 1e-9);
        // 输出 = 输入 × completion_ratio(5) = 1.8
        let output = p.output_price_per_m(m, "claude5").expect("priced");
        assert!((output - 1.8).abs() < 1e-9);
    }

    #[test]
    fn per_call_models_have_no_token_price() {
        let p = sample();
        let seedance = &p.models[1];
        assert!(p.input_price_per_m(seedance, "seedance").is_none());
    }
}
