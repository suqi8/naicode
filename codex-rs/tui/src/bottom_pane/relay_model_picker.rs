//! 酸奶中转站专用模型选择器。
//!
//! 与通用 [`ListSelectionView`] 不同，本视图将「分组选择」与「模型选择」合并到单一界面：
//! 左侧固定展示分组列表，右侧实时显示当前分组内的模型及完整价格信息。
//!
//! 支持三种宽度布局：
//! - `>= 96`：左侧 18 列分组 + 分隔线 + 右侧模型（四列价格）
//! - `72..=95`：左侧分组 + 右侧模型（两列价格）
//! - `< 72`：顶行显示当前组名，仅右侧单列价格

use std::cell::Cell;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::BottomPaneView;
use crate::bottom_pane::ViewCompletion;
use crate::bottom_pane::scroll_state::ScrollState;
use crate::product_palette;
use crate::render::renderable::Renderable;
use codex_login::GroupInfo;
use codex_login::PricingModel;
use codex_login::format_price_value;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

/// 每个模型固定占两行：模型名和高频价格摘要。
const CARD_HEIGHT: usize = 2;

const READY_HEIGHT_WIDE: u16 = 14;
const READY_HEIGHT_NARROW: u16 = 13;
const DETAIL_HEIGHT: u16 = 2;

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
    /// 最近一次渲染中模型列表可见卡片数。
    visible_model_cards: Cell<usize>,
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
            visible_model_cards: Cell::new(1),
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

    // --- 状态更新 ---

    /// 数据加载完成后将 picker 从 Loading 切换到 Ready 或 Error。
    pub(crate) fn set_pricing(&mut self, result: Result<codex_login::RelayPricing, String>) {
        match result {
            Ok(pricing) => {
                let selected = pricing
                    .selected_group
                    .clone()
                    .or_else(|| pricing.groups().into_iter().next().map(|g| g.name));
                self.selected_group = selected;
                self.state = RelayPickerState::Ready { pricing };
                self.init_group_scroll();
                self.model_scroll.selected_idx = Some(0);
                self.model_scroll.scroll_top = 0;
            }
            Err(message) => {
                self.state = RelayPickerState::Error { message };
            }
        }
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
        let tail_str: String = all_chars[all_chars.len().saturating_sub(tail)..]
            .iter()
            .collect();
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
            self.app_event_tx.send(AppEvent::OpenRelayReasoningPopup {
                group: group_name,
                model: model.model_name.clone(),
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

    fn format_price(value: Option<f64>, currency_symbol: &str) -> String {
        let formatted = format_price_value(value);
        if formatted == "—" {
            formatted
        } else {
            format!("{currency_symbol}{formatted}")
        }
    }

    fn detail_line(
        price: Option<&codex_login::EffectivePrice>,
        display: &codex_login::PricingDisplay,
        width: usize,
    ) -> Line<'static> {
        let palette = product_palette::current();
        let muted = Style::default().fg(palette.border_muted);
        let value = Style::default().fg(palette.accent_bright);
        let symbol = price
            .map(|price| price.currency_symbol.as_str())
            .filter(|symbol| !symbol.is_empty())
            .unwrap_or(display.currency_symbol.as_str());
        let token_unit = if display.token_unit.is_empty() {
            "1M tokens"
        } else {
            display.token_unit.as_str()
        };

        let mut fields = Vec::new();
        if let Some(price) = price {
            for (label, amount) in [
                ("图片", price.image_input),
                ("音频入", price.audio_input),
                ("音频出", price.audio_output),
                ("按次", price.request),
                ("缓存1h", price.cache_create_1h),
            ] {
                if amount.is_some() {
                    fields.push((label, Self::format_price(amount, symbol)));
                }
            }
            if price.basis == "dynamic_expression" {
                fields.push(("计费", "动态".to_string()));
            }
        }

        if fields.is_empty() {
            fields.push(("单位", token_unit.to_string()));
        }

        let mut spans = vec![Span::styled("详情  ", muted)];
        for (index, (label, amount)) in fields.into_iter().enumerate() {
            if index > 0 {
                spans.push(Span::styled("  ", muted));
            }
            spans.push(Span::styled(format!("{label} "), muted));
            spans.push(Span::styled(amount, value));
        }
        if width >= 72 && !token_unit.is_empty() {
            spans.push(Span::styled(format!("  · {token_unit}"), muted));
        }
        Line::from(spans)
    }

    /// 构建模型名和高频价格摘要两行。
    fn model_card_lines(
        model: &PricingModel,
        price: Option<&codex_login::EffectivePrice>,
        name_max_cols: usize,
        is_selected: bool,
        currency_symbol: &str,
        mode: LayoutMode,
    ) -> Vec<Line<'static>> {
        let palette = product_palette::current();
        let name_style = if is_selected {
            Style::default()
                .fg(palette.selection_foreground)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().add_modifier(Modifier::BOLD)
        };
        let dim = Style::default().fg(palette.border_muted);
        let price_style = Style::default().fg(palette.accent_bright);

        let name = Self::truncate_middle(&model.model_name, name_max_cols);
        let name_line = Line::from(vec![Span::styled(name, name_style)]);

        let sym = currency_symbol;

        let input_s = Self::format_price(price.and_then(|p| p.input), sym);
        let output_s = Self::format_price(price.and_then(|p| p.output), sym);
        let cache_r = Self::format_price(price.and_then(|p| p.cache_read), sym);
        let cache_w = Self::format_price(
            price.and_then(|p| p.cache_create_5m.or(p.cache_create_1h)),
            sym,
        );

        let price_line = match mode {
            LayoutMode::Wide => Line::from(vec![
                Span::styled("  输入 ", dim),
                Span::styled(input_s, price_style),
                Span::styled("  输出 ", dim),
                Span::styled(output_s, price_style),
                Span::styled("  缓存读 ", dim),
                Span::styled(cache_r, price_style),
                Span::styled("  缓存写 ", dim),
                Span::styled(cache_w, price_style),
            ]),
            LayoutMode::Medium => Line::from(vec![
                Span::styled("  入 ", dim),
                Span::styled(input_s, price_style),
                Span::styled("  出 ", dim),
                Span::styled(output_s, price_style),
                Span::styled("  缓存 ", dim),
                Span::styled(cache_r, price_style),
                Span::styled(" / ", dim),
                Span::styled(cache_w, price_style),
            ]),
            LayoutMode::Narrow => Line::from(vec![
                Span::styled("  入 ", dim),
                Span::styled(input_s, price_style),
                Span::styled("  出 ", dim),
                Span::styled(output_s, price_style),
            ]),
        };

        vec![name_line, price_line]
    }

    // --- 分段渲染 ---

    fn render_loading(area: Rect, buf: &mut Buffer) {
        let palette = product_palette::current();
        let p =
            Paragraph::new("正在获取分组与价格…").style(Style::default().fg(palette.accent_bright));
        ratatui::widgets::Widget::render(p, area, buf);
    }

    fn render_error(msg: &str, area: Rect, buf: &mut Buffer) {
        let palette = product_palette::current();
        let text = format!("{msg}\n\n按 Esc 关闭");
        let p = Paragraph::new(text).style(Style::default().fg(palette.status_error));
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
                let palette = product_palette::current();
                let header = Line::from(vec![
                    Span::styled("组: ", Style::default().fg(palette.border_muted)),
                    Span::styled(
                        group_name.to_string(),
                        Style::default()
                            .fg(palette.accent_bright)
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
        let palette = product_palette::current();
        let focused = self.focus_side == FocusSide::Groups;
        let border_style = Style::default().fg(if focused {
            palette.border_focused
        } else {
            palette.border_muted
        });

        let inner = Block::default()
            .title(Span::styled(
                "分组",
                Style::default().add_modifier(Modifier::BOLD),
            ))
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
                        .fg(palette.selection_foreground)
                        .bg(palette.selection_background)
                        .add_modifier(Modifier::BOLD)
                } else if is_cur {
                    Style::default().fg(palette.accent_bright)
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
        let palette = product_palette::current();
        for y in area.y..area.y + area.height {
            if let Some(cell) = buf.cell_mut((area.x, y)) {
                cell.set_symbol("│");
                cell.set_style(Style::default().fg(palette.border_muted));
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
        let palette = product_palette::current();
        let focused = self.focus_side == FocusSide::Models;
        let border_style = Style::default().fg(if focused {
            palette.border_focused
        } else {
            palette.border_muted
        });

        // 标题行：显示模型数量或搜索提示
        let title = if !self.search_query.is_empty() {
            Span::styled(
                format!("模型  / {}", self.search_query),
                Style::default()
                    .fg(palette.accent_bright)
                    .add_modifier(Modifier::BOLD),
            )
        } else if self.is_searching {
            Span::styled(
                "模型  / 输入关键词…",
                Style::default().fg(palette.border_muted),
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
            let p = Paragraph::new(msg).style(Style::default().fg(palette.border_muted));
            ratatui::widgets::Widget::render(p, inner_area, buf);
            return;
        }

        let detail_area = if inner_area.height > DETAIL_HEIGHT + 2 {
            let model_height = inner_area.height.saturating_sub(DETAIL_HEIGHT);
            let detail_y = inner_area.y + model_height;
            Some(Rect::new(
                inner_area.x,
                detail_y,
                inner_area.width,
                DETAIL_HEIGHT,
            ))
        } else {
            None
        };
        let list_area = if let Some(detail_area) = detail_area {
            Rect::new(
                inner_area.x,
                inner_area.y,
                inner_area.width,
                detail_area.y.saturating_sub(inner_area.y),
            )
        } else {
            inner_area
        };
        self.visible_model_cards
            .set(((list_area.height as usize) / CARD_HEIGHT).max(1));

        // 名称列可用宽度（扣去前导空格 2 列）。
        let name_max = (inner_area.width as usize).saturating_sub(2);

        let scroll_top = self.model_scroll.scroll_top;
        let sel_idx = self.model_scroll.selected_idx.unwrap_or(0);

        let mut y = list_area.y;
        let y_max = list_area.y + list_area.height;

        for (i, model) in filtered.iter().enumerate().skip(scroll_top) {
            if y >= y_max {
                break;
            }
            let price = pricing.effective_price(model, group_name);
            let is_selected = i == sel_idx;
            let lines = Self::model_card_lines(model, price, name_max, is_selected, sym, mode);

            // 高亮背景覆盖整张卡片。
            let card_bg = if is_selected && focused {
                Style::default()
                    .bg(palette.selection_background)
                    .fg(palette.selection_foreground)
            } else {
                Style::default()
            };

            for line in lines {
                if y >= y_max {
                    break;
                }
                let line_area = Rect::new(list_area.x, y, list_area.width, 1);
                // 先填背景色
                if is_selected && focused {
                    for x in line_area.x..line_area.x + line_area.width {
                        if let Some(cell) = buf.cell_mut((x, y)) {
                            cell.set_style(card_bg);
                        }
                    }
                }
                let para = Paragraph::new(line).style(card_bg);
                ratatui::widgets::Widget::render(para, line_area, buf);
                y += 1;
            }
        }

        if let Some(detail_area) = detail_area
            && let Some(model) = filtered.get(sel_idx)
        {
            let separator_area = Rect::new(detail_area.x, detail_area.y, detail_area.width, 1);
            for x in separator_area.x..separator_area.x + separator_area.width {
                if let Some(cell) = buf.cell_mut((x, separator_area.y)) {
                    cell.set_symbol("─");
                    cell.set_style(Style::default().fg(palette.border_muted));
                }
            }
            let detail = Self::detail_line(
                pricing.effective_price(model, group_name),
                &pricing.display,
                detail_area.width as usize,
            );
            let detail_text_area = Rect::new(
                detail_area.x,
                detail_area.y + 1,
                detail_area.width,
                detail_area.height.saturating_sub(1),
            );
            ratatui::widgets::Widget::render(Paragraph::new(detail), detail_text_area, buf);
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

    fn desired_height(&self, width: u16) -> u16 {
        match self.state {
            RelayPickerState::Loading => 5,
            RelayPickerState::Error { .. } => 7,
            RelayPickerState::Ready { .. } => match LayoutMode::from_width(width) {
                LayoutMode::Wide | LayoutMode::Medium => READY_HEIGHT_WIDE,
                LayoutMode::Narrow => READY_HEIGHT_NARROW,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// BottomPaneView
// ---------------------------------------------------------------------------

impl BottomPaneView for RelayModelPicker {
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

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
                        let visible_cards = self.visible_model_cards.get();
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
                        let visible_cards = self.visible_model_cards.get();
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
    use codex_login::RelayPricing;
    use codex_login::{EffectivePrice, PricingDisplay, PricingModel};
    use std::collections::HashMap;

    // --- 辅助构造器 ---

    fn make_pricing(
        groups: &[&str],
        model_name: &str,
        group_prices: &[(&str, f64, f64)],
    ) -> RelayPricing {
        let mut group_ratio: HashMap<String, f64> = HashMap::new();
        let mut usable_group: HashMap<String, String> = HashMap::new();
        for g in groups {
            group_ratio.insert(g.to_string(), 1.0);
            usable_group.insert(g.to_string(), g.to_string());
        }
        let mut effective_prices: HashMap<String, EffectivePrice> = HashMap::new();
        for (g, inp, out) in group_prices {
            effective_prices.insert(
                g.to_string(),
                EffectivePrice {
                    input: Some(*inp),
                    output: Some(*out),
                    basis: "per_million_tokens".to_string(),
                    currency_symbol: "¥".to_string(),
                    ..Default::default()
                },
            );
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
        assert!(
            result.contains("..."),
            "expected middle truncation, got: {result}"
        );
        assert!(result.len() <= 12, "result too long: {result}");
        // head and tail preserved
        assert!(result.starts_with("gpt"), "head not preserved: {result}");
    }

    #[test]
    fn truncate_very_short_limit() {
        let result = RelayModelPicker::truncate_middle("hello", 3);
        assert_eq!(result.chars().count(), 3);
    }

    #[test]
    fn picker_height_stays_compact() {
        let pricing = make_pricing(&["default"], "gpt-test", &[("default", 0.2, 1.6)]);
        let picker = make_picker(pricing);

        assert_eq!(picker.desired_height(100), READY_HEIGHT_WIDE);
        assert_eq!(picker.desired_height(60), READY_HEIGHT_NARROW);
        assert!(picker.desired_height(100) < 20);
    }

    #[test]
    fn missing_price_has_no_currency_prefix() {
        assert_eq!(RelayModelPicker::format_price(None, "$"), "—");
        assert_eq!(RelayModelPicker::format_price(Some(0.0), "$"), "$0");
    }

    // --- Loading 状态不渲染"无匹配" ---

    #[test]
    fn loading_state_does_not_contain_no_match_text() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let picker = RelayModelPicker::new(RelayPickerState::Loading, dummy_tx());

        terminal
            .draw(|frame| {
                picker.render(frame.area(), frame.buffer_mut());
            })
            .unwrap();

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
            state.scroll_top,
            sel
        );
        assert!(
            sel < state.scroll_top + visible_cards,
            "selected({}) not visible, scroll_top={}, visible={}",
            sel,
            state.scroll_top,
            visible_cards
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
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let pricing = make_pricing(
            &["vip", "default"],
            "gpt-test",
            &[("vip", 0.2, 1.6), ("default", 0.4, 3.2)],
        );
        let picker = make_picker(pricing);

        // Wide layout (96 cols)
        let backend = TestBackend::new(100, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                picker.render(frame.area(), frame.buffer_mut());
            })
            .unwrap();

        // Medium layout (80 cols)
        let backend2 = TestBackend::new(80, 20);
        let mut terminal2 = Terminal::new(backend2).unwrap();
        terminal2
            .draw(|frame| {
                picker.render(frame.area(), frame.buffer_mut());
            })
            .unwrap();

        // Narrow layout (60 cols)
        let backend3 = TestBackend::new(60, 20);
        let mut terminal3 = Terminal::new(backend3).unwrap();
        terminal3
            .draw(|frame| {
                picker.render(frame.area(), frame.buffer_mut());
            })
            .unwrap();
    }
}
