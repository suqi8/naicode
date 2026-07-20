//! Catalog and accessors for keymap actions shown by `/keymap`.
//!
//! The descriptor table is the single UI-facing inventory of configurable
//! actions. Each descriptor ties together the config path segment, user-facing
//! context label, stable action name, and short description used by the picker
//! and action menu.
//!
//! The accessors below deliberately mirror the descriptor table for both the
//! editable root config and the resolved runtime keymap. Keeping those matches
//! in one module makes it easier to audit a new action: if it appears in the
//! catalog, it must also be readable from runtime state and writable in
//! `TuiKeymap`.

use std::collections::BTreeSet;

use codex_config::types::KeybindingsSpec;
use codex_config::types::TuiKeymap;
use crossterm::event::KeyEvent;

use crate::key_hint::KeyBinding;
use crate::keymap::RuntimeKeymap;

#[derive(Clone, Copy, Debug)]
pub(super) struct KeymapActionDescriptor {
    /// Config context segment, such as `composer` in `tui.keymap.composer.submit`.
    pub(super) context: &'static str,
    /// Human-readable group label shown in the picker.
    pub(super) context_label: &'static str,
    /// Config action segment, such as `submit` in `tui.keymap.composer.submit`.
    pub(super) action: &'static str,
    /// Short user-facing explanation of what the action does.
    pub(super) description: &'static str,
    /// Feature required before the action appears in `/keymap`.
    required_feature: Option<KeymapActionFeature>,
}

const fn action(
    context: &'static str,
    context_label: &'static str,
    action: &'static str,
    description: &'static str,
) -> KeymapActionDescriptor {
    KeymapActionDescriptor {
        context,
        context_label,
        action,
        description,
        required_feature: None,
    }
}

const fn gated_action(
    context: &'static str,
    context_label: &'static str,
    action: &'static str,
    description: &'static str,
    required_feature: KeymapActionFeature,
) -> KeymapActionDescriptor {
    KeymapActionDescriptor {
        context,
        context_label,
        action,
        description,
        required_feature: Some(required_feature),
    }
}

#[derive(Clone, Copy, Debug)]
enum KeymapActionFeature {
    FastMode,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct KeymapActionFilter {
    pub(crate) fast_mode_enabled: bool,
}

impl KeymapActionDescriptor {
    pub(super) fn is_visible(self, filter: KeymapActionFilter) -> bool {
        match self.required_feature {
            None => true,
            Some(KeymapActionFeature::FastMode) => filter.fast_mode_enabled,
        }
    }
}

#[rustfmt::skip]
pub(super) const KEYMAP_ACTIONS: &[KeymapActionDescriptor] = &[
    action("global", "Global", "open_transcript", "打开转录记录浮层。"),
    action("global", "Global", "open_external_editor", "在外部编辑器中打开当前草稿。"),
    action("global", "Global", "copy", "将最近一条智能体回复复制到剪贴板。"),
    action("global", "Global", "clear_terminal", "清空终端界面。"),
    action("global", "Global", "toggle_vim_mode", "开启或关闭 Vim 输入模式。"),
    gated_action("global", "Global", "toggle_fast_mode", "开启或关闭快速模式。", KeymapActionFeature::FastMode),
    action("global", "Global", "toggle_raw_output", "切换原始回滚输出模式。"),
    action("chat", "Chat", "interrupt_turn", "中断当前回合。"),
    action("chat", "Chat", "decrease_reasoning_effort", "降低推理强度。"),
    action("chat", "Chat", "increase_reasoning_effort", "提高推理强度。"),
    action("chat", "Chat", "edit_queued_message", "编辑最近排队的消息。"),
    action("composer", "Composer", "submit", "提交当前输入框草稿。"),
    action("composer", "Composer", "queue", "在任务运行时将草稿加入队列。"),
    action("composer", "Composer", "toggle_shortcuts", "显示或隐藏输入框快捷键浮层。"),
    action("composer", "Composer", "history_search_previous", "打开历史搜索或跳到上一个匹配项。"),
    action("composer", "Composer", "history_search_next", "跳到下一个历史搜索匹配项。"),
    action("editor", "Editor", "insert_newline", "在编辑器中插入换行。"),
    action("editor", "Editor", "move_left", "向左移动光标。"),
    action("editor", "Editor", "move_right", "向右移动光标。"),
    action("editor", "Editor", "move_up", "向上移动光标。"),
    action("editor", "Editor", "move_down", "向下移动光标。"),
    action("editor", "Editor", "move_word_left", "移动到上一个词的开头。"),
    action("editor", "Editor", "move_word_right", "移动到下一个词的结尾。"),
    action("editor", "Editor", "move_line_start", "移动到行首。"),
    action("editor", "Editor", "move_line_end", "移动到行尾。"),
    action("editor", "Editor", "delete_backward", "向左删除一个字符。"),
    action("editor", "Editor", "delete_forward", "向右删除一个字符。"),
    action("editor", "Editor", "delete_backward_word", "删除上一个词。"),
    action("editor", "Editor", "delete_forward_word", "删除下一个词。"),
    action("editor", "Editor", "kill_line_start", "删除从光标到行首的内容。"),
    action("editor", "Editor", "kill_whole_line", "删除当前行。"),
    action("editor", "Editor", "kill_line_end", "删除从光标到行尾的内容。"),
    action("editor", "Editor", "yank", "粘贴删除缓冲区内容。"),
    action("vim_normal", "Vim normal", "enter_insert", "在光标处进入插入模式。"),
    action("vim_normal", "Vim normal", "append_after_cursor", "在光标之后进入插入模式。"),
    action("vim_normal", "Vim normal", "append_line_end", "在行尾进入插入模式。"),
    action("vim_normal", "Vim normal", "insert_line_start", "在第一个非空白字符处进入插入模式。"),
    action("vim_normal", "Vim normal", "open_line_below", "在下方新开一行并进入插入模式。"),
    action("vim_normal", "Vim normal", "open_line_above", "在上方新开一行并进入插入模式。"),
    action("vim_normal", "Vim normal", "move_left", "在 Vim 普通模式下向左移动。"),
    action("vim_normal", "Vim normal", "move_right", "在 Vim 普通模式下向右移动。"),
    action("vim_normal", "Vim normal", "move_up", "在 Vim 普通模式下向上移动或调出更早的历史。"),
    action("vim_normal", "Vim normal", "move_down", "在 Vim 普通模式下向下移动或调出更新的历史。"),
    action("vim_normal", "Vim normal", "move_word_forward", "移动到下一个词的开头。"),
    action("vim_normal", "Vim normal", "move_word_backward", "移动到上一个词的开头。"),
    action("vim_normal", "Vim normal", "move_word_end", "移动到当前或下一个词的结尾。"),
    action("vim_normal", "Vim normal", "move_line_start", "移动到行首。"),
    action("vim_normal", "Vim normal", "move_line_end", "移动到行尾。"),
    action("vim_normal", "Vim normal", "delete_char", "删除光标下的字符。"),
    action("vim_normal", "Vim normal", "substitute_char", "删除光标下的字符并进入插入模式。"),
    action("vim_normal", "Vim normal", "delete_to_line_end", "删除从光标到行尾的内容。"),
    action("vim_normal", "Vim normal", "change_to_line_end", "更改从光标到行尾的内容并进入插入模式。"),
    action("vim_normal", "Vim normal", "yank_line", "复制整行。"),
    action("vim_normal", "Vim normal", "paste_after", "在光标之后粘贴。"),
    action("vim_normal", "Vim normal", "start_delete_operator", "开始删除操作符并等待一个动作。"),
    action("vim_normal", "Vim normal", "start_yank_operator", "开始复制操作符并等待一个动作。"),
    action("vim_normal", "Vim normal", "start_change_operator", "开始更改操作符并等待一个文本对象。"),
    action("vim_normal", "Vim normal", "cancel_operator", "取消待定的 Vim 操作符。"),
    action("vim_operator", "Vim operator", "delete_line", "重复删除操作符以删除整行。"),
    action("vim_operator", "Vim operator", "yank_line", "重复复制操作符以复制整行。"),
    action("vim_operator", "Vim operator", "motion_left", "操作符动作向左。"),
    action("vim_operator", "Vim operator", "motion_right", "操作符动作向右。"),
    action("vim_operator", "Vim operator", "motion_up", "操作符动作向上。"),
    action("vim_operator", "Vim operator", "motion_down", "操作符动作向下。"),
    action("vim_operator", "Vim operator", "motion_word_forward", "操作符动作到下一个词的开头。"),
    action("vim_operator", "Vim operator", "motion_word_backward", "操作符动作到上一个词的开头。"),
    action("vim_operator", "Vim operator", "motion_word_end", "操作符动作到词的结尾。"),
    action("vim_operator", "Vim operator", "motion_line_start", "操作符动作到行首。"),
    action("vim_operator", "Vim operator", "motion_line_end", "操作符动作到行尾。"),
    action("vim_operator", "Vim operator", "select_inner_text_object", "选择内部文本对象。"),
    action("vim_operator", "Vim operator", "select_around_text_object", "选择外围文本对象。"),
    action("vim_operator", "Vim operator", "cancel", "取消待定的操作符。"),
    action("vim_text_object", "Vim text object", "word", "以当前词为目标。"),
    action("vim_text_object", "Vim text object", "big_word", "以当前 WORD 为目标。"),
    action("vim_text_object", "Vim text object", "parentheses", "以包围的圆括号为目标。"),
    action("vim_text_object", "Vim text object", "brackets", "以包围的方括号为目标。"),
    action("vim_text_object", "Vim text object", "braces", "以包围的花括号为目标。"),
    action("vim_text_object", "Vim text object", "double_quote", "以包围的双引号为目标。"),
    action("vim_text_object", "Vim text object", "single_quote", "以包围的单引号为目标。"),
    action("vim_text_object", "Vim text object", "backtick", "以包围的反引号为目标。"),
    action("vim_text_object", "Vim text object", "cancel", "取消待定的文本对象。"),
    action("pager", "Pager", "scroll_up", "向上滚动一行。"),
    action("pager", "Pager", "scroll_down", "向下滚动一行。"),
    action("pager", "Pager", "page_up", "向上滚动一页。"),
    action("pager", "Pager", "page_down", "向下滚动一页。"),
    action("pager", "Pager", "half_page_up", "向上滚动半页。"),
    action("pager", "Pager", "half_page_down", "向下滚动半页。"),
    action("pager", "Pager", "jump_top", "跳到开头。"),
    action("pager", "Pager", "jump_bottom", "跳到末尾。"),
    action("pager", "Pager", "close", "关闭分页浮层。"),
    action("pager", "Pager", "close_transcript", "关闭转录记录浮层。"),
    action("list", "List", "move_up", "向上移动列表选择。"),
    action("list", "List", "move_down", "向下移动列表选择。"),
    action("list", "List", "move_left", "在列表选择器中向左横向移动。"),
    action("list", "List", "move_right", "在列表选择器中向右横向移动。"),
    action("list", "List", "page_up", "向上翻一页移动列表选择。"),
    action("list", "List", "page_down", "向下翻一页移动列表选择。"),
    action("list", "List", "jump_top", "跳到第一个列表项。"),
    action("list", "List", "jump_bottom", "跳到最后一个列表项。"),
    action("list", "List", "accept", "接受当前列表选择。"),
    action("list", "List", "cancel", "取消并关闭选择视图。"),
    action("approval", "Approval", "open_fullscreen", "全屏打开审批详情。"),
    action("approval", "Approval", "open_thread", "在可用时打开审批来源线程。"),
    action("approval", "Approval", "approve", "批准首选选项。"),
    action("approval", "Approval", "approve_for_session", "在可用时按会话批准。"),
    action("approval", "Approval", "approve_for_prefix", "在可用时以执行策略前缀批准。"),
    action("approval", "Approval", "deny", "在可用时选择显式拒绝选项。"),
    action("approval", "Approval", "decline", "拒绝并提供纠正性指导。"),
    action("approval", "Approval", "cancel", "取消一个征询请求。"),
];

/// Convert a stable action identifier into a display label.
///
/// This is intentionally presentation-only: the returned string must never be
/// parsed back into an action name, because underscores and casing are part of
/// the stable config contract.
pub(super) fn action_label(action: &str) -> String {
    action
        .split('_')
        .map(|word| {
            let mut chars = word.chars();
            let Some(first) = chars.next() else {
                return String::new();
            };
            format!("{}{}", first.to_ascii_uppercase(), chars.as_str())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[rustfmt::skip]
/// Return the mutable root-config binding slot for one catalog action.
///
/// The returned `Option<KeybindingsSpec>` distinguishes three states that the
/// editor must preserve: absent means use fallback/default resolution, `Some`
/// with one or more keys is a custom binding, and `Some(Many([]))` is an
/// explicit unbind.
pub(super) fn binding_slot<'a>(
    keymap: &'a mut TuiKeymap,
    context: &str,
    action: &str,
) -> Option<&'a mut Option<KeybindingsSpec>> {
    match (context, action) {
        ("global", "open_transcript") => Some(&mut keymap.global.open_transcript),
        ("global", "open_external_editor") => Some(&mut keymap.global.open_external_editor),
        ("global", "copy") => Some(&mut keymap.global.copy),
        ("global", "clear_terminal") => Some(&mut keymap.global.clear_terminal),
        ("global", "toggle_vim_mode") => Some(&mut keymap.global.toggle_vim_mode),
        ("global", "toggle_fast_mode") => Some(&mut keymap.global.toggle_fast_mode),
        ("global", "toggle_raw_output") => Some(&mut keymap.global.toggle_raw_output),
        ("chat", "interrupt_turn") => Some(&mut keymap.chat.interrupt_turn),
        ("chat", "decrease_reasoning_effort") => Some(&mut keymap.chat.decrease_reasoning_effort),
        ("chat", "increase_reasoning_effort") => Some(&mut keymap.chat.increase_reasoning_effort),
        ("chat", "edit_queued_message") => Some(&mut keymap.chat.edit_queued_message),
        ("composer", "submit") => Some(&mut keymap.composer.submit),
        ("composer", "queue") => Some(&mut keymap.composer.queue),
        ("composer", "toggle_shortcuts") => Some(&mut keymap.composer.toggle_shortcuts),
        ("composer", "history_search_previous") => Some(&mut keymap.composer.history_search_previous),
        ("composer", "history_search_next") => Some(&mut keymap.composer.history_search_next),
        ("editor", "insert_newline") => Some(&mut keymap.editor.insert_newline),
        ("editor", "move_left") => Some(&mut keymap.editor.move_left),
        ("editor", "move_right") => Some(&mut keymap.editor.move_right),
        ("editor", "move_up") => Some(&mut keymap.editor.move_up),
        ("editor", "move_down") => Some(&mut keymap.editor.move_down),
        ("editor", "move_word_left") => Some(&mut keymap.editor.move_word_left),
        ("editor", "move_word_right") => Some(&mut keymap.editor.move_word_right),
        ("editor", "move_line_start") => Some(&mut keymap.editor.move_line_start),
        ("editor", "move_line_end") => Some(&mut keymap.editor.move_line_end),
        ("editor", "delete_backward") => Some(&mut keymap.editor.delete_backward),
        ("editor", "delete_forward") => Some(&mut keymap.editor.delete_forward),
        ("editor", "delete_backward_word") => Some(&mut keymap.editor.delete_backward_word),
        ("editor", "delete_forward_word") => Some(&mut keymap.editor.delete_forward_word),
        ("editor", "kill_line_start") => Some(&mut keymap.editor.kill_line_start),
        ("editor", "kill_whole_line") => Some(&mut keymap.editor.kill_whole_line),
        ("editor", "kill_line_end") => Some(&mut keymap.editor.kill_line_end),
        ("editor", "yank") => Some(&mut keymap.editor.yank),
        ("vim_normal", "enter_insert") => Some(&mut keymap.vim_normal.enter_insert),
        ("vim_normal", "append_after_cursor") => Some(&mut keymap.vim_normal.append_after_cursor),
        ("vim_normal", "append_line_end") => Some(&mut keymap.vim_normal.append_line_end),
        ("vim_normal", "insert_line_start") => Some(&mut keymap.vim_normal.insert_line_start),
        ("vim_normal", "open_line_below") => Some(&mut keymap.vim_normal.open_line_below),
        ("vim_normal", "open_line_above") => Some(&mut keymap.vim_normal.open_line_above),
        ("vim_normal", "move_left") => Some(&mut keymap.vim_normal.move_left),
        ("vim_normal", "move_right") => Some(&mut keymap.vim_normal.move_right),
        ("vim_normal", "move_up") => Some(&mut keymap.vim_normal.move_up),
        ("vim_normal", "move_down") => Some(&mut keymap.vim_normal.move_down),
        ("vim_normal", "move_word_forward") => Some(&mut keymap.vim_normal.move_word_forward),
        ("vim_normal", "move_word_backward") => Some(&mut keymap.vim_normal.move_word_backward),
        ("vim_normal", "move_word_end") => Some(&mut keymap.vim_normal.move_word_end),
        ("vim_normal", "move_line_start") => Some(&mut keymap.vim_normal.move_line_start),
        ("vim_normal", "move_line_end") => Some(&mut keymap.vim_normal.move_line_end),
        ("vim_normal", "delete_char") => Some(&mut keymap.vim_normal.delete_char),
        ("vim_normal", "substitute_char") => Some(&mut keymap.vim_normal.substitute_char),
        ("vim_normal", "delete_to_line_end") => Some(&mut keymap.vim_normal.delete_to_line_end),
        ("vim_normal", "change_to_line_end") => Some(&mut keymap.vim_normal.change_to_line_end),
        ("vim_normal", "yank_line") => Some(&mut keymap.vim_normal.yank_line),
        ("vim_normal", "paste_after") => Some(&mut keymap.vim_normal.paste_after),
        ("vim_normal", "start_delete_operator") => Some(&mut keymap.vim_normal.start_delete_operator),
        ("vim_normal", "start_yank_operator") => Some(&mut keymap.vim_normal.start_yank_operator),
        ("vim_normal", "start_change_operator") => Some(&mut keymap.vim_normal.start_change_operator),
        ("vim_normal", "cancel_operator") => Some(&mut keymap.vim_normal.cancel_operator),
        ("vim_operator", "delete_line") => Some(&mut keymap.vim_operator.delete_line),
        ("vim_operator", "yank_line") => Some(&mut keymap.vim_operator.yank_line),
        ("vim_operator", "motion_left") => Some(&mut keymap.vim_operator.motion_left),
        ("vim_operator", "motion_right") => Some(&mut keymap.vim_operator.motion_right),
        ("vim_operator", "motion_up") => Some(&mut keymap.vim_operator.motion_up),
        ("vim_operator", "motion_down") => Some(&mut keymap.vim_operator.motion_down),
        ("vim_operator", "motion_word_forward") => Some(&mut keymap.vim_operator.motion_word_forward),
        ("vim_operator", "motion_word_backward") => Some(&mut keymap.vim_operator.motion_word_backward),
        ("vim_operator", "motion_word_end") => Some(&mut keymap.vim_operator.motion_word_end),
        ("vim_operator", "motion_line_start") => Some(&mut keymap.vim_operator.motion_line_start),
        ("vim_operator", "motion_line_end") => Some(&mut keymap.vim_operator.motion_line_end),
        ("vim_operator", "select_inner_text_object") => Some(&mut keymap.vim_operator.select_inner_text_object),
        ("vim_operator", "select_around_text_object") => Some(&mut keymap.vim_operator.select_around_text_object),
        ("vim_operator", "cancel") => Some(&mut keymap.vim_operator.cancel),
        ("vim_text_object", "word") => Some(&mut keymap.vim_text_object.word),
        ("vim_text_object", "big_word") => Some(&mut keymap.vim_text_object.big_word),
        ("vim_text_object", "parentheses") => Some(&mut keymap.vim_text_object.parentheses),
        ("vim_text_object", "brackets") => Some(&mut keymap.vim_text_object.brackets),
        ("vim_text_object", "braces") => Some(&mut keymap.vim_text_object.braces),
        ("vim_text_object", "double_quote") => Some(&mut keymap.vim_text_object.double_quote),
        ("vim_text_object", "single_quote") => Some(&mut keymap.vim_text_object.single_quote),
        ("vim_text_object", "backtick") => Some(&mut keymap.vim_text_object.backtick),
        ("vim_text_object", "cancel") => Some(&mut keymap.vim_text_object.cancel),
        ("pager", "scroll_up") => Some(&mut keymap.pager.scroll_up),
        ("pager", "scroll_down") => Some(&mut keymap.pager.scroll_down),
        ("pager", "page_up") => Some(&mut keymap.pager.page_up),
        ("pager", "page_down") => Some(&mut keymap.pager.page_down),
        ("pager", "half_page_up") => Some(&mut keymap.pager.half_page_up),
        ("pager", "half_page_down") => Some(&mut keymap.pager.half_page_down),
        ("pager", "jump_top") => Some(&mut keymap.pager.jump_top),
        ("pager", "jump_bottom") => Some(&mut keymap.pager.jump_bottom),
        ("pager", "close") => Some(&mut keymap.pager.close),
        ("pager", "close_transcript") => Some(&mut keymap.pager.close_transcript),
        ("list", "move_up") => Some(&mut keymap.list.move_up),
        ("list", "move_down") => Some(&mut keymap.list.move_down),
        ("list", "move_left") => Some(&mut keymap.list.move_left),
        ("list", "move_right") => Some(&mut keymap.list.move_right),
        ("list", "page_up") => Some(&mut keymap.list.page_up),
        ("list", "page_down") => Some(&mut keymap.list.page_down),
        ("list", "jump_top") => Some(&mut keymap.list.jump_top),
        ("list", "jump_bottom") => Some(&mut keymap.list.jump_bottom),
        ("list", "accept") => Some(&mut keymap.list.accept),
        ("list", "cancel") => Some(&mut keymap.list.cancel),
        ("approval", "open_fullscreen") => Some(&mut keymap.approval.open_fullscreen),
        ("approval", "open_thread") => Some(&mut keymap.approval.open_thread),
        ("approval", "approve") => Some(&mut keymap.approval.approve),
        ("approval", "approve_for_session") => Some(&mut keymap.approval.approve_for_session),
        ("approval", "approve_for_prefix") => Some(&mut keymap.approval.approve_for_prefix),
        ("approval", "deny") => Some(&mut keymap.approval.deny),
        ("approval", "decline") => Some(&mut keymap.approval.decline),
        ("approval", "cancel") => Some(&mut keymap.approval.cancel),
        _ => None,
    }
}

#[rustfmt::skip]
/// Return the resolved runtime bindings for one catalog action.
///
/// This reads from [`RuntimeKeymap`] rather than root config so UI labels show
/// the actual active binding after defaults, global fallback, explicit
/// unbinding, and duplicate-key validation have already been applied.
pub(super) fn bindings_for_action<'a>(
    runtime_keymap: &'a RuntimeKeymap,
    context: &str,
    action: &str,
) -> Option<&'a [KeyBinding]> {
    match (context, action) {
        ("global", "open_transcript") => Some(runtime_keymap.app.open_transcript.as_slice()),
        ("global", "open_external_editor") => Some(runtime_keymap.app.open_external_editor.as_slice()),
        ("global", "copy") => Some(runtime_keymap.app.copy.as_slice()),
        ("global", "clear_terminal") => Some(runtime_keymap.app.clear_terminal.as_slice()),
        ("global", "toggle_vim_mode") => Some(runtime_keymap.app.toggle_vim_mode.as_slice()),
        ("global", "toggle_fast_mode") => Some(runtime_keymap.app.toggle_fast_mode.as_slice()),
        ("global", "toggle_raw_output") => Some(runtime_keymap.app.toggle_raw_output.as_slice()),
        ("chat", "interrupt_turn") => Some(runtime_keymap.chat.interrupt_turn.as_slice()),
        ("chat", "decrease_reasoning_effort") => Some(runtime_keymap.chat.decrease_reasoning_effort.as_slice()),
        ("chat", "increase_reasoning_effort") => Some(runtime_keymap.chat.increase_reasoning_effort.as_slice()),
        ("chat", "edit_queued_message") => Some(runtime_keymap.chat.edit_queued_message.as_slice()),
        ("composer", "submit") => Some(runtime_keymap.composer.submit.as_slice()),
        ("composer", "queue") => Some(runtime_keymap.composer.queue.as_slice()),
        ("composer", "toggle_shortcuts") => Some(runtime_keymap.composer.toggle_shortcuts.as_slice()),
        ("composer", "history_search_previous") => Some(runtime_keymap.composer.history_search_previous.as_slice()),
        ("composer", "history_search_next") => Some(runtime_keymap.composer.history_search_next.as_slice()),
        ("editor", "insert_newline") => Some(runtime_keymap.editor.insert_newline.as_slice()),
        ("editor", "move_left") => Some(runtime_keymap.editor.move_left.as_slice()),
        ("editor", "move_right") => Some(runtime_keymap.editor.move_right.as_slice()),
        ("editor", "move_up") => Some(runtime_keymap.editor.move_up.as_slice()),
        ("editor", "move_down") => Some(runtime_keymap.editor.move_down.as_slice()),
        ("editor", "move_word_left") => Some(runtime_keymap.editor.move_word_left.as_slice()),
        ("editor", "move_word_right") => Some(runtime_keymap.editor.move_word_right.as_slice()),
        ("editor", "move_line_start") => Some(runtime_keymap.editor.move_line_start.as_slice()),
        ("editor", "move_line_end") => Some(runtime_keymap.editor.move_line_end.as_slice()),
        ("editor", "delete_backward") => Some(runtime_keymap.editor.delete_backward.as_slice()),
        ("editor", "delete_forward") => Some(runtime_keymap.editor.delete_forward.as_slice()),
        ("editor", "delete_backward_word") => Some(runtime_keymap.editor.delete_backward_word.as_slice()),
        ("editor", "delete_forward_word") => Some(runtime_keymap.editor.delete_forward_word.as_slice()),
        ("editor", "kill_line_start") => Some(runtime_keymap.editor.kill_line_start.as_slice()),
        ("editor", "kill_whole_line") => Some(runtime_keymap.editor.kill_whole_line.as_slice()),
        ("editor", "kill_line_end") => Some(runtime_keymap.editor.kill_line_end.as_slice()),
        ("editor", "yank") => Some(runtime_keymap.editor.yank.as_slice()),
        ("vim_normal", "enter_insert") => Some(runtime_keymap.vim_normal.enter_insert.as_slice()),
        ("vim_normal", "append_after_cursor") => Some(runtime_keymap.vim_normal.append_after_cursor.as_slice()),
        ("vim_normal", "append_line_end") => Some(runtime_keymap.vim_normal.append_line_end.as_slice()),
        ("vim_normal", "insert_line_start") => Some(runtime_keymap.vim_normal.insert_line_start.as_slice()),
        ("vim_normal", "open_line_below") => Some(runtime_keymap.vim_normal.open_line_below.as_slice()),
        ("vim_normal", "open_line_above") => Some(runtime_keymap.vim_normal.open_line_above.as_slice()),
        ("vim_normal", "move_left") => Some(runtime_keymap.vim_normal.move_left.as_slice()),
        ("vim_normal", "move_right") => Some(runtime_keymap.vim_normal.move_right.as_slice()),
        ("vim_normal", "move_up") => Some(runtime_keymap.vim_normal.move_up.as_slice()),
        ("vim_normal", "move_down") => Some(runtime_keymap.vim_normal.move_down.as_slice()),
        ("vim_normal", "move_word_forward") => Some(runtime_keymap.vim_normal.move_word_forward.as_slice()),
        ("vim_normal", "move_word_backward") => Some(runtime_keymap.vim_normal.move_word_backward.as_slice()),
        ("vim_normal", "move_word_end") => Some(runtime_keymap.vim_normal.move_word_end.as_slice()),
        ("vim_normal", "move_line_start") => Some(runtime_keymap.vim_normal.move_line_start.as_slice()),
        ("vim_normal", "move_line_end") => Some(runtime_keymap.vim_normal.move_line_end.as_slice()),
        ("vim_normal", "delete_char") => Some(runtime_keymap.vim_normal.delete_char.as_slice()),
        ("vim_normal", "substitute_char") => Some(runtime_keymap.vim_normal.substitute_char.as_slice()),
        ("vim_normal", "delete_to_line_end") => Some(runtime_keymap.vim_normal.delete_to_line_end.as_slice()),
        ("vim_normal", "change_to_line_end") => Some(runtime_keymap.vim_normal.change_to_line_end.as_slice()),
        ("vim_normal", "yank_line") => Some(runtime_keymap.vim_normal.yank_line.as_slice()),
        ("vim_normal", "paste_after") => Some(runtime_keymap.vim_normal.paste_after.as_slice()),
        ("vim_normal", "start_delete_operator") => Some(runtime_keymap.vim_normal.start_delete_operator.as_slice()),
        ("vim_normal", "start_yank_operator") => Some(runtime_keymap.vim_normal.start_yank_operator.as_slice()),
        ("vim_normal", "start_change_operator") => Some(runtime_keymap.vim_normal.start_change_operator.as_slice()),
        ("vim_normal", "cancel_operator") => Some(runtime_keymap.vim_normal.cancel_operator.as_slice()),
        ("vim_operator", "delete_line") => Some(runtime_keymap.vim_operator.delete_line.as_slice()),
        ("vim_operator", "yank_line") => Some(runtime_keymap.vim_operator.yank_line.as_slice()),
        ("vim_operator", "motion_left") => Some(runtime_keymap.vim_operator.motion_left.as_slice()),
        ("vim_operator", "motion_right") => Some(runtime_keymap.vim_operator.motion_right.as_slice()),
        ("vim_operator", "motion_up") => Some(runtime_keymap.vim_operator.motion_up.as_slice()),
        ("vim_operator", "motion_down") => Some(runtime_keymap.vim_operator.motion_down.as_slice()),
        ("vim_operator", "motion_word_forward") => Some(runtime_keymap.vim_operator.motion_word_forward.as_slice()),
        ("vim_operator", "motion_word_backward") => Some(runtime_keymap.vim_operator.motion_word_backward.as_slice()),
        ("vim_operator", "motion_word_end") => Some(runtime_keymap.vim_operator.motion_word_end.as_slice()),
        ("vim_operator", "motion_line_start") => Some(runtime_keymap.vim_operator.motion_line_start.as_slice()),
        ("vim_operator", "motion_line_end") => Some(runtime_keymap.vim_operator.motion_line_end.as_slice()),
        ("vim_operator", "select_inner_text_object") => Some(runtime_keymap.vim_operator.select_inner_text_object.as_slice()),
        ("vim_operator", "select_around_text_object") => Some(runtime_keymap.vim_operator.select_around_text_object.as_slice()),
        ("vim_operator", "cancel") => Some(runtime_keymap.vim_operator.cancel.as_slice()),
        ("vim_text_object", "word") => Some(runtime_keymap.vim_text_object.word.as_slice()),
        ("vim_text_object", "big_word") => Some(runtime_keymap.vim_text_object.big_word.as_slice()),
        ("vim_text_object", "parentheses") => Some(runtime_keymap.vim_text_object.parentheses.as_slice()),
        ("vim_text_object", "brackets") => Some(runtime_keymap.vim_text_object.brackets.as_slice()),
        ("vim_text_object", "braces") => Some(runtime_keymap.vim_text_object.braces.as_slice()),
        ("vim_text_object", "double_quote") => Some(runtime_keymap.vim_text_object.double_quote.as_slice()),
        ("vim_text_object", "single_quote") => Some(runtime_keymap.vim_text_object.single_quote.as_slice()),
        ("vim_text_object", "backtick") => Some(runtime_keymap.vim_text_object.backtick.as_slice()),
        ("vim_text_object", "cancel") => Some(runtime_keymap.vim_text_object.cancel.as_slice()),
        ("pager", "scroll_up") => Some(runtime_keymap.pager.scroll_up.as_slice()),
        ("pager", "scroll_down") => Some(runtime_keymap.pager.scroll_down.as_slice()),
        ("pager", "page_up") => Some(runtime_keymap.pager.page_up.as_slice()),
        ("pager", "page_down") => Some(runtime_keymap.pager.page_down.as_slice()),
        ("pager", "half_page_up") => Some(runtime_keymap.pager.half_page_up.as_slice()),
        ("pager", "half_page_down") => Some(runtime_keymap.pager.half_page_down.as_slice()),
        ("pager", "jump_top") => Some(runtime_keymap.pager.jump_top.as_slice()),
        ("pager", "jump_bottom") => Some(runtime_keymap.pager.jump_bottom.as_slice()),
        ("pager", "close") => Some(runtime_keymap.pager.close.as_slice()),
        ("pager", "close_transcript") => Some(runtime_keymap.pager.close_transcript.as_slice()),
        ("list", "move_up") => Some(runtime_keymap.list.move_up.as_slice()),
        ("list", "move_down") => Some(runtime_keymap.list.move_down.as_slice()),
        ("list", "move_left") => Some(runtime_keymap.list.move_left.as_slice()),
        ("list", "move_right") => Some(runtime_keymap.list.move_right.as_slice()),
        ("list", "page_up") => Some(runtime_keymap.list.page_up.as_slice()),
        ("list", "page_down") => Some(runtime_keymap.list.page_down.as_slice()),
        ("list", "jump_top") => Some(runtime_keymap.list.jump_top.as_slice()),
        ("list", "jump_bottom") => Some(runtime_keymap.list.jump_bottom.as_slice()),
        ("list", "accept") => Some(runtime_keymap.list.accept.as_slice()),
        ("list", "cancel") => Some(runtime_keymap.list.cancel.as_slice()),
        ("approval", "open_fullscreen") => Some(runtime_keymap.approval.open_fullscreen.as_slice()),
        ("approval", "open_thread") => Some(runtime_keymap.approval.open_thread.as_slice()),
        ("approval", "approve") => Some(runtime_keymap.approval.approve.as_slice()),
        ("approval", "approve_for_session") => Some(runtime_keymap.approval.approve_for_session.as_slice()),
        ("approval", "approve_for_prefix") => Some(runtime_keymap.approval.approve_for_prefix.as_slice()),
        ("approval", "deny") => Some(runtime_keymap.approval.deny.as_slice()),
        ("approval", "decline") => Some(runtime_keymap.approval.decline.as_slice()),
        ("approval", "cancel") => Some(runtime_keymap.approval.cancel.as_slice()),
        _ => None,
    }
}

/// Format a resolved binding list for compact menu display.
///
/// Duplicate runtime variants that normalize to the same config spec are shown
/// once so compatibility defaults, such as alternate SHIFT reporting forms, do
/// not look like separate user choices.
pub(super) fn format_binding_summary(bindings: &[KeyBinding]) -> String {
    let mut seen = BTreeSet::new();
    let specs = bindings
        .iter()
        .filter_map(|binding| super::binding_to_config_key_spec(*binding).ok())
        .filter(|spec| seen.insert(spec.clone()))
        .collect::<Vec<_>>();
    if specs.is_empty() {
        "unbound".to_string()
    } else {
        specs.join(", ")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum KeymapDebugBindingSource {
    Custom,
    CustomGlobal,
    Default,
}

impl KeymapDebugBindingSource {
    pub(super) const fn label(&self) -> &'static str {
        match self {
            Self::Custom => "Custom",
            Self::CustomGlobal => "Custom global",
            Self::Default => "Default",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct KeymapDebugActionMatch {
    pub(super) context: &'static str,
    pub(super) action: &'static str,
    pub(super) label: String,
    pub(super) description: &'static str,
    pub(super) source: KeymapDebugBindingSource,
}

pub(super) fn matching_actions_for_key_event(
    runtime_keymap: &RuntimeKeymap,
    keymap_config: &TuiKeymap,
    event: KeyEvent,
) -> Vec<KeymapDebugActionMatch> {
    KEYMAP_ACTIONS
        .iter()
        .filter_map(|descriptor| {
            let bindings =
                bindings_for_action(runtime_keymap, descriptor.context, descriptor.action)?;
            bindings
                .iter()
                .any(|binding| binding.is_press(event))
                .then(|| KeymapDebugActionMatch {
                    context: descriptor.context,
                    action: descriptor.action,
                    label: action_label(descriptor.action),
                    description: descriptor.description,
                    source: debug_binding_source(keymap_config, descriptor),
                })
        })
        .collect()
}

fn debug_binding_source(
    keymap_config: &TuiKeymap,
    descriptor: &KeymapActionDescriptor,
) -> KeymapDebugBindingSource {
    let mut keymap_config = keymap_config.clone();
    let Some(slot) = binding_slot(&mut keymap_config, descriptor.context, descriptor.action) else {
        return KeymapDebugBindingSource::Default;
    };
    if slot.is_some() {
        return KeymapDebugBindingSource::Custom;
    }

    let Some(global_slot) = global_fallback_slot(&mut keymap_config, descriptor) else {
        return KeymapDebugBindingSource::Default;
    };
    if global_slot.is_some() {
        KeymapDebugBindingSource::CustomGlobal
    } else {
        KeymapDebugBindingSource::Default
    }
}

fn global_fallback_slot<'a>(
    keymap: &'a mut TuiKeymap,
    descriptor: &KeymapActionDescriptor,
) -> Option<&'a mut Option<KeybindingsSpec>> {
    if descriptor.context != "composer" {
        return None;
    }

    match descriptor.action {
        "submit" => Some(&mut keymap.global.submit),
        "queue" => Some(&mut keymap.global.queue),
        "toggle_shortcuts" => Some(&mut keymap.global.toggle_shortcuts),
        _ => None,
    }
}
