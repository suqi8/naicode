//! Model, collaboration, and reasoning popups for `ChatWidget`.
//!
//! These surfaces are tightly related because changing one often redirects
//! into another, especially while Plan mode is active.

use super::*;

const ULTRA_REASONING_CONCURRENCY_WARNING_THRESHOLD: usize = 8;

impl ChatWidget {
    /// Open a popup to choose a quick auto model. Selecting "All models"
    /// opens the full picker with every available preset.
    pub(crate) fn open_model_popup(&mut self) {
        if !self.is_session_configured() {
            self.add_info_message("启动完成前无法选择模型。".to_string(), /*hint*/ None);
            return;
        }

        let presets: Vec<ModelPreset> = match self.model_catalog.try_list_models() {
            Ok(models) => models,
            Err(_) => {
                self.add_info_message(
                    "模型列表正在更新，请稍候再试 /model。".to_string(),
                    /*hint*/ None,
                );
                return;
            }
        };
        self.open_model_popup_with_presets(presets);
    }

    /// Push pricing data into the RelayModelPicker currently on the view stack.
    /// Returns true (no-op fallback removed; picker is always opened first).
    pub(crate) fn update_relay_picker(
        &mut self,
        result: Result<codex_login::RelayPricing, String>,
    ) -> bool {
        self.bottom_pane.update_relay_picker(result);
        true
    }

    /// naicode: 打开酸奶中转站模型选择器。显示新的专用 picker（Loading 状态），
    /// 然后触发授权 catalog 拉取；数据到达后 picker 自动切换到 Ready 状态。
    pub(crate) fn open_relay_group_popup(&mut self) {
        let tx = self.app_event_tx.clone();
        self.bottom_pane.show_relay_picker(tx.clone());
        self.request_redraw();
        let codex_home = self.config.codex_home.clone();
        tokio::spawn(async move {
            let result = codex_login::fetch_pricing_with_auth(
                &codex_home,
                codex_config::types::AuthCredentialsStoreMode::Auto,
                codex_login::AuthKeyringBackendKind::default(),
            )
            .await
            .map(|p| *Box::new(p));
            tx.send(AppEvent::OpenRelayGroups {
                result: result.map(Box::new),
            });
        });
    }

    /// 收到定价数据后展示分组选择器：每个分组列出倍率与可用模型数，
    /// 选中即发起换组（不新建 key）。
    pub(crate) fn open_relay_groups_list(
        &mut self,
        result: Result<Box<codex_login::RelayPricing>, String>,
    ) {
        let pricing = match result {
            Ok(p) => *p,
            Err(e) => {
                // 用弹层展示错误（替换掉「加载中」弹层，按 Esc 关闭即消失），
                // 不写进滚动历史。
                self.bottom_pane.show_selection_view(SelectionViewParams {
                    title: Some("酸奶中转站分组".to_string()),
                    subtitle: Some(format!("获取分组失败：{e}")),
                    footer_hint: Some(standard_popup_hint_line()),
                    items: Vec::new(),
                    ..Default::default()
                });
                self.request_redraw();
                return;
            }
        };
        let groups = pricing.groups();
        if groups.is_empty() {
            self.bottom_pane.show_selection_view(SelectionViewParams {
                title: Some("酸奶中转站分组".to_string()),
                subtitle: Some("当前没有可用分组。".to_string()),
                footer_hint: Some(standard_popup_hint_line()),
                items: Vec::new(),
                ..Default::default()
            });
            self.request_redraw();
            return;
        }

        let items: Vec<SelectionItem> = groups
            .iter()
            .map(|g| {
                let model_count = pricing.models_in_group(&g.name).len();
                let description = Some(format!("倍率 {} · {model_count} 个可用模型", g.ratio));
                let name = if g.desc.is_empty() {
                    g.name.clone()
                } else {
                    format!("{}（{}）", g.name, g.desc)
                };
                let group_name = g.name.clone();
                let pricing_for_action = pricing.clone();
                SelectionItem {
                    name,
                    description,
                    actions: vec![Box::new(move |tx| {
                        tx.send(AppEvent::OpenRelayModels {
                            pricing: Box::new(pricing_for_action.clone()),
                            group: group_name.clone(),
                        });
                    })],
                    // 选分组不关闭弹层，转入「该分组模型列表」子弹层；
                    // 子层选定模型后再一并收起父层。
                    dismiss_parent_on_child_accept: true,
                    ..Default::default()
                }
            })
            .collect();

        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("酸奶中转站分组".to_string()),
            subtitle: Some(
                "选分组后展示该分组内的完整价格；选模型后再选思考等级".to_string(),
            ),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            ..Default::default()
        });
        self.request_redraw();
    }

    /// 展示某分组下「可用模型 + 价格」列表（只列该分组内的模型，非全部）。
    /// 选中模型：切到该模型所属分组（隐式换组，不新建 key）并把它设为默认模型。
    pub(crate) fn open_relay_models_list(
        &mut self,
        pricing: codex_login::RelayPricing,
        group: String,
    ) {
        let models = pricing.models_in_group(&group);
        if models.is_empty() {
            self.add_info_message(
                format!("分组「{group}」下暂无可用模型。"),
                /*hint*/ None,
            );
            return;
        }

        let current_model = self.current_model();
        let mut items: Vec<SelectionItem> = Vec::new();
        for m in models {
            let price = match pricing.effective_price(m, &group) {
                Some(ep) if ep.input.is_some() || ep.output.is_some() => {
                    let sym = if ep.currency_symbol.is_empty() {
                        pricing.display.currency_symbol.as_str()
                    } else {
                        ep.currency_symbol.as_str()
                    };
                    let unit = &pricing.display.token_unit;
                    let inp = codex_login::format_price_value(ep.input);
                    let out = codex_login::format_price_value(ep.output);
                    format!("输入 {sym}{inp}/{unit} · 输出 {sym}{out}/{unit}")
                }
                _ => "按次计费".to_string(),
            };
            let model_name = m.model_name.clone();
            let group_for_action = group.clone();
            let model_for_action = model_name.clone();
            items.push(SelectionItem {
                name: model_name.clone(),
                description: Some(price),
                is_current: model_name == current_model,
                actions: vec![Box::new(move |tx| {
                    tx.send(AppEvent::PendingRelayModelSelection {
                        group: group_for_action.clone(),
                        model: model_for_action.clone(),
                        effort: None,
                    });
                })],
                dismiss_on_select: true,
                ..Default::default()
            });
        }

        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some(format!("分组「{group}」的可用模型")),
            subtitle: Some(format!(
                "选中即切到该分组并设为默认模型（{}/{}）",
                pricing.display.currency_symbol,
                pricing.display.token_unit,
            )),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            ..Default::default()
        });
        self.request_redraw();
    }

    fn model_menu_header(&self, title: &str, subtitle: &str) -> Box<dyn Renderable> {
        let title = title.to_string();
        let subtitle = subtitle.to_string();
        let mut header = ColumnRenderable::new();
        header.push(Line::from(title.bold()));
        header.push(Line::from(subtitle.dim()));
        if let Some(warning) = self.model_menu_warning_line() {
            header.push(warning);
        }
        Box::new(header)
    }

    fn model_menu_warning_line(&self) -> Option<Line<'static>> {
        let base_url = self.custom_openai_base_url()?;
        let warning = format!(
            "警告：OpenAI base URL 已被覆盖为 {base_url}。选择模型可能不受支持或无法正常工作。"
        );
        Some(Line::from(warning.red()))
    }

    fn custom_openai_base_url(&self) -> Option<String> {
        if !self.config.model_provider.is_openai() {
            return None;
        }

        let base_url = self.config.model_provider.base_url.as_ref()?;
        let trimmed = base_url.trim();
        if trimmed.is_empty() {
            return None;
        }

        let normalized = trimmed.trim_end_matches('/');
        if normalized == DEFAULT_OPENAI_BASE_URL {
            return None;
        }

        Some(trimmed.to_string())
    }

    pub(crate) fn open_model_popup_with_presets(&mut self, presets: Vec<ModelPreset>) {
        let presets: Vec<ModelPreset> = presets
            .into_iter()
            .filter(|preset| preset.show_in_picker)
            .collect();

        let current_model = self.current_model();
        let current_label = presets
            .iter()
            .find(|preset| preset.model.as_str() == current_model)
            .map(|preset| preset.model.to_string())
            .unwrap_or_else(|| self.model_display_name().to_string());

        let (mut auto_presets, other_presets): (Vec<ModelPreset>, Vec<ModelPreset>) = presets
            .into_iter()
            .partition(|preset| Self::is_auto_model(&preset.model));

        if auto_presets.is_empty() {
            self.open_all_models_popup(other_presets);
            return;
        }

        auto_presets.sort_by_key(|preset| Self::auto_model_order(&preset.model));
        let mut items: Vec<SelectionItem> = auto_presets
            .into_iter()
            .map(|preset| {
                let description =
                    (!preset.description.is_empty()).then_some(preset.description.clone());
                let model = preset.model.clone();
                let should_prompt_plan_mode_scope = self.should_prompt_plan_mode_reasoning_scope(
                    model.as_str(),
                    Some(preset.default_reasoning_effort.clone()),
                );
                let actions = self.model_selection_actions(
                    model.clone(),
                    Some(preset.default_reasoning_effort.clone()),
                    should_prompt_plan_mode_scope,
                );
                SelectionItem {
                    name: model.clone(),
                    description,
                    is_current: model.as_str() == current_model,
                    is_default: preset.is_default,
                    actions,
                    dismiss_on_select: true,
                    ..Default::default()
                }
            })
            .collect();

        if !other_presets.is_empty() {
            let all_models = other_presets;
            let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                tx.send(AppEvent::OpenAllModelsPopup {
                    models: all_models.clone(),
                });
            })];

            let is_current = !items.iter().any(|item| item.is_current);
            let description = Some(format!("选择特定的模型和推理级别（当前：{current_label}）"));

            items.push(SelectionItem {
                name: "全部模型".to_string(),
                description,
                is_current,
                actions,
                dismiss_on_select: true,
                ..Default::default()
            });
        }

        let header = self.model_menu_header("选择模型", "选择快捷自动模式，或浏览全部模型。");
        self.bottom_pane.show_selection_view(SelectionViewParams {
            footer_hint: Some(standard_popup_hint_line()),
            items,
            header,
            ..Default::default()
        });
    }

    fn is_auto_model(model: &str) -> bool {
        model.starts_with("codex-auto-")
    }

    fn auto_model_order(model: &str) -> usize {
        match model {
            "codex-auto-fast" => 0,
            "codex-auto-balanced" => 1,
            "codex-auto-thorough" => 2,
            _ => 3,
        }
    }

    pub(crate) fn open_all_models_popup(&mut self, presets: Vec<ModelPreset>) {
        if presets.is_empty() {
            self.add_info_message("当前没有其他可用模型。".to_string(), /*hint*/ None);
            return;
        }

        let mut items: Vec<SelectionItem> = Vec::new();
        for preset in presets.into_iter() {
            let description =
                (!preset.description.is_empty()).then_some(preset.description.to_string());
            let is_current = preset.model.as_str() == self.current_model();
            let single_supported_effort = preset.supported_reasoning_efforts.len() == 1;
            let preset_for_action = preset.clone();
            let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                let preset_for_event = preset_for_action.clone();
                tx.send(AppEvent::OpenReasoningPopup {
                    model: preset_for_event,
                });
            })];
            items.push(SelectionItem {
                name: preset.model.clone(),
                description,
                is_current,
                is_default: preset.is_default,
                actions,
                dismiss_on_select: single_supported_effort,
                dismiss_parent_on_child_accept: !single_supported_effort,
                ..Default::default()
            });
        }

        let header = self.model_menu_header(
            "选择模型和推理级别",
            "运行 codex -m <model_name> 或在 config.toml 中配置以使用旧版模型",
        );
        self.bottom_pane.show_selection_view(SelectionViewParams {
            footer_hint: Some(self.bottom_pane.standard_popup_hint_line()),
            items,
            header,
            ..Default::default()
        });
    }

    fn model_selection_actions(
        &self,
        model_for_action: String,
        effort_for_action: Option<ReasoningEffortConfig>,
        should_prompt_plan_mode_scope: bool,
    ) -> Vec<SelectionAction> {
        let warning = effort_for_action
            .as_ref()
            .and_then(|effort| self.ultra_reasoning_concurrency_warning(effort));
        vec![Box::new(move |tx| {
            if should_prompt_plan_mode_scope {
                tx.send(AppEvent::OpenPlanReasoningScopePrompt {
                    model: model_for_action.clone(),
                    effort: effort_for_action.clone(),
                });
                return;
            }

            tx.send(AppEvent::UpdateModel(model_for_action.clone()));
            tx.send(AppEvent::UpdateReasoningEffort(effort_for_action.clone()));
            tx.send(AppEvent::PersistModelSelection {
                model: model_for_action.clone(),
                effort: effort_for_action.clone(),
            });
            if let Some(warning) = warning.clone() {
                tx.send(AppEvent::InsertHistoryCell(Box::new(
                    history_cell::new_warning_event(warning),
                )));
            }
        })]
    }

    fn should_prompt_plan_mode_reasoning_scope(
        &self,
        selected_model: &str,
        selected_effort: Option<ReasoningEffortConfig>,
    ) -> bool {
        if !self.collaboration_modes_enabled()
            || self.active_mode_kind() != ModeKind::Plan
            || selected_model != self.current_model()
        {
            return false;
        }

        // Prompt whenever the selection is not a true no-op for both:
        // 1) the active Plan-mode effective reasoning, and
        // 2) the stored global defaults that would be updated by the fallback path.
        selected_effort != self.effective_reasoning_effort()
            || selected_model != self.current_collaboration_mode.model()
            || selected_effort != self.current_collaboration_mode.reasoning_effort()
    }

    pub(crate) fn open_plan_reasoning_scope_prompt(
        &mut self,
        model: String,
        effort: Option<ReasoningEffortConfig>,
    ) {
        let reasoning_phrase = match effort.as_ref() {
            Some(ReasoningEffortConfig::None) => "无推理".to_string(),
            Some(selected_effort) => {
                format!(
                    "{}推理",
                    Self::reasoning_effort_sentence_label(selected_effort)
                )
            }
            None => "所选推理".to_string(),
        };
        let plan_only_description = format!("在 Plan 模式下始终使用{reasoning_phrase}。");
        let plan_reasoning_source = if let Some(plan_override) =
            self.config.plan_mode_reasoning_effort.as_ref()
        {
            format!(
                "用户设定的 Plan 覆盖值（{}）",
                Self::reasoning_effort_sentence_label(plan_override)
            )
        } else if let Some(plan_mask) = collaboration_modes::plan_mask(self.model_catalog.as_ref())
        {
            match plan_mask
                .reasoning_effort
                .as_ref()
                .and_then(|effort| effort.as_ref())
            {
                Some(plan_effort) => format!(
                    "内置 Plan 默认值（{}）",
                    Self::reasoning_effort_sentence_label(plan_effort)
                ),
                None => "内置 Plan 默认值（无推理）".to_string(),
            }
        } else {
            "内置 Plan 默认值".to_string()
        };
        let all_modes_description = format!(
            "设置全局默认推理级别以及 Plan 模式覆盖值。这将替换当前的{plan_reasoning_source}。"
        );
        let subtitle = format!("选择在何处应用{reasoning_phrase}。");
        let warning = effort
            .as_ref()
            .and_then(|effort| self.ultra_reasoning_concurrency_warning(effort));

        let plan_only_actions: Vec<SelectionAction> = vec![Box::new({
            let model = model.clone();
            let effort = effort.clone();
            let warning = warning.clone();
            move |tx| {
                tx.send(AppEvent::UpdateModel(model.clone()));
                tx.send(AppEvent::UpdatePlanModeReasoningEffort(effort.clone()));
                tx.send(AppEvent::PersistPlanModeReasoningEffort(effort.clone()));
                if let Some(warning) = warning.clone() {
                    tx.send(AppEvent::InsertHistoryCell(Box::new(
                        history_cell::new_warning_event(warning),
                    )));
                }
            }
        })];
        let all_modes_actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
            tx.send(AppEvent::UpdateModel(model.clone()));
            tx.send(AppEvent::UpdateReasoningEffort(effort.clone()));
            tx.send(AppEvent::UpdatePlanModeReasoningEffort(effort.clone()));
            tx.send(AppEvent::PersistPlanModeReasoningEffort(effort.clone()));
            tx.send(AppEvent::PersistModelSelection {
                model: model.clone(),
                effort: effort.clone(),
            });
            if let Some(warning) = warning.clone() {
                tx.send(AppEvent::InsertHistoryCell(Box::new(
                    history_cell::new_warning_event(warning),
                )));
            }
        })];

        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some(PLAN_MODE_REASONING_SCOPE_TITLE.to_string()),
            subtitle: Some(subtitle),
            footer_hint: Some(standard_popup_hint_line()),
            items: vec![
                SelectionItem {
                    name: PLAN_MODE_REASONING_SCOPE_PLAN_ONLY.to_string(),
                    description: Some(plan_only_description),
                    actions: plan_only_actions,
                    dismiss_on_select: true,
                    ..Default::default()
                },
                SelectionItem {
                    name: PLAN_MODE_REASONING_SCOPE_ALL_MODES.to_string(),
                    description: Some(all_modes_description),
                    actions: all_modes_actions,
                    dismiss_on_select: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        self.notify(Notification::PlanModePrompt {
            title: PLAN_MODE_REASONING_SCOPE_TITLE.to_string(),
        });
    }

    /// Open a popup to choose the reasoning effort (stage 2) for the given model.
    pub(crate) fn open_reasoning_popup(&mut self, preset: ModelPreset) {
        let default_effort = preset.default_reasoning_effort;
        let supported = preset.supported_reasoning_efforts;
        let in_plan_mode =
            self.collaboration_modes_enabled() && self.active_mode_kind() == ModeKind::Plan;

        let warn_effort = if supported
            .iter()
            .any(|option| option.effort == ReasoningEffortConfig::XHigh)
        {
            Some(ReasoningEffortConfig::XHigh)
        } else if supported
            .iter()
            .any(|option| option.effort == ReasoningEffortConfig::High)
        {
            Some(ReasoningEffortConfig::High)
        } else {
            None
        };
        let warning_text = warn_effort.as_ref().map(|effort| {
            let effort_label = Self::reasoning_effort_label(effort);
            format!("⚠ {effort_label}推理级别可能会快速消耗 Plus 套餐的速率限额。")
        });
        let warn_for_model = preset.model.starts_with("gpt-5.1-codex")
            || preset.model.starts_with("gpt-5.1-codex-max")
            || preset.model.starts_with("gpt-5.2");

        let mut choices: Vec<ReasoningEffortConfig> = supported
            .iter()
            .map(|option| option.effort.clone())
            .collect();
        if choices.is_empty() {
            choices.push(default_effort.clone());
        }

        if choices.len() == 1 {
            let selected_effort = choices.first().cloned();
            let selected_model = preset.model;
            if self
                .should_prompt_plan_mode_reasoning_scope(&selected_model, selected_effort.clone())
            {
                self.app_event_tx
                    .send(AppEvent::OpenPlanReasoningScopePrompt {
                        model: selected_model,
                        effort: selected_effort,
                    });
            } else {
                self.apply_model_and_effort(selected_model, selected_effort);
            }
            return;
        }

        let default_choice = choices
            .contains(&default_effort)
            .then(|| default_effort.clone())
            .or_else(|| choices.first().cloned())
            .or(Some(default_effort));

        let model_slug = preset.model.to_string();
        let is_current_model = self.current_model() == preset.model.as_str();
        let highlight_choice = if is_current_model {
            if in_plan_mode {
                self.config
                    .plan_mode_reasoning_effort
                    .clone()
                    .or_else(|| self.effective_reasoning_effort())
            } else {
                self.effective_reasoning_effort()
            }
        } else {
            default_choice.clone()
        };
        let selection_choice = highlight_choice.clone().or_else(|| default_choice.clone());
        let initial_selected_idx = choices
            .iter()
            .position(|choice| Some(choice) == selection_choice.as_ref());
        let mut items: Vec<SelectionItem> = Vec::new();
        for choice in choices.iter() {
            let effort = choice.clone();
            let warning = self.ultra_reasoning_concurrency_warning(&effort);
            let mut effort_label = Self::reasoning_effort_label(&effort);
            if Some(choice) == default_choice.as_ref() {
                effort_label.push_str("（默认）");
            }

            let description = supported
                .iter()
                .find(|option| option.effort == effort)
                .map(|option| option.description.to_string())
                .filter(|text| !text.is_empty());

            let show_warning = warn_for_model && warn_effort.as_ref() == Some(&effort);
            let selected_description = if show_warning {
                warning_text.as_ref().map(|warning_message| {
                    description.as_ref().map_or_else(
                        || warning_message.clone(),
                        |d| format!("{d}\n{warning_message}"),
                    )
                })
            } else {
                None
            };

            let model_for_action = model_slug.clone();
            let choice_effort = Some(effort);
            let should_prompt_plan_mode_scope = self.should_prompt_plan_mode_reasoning_scope(
                model_slug.as_str(),
                choice_effort.clone(),
            );
            let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                if should_prompt_plan_mode_scope {
                    tx.send(AppEvent::OpenPlanReasoningScopePrompt {
                        model: model_for_action.clone(),
                        effort: choice_effort.clone(),
                    });
                } else {
                    tx.send(AppEvent::UpdateModel(model_for_action.clone()));
                    tx.send(AppEvent::UpdateReasoningEffort(choice_effort.clone()));
                    tx.send(AppEvent::PersistModelSelection {
                        model: model_for_action.clone(),
                        effort: choice_effort.clone(),
                    });
                    if let Some(warning) = warning.clone() {
                        tx.send(AppEvent::InsertHistoryCell(Box::new(
                            history_cell::new_warning_event(warning),
                        )));
                    }
                }
            })];

            items.push(SelectionItem {
                name: effort_label,
                description,
                selected_description,
                is_current: is_current_model && Some(choice) == highlight_choice.as_ref(),
                actions,
                dismiss_on_select: true,
                ..Default::default()
            });
        }

        let mut header = ColumnRenderable::new();
        header.push(Line::from(format!("为 {model_slug} 选择推理级别").bold()));

        self.bottom_pane.show_selection_view(SelectionViewParams {
            header: Box::new(header),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            initial_selected_idx,
            ..Default::default()
        });
    }

    pub(super) fn reasoning_effort_label(effort: &ReasoningEffortConfig) -> String {
        match effort {
            ReasoningEffortConfig::None => "无".to_string(),
            ReasoningEffortConfig::Minimal => "极简".to_string(),
            ReasoningEffortConfig::Low => "低".to_string(),
            ReasoningEffortConfig::Medium => "中".to_string(),
            ReasoningEffortConfig::High => "高".to_string(),
            ReasoningEffortConfig::XHigh => "超高".to_string(),
            ReasoningEffortConfig::Max => "最高".to_string(),
            ReasoningEffortConfig::Ultra => "极限".to_string(),
            ReasoningEffortConfig::Custom(value) => value.clone(),
        }
    }

    pub(super) fn reasoning_effort_sentence_label(effort: &ReasoningEffortConfig) -> String {
        match effort {
            ReasoningEffortConfig::Custom(value) => value.clone(),
            effort => Self::reasoning_effort_label(effort).to_lowercase(),
        }
    }

    pub(super) fn ultra_reasoning_concurrency_warning(
        &self,
        effort: &ReasoningEffortConfig,
    ) -> Option<String> {
        if effort != &ReasoningEffortConfig::Ultra {
            return None;
        }

        let max_threads = self
            .config
            .multi_agent_v2
            .max_concurrent_threads_per_session;
        if max_threads < ULTRA_REASONING_CONCURRENCY_WARNING_THRESHOLD {
            return None;
        }

        let max_subagents = max_threads.saturating_sub(1);
        Some(format!(
            "极限推理可能会主动使用多个智能体。当前会话配置为 {max_threads} 个并发线程，\
             最多 {max_subagents} 个子智能体，这会使用量快速增长。可考虑将 \
             features.multi_agent_v2.max_concurrent_threads_per_session 设置为低于 8。"
        ))
    }

    pub(super) fn apply_model_and_effort_without_persist(
        &self,
        model: String,
        effort: Option<ReasoningEffortConfig>,
    ) {
        let warning = effort
            .as_ref()
            .and_then(|effort| self.ultra_reasoning_concurrency_warning(effort));
        self.app_event_tx.send(AppEvent::UpdateModel(model));
        self.app_event_tx
            .send(AppEvent::UpdateReasoningEffort(effort));
        if let Some(warning) = warning {
            self.app_event_tx.send(AppEvent::InsertHistoryCell(Box::new(
                history_cell::new_warning_event(warning),
            )));
        }
    }

    fn apply_model_and_effort(&self, model: String, effort: Option<ReasoningEffortConfig>) {
        self.apply_model_and_effort_without_persist(model.clone(), effort.clone());
        self.app_event_tx
            .send(AppEvent::PersistModelSelection { model, effort });
    }
}
