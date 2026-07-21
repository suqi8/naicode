//! Settings-adjacent popup surfaces for `ChatWidget`.
//!
//! This keeps theme, personality, and experimental-feature UI out of the main
//! orchestration module without changing their event wiring.

use super::*;
use crate::bottom_pane::custom_prompt_view::CustomPromptView;

fn relay_ratio(table: Option<&toml::map::Map<String, toml::Value>>, key: &str) -> f64 {
    table
        .and_then(|relay| relay.get(key))
        .and_then(|value| {
            value
                .as_float()
                .or_else(|| value.as_integer().map(|value| value as f64))
        })
        .unwrap_or(0.0)
}

fn ratio_label(value: f64) -> String {
    if value > 0.0 {
        value.to_string()
    } else {
        "不限".to_string()
    }
}

impl ChatWidget {
    pub(crate) fn open_config_popup(&mut self) {
        let effective_config = self.config.config_layer_stack.effective_config();
        let relay = effective_config
            .get("relay")
            .and_then(toml::Value::as_table);
        let enabled = relay
            .and_then(|relay| relay.get("auto_switch_enabled"))
            .and_then(toml::Value::as_bool)
            .or_else(|| {
                relay
                    .and_then(|relay| relay.get("auto_select_lowest_ratio"))
                    .and_then(toml::Value::as_bool)
            })
            .unwrap_or(true);
        let min_ratio = relay_ratio(relay, "min_group_ratio");
        let max_ratio = relay_ratio(relay, "max_group_ratio");
        let mut items = vec![SelectionItem {
            name: format!("[{}] 自动选择可用分组", if enabled { "x" } else { " " }),
            description: Some(
                "按倍率从低到高选择支持当前模型的分组；失败时由中转站自动重试。".to_string(),
            ),
            is_current: enabled,
            actions: vec![Box::new(move |tx| {
                tx.send(AppEvent::PersistRelayRouting {
                    enabled: !enabled,
                    min_ratio,
                    max_ratio,
                });
            })],
            dismiss_on_select: true,
            ..Default::default()
        }];

        if enabled {
            items.push(SelectionItem {
                name: format!("最低分组倍率：{}", ratio_label(min_ratio)),
                description: Some("低于此倍率的分组不会参与自动选择；0 表示不限制。".to_string()),
                actions: vec![Box::new(move |tx| {
                    tx.send(AppEvent::OpenRelayRatioPrompt {
                        edit_minimum: true,
                        current: min_ratio,
                        other: max_ratio,
                    });
                })],
                dismiss_on_select: true,
                ..Default::default()
            });
            items.push(SelectionItem {
                name: format!("最高分组倍率：{}", ratio_label(max_ratio)),
                description: Some("高于此倍率的分组不会参与自动选择；0 表示不限制。".to_string()),
                actions: vec![Box::new(move |tx| {
                    tx.send(AppEvent::OpenRelayRatioPrompt {
                        edit_minimum: false,
                        current: max_ratio,
                        other: min_ratio,
                    });
                })],
                dismiss_on_select: true,
                ..Default::default()
            });
        }

        items.extend([
            SelectionItem {
                name: "模型与推理强度".to_string(),
                description: Some("选择默认模型，并设置模型支持的思考等级。".to_string()),
                actions: vec![Box::new(|tx| tx.send(AppEvent::OpenRelayModelPicker))],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "权限与沙箱".to_string(),
                description: Some("配置命令审批方式、文件访问范围和网络权限。".to_string()),
                actions: vec![Box::new(|tx| tx.send(AppEvent::OpenPermissionsPopup))],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "技能管理".to_string(),
                description: Some("启用或关闭已安装的技能。".to_string()),
                actions: vec![Box::new(|tx| tx.send(AppEvent::OpenManageSkillsPopup))],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "界面主题".to_string(),
                description: Some("选择 NaiCode 的配色主题。".to_string()),
                actions: vec![Box::new(|tx| tx.send(AppEvent::OpenThemeSettings))],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "快捷键".to_string(),
                description: Some("查看和修改键盘操作。".to_string()),
                actions: vec![Box::new(|tx| tx.send(AppEvent::OpenKeymapSettings))],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "实验功能".to_string(),
                description: Some("启用或关闭尚在测试中的功能。".to_string()),
                actions: vec![Box::new(|tx| tx.send(AppEvent::OpenExperimentalSettings))],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "记忆".to_string(),
                description: Some("配置会话记忆的读取、生成与清理。".to_string()),
                actions: vec![Box::new(|tx| tx.send(AppEvent::OpenMemoriesSettings))],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "MCP 服务器".to_string(),
                description: Some("查看工具服务器及其登录状态。".to_string()),
                actions: vec![Box::new(|tx| tx.send(AppEvent::OpenMcpSettings))],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "应用连接器".to_string(),
                description: Some("查看可连接的外部应用。".to_string()),
                actions: vec![Box::new(|tx| tx.send(AppEvent::OpenAppsSettings))],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "插件".to_string(),
                description: Some("查看和管理 NaiCode 插件。".to_string()),
                actions: vec![Box::new(|tx| tx.send(AppEvent::OpenPluginsSettings))],
                dismiss_on_select: true,
                ..Default::default()
            },
        ]);

        let mut header = ColumnRenderable::new();
        header.push(Line::from("配置".bold()));
        header.push(Line::from("使用上下键选择，按 Enter 修改。".dim()));
        self.bottom_pane.show_selection_view(SelectionViewParams {
            header: Box::new(header),
            items,
            footer_hint: Some(standard_popup_hint_line()),
            ..Default::default()
        });
    }

    pub(crate) fn open_relay_ratio_prompt(&mut self, edit_minimum: bool, current: f64, other: f64) {
        let tx = self.app_event_tx.clone();
        let title = if edit_minimum {
            "设置最低分组倍率"
        } else {
            "设置最高分组倍率"
        };
        let view = CustomPromptView::new(
            title.to_string(),
            "输入倍率，0 表示不限制".to_string(),
            current.to_string(),
            Some("例如：0.1、0.15；按 Enter 保存".to_string()),
            Box::new(move |text| {
                let Ok(value) = text.trim().parse::<f64>() else {
                    return;
                };
                let (min_ratio, max_ratio) = if edit_minimum {
                    (value, other)
                } else {
                    (other, value)
                };
                tx.send(AppEvent::PersistRelayRouting {
                    enabled: true,
                    min_ratio,
                    max_ratio,
                });
            }),
        );
        self.bottom_pane.show_view(Box::new(view));
    }

    pub(crate) fn open_theme_picker(&mut self) {
        let codex_home = codex_utils_home_dir::find_codex_home().ok();
        let terminal_width = self
            .last_rendered_width
            .get()
            .and_then(|width| u16::try_from(width).ok());
        let params = crate::theme_picker::build_theme_picker_params(
            self.config.tui_theme.as_deref(),
            codex_home.as_deref(),
            terminal_width,
        );
        self.bottom_pane.show_selection_view(params);
    }

    pub(crate) fn open_personality_popup(&mut self) {
        if !self.is_session_configured() {
            self.add_info_message(
                "个性选择在启动完成前不可用。".to_string(),
                /*hint*/ None,
            );
            return;
        }
        if !self.current_model_supports_personality() {
            let current_model = self.current_model();
            self.add_error_message(format!(
                "当前模型（{current_model}）不支持个性。请使用 /model 选择其他模型。"
            ));
            return;
        }
        self.open_personality_popup_for_current_model();
    }

    fn open_personality_popup_for_current_model(&mut self) {
        let current_personality = self.config.personality.unwrap_or(Personality::Friendly);
        let personalities = [Personality::Friendly, Personality::Pragmatic];
        let supports_personality = self.current_model_supports_personality();

        let items: Vec<SelectionItem> = personalities
            .into_iter()
            .map(|personality| {
                let name = Self::personality_label(personality).to_string();
                let description = Some(Self::personality_description(personality).to_string());
                let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                    tx.send(AppEvent::CodexOp(AppCommand::override_turn_context(
                        /*cwd*/ None,
                        /*approval_policy*/ None,
                        /*approvals_reviewer*/ None,
                        /*permission_profile*/ None,
                        /*active_permission_profile*/ None,
                        /*windows_sandbox_level*/ None,
                        /*model*/ None,
                        /*effort*/ None,
                        /*summary*/ None,
                        /*service_tier*/ None,
                        /*collaboration_mode*/ None,
                        Some(personality),
                    )));
                    tx.send(AppEvent::UpdatePersonality(personality));
                    tx.send(AppEvent::PersistPersonalitySelection { personality });
                })];
                SelectionItem {
                    name,
                    description,
                    is_current: current_personality == personality,
                    is_disabled: !supports_personality,
                    actions,
                    dismiss_on_select: true,
                    ..Default::default()
                }
            })
            .collect();

        let mut header = ColumnRenderable::new();
        header.push(Line::from("选择个性".bold()));
        header.push(Line::from("为 naicode 选择一种交流风格。".dim()));

        self.bottom_pane.show_selection_view(SelectionViewParams {
            header: Box::new(header),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            ..Default::default()
        });
    }

    pub(crate) fn open_experimental_popup(&mut self) {
        let features: Vec<ExperimentalFeatureItem> = FEATURES
            .iter()
            .filter_map(|spec| {
                let name = spec.stage.experimental_menu_name()?;
                let description = spec.stage.experimental_menu_description()?;
                Some(ExperimentalFeatureItem {
                    feature: spec.id,
                    name: name.to_string(),
                    description: description.to_string(),
                    enabled: self.config.features.enabled(spec.id),
                })
            })
            .collect();

        let view = ExperimentalFeaturesView::new(
            features,
            self.app_event_tx.clone(),
            self.bottom_pane.list_keymap(),
        );
        self.bottom_pane.show_view(Box::new(view));
    }

    fn personality_label(personality: Personality) -> &'static str {
        match personality {
            Personality::None => "无",
            Personality::Friendly => "友好",
            Personality::Pragmatic => "务实",
        }
    }

    fn personality_description(personality: Personality) -> &'static str {
        match personality {
            Personality::None => "没有个性指令。",
            Personality::Friendly => "热情、协作、乐于助人。",
            Personality::Pragmatic => "简洁、专注任务、直接。",
        }
    }
}
