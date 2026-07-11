//! Session headers, onboarding guidance, and transcript cards.

use super::*;
use crate::product_palette;

pub(crate) const SESSION_HEADER_MAX_INNER_WIDTH: usize = 56; // Just an eyeballed value

pub(crate) fn card_inner_width(width: u16, max_inner_width: usize) -> Option<usize> {
    if width < 4 {
        return None;
    }
    let inner_width = std::cmp::min(width.saturating_sub(4) as usize, max_inner_width);
    Some(inner_width)
}

/// Render `lines` inside a border sized to the widest span in the content.
pub(crate) fn with_border(lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    with_border_internal(lines, /*forced_inner_width*/ None)
}

/// Render `lines` inside a border whose inner width is at least `inner_width`.
///
/// This is useful when callers have already clamped their content to a
/// specific width and want the border math centralized here instead of
/// duplicating padding logic in the TUI widgets themselves.
pub(crate) fn with_border_with_inner_width(
    lines: Vec<Line<'static>>,
    inner_width: usize,
) -> Vec<Line<'static>> {
    with_border_internal(lines, Some(inner_width))
}

fn with_border_internal(
    lines: Vec<Line<'static>>,
    forced_inner_width: Option<usize>,
) -> Vec<Line<'static>> {
    let max_line_width = lines
        .iter()
        .map(|line| {
            line.iter()
                .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
                .sum::<usize>()
        })
        .max()
        .unwrap_or(0);
    let content_width = forced_inner_width
        .unwrap_or(max_line_width)
        .max(max_line_width);

    let mut out = Vec::with_capacity(lines.len() + 2);
    let border_inner_width = content_width + 2;
    out.push(vec![format!("╭{}╮", "─".repeat(border_inner_width)).dim()].into());

    for line in lines.into_iter() {
        let used_width: usize = line
            .iter()
            .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
            .sum();
        let span_count = line.spans.len();
        let mut spans: Vec<Span<'static>> = Vec::with_capacity(span_count + 4);
        spans.push(Span::from("│ ").dim());
        spans.extend(line);
        if used_width < content_width {
            spans.push(Span::from(" ".repeat(content_width - used_width)).dim());
        }
        spans.push(Span::from(" │").dim());
        out.push(Line::from(spans));
    }

    out.push(vec![format!("╰{}╯", "─".repeat(border_inner_width)).dim()].into());

    out
}

/// Return the emoji followed by a hair space (U+200A).
/// Using only the hair space avoids excessive padding after the emoji while
/// still providing a small visual gap across terminals.
pub(crate) fn padded_emoji(emoji: &str) -> String {
    format!("{emoji}\u{200A}")
}

#[derive(Debug)]
struct TooltipHistoryCell {
    tip: String,
    cwd: PathBuf,
}

impl TooltipHistoryCell {
    fn new(tip: String, cwd: &Path) -> Self {
        Self {
            tip,
            cwd: cwd.to_path_buf(),
        }
    }
}

impl HistoryCell for TooltipHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let indent = "  ";
        let indent_width = UnicodeWidthStr::width(indent);
        let wrap_width = usize::from(width.max(1))
            .saturating_sub(indent_width)
            .max(1);
        let mut lines: Vec<Line<'static>> = Vec::new();
        append_markdown(
            &format!("**提示：** {}", self.tip),
            Some(wrap_width),
            Some(self.cwd.as_path()),
            &mut lines,
        );

        prefix_lines(lines, indent.into(), indent.into())
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        vec![Line::from(format!("提示: {}", self.tip))]
    }
}

#[derive(Debug)]
pub struct SessionInfoCell(CompositeHistoryCell);

impl HistoryCell for SessionInfoCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        self.0.display_lines(width)
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.0.desired_height(width)
    }

    fn transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        self.0.transcript_lines(width)
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        self.0.raw_lines()
    }
}

pub(crate) fn new_session_info(
    config: &Config,
    requested_model: &str,
    session: &ThreadSessionState,
    is_first_event: bool,
    tooltip_override: Option<String>,
    auth_plan: Option<PlanType>,
    show_fast_status: bool,
) -> SessionInfoCell {
    let mut parts: Vec<Box<dyn HistoryCell>> = Vec::new();

    if is_first_event {
        // Full Logo header box — only for fresh sessions. /clear, resume, fork, and
        // replay skip the Logo entirely and rely on the compact session summary that
        // the caller appends separately.
        let header = SessionHeaderHistoryCell::new(
            session.model.clone(),
            session.reasoning_effort.clone(),
            show_fast_status,
            config.cwd.to_path_buf(),
            CODEX_CLI_VERSION,
        )
        .with_yolo_mode(has_yolo_permissions(
            session.approval_policy,
            &session.permission_profile,
        ));
        parts.push(Box::new(header));

        // Help lines below the header (new copy and list)
        let help_lines: Vec<Line<'static>> = vec![
            "  开始前，请描述任务或尝试以下命令之一：".dim().into(),
            Line::from(""),
            Line::from(vec![
                "  ".into(),
                "/init".into(),
                " - 创建包含 naicode 使用说明的 AGENTS.md 文件".dim(),
            ]),
            Line::from(vec![
                "  ".into(),
                "/status".into(),
                " - 显示当前会话配置".dim(),
            ]),
            Line::from(vec![
                "  ".into(),
                "/permissions".into(),
                " - 选择允许 naicode 执行的操作".dim(),
            ]),
            Line::from(vec![
                "  ".into(),
                "/model".into(),
                " - 选择使用的模型和推理强度".dim(),
            ]),
            Line::from(vec![
                "  ".into(),
                "/review".into(),
                " - 审查改动并发现问题".dim(),
            ]),
        ];

        parts.push(Box::new(PlainHistoryCell { lines: help_lines }));
    } else {
        // No Logo for /clear, resume, fork, or replay. Only show tooltips or a
        // model-change notice when relevant.
        if config.show_tooltips
            && let Some(tooltips) = tooltip_override
                .or_else(|| tooltips::get_tooltip(auth_plan, show_fast_status))
                .map(|tip| TooltipHistoryCell::new(tip, &config.cwd))
        {
            parts.push(Box::new(tooltips));
        }
        if requested_model != session.model.as_str() {
            let lines = vec![
                "模型已更改：".magenta().bold().into(),
                format!("请求的模型：{requested_model}").into(),
                format!("实际使用：{}", session.model).into(),
            ];
            parts.push(Box::new(PlainHistoryCell { lines }));
        }
    }

    SessionInfoCell(CompositeHistoryCell { parts })
}

pub(crate) fn is_yolo_mode(config: &Config) -> bool {
    has_yolo_permissions(
        AskForApproval::from(config.permissions.approval_policy.value()),
        &config.permissions.effective_permission_profile(),
    )
}

pub(crate) fn has_yolo_permissions(
    approval_policy: AskForApproval,
    permission_profile: &PermissionProfile,
) -> bool {
    approval_policy == AskForApproval::Never
        && matches!(
            permission_profile,
            PermissionProfile::Disabled
                | PermissionProfile::Managed {
                    file_system: ManagedFileSystemPermissions::Unrestricted,
                    network: NetworkSandboxPolicy::Enabled,
                }
        )
}
#[derive(Debug)]
pub(crate) struct SessionHeaderHistoryCell {
    version: &'static str,
    model: String,
    model_style: Style,
    reasoning_effort: Option<ReasoningEffortConfig>,
    show_fast_status: bool,
    directory: PathBuf,
    yolo_mode: bool,
}

impl SessionHeaderHistoryCell {
    pub(crate) fn new(
        model: String,
        reasoning_effort: Option<ReasoningEffortConfig>,
        show_fast_status: bool,
        directory: PathBuf,
        version: &'static str,
    ) -> Self {
        Self::new_with_style(
            model,
            Style::default(),
            reasoning_effort,
            show_fast_status,
            directory,
            version,
        )
    }

    pub(crate) fn new_with_style(
        model: String,
        model_style: Style,
        reasoning_effort: Option<ReasoningEffortConfig>,
        show_fast_status: bool,
        directory: PathBuf,
        version: &'static str,
    ) -> Self {
        Self {
            version,
            model,
            model_style,
            reasoning_effort,
            show_fast_status,
            directory,
            yolo_mode: false,
        }
    }

    pub(crate) fn with_yolo_mode(mut self, yolo_mode: bool) -> Self {
        self.yolo_mode = yolo_mode;
        self
    }

    fn format_directory(&self, max_width: Option<usize>) -> String {
        Self::format_directory_inner(&self.directory, max_width)
    }

    pub(crate) fn format_directory_inner(directory: &Path, max_width: Option<usize>) -> String {
        let formatted = if let Some(rel) = relativize_to_home(directory) {
            if rel.as_os_str().is_empty() {
                "~".to_string()
            } else {
                format!("~{}{}", std::path::MAIN_SEPARATOR, rel.display())
            }
        } else {
            directory.display().to_string()
        };

        if let Some(max_width) = max_width {
            if max_width == 0 {
                return String::new();
            }
            if UnicodeWidthStr::width(formatted.as_str()) > max_width {
                return crate::text_formatting::center_truncate_path(&formatted, max_width);
            }
        }

        formatted
    }

    fn reasoning_label(&self) -> Option<&str> {
        self.reasoning_effort
            .as_ref()
            .map(ReasoningEffortConfig::as_str)
    }
}

impl HistoryCell for SessionHeaderHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        if width < 6 {
            return Vec::new();
        }
        let w = width as usize;

        let palette = product_palette::current();
        let logo_style = Style::default().fg(palette.accent).bold();
        let brand_style = Style::default().fg(palette.accent_bright).bold();
        let accent = Style::default().fg(palette.accent);
        let muted = Style::default().fg(palette.border_muted);
        let dim_plain = Style::default().add_modifier(Modifier::DIM);

        // ASCII art logo rows (each ~84 cols). Shown only on wide terminals.
        const ART: &[&str] = &[
            "                               _..._       .-'''-.                                     ",
            "                            .-'_..._''.   '   _    \\ _______                           ",
            "   _..._            .--.  .' .'      '.\\/   /` '.   \\\\  ___ `'.         __.....__      ",
            " .'     '.          |__| / .'          .   |     \\  ' ' |--.\\ \\    .-''         '.    ",
            " .   .-.   .        .--.. '            |   '      |  '| |    \\  '  /     .-''\"'-.  `.  ",
            " |  '   '  |    __  |  || |            \\    \\     / / | |     |  '/     /________\\   \\ ",
            " |  |   |  | .:--.'. |  || |             `.   ` ..' /  | |     |  ||                  |",
            " |  |   |  |/ |   \\ ||  |. '                '-...-'`   | |     ' .'\\ .-------------'",
            " |  |   |  |`\" __ | ||  | \\ '.          .              | |___.' /'  \\    '-.____...---.",
            " |  |   |  | .'.''| ||__|  '. `._____.-'/             /_______.'/    `.             .'  ",
            " |  |   |  |/ /   | |_       `-.______ /              \\_______|/       `''-...... -'    ",
            " |  |   |  |\\ \\._,\\ '/                `                                                 ",
            " '--'   '--' `--'  `\"                                                                   ",
        ];
        const ART_MIN_WIDTH: usize = 86;

        let mut out: Vec<Line<'static>> = Vec::new();

        if w >= ART_MIN_WIDTH {
            for art_line in ART {
                let line_w = UnicodeWidthStr::width(*art_line);
                let row = if line_w < w {
                    format!("{}{}", art_line, " ".repeat(w - line_w))
                } else {
                    art_line.chars().take(w).collect()
                };
                out.push(Line::from(Span::styled(row, logo_style)));
            }
        } else {
            // Narrow terminal: compact one-line logo
            out.push(Line::from(vec![
                Span::styled("*** NAICODE ***", logo_style),
                Span::raw("  "),
                Span::styled("酸奶中转站", brand_style),
                Span::raw("  "),
                Span::styled(format!("v{}", self.version), dim_plain),
            ]));
        }

        // Separator
        out.push(Line::from(Span::styled(
            "─".repeat(w.min(ART_MIN_WIDTH)),
            muted,
        )));

        // Info line
        let reasoning_label = self.reasoning_label();
        let model_part: String = {
            let mut s = self.model.clone();
            if let Some(r) = reasoning_label {
                s.push_str(&format!("  {r}"));
            }
            if self.show_fast_status {
                s.push_str("  fast");
            }
            s
        };
        let dir_max = (w / 3).max(8);
        let dir = self.format_directory(Some(dir_max));

        let mut info_spans: Vec<Span<'static>> = vec![
            Span::styled("模型 ", muted),
            Span::raw(model_part),
            Span::styled("   ·   ", muted),
            Span::styled("目录 ", muted),
            Span::raw(dir),
            Span::styled("   ·   ", muted),
            Span::styled("/model", accent),
            Span::styled(" 切换", dim_plain),
        ];
        if self.yolo_mode {
            info_spans.push(Span::styled(
                "   YOLO 模式",
                Style::default()
                    .fg(palette.status_warning)
                    .bold(),
            ));
        }
        out.push(Line::from(info_spans));
        out.push(Line::from(""));
        out
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        let mut lines = vec![
            Line::from(format!("naicode (v{})", self.version)),
            Line::from(format!(
                "model: {}{}",
                self.model,
                self.reasoning_label()
                    .map(|reasoning| format!(" {reasoning}"))
                    .unwrap_or_default()
            )),
            Line::from(format!(
                "directory: {}",
                self.format_directory(/*max_width*/ None)
            )),
        ];
        if self.yolo_mode {
            lines.push(Line::from("permissions: YOLO 模式"));
        }
        lines
    }
}
