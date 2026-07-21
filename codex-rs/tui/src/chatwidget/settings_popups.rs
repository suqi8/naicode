//! Settings-adjacent popup surfaces for `ChatWidget`.
//!
//! This keeps theme, personality, and experimental-feature UI out of the main
//! orchestration module without changing their event wiring.

use super::*;

fn collect_config_schema_paths(
    schema: &serde_json::Value,
    root: &serde_json::Value,
    prefix: &str,
    depth: usize,
    paths: &mut std::collections::BTreeSet<String>,
) {
    if depth > 12 {
        if !prefix.is_empty() {
            paths.insert(format!("config.{prefix}"));
        }
        return;
    }

    if let Some(reference) = schema.get("$ref").and_then(serde_json::Value::as_str)
        && let Some(name) = reference.strip_prefix("#/definitions/")
        && let Some(definition) = root.get("definitions").and_then(|defs| defs.get(name))
    {
        collect_config_schema_paths(definition, root, prefix, depth + 1, paths);
        return;
    }

    if let Some(properties) = schema
        .get("properties")
        .and_then(serde_json::Value::as_object)
        && !properties.is_empty()
    {
        for (key, property) in properties {
            let path = if prefix.is_empty() {
                key.to_string()
            } else {
                format!("{prefix}.{key}")
            };
            collect_config_schema_paths(property, root, &path, depth + 1, paths);
        }
        return;
    }

    for keyword in ["allOf", "anyOf", "oneOf"] {
        if let Some(variants) = schema.get(keyword).and_then(serde_json::Value::as_array) {
            for variant in variants {
                collect_config_schema_paths(variant, root, prefix, depth + 1, paths);
            }
            return;
        }
    }

    if !prefix.is_empty() {
        paths.insert(format!("config.{prefix}"));
    }
}

fn flatten_config_value(value: &toml::Value, prefix: &str, rows: &mut Vec<(String, String)>) {
    match value {
        toml::Value::Table(table) => {
            for (key, value) in table {
                let path = if prefix.is_empty() {
                    key.to_string()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten_config_value(value, &path, rows);
            }
        }
        _ => rows.push((format!("config.{prefix}"), value.to_string())),
    }
}

fn redact_config_value(path: &str, value: &str) -> String {
    let sensitive = ["token", "secret", "password", "api_key", "apikey"];
    if sensitive
        .iter()
        .any(|needle| path.to_ascii_lowercase().contains(needle))
    {
        "<已隐藏>".to_string()
    } else {
        value.to_string()
    }
}

impl ChatWidget {
    pub(crate) fn open_config_popup(&mut self) {
        let enabled = self
            .config
            .config_layer_stack
            .effective_config()
            .get("relay")
            .and_then(toml::Value::as_table)
            .and_then(|relay| relay.get("auto_select_lowest_ratio"))
            .and_then(toml::Value::as_bool)
            .unwrap_or(true);
        let mut items = vec![SelectionItem {
            name: format!(
                "Relay 自动最低倍率：{}",
                if enabled { "开启" } else { "关闭" }
            ),
            description: Some(
                "请求当前模型时自动选择可用的最低倍率分组；关闭后保留手动选择。".to_string(),
            ),
            is_current: enabled,
            actions: vec![Box::new(move |tx| {
                tx.send(AppEvent::PersistRelayAutoSelection { enabled: !enabled });
            })],
            dismiss_on_select: true,
            ..Default::default()
        }];

        let mut rows = Vec::new();
        flatten_config_value(
            &self.config.config_layer_stack.effective_config(),
            "",
            &mut rows,
        );
        let mut current_values = rows
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>();
        current_values
            .entry("config.relay.auto_select_lowest_ratio".to_string())
            .or_insert_with(|| enabled.to_string());

        let mut available_paths = std::collections::BTreeSet::new();
        if let Ok(schema) = serde_json::to_value(codex_config::schema::config_schema()) {
            collect_config_schema_paths(&schema, &schema, "", 0, &mut available_paths);
        }
        available_paths.extend(current_values.keys().cloned());
        items.extend(available_paths.into_iter().map(|path| {
            SelectionItem {
                name: path.clone(),
                description: Some(
                    current_values
                        .get(&path)
                        .map(|value| redact_config_value(&path, value))
                        .unwrap_or_else(|| "<未设置>".to_string()),
                ),
                is_disabled: true,
                ..Default::default()
            }
        }));

        let mut header = ColumnRenderable::new();
        header.push(Line::from("配置".bold()));
        header.push(Line::from(
            "显示当前有效的 config.xxx；可编辑项使用 Codex 原有配置格式。".dim(),
        ));
        self.bottom_pane.show_selection_view(SelectionViewParams {
            header: Box::new(header),
            items,
            is_searchable: true,
            search_placeholder: Some("筛选 config.xxx".to_string()),
            footer_hint: Some(standard_popup_hint_line()),
            ..Default::default()
        });
    }

    pub(super) fn open_theme_picker(&mut self) {
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
