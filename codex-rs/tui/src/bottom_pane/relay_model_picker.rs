//! 酸奶中转站专用模型选择器。
//!
//! 与通用 [`ListSelectionView`] 不同，本视图将「分组选择」与「模型选择」合并到单一界面：
//! 左侧固定展示分组列表，右侧实时显示当前分组内的模型及完整价格信息。
//!
//! 支持三种宽度布局：
//! - `>= 96`：左侧 18 列分组 + 分隔线 + 右侧模型（四列价格）
//! - `72..=95`：左侧分组 + 右侧模型（两列价格）
//! - `< 72`：顶行显示当前组名，仅右侧单列价格

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::BottomPaneView;
use crate::bottom_pane::ViewCompletion;
use crate::bottom_pane::scroll_state::ScrollState;
use crate::render::renderable::Renderable;
use codex_login::format_price_value;
use codex_login::GroupInfo;
use codex_login::PricingModel;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

/// 每张模型卡片固定占用的终端行数（模型名 + 价格行1 + 价格行2）。
const CARD_HEIGHT: usize = 3;

/// 宽屏模式下左侧分组列表的列数（含左边框内边距）。
const GROUP_COL_WIDTH: u16 = 18;

// ---------------------------------------------------------------------------
// 公开类型
// ---------------------------------------------------------------------------

/// RelayModelPicker 的三态。
pub(crate) enum RelayPickerState {
    /// 正在从中转站拉取分组/价格数据。
    Loading,
    /// 数据已就绪。
    Ready { pricing: codex_login::RelayPricing },
    /// 拉取失败，携带错误原因。
    Error { message: String },
}

/// 当前键盘焦点侧。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum FocusSide {
    Groups,
    Models,
}

/// 酸奶中转站专用模型选择器。
///
/// 实现 [`BottomPaneView`]，可直接推入 `BottomPane` 的 view_stack。
pub(crate) struct RelayModelPicker {
    state: RelayPickerState,
    /// 左侧分组列表游标。
    group_scroll: ScrollState,
    /// 右侧模型列表游标（以模型索引为单位，切组时重置）。
    model_scroll: ScrollState,
    /// 搜索框内容（切组后保留，在新组内重新过滤）。
    search_query: String,
    /// 是否处于搜索输入模式（按 `/` 后激活）。
    is_searching: bool,
    /// 当前焦点侧。
    focus_side: FocusSide,
    is_complete: bool,
    completion: Option<ViewCompletion>,
    app_event_tx: AppEventSender,
    /// 当前已选分组名；None 时使用第一个分组。
    selected_group: Option<String>,
}

// ---------------------------------------------------------------------------
// 内部辅助
// ---------------------------------------------------------------------------

/// 渲染宽度模式。
#[derive(Clone, Copy)]
enum LayoutMode {
    /// 宽度 >= 96：左右分栏 + 四列价格。
    Wide,
    /// 宽度 72..=95：左右分栏 + 两列价格。
    Medium,
    /// 宽度 < 72：仅右侧 + 单列价格（顶行显示当前组名）。
    Narrow,
}

impl LayoutMode {
    fn from_width(w: u16) -> Self {
        if w >= 96 {
            LayoutMode::Wide
        } else if w >= 72 {
            LayoutMode::Medium
        } else {
            LayoutMode::Narrow
        }
    }
}

impl RelayModelPicker {
    pub(crate) fn new(state: RelayPickerState, app_event_tx: AppEventSender) -> Self {
        // 优先使用服务端返回的 selected_group，否则取分组列表第一个。
        let selected_group = if let RelayPickerState::Ready { ref pricing } = state {
            pricing
                .selected_group
                .clone()
                .or_else(|| pricing.groups().into_iter().next().map(|g| g.name))
        } else {
            None
        };

        let mut picker = Self {
            state,
            group_scroll: ScrollState::new(),
            model_scroll: ScrollState::new(),
            search_query: String::new(),
            is_searching: false,
            focus_side: FocusSide::Groups,
            is_complete: false,
            completion: None,
            app_event_tx,
            selected_group,
        };
        picker.init_group_scroll();
        picker.model_scroll.selected_idx = Some(0);
        picker
    }

    // --- 初始化 ---

    /// 将分组列表游标定位到 `selected_group` 所在行。
    fn init_group_scroll(&mut self) {
        let RelayPickerState::Ready { ref pricing } = self.state else {
            return;
        };
        let groups = pricing.groups();
        let n = groups.len();
        if n == 0 {
            return;
        }
        let idx = self
            .selected_group
            .as_ref()
            .and_then(|name| groups.iter().position(|g| &g.name == name))
            .unwrap_or(0);
        self.group_scroll.selected_idx = Some(idx);
        self.group_scroll.ensure_visible(n, 10);
    }

    // --- 辅助查询 ---

    /// 当前有效分组名（借用 groups 切片）。
    fn current_group_name<'g>(&'g self, groups: &'g [GroupInfo]) -> Option<&'g str> {
        if let Some(name) = &self.selected_group {
            // 验证分组仍存在（pricing 刷新后可能消失）。
            if groups.iter().any(|g| g.name == *name) {
                return Some(name.as_str());
            }
        }
        groups.first().map(|g| g.name.as_str())
    }

    /// 对 `models` 按 `search_query` 过滤（大小写不敏感）。
    fn filtered_models<'a>(&self, models: Vec<&'a PricingModel>) -> Vec<&'a PricingModel> {
        if self.search_query.is_empty() {
            return models;
        }
        let q = self.search_query.to_lowercase();
        models
            .into_iter()
            .filter(|m| m.model_name.to_lowercase().contains(&q))
            .collect()
    }

    /// 中间省略截断：`abc...xyz`，用字节长度估算字符宽度。
    fn truncate_middle(name: &str, max_cols: usize) -> String {
        if max_cols < 4 {
            return name.chars().take(max_cols).collect();
        }
        if name.len() <= max_cols {
            return name.to_string();
        }
        let keep = max_cols.saturating_sub(3);
        let head = keep / 2;
        let tail = keep - head;
        let head_str: String = name.chars().take(head).collect();
        let all_chars: Vec<char> = name.chars().collect();
        let tail_str: String = all_chars[all_chars.len().saturating_sub(tail)..].iter().collect();
        format!("{head_str}...{tail_str}")
    }

    // --- 状态变更 ---

    fn close_cancelled(&mut self) {
        self.is_complete = true;
        self.completion = Some(ViewCompletion::Cancelled);
    }

    fn confirm_model_selection(&mut self) {
        let RelayPickerState::Ready { ref pricing } = self.state else {
            return;
        };
        let groups = pricing.groups();
        let group_name = match self.current_group_name(&groups) {
            Some(g) => g.to_string(),
            None => return,
        };
        let raw_models = pricing.models_in_group(&group_name);
        let filtered = self.filtered_models(raw_models);
        let idx = self.model_scroll.selected_idx.unwrap_or(0);
        if let Some(model) = filtered.get(idx) {
            self.app_event_tx.send(AppEvent::PendingRelayModelSelection {
                group: group_name,
                model: model.model_name.clone(),
                effort: None,
            });
            self.is_complete = true;
            self.completion = Some(ViewCompletion::Accepted);
        }
    }

    /// 将 `selected_group` 同步为当前 group_scroll 游标指向的分组。
    fn sync_group_from_scroll(&mut self) {
        let RelayPickerState::Ready { ref pricing } = self.state else {
            return;
        };
        let groups = pricing.groups();
        let idx = self.group_scroll.selected_idx.unwrap_or(0);
        if let Some(g) = groups.get(idx) {
            if self.selected_group.as_deref() != Some(g.name.as_str()) {
                self.selected_group = Some(g.name.clone());
                // 切组时重置模型游标（保留 search_query）。
                self.model_scroll = ScrollState::new();
                self.model_scroll.selected_idx = Some(0);
            }
        }
    }

    // --- 渲染辅助 ---

    /// 构建单张模型卡片的行列表（CARD_HEIGHT 行）。
    fn model_card_lines(
        model: &PricingModel,
        price: Option<&codex_login::EffectivePrice>,
        name_max_cols: usize,
        is_selected: bool,
        currency_symbol: &str,
        mode: LayoutMode,
    ) -> Vec<Line<'static>> {
        let name_style = if is_selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let dim = Style::default().fg(Color::DarkGray);
        let price_style = Style::default().fg(Color::Cyan);

        let name = Self::truncate_middle(&model.model_name, name_max_cols);
        let name_line = Line::from(vec![Span::styled(name, name_style)]);

        let sym = currency_symbol;
        let input_s = format_price_value(price.and_then(|p| p.input));
        let output_s = format_price_value(price.and_then(|p| p.output));
        let cache_read_s = format_price_value(price.and_then(|p| p.cache_read));
        let cache_write_s = format_price_value(
            price.and_then(|p| p.cache_create_5m.or(p.cache_create_1h)),
        );

        match mode {
            LayoutMode::Wide | LayoutMode::Medium => {
                let price_line1 = Line::from(vec![
                    Span::styled("  输入 ", dim),
                    Span::styled(format!("{sym}{input_s}"), price_style),
                    Span::styled("  输出 ", dim),
                    Span::styled(format!("{sym}{output_s}"), price_style),
                ]);
                let price_line2 = Line::from(vec![
                    Span::styled("  缓存读 ", dim),
                    Span::styled(format!("{sym}{cache_read_s}"), price_style),
                    Span::styled("  缓存写 ", dim),
                    Span::styled(format!("{sym}{cache_write_s}"), price_style),
                ]);
                vec![name_line, price_line1, price_line2]
            }
            LayoutMode::Narrow => {
                let price_line1 = Line::from(vec![
                    Span::styled("  输入 ", dim),
                    Span::styled(format!("{sym}{input_s}"), price_style),
                    Span::styled("  输出 ", dim),
                    Span::styled(format!("{sym}{output_s}"), price_style),
                ]);
                // 窄模式：第三行显示缓存（若均为 — 则显示空行作分隔）
                let price_line2 = if cache_read_s == "—" && cache_write_s == "—" {
                    Line::from("")
                } else {
                    Line::from(vec![
                        Span::styled("  缓存读 ", dim),
                        Span::styled(format!("{sym}{cache_read_s}"), price_style),
                        Span::styled("  缓存写 ", dim),
                        Span::styled(format!("{sym}{cache_write_s}"), price_style),
                    ])
                };
                vec![name_line, price_line1, price_line2]
            }
        }
    }

    // --- 分段渲染 ---

    fn render_loading(area: Rect, buf: &mut Buffer) {
        let p = Paragraph::new("正在获取分组与价格…")
            .style(Style::default().fg(Color::Gray));
        ratatui::widgets::Widget::render(p, area, buf);
    }

    fn render_error(msg: &str, area: Rect, buf: &mut Buffer) {
        let text = format!("{msg}\n\n按 Esc 关闭");
        let p = Paragraph::new(text).style(Style::default().fg(Color::Red));
        ratatui::widgets::Widget::render(p, area, buf);
    }

    fn render_ready(&self, pricing: &codex_login::RelayPricing, area: Rect, buf: &mut Buffer) {
        let mode = LayoutMode::from_width(area.width);
        let groups = pricing.groups();
        let group_name = self.current_group_name(&groups).unwrap_or("");
        let sym = &pricing.display.currency_symbol;
        let sym = if sym.is_empty() { "¥" } else { sym.as_str() };

        match mode {
            LayoutMode::Wide | LayoutMode::Medium => {
                // 水平分割：[groups | divider | models]
                let chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Length(GROUP_COL_WIDTH),
                        Constraint::Length(1),
                        Constraint::Min(0),
                    ])
                    .split(area);

                self.render_group_list(&groups, group_name, chunks[0], buf);
                Self::render_divider(chunks[1], buf);
                self.render_model_list(pricing, group_name, sym, mode, chunks[2], buf);
            }
            LayoutMode::Narrow => {
                // 顶行：当前组名；剩余空间：模型列表
                if area.height == 0 {
                    return;
                }
                let header_area = Rect::new(area.x, area.y, area.width, 1);
                let model_area = Rect::new(
                    area.x,
                    area.y + 1,
                    area.width,
                    area.height.saturating_sub(1),
                );
                let header = Line::from(vec![
                    Span::styled("组: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        group_name.to_string(),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]);
                let header_para = Paragraph::new(header);
                ratatui::widgets::Widget::render(header_para, header_area, buf);
                self.render_model_list(pricing, group_name, sym, mode, model_area, buf);
            }
        }
    }

    fn render_group_list(
        &self,
        groups: &[GroupInfo],
        current_group: &str,
        area: Rect,
        buf: &mut Buffer,
    ) {
        let focused = self.focus_side == FocusSide::Groups;
        let border_style = if focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let inner = Block::default()
            .title(Span::styled("分组", Style::default().add_modifier(Modifier::BOLD)))
            .borders(Borders::ALL)
            .border_style(border_style);

        let inner_area = inner.inner(area);
        ratatui::widgets::Widget::render(inner, area, buf);

        let scroll_top = self.group_scroll.scroll_top;
        let selected = self.group_scroll.selected_idx;
        let visible = inner_area.height as usize;

        let items: Vec<ListItem> = groups
            .iter()
            .enumerate()
            .skip(scroll_top)
            .take(visible)
            .map(|(i, g)| {
                let is_cur = g.name == current_group;
                let is_sel = selected == Some(i);
                let marker = if is_cur { "●" } else { " " };
                let label = format!("{marker} {}", g.name);
                let style = if is_sel && focused {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD | Modifier::REVERSED)
                } else if is_cur {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default()
                };
                ListItem::new(Line::from(Span::styled(label, style)))
            })
            .collect();

        let list = List::new(items);
        ratatui::widgets::Widget::render(list, inner_area, buf);
    }

    fn render_divider(area: Rect, buf: &mut Buffer) {
        for y in area.y..area.y + area.height {
            if let Some(cell) = buf.cell_mut((area.x, y)) {
                cell.set_symbol("│");
                cell.set_style(Style::default().fg(Color::DarkGray));
            }
        }
    }

    fn render_model_list(
        &self,
        pricing: &codex_login::RelayPricing,
        group_name: &str,
        sym: &str,
        mode: LayoutMode,
        area: Rect,
        buf: &mut Buffer,
    ) {
        let focused = self.focus_side == FocusSide::Models;
        let border_style = if focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        // 标题行：显示模型数量或搜索提示
        let title = if !self.search_query.is_empty() {
            Span::styled(
                format!("模型  / {}", self.search_query),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
        } else if self.is_searching {
            Span::styled(
                "模型  / 输入关键词…",
                Style::default().fg(Color::DarkGray),
            )
        } else {
            Span::styled("模型", Style::default().add_modifier(Modifier::BOLD))
        };

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(border_style);

        let inner_area = block.inner(area);
        ratatui::widgets::Widget::render(block, area, buf);

        if inner_area.height == 0 {
            return;
        }

        let raw_models = pricing.models_in_group(group_name);
        let filtered = self.filtered_models(raw_models);

        if filtered.is_empty() {
            let msg = "（无匹配模型）";
            let p = Paragraph::new(msg).style(Style::default().fg(Color::DarkGray));
            ratatui::widgets::Widget::render(p, inner_area, buf);
            return;
        }

        // 名称列可用宽度（扣去前导空格 2 列）。
        let name_max = (inner_area.width as usize).saturating_sub(2);

        let scroll_top = self.model_scroll.scroll_top;
        let sel_idx = self.model_scroll.selected_idx.unwrap_or(0);

        let mut y = inner_area.y;
        let y_max = inner_area.y + inner_area.height;

        for (i, model) in filtered.iter().enumerate().skip(scroll_top) {
            if y >= y_max {
                break;
            }
            let price = pricing.effective_price(model, group_name);
            let is_selected = i == sel_idx;
            let lines = Self::model_card_lines(model, price, name_max, is_selected, sym, mode);

            // 高亮背景覆盖整张卡片。
            let card_bg = if is_selected && focused {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };

            for line in lines {
                if y >= y_max {
                    break;
                }
                let line_area = Rect::new(inner_area.x, y, inner_area.width, 1);
                // 先填背景色
                if is_selected && focused {
                    for x in line_area.x..line_area.x + line_area.width {
                        if let Some(cell) = buf.cell_mut((x, y)) {
                            cell.set_style(card_bg);
                        }
                    }
                }
                let para = Paragraph::new(line);
                ratatui::widgets::Widget::render(para, line_area, buf);
                y += 1;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Renderable
// ---------------------------------------------------------------------------

impl Renderable for RelayModelPicker {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        match &self.state {
            RelayPickerState::Loading => Self::render_loading(area, buf),
            RelayPickerState::Error { message } => Self::render_error(message, area, buf),
            RelayPickerState::Ready { pricing } => self.render_ready(pricing, area, buf),
        }
    }

    fn desired_height(&self, _width: u16) -> u16 {
        // 作为弹层视图，高度由 BottomPane 的区域决定；此处返回一个合理的最小值。
        20
    }
}

// ---------------------------------------------------------------------------
// BottomPaneView
// ---------------------------------------------------------------------------

impl BottomPaneView for RelayModelPicker {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        // 搜索输入模式：所有可打印字符追加到 search_query。
        if self.is_searching {
            match key_event.code {
                KeyCode::Char(c) => {
                    self.search_query.push(c);
                    // 切换搜索词后重置模型游标。
                    self.model_scroll.selected_idx = Some(0);
                    self.model_scroll.scroll_top = 0;
                    return;
                }
                KeyCode::Backspace => {
                    self.search_query.pop();
                    self.model_scroll.selected_idx = Some(0);
                    self.model_scroll.scroll_top = 0;
                    return;
                }
                KeyCode::Esc => {
                    if !self.search_query.is_empty() {
                        self.search_query.clear();
                        self.model_scroll.selected_idx = Some(0);
                        self.model_scroll.scroll_top = 0;
                    } else {
                        self.is_searching = false;
                    }
                    return;
                }
                KeyCode::Enter => {
                    // 在搜索模式下按 Enter：确认当前高亮的模型。
                    self.is_searching = false;
                    self.confirm_model_selection();
                    return;
                }
                _ => {
                    // 其他键退出搜索模式，继续处理。
                    self.is_searching = false;
                }
            }
        }

        let RelayPickerState::Ready { ref pricing } = self.state else {
            if key_event.code == KeyCode::Esc {
                self.close_cancelled();
            }
            return;
        };

        let groups = pricing.groups();
        let n_groups = groups.len();
        let group_name = self.current_group_name(&groups).unwrap_or("").to_string();
        let raw_models = pricing.models_in_group(&group_name);
        let n_models = self.filtered_models(raw_models).len();

        match key_event.code {
            // --- 焦点切换 ---
            KeyCode::Left => {
                self.focus_side = FocusSide::Groups;
            }
            KeyCode::Right => {
                self.focus_side = FocusSide::Models;
            }

            // --- 导航 ---
            KeyCode::Up => match self.focus_side {
                FocusSide::Groups => {
                    self.group_scroll.move_up_wrap(n_groups);
                    self.group_scroll.ensure_visible(n_groups, 10);
                    self.sync_group_from_scroll();
                }
                FocusSide::Models => {
                    if n_models > 0 {
                        self.model_scroll.move_up_wrap(n_models);
                        let visible_cards = (20 / CARD_HEIGHT).max(1);
                        self.model_scroll.ensure_visible(n_models, visible_cards);
                    }
                }
            },
            KeyCode::Down => match self.focus_side {
                FocusSide::Groups => {
                    self.group_scroll.move_down_wrap(n_groups);
                    self.group_scroll.ensure_visible(n_groups, 10);
                    self.sync_group_from_scroll();
                }
                FocusSide::Models => {
                    if n_models > 0 {
                        self.model_scroll.move_down_wrap(n_models);
                        let visible_cards = (20 / CARD_HEIGHT).max(1);
                        self.model_scroll.ensure_visible(n_models, visible_cards);
                    }
                }
            },

            // --- 确认 ---
            KeyCode::Enter => {
                if self.focus_side == FocusSide::Models {
                    self.confirm_model_selection();
                } else {
                    // 在分组侧按 Enter：将焦点切到模型侧。
                    self.focus_side = FocusSide::Models;
                }
            }

            // --- 搜索激活 ---
            KeyCode::Char('/') => {
                self.is_searching = true;
            }

            // --- 关闭 ---
            KeyCode::Esc => {
                if !self.search_query.is_empty() {
                    // 第一级 Esc：清除搜索词。
                    self.search_query.clear();
                    self.model_scroll.selected_idx = Some(0);
                    self.model_scroll.scroll_top = 0;
                } else {
                    self.close_cancelled();
                }
            }

            _ => {}
        }
    }

    fn is_complete(&self) -> bool {
        self.is_complete
    }

    fn completion(&self) -> Option<ViewCompletion> {
        self.completion
    }

    fn on_ctrl_c(&mut self) -> crate::bottom_pane::CancellationEvent {
        self.close_cancelled();
        crate::bottom_pane::CancellationEvent::Handled
    }

    fn prefer_esc_to_handle_key_event(&self) -> bool {
        // 让 Esc 走 handle_key_event 而非直接触发 on_ctrl_c，
        // 以便二级 Esc 逻辑（先清搜索词再关闭）生效。
        true
    }
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use codex_login::{EffectivePrice, PricingDisplay, PricingModel};
    use codex_login::RelayPricing;
    use std::collections::HashMap;

    // --- 辅助构造器 ---

    fn make_pricing(groups: &[&str], model_name: &str, group_prices: &[(&str, f64, f64)]) -> RelayPricing {
        let mut group_ratio: HashMap<String, f64> = HashMap::new();
        let mut usable_group: HashMap<String, String> = HashMap::new();
        for g in groups {
            group_ratio.insert(g.to_string(), 1.0);
            usable_group.insert(g.to_string(), g.to_string());
        }
        let mut effective_prices: HashMap<String, EffectivePrice> = HashMap::new();
        for (g, inp, out) in group_prices {
            effective_prices.insert(g.to_string(), EffectivePrice {
                input: Some(*inp),
                output: Some(*out),
                basis: "per_million_tokens".to_string(),
                currency_symbol: "¥".to_string(),
                ..Default::default()
            });
        }
        let model = PricingModel {
            model_name: model_name.to_string(),
            enable_groups: groups.iter().map(|g| g.to_string()).collect(),
            effective_prices,
            ..Default::default()
        };
        RelayPricing {
            models: vec![model],
            group_ratio,
            usable_group,
            selected_group: Some(groups.first().copied().unwrap_or("").to_string()),
            display: PricingDisplay {
                currency_symbol: "¥".to_string(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn dummy_tx() -> AppEventSender {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        crate::app_event_sender::AppEventSender::new(tx)
    }

    fn make_picker(pricing: RelayPricing) -> RelayModelPicker {
        RelayModelPicker::new(RelayPickerState::Ready { pricing }, dummy_tx())
    }

    // --- LayoutMode 宽度边界 ---

    #[test]
    fn layout_mode_boundary_96_wide() {
        assert!(matches!(LayoutMode::from_width(96), LayoutMode::Wide));
    }

    #[test]
    fn layout_mode_boundary_95_medium() {
        assert!(matches!(LayoutMode::from_width(95), LayoutMode::Medium));
    }

    #[test]
    fn layout_mode_boundary_72_medium() {
        assert!(matches!(LayoutMode::from_width(72), LayoutMode::Medium));
    }

    #[test]
    fn layout_mode_boundary_71_narrow() {
        assert!(matches!(LayoutMode::from_width(71), LayoutMode::Narrow));
    }

    // --- truncate_middle ---

    #[test]
    fn truncate_no_truncation_needed() {
        assert_eq!(RelayModelPicker::truncate_middle("abc", 10), "abc");
    }

    #[test]
    fn truncate_exactly_fits() {
        assert_eq!(RelayModelPicker::truncate_middle("abcde", 5), "abcde");
    }

    #[test]
    fn truncate_long_name_middle_dots() {
        let result = RelayModelPicker::truncate_middle("gpt-4o-mini-2024-07-18", 12);
        assert!(result.contains("..."), "expected middle truncation, got: {result}");
        assert!(result.len() <= 12, "result too long: {result}");
        // head and tail preserved
        assert!(result.starts_with("gpt"), "head not preserved: {result}");
    }

    #[test]
    fn truncate_very_short_limit() {
        let result = RelayModelPicker::truncate_middle("hello", 3);
        assert_eq!(result.chars().count(), 3);
    }

    // --- Loading 状态不渲染"无匹配" ---

    #[test]
    fn loading_state_does_not_contain_no_match_text() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let picker = RelayModelPicker::new(RelayPickerState::Loading, dummy_tx());

        terminal.draw(|frame| {
            picker.render(frame.area(), frame.buffer_mut());
        }).unwrap();

        let rendered = terminal.backend().to_string();
        assert!(
            !rendered.contains("无匹配"),
            "Loading 状态不应显示'无匹配'，但得到：{rendered}"
        );
        assert!(
            rendered.contains("正在获取"),
            "Loading 状态应显示加载文字，但得到：{rendered}"
        );
    }

    // --- 搜索只过滤当前组，切组保留 query ---

    #[test]
    fn search_filters_current_group_only() {
        let pricing = make_pricing(
            &["groupA", "groupB"],
            "alpha-model",
            &[("groupA", 0.1, 0.5), ("groupB", 0.2, 0.6)],
        );
        let mut picker = make_picker(pricing);
        picker.search_query = "alpha".to_string();

        // 分组 A：模型名含 "alpha"，应匹配
        picker.selected_group = Some("groupA".to_string());
        let pricing_ref = match &picker.state {
            RelayPickerState::Ready { pricing } => pricing,
            _ => panic!(),
        };
        let raw = pricing_ref.models_in_group("groupA");
        let filtered = picker.filtered_models(raw);
        assert_eq!(filtered.len(), 1, "groupA 应有1个匹配模型");

        // 搜索 "beta"（不存在）
        picker.search_query = "beta".to_string();
        let pricing_ref = match &picker.state {
            RelayPickerState::Ready { pricing } => pricing,
            _ => panic!(),
        };
        let raw = pricing_ref.models_in_group("groupA");
        let filtered = picker.filtered_models(raw);
        assert_eq!(filtered.len(), 0, "搜索 beta 应无结果");
    }

    #[test]
    fn search_query_preserved_on_group_switch() {
        let pricing = make_pricing(
            &["g1", "g2"],
            "my-model",
            &[("g1", 0.1, 0.5), ("g2", 0.2, 0.6)],
        );
        let mut picker = make_picker(pricing);
        picker.search_query = "my".to_string();
        picker.selected_group = Some("g1".to_string());

        // 切组（修改 group_scroll 游标到 g2，调用 sync_group_from_scroll）
        picker.group_scroll.selected_idx = Some(1);
        picker.sync_group_from_scroll();

        // search_query 应保留
        assert_eq!(picker.search_query, "my", "切组后搜索词应保留");
        assert_eq!(picker.selected_group.as_deref(), Some("g2"), "组应已切换");
        // 模型游标应重置
        assert_eq!(picker.model_scroll.selected_idx, Some(0), "模型游标应重置");
    }

    // --- 整卡滚动：ensure_visible 以模型索引为单位 ---

    #[test]
    fn scroll_state_card_visibility() {
        // 使用 ScrollState 的通用 ensure_visible 确认整卡不越界
        let mut state = ScrollState::new();
        let n_models = 10;
        let visible_cards = 3; // (20 行 / 3 行/卡)

        state.clamp_selection(n_models);
        // 向下翻到最后一张卡
        for _ in 0..n_models {
            state.move_down_wrap(n_models);
        }
        state.ensure_visible(n_models, visible_cards);

        // scroll_top + visible_cards 应 >= selected_idx
        let sel = state.selected_idx.unwrap();
        assert!(
            state.scroll_top <= sel,
            "scroll_top({}) > selected({})",
            state.scroll_top, sel
        );
        assert!(
            sel < state.scroll_top + visible_cards,
            "selected({}) not visible, scroll_top={}, visible={}",
            sel, state.scroll_top, visible_cards
        );
    }

    // --- 所有模型始终显示完整价格（format_price_value 委托） ---

    #[test]
    fn price_display_shows_all_channels() {
        use codex_login::format_price_value;

        let ep = EffectivePrice {
            input: Some(0.12),
            output: Some(0.48),
            cache_read: Some(0.02),
            cache_create_5m: Some(0.15),
            cache_create_1h: None,
            request: None,
            ..Default::default()
        };

        assert_eq!(format_price_value(ep.input), "0.12");
        assert_eq!(format_price_value(ep.output), "0.48");
        assert_eq!(format_price_value(ep.cache_read), "0.02");
        assert_eq!(format_price_value(ep.cache_create_5m), "0.15");
        assert_eq!(format_price_value(ep.cache_create_1h), "—");
        assert_eq!(format_price_value(ep.request), "—");
    }

    // --- Ready 状态：有分组有模型时正常渲染，不崩溃 ---

    #[test]
    fn ready_state_renders_without_panic() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let pricing = make_pricing(&["vip", "default"], "gpt-test", &[
            ("vip", 0.2, 1.6),
            ("default", 0.4, 3.2),
        ]);
        let picker = make_picker(pricing);

        // Wide layout (96 cols)
        let backend = TestBackend::new(100, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| {
            picker.render(frame.area(), frame.buffer_mut());
        }).unwrap();

        // Medium layout (80 cols)
        let backend2 = TestBackend::new(80, 20);
        let mut terminal2 = Terminal::new(backend2).unwrap();
        terminal2.draw(|frame| {
            picker.render(frame.area(), frame.buffer_mut());
        }).unwrap();

        // Narrow layout (60 cols)
        let backend3 = TestBackend::new(60, 20);
        let mut terminal3 = Terminal::new(backend3).unwrap();
        terminal3.draw(|frame| {
            picker.render(frame.area(), frame.buffer_mut());
        }).unwrap();
    }
}
