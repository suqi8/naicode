//! 酸奶中转站公告（公开端点，无需登录）。
//!
//! `/api/notice` 返回中转站后台设置的「系统公告」（new-api 的 OptionMap["Notice"]，
//! 通常是一段 Markdown/HTML 文本）。naicode 启动时把它作为「提示」行展示，
//! 取代上游 codex 里写死的模型营销文案。

use crate::relay::api::RELAY_BASE_URL;

const USER_AGENT: &str = "naicode-cli";

/// 匿名拉取中转站公告，清洗为单行纯文本。None 表示无公告或拉取失败。
///
/// 超时短（2s）：作为启动提示，拿不到就不显示，绝不阻塞启动。
pub fn fetch_notice_blocking() -> Option<String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .no_proxy()
        .build()
        .ok()?;
    let resp = client
        .get(format!("{RELAY_BASE_URL}/api/notice"))
        .timeout(std::time::Duration::from_millis(2000))
        .send()
        .ok()?;
    let body: serde_json::Value = resp.json().ok()?;
    if !body
        .get("success")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }
    let raw = body.get("data").and_then(serde_json::Value::as_str)?;
    let cleaned = clean_notice(raw);
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

/// 把公告的 HTML/Markdown 压成适合单行「提示」展示的纯文本：
/// 去标签、折叠空白、去常见 Markdown 记号，超长截断。
fn clean_notice(raw: &str) -> String {
    // 去 HTML 标签。
    let mut out = String::with_capacity(raw.len());
    let mut in_tag = false;
    for ch in raw.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    // 常见 HTML 实体与 Markdown 记号简单还原/剥离。
    let out = out
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace(['#', '*', '`', '>'], " ");
    // 折叠所有空白（含换行）为单空格。
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    // 截断，避免提示行过长。
    const MAX_CHARS: usize = 200;
    if collapsed.chars().count() > MAX_CHARS {
        let truncated: String = collapsed.chars().take(MAX_CHARS).collect();
        format!("{truncated}…")
    } else {
        collapsed
    }
}

#[cfg(test)]
mod tests {
    use super::clean_notice;

    #[test]
    fn strips_html_and_collapses_whitespace() {
        let raw = "<h2>公告</h2>\n<p>今日   维护\n\n完毕</p>";
        assert_eq!(clean_notice(raw), "公告 今日 维护 完毕");
    }

    #[test]
    fn strips_markdown_markers() {
        let raw = "## 重要\n- **充值**有礼";
        assert_eq!(clean_notice(raw), "重要 - 充值 有礼");
    }

    #[test]
    fn truncates_overlong() {
        let raw = "字".repeat(300);
        let cleaned = clean_notice(&raw);
        assert!(cleaned.chars().count() <= 201);
        assert!(cleaned.ends_with('…'));
    }
}
