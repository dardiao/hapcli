//! 单个终端会话标签页：持有内核会话、快照与交互状态。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::time::Instant;

use eframe::egui::{self, FontId, PointerButton, Pos2, Rect, Response, Vec2};
use hapcli_terminal::{
    GraphicsOptions, SerialSessionConfig, TerminalEncoding, TerminalSession, TerminalSessionKind,
    TerminalSearchMatch, TerminalSnapshot, TelnetSessionConfig, TrzszTransferPolicy,
};

use crate::keys;
use crate::render::{
    self, ScrollCommand, TextSelection, cell_at, scrollbar_track_rect, select_line,
    select_word_at, selected_text, viewport_highlights,
};
use crate::trzsz::{TrzszPromptRequest, TrzszWorkerEvent};
use zeroize::Zeroizing;

static TRZSZ_OWNER_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug)]
pub struct TerminalPrefs {
    pub copy_on_select: bool,
    pub middle_click_paste: bool,
}

fn new_trzsz_owner_id() -> String {
    let sequence = TRZSZ_OWNER_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("egui-{}-{sequence}", std::process::id())
}

pub struct TerminalTab {
    pub session: TerminalSession,
    pub snapshot: TerminalSnapshot,
    pub last_terminal_size: (usize, usize),
    pub scroll_accum: f32,
    pub focused: bool,
    pub trzsz_prompt: Option<TrzszPromptRequest>,
    pub trzsz_active: bool,
    pub trzsz_rx: Option<Receiver<TrzszWorkerEvent>>,
    pub trzsz_status: Option<String>,
    pub trzsz_owner_id: String,
    /// SSH 连接成功后写入钥匙串的 (key, 密码)。
    pub pending_keychain_save: Option<(String, Zeroizing<String>)>,
    pub keychain_status: Option<String>,
    pub selection: Option<TextSelection>,
    selection_active: bool,
    selection_dragged: bool,
    last_rect: Option<Rect>,
    last_layer_id: Option<egui::LayerId>,
    last_response_id: Option<egui::Id>,
    /// SSH 重连所需配置；非 SSH 会话为 None。
    pub ssh_reconnect_config: Option<hapcli_ssh::SshConfig>,
    pub ssh_registry: Option<hapcli_ssh::SshConnectionRegistry>,
    pub ever_connected: bool,
    pub reconnect_attempts: u32,
    pub reconnect_at: Option<Instant>,
    pub reconnect_status: Option<String>,
    pub reconnect_dismissed: bool,
    pub search_open: bool,
    pub search_query: String,
    pub search_matches: Vec<TerminalSearchMatch>,
    pub search_current: Option<usize>,
    pub search_focus_requested: bool,
    pub sftp: Option<crate::sftp::SftpPanelState>,
    pub forward: Option<crate::forward::ForwardPanel>,
    /// 静态标签：本地会话或 `user@host` 基础标签。
    base_label: String,
}

impl TerminalTab {
    pub fn new_local(ctx: &egui::Context, cols: usize, rows: usize) -> anyhow::Result<Self> {
        let session = enable_trzsz(TerminalSession::local_default(cols, rows)?);
        let snapshot = session.snapshot();
        Self::spawn_activity_thread(&session, ctx);
        Ok(Self {
            session,
            snapshot,
            last_terminal_size: (cols, rows),
            scroll_accum: 0.0,
            focused: false,
            trzsz_prompt: None,
            trzsz_active: false,
            trzsz_rx: None,
            trzsz_status: None,
            trzsz_owner_id: new_trzsz_owner_id(),
            pending_keychain_save: None,
            keychain_status: None,
            selection: None,
            selection_active: false,
            selection_dragged: false,
            last_rect: None,
            last_layer_id: None,
            last_response_id: None,
            ssh_reconnect_config: None,
            ssh_registry: None,
            ever_connected: false,
            reconnect_attempts: 0,
            reconnect_at: None,
            reconnect_status: None,
            reconnect_dismissed: false,
            search_open: false,
            search_query: String::new(),
            search_matches: Vec::new(),
            search_current: None,
            search_focus_requested: false,
            sftp: None,
            forward: None,
            base_label: "本地".to_string(),
        })
    }

    pub fn new_ssh(
        ctx: &egui::Context,
        config: hapcli_terminal::SshSessionConfig,
        reconnect_config: Option<hapcli_ssh::SshConfig>,
        ssh_registry: Option<hapcli_ssh::SshConnectionRegistry>,
        base_label: String,
        cols: usize,
        rows: usize,
    ) -> Self {
        let session = enable_trzsz(TerminalSession::ssh(config, cols, rows));
        let snapshot = session.snapshot();
        Self::spawn_activity_thread(&session, ctx);
        Self {
            session,
            snapshot,
            last_terminal_size: (cols, rows),
            scroll_accum: 0.0,
            focused: false,
            trzsz_prompt: None,
            trzsz_active: false,
            trzsz_rx: None,
            trzsz_status: None,
            trzsz_owner_id: new_trzsz_owner_id(),
            pending_keychain_save: None,
            keychain_status: None,
            selection: None,
            selection_active: false,
            selection_dragged: false,
            last_rect: None,
            last_layer_id: None,
            last_response_id: None,
            ssh_reconnect_config: reconnect_config,
            ssh_registry,
            ever_connected: false,
            reconnect_attempts: 0,
            reconnect_at: None,
            reconnect_status: None,
            reconnect_dismissed: false,
            search_open: false,
            search_query: String::new(),
            search_matches: Vec::new(),
            search_current: None,
            search_focus_requested: false,
            sftp: None,
            forward: None,
            base_label,
        }
    }

    pub fn new_telnet(
        ctx: &egui::Context,
        config: TelnetSessionConfig,
        base_label: String,
        cols: usize,
        rows: usize,
    ) -> Self {
        let session = enable_trzsz(TerminalSession::telnet_with_graphics_and_encoding(
            config,
            cols,
            rows,
            GraphicsOptions::default(),
            TerminalEncoding::Utf8,
            1000,
        ));
        let snapshot = session.snapshot();
        Self::spawn_activity_thread(&session, ctx);
        Self {
            session,
            snapshot,
            last_terminal_size: (cols, rows),
            scroll_accum: 0.0,
            focused: false,
            trzsz_prompt: None,
            trzsz_active: false,
            trzsz_rx: None,
            trzsz_status: None,
            trzsz_owner_id: new_trzsz_owner_id(),
            pending_keychain_save: None,
            keychain_status: None,
            selection: None,
            selection_active: false,
            selection_dragged: false,
            last_rect: None,
            last_layer_id: None,
            last_response_id: None,
            ssh_reconnect_config: None,
            ssh_registry: None,
            ever_connected: false,
            reconnect_attempts: 0,
            reconnect_at: None,
            reconnect_status: None,
            reconnect_dismissed: false,
            search_open: false,
            search_query: String::new(),
            search_matches: Vec::new(),
            search_current: None,
            search_focus_requested: false,
            sftp: None,
            forward: None,
            base_label,
        }
    }

    pub fn new_serial(
        ctx: &egui::Context,
        config: SerialSessionConfig,
        base_label: String,
        cols: usize,
        rows: usize,
    ) -> Result<Self, hapcli_terminal::SerialError> {
        let session = TerminalSession::serial_with_graphics_and_encoding(
            config,
            cols,
            rows,
            GraphicsOptions::default(),
            TerminalEncoding::Utf8,
            1000,
        )?;
        let snapshot = session.snapshot();
        Self::spawn_activity_thread(&session, ctx);
        Ok(Self {
            session,
            snapshot,
            last_terminal_size: (cols, rows),
            scroll_accum: 0.0,
            focused: false,
            trzsz_prompt: None,
            trzsz_active: false,
            trzsz_rx: None,
            trzsz_status: None,
            trzsz_owner_id: new_trzsz_owner_id(),
            pending_keychain_save: None,
            keychain_status: None,
            selection: None,
            selection_active: false,
            selection_dragged: false,
            last_rect: None,
            last_layer_id: None,
            last_response_id: None,
            ssh_reconnect_config: None,
            ssh_registry: None,
            ever_connected: false,
            reconnect_attempts: 0,
            reconnect_at: None,
            reconnect_status: None,
            reconnect_dismissed: false,
            search_open: false,
            search_query: String::new(),
            search_matches: Vec::new(),
            search_current: None,
            search_focus_requested: false,
            sftp: None,
            forward: None,
            base_label,
        })
    }

    /// 结束一次 Trzsz 传输并复位状态。
    pub fn finish_trzsz(&mut self) {
        self.trzsz_active = false;
        self.trzsz_rx = None;
        self.trzsz_prompt = None;
        self.session.finish_trzsz_transfer();
    }

    /// 用保存的 SSH 配置重建会话（断线重连）。
    pub fn reconnect_with(&mut self, ctx: &egui::Context, cols: usize, rows: usize) {
        let Some(config) = self.ssh_reconnect_config.clone() else {
            return;
        };
        self.session.shutdown();

        let session_config = crate::connect::build_reconnect_session_config(&config);
        let session_config = if let Some(registry) = &self.ssh_registry {
            session_config.with_registry(
                registry.clone(),
                hapcli_ssh::ConnectionConsumer::Terminal("egui-terminal".to_string()),
            )
        } else {
            session_config
        };
        let mut session = TerminalSession::ssh(session_config, cols, rows);
        session.set_trzsz_policy(Some(TrzszTransferPolicy::default()));
        Self::spawn_activity_thread(&session, ctx);

        self.session = session;
        self.snapshot = self.session.snapshot();
        self.trzsz_prompt = None;
        self.trzsz_active = false;
        self.trzsz_rx = None;
        self.trzsz_status = None;
        self.selection = None;
        self.selection_active = false;
        self.reconnect_dismissed = false;
    }

    /// 内核有输出时唤醒 egui 重绘；会话销毁后线程自动退出。
    fn spawn_activity_thread(session: &TerminalSession, ctx: &egui::Context) {
        let activity = session.activity_receiver();
        let ctx = ctx.clone();
        std::thread::spawn(move || loop {
            let alive = pollster::block_on(activity.notified());
            ctx.request_repaint();
            if !alive {
                break;
            }
        });
    }

    /// 标签页显示名：本地跟随 shell 标题，SSH 显示连接状态。
    pub fn display_label(&self) -> String {
        let status = self.session.status();
        match status.kind {
            TerminalSessionKind::LocalPty => {
                let title = status.title.as_deref().unwrap_or("zsh");
                format!("本地 · {title}")
            }
            _ => {
                let base = status
                    .title
                    .as_deref()
                    .unwrap_or(self.base_label.as_str());
                if !status.lifecycle.is_running() {
                    format!("{base} ✗")
                } else if status.title.is_none() {
                    format!("{base} …")
                } else {
                    base.to_string()
                }
            }
        }
    }

    pub fn resize(&mut self, cols: usize, rows: usize, cell_size: Vec2) {
        if (cols, rows) != self.last_terminal_size {
            self.last_terminal_size = (cols, rows);
            let _ = self.session.resize_with_cell_size(
                cols,
                rows,
                cell_size.x as u16,
                cell_size.y as u16,
            );
        }
    }

    /// 处理本标签页的键盘 / 鼠标事件。
    pub fn process_input(
        &mut self,
        ctx: &egui::Context,
        cell_size: Vec2,
        prefs: TerminalPrefs,
    ) {
        let mut writes: Vec<Vec<u8>> = Vec::new();
        let mut scroll_lines: i32 = 0;
        let mut focus_changed = false;
        let mut copy_requested = false;
        // 键盘归属：无焦点（兜底给终端）或焦点就是终端自身时，才把输入转发给终端。
        let terminal_owns_keys = ctx.memory(|memory| match memory.focused() {
            None => true,
            Some(id) => self.last_response_id == Some(id),
        });

        // 阶段一：在输入锁内只收集状态；ctx 的加锁方法（图层命中、剪贴板）必须放到阶段二，
        // 否则会在 `ctx.input` 持锁期间再次取锁而死锁。
        let mut latest_pos: Option<Pos2> = None;
        let mut primary_down = false;
        let mut double_clicked = false;
        let mut triple_clicked = false;
        let mut pointer_events: Vec<(Pos2, bool)> = Vec::new();
        ctx.input(|i| {
            latest_pos = i.pointer.latest_pos();
            primary_down = i.pointer.primary_down();
            double_clicked = i.pointer.button_double_clicked(PointerButton::Primary);
            triple_clicked = i.pointer.button_triple_clicked(PointerButton::Primary);

            for event in &i.events {
                match event {
                    egui::Event::Copy => {
                        copy_requested = true;
                    }
                    egui::Event::Text(text) => {
                        if terminal_owns_keys
                            && !self.search_open
                            && text.chars().any(|c| !c.is_control())
                            && !i.modifiers.mac_cmd
                        {
                            let mut bytes = text.as_bytes().to_vec();
                            if i.modifiers.alt {
                                bytes.insert(0, 0x1b);
                            }
                            writes.push(bytes);
                        }
                    }
                    egui::Event::Paste(text) => {
                        if terminal_owns_keys && !self.search_open {
                            let _ = self.session.paste_text(text);
                        }
                    }
                    egui::Event::Ime(egui::ImeEvent::Commit(text)) => {
                        if terminal_owns_keys
                            && !self.search_open
                            && !text.is_empty()
                            && !i.modifiers.mac_cmd
                        {
                            writes.push(text.as_bytes().to_vec());
                        }
                    }
                    egui::Event::Key {
                        key,
                        pressed,
                        modifiers,
                        ..
                    } => {
                        // 搜索框持有键盘时，Esc/Enter 控制搜索导航。
                        if self.search_open && !terminal_owns_keys {
                            if *pressed {
                                match key {
                                    egui::Key::Escape => {
                                        self.search_open = false;
                                    }
                                    egui::Key::Enter => {
                                        if modifiers.shift {
                                            self.search_prev();
                                        } else {
                                            self.search_next();
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            continue;
                        }
                        // 焦点在其他控件（弹窗输入框等）时不转发键盘。
                        if !terminal_owns_keys {
                            continue;
                        }
                        if *pressed && modifiers.command && *key == egui::Key::F {
                            self.search_open = true;
                            self.search_focus_requested = true;
                            self.refresh_search();
                            continue;
                        }
                        if let Some(bytes) =
                            keys::key_event_to_bytes(*key, *modifiers, *pressed)
                        {
                            writes.push(bytes);
                        }
                    }
                    egui::Event::MouseWheel {
                        delta,
                        unit,
                        modifiers,
                        ..
                    } => {
                        if !modifiers.ctrl && !modifiers.command {
                            let lines = match unit {
                                egui::MouseWheelUnit::Line => delta.y,
                                egui::MouseWheelUnit::Point => delta.y / cell_size.y,
                                egui::MouseWheelUnit::Page => delta.y * 0.8,
                            };
                            self.scroll_accum += lines;
                            let whole = self.scroll_accum.trunc() as i32;
                            self.scroll_accum -= whole as f32;
                            scroll_lines += whole;
                        }
                    }
                    egui::Event::PointerButton {
                        pos,
                        button,
                        pressed,
                        ..
                    } => {
                        match button {
                            PointerButton::Primary => {
                                pointer_events.push((*pos, *pressed));
                            }
                            PointerButton::Middle => {
                                if *pressed && prefs.middle_click_paste {
                                    self.paste_clipboard();
                                }
                            }
                            _ => {}
                        }
                    }
                    egui::Event::WindowFocused(focused) => {
                        self.focused = *focused;
                        focus_changed = true;
                    }
                    _ => {}
                }
            }
        });

        // 阶段二：退出输入锁后处理指针事件与剪贴板。
        for (pos, pressed) in pointer_events {
            if pressed {
                if self.pointer_over_terminal(ctx, pos) {
                    if let Some((row, col)) = self.cell_at_pos(pos, cell_size) {
                        self.selection = Some(TextSelection {
                            anchor: (row, col),
                            active: (row, col),
                        });
                        self.selection_active = true;
                        self.selection_dragged = false;
                    }
                }
                if !self.focused {
                    self.focused = true;
                    focus_changed = true;
                }
            } else {
                // 多击标记只在释放帧生效，因此在此判定。
                if triple_clicked {
                    if self.pointer_over_terminal(ctx, pos) {
                        if let Some((row, _)) = self.cell_at_pos(pos, cell_size) {
                            self.selection = Some(select_line(row, self.snapshot.cols));
                            if prefs.copy_on_select {
                                self.copy_selection(ctx);
                            }
                        }
                    }
                } else if double_clicked {
                    if self.pointer_over_terminal(ctx, pos) {
                        if let Some((row, col)) = self.cell_at_pos(pos, cell_size) {
                            self.selection = Some(select_word_at(&self.snapshot, row, col));
                            if prefs.copy_on_select {
                                self.copy_selection(ctx);
                            }
                        }
                    }
                } else if !self.selection_dragged {
                    // 单击：不产生选区。
                    self.selection = None;
                } else if prefs.copy_on_select {
                    self.copy_selection(ctx);
                }
                self.selection_active = false;
            }
        }

        // 拖选更新（每帧跟随指针）。
        if self.selection_active && primary_down {
            if let Some(pos) = latest_pos {
                if self.pointer_over_terminal(ctx, pos) {
                    if let Some((row, col)) = self.cell_at_pos(pos, cell_size) {
                        if let Some(mut selection) = self.selection {
                            if selection.active != (row, col) {
                                selection.active = (row, col);
                                self.selection = Some(selection);
                                self.selection_dragged = true;
                            }
                        }
                    }
                }
            }
        }

        if copy_requested {
            self.copy_selection(ctx);
        }
        if focus_changed {
            let _ = self.session.set_focused(self.focused);
        }
        if scroll_lines != 0 {
            self.selection = None;
            self.session.scroll_lines(scroll_lines);
        }
        for bytes in writes {
            self.selection = None;
            let _ = self.session.write_input(&bytes);
        }
    }

    fn cell_at_pos(&self, pos: Pos2, cell_size: Vec2) -> Option<(usize, usize)> {
        let rect = self.last_rect?;
        let (row, col) = cell_at(rect, cell_size, pos)?;
        if row < self.snapshot.rows && col < self.snapshot.cols {
            Some((row, col))
        } else {
            None
        }
    }

    /// 指针是否落在终端自己的图层上（弹窗/面板覆盖时不当作文本选择）。
    fn pointer_over_terminal(&self, ctx: &egui::Context, pos: Pos2) -> bool {
        if let Some(layer) = ctx.layer_id_at(pos) {
            if self.last_layer_id != Some(layer) {
                return false;
            }
        }
        // 滚动条轨道不参与文本选择。
        if let Some(rect) = self.last_rect {
            if let Some(track) = scrollbar_track_rect(rect) {
                if track.contains(pos) {
                    return false;
                }
            }
        }
        true
    }

    fn copy_selection(&self, ctx: &egui::Context) {
        if let Some(selection) = self.selection {
            let text = selected_text(&self.snapshot, &selection);
            if !text.is_empty() {
                ctx.copy_text(text);
            }
        }
    }

    fn paste_clipboard(&mut self) {
        let Ok(mut clipboard) = arboard::Clipboard::new() else {
            return;
        };
        let Ok(text) = clipboard.get_text() else {
            return;
        };
        if !text.is_empty() {
            let _ = self.session.paste_text(&text);
        }
    }

    /// 按当前查询刷新匹配结果（空查询清空）。
    pub fn refresh_search(&mut self) {
        let query = self.search_query.trim().to_string();
        if query.is_empty() {
            self.search_matches.clear();
            self.search_current = None;
            return;
        }
        self.search_matches = self.session.search_matches(&query);
        if self.search_current.is_none_or(|index| index >= self.search_matches.len()) {
            self.search_current = (!self.search_matches.is_empty()).then_some(0);
        }
    }

    pub fn search_next(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        self.search_focus_requested = true;
        let next = match self.search_current {
            Some(index) => (index + 1) % self.search_matches.len(),
            None => 0,
        };
        self.search_current = Some(next);
        self.scroll_to_search_match(next);
    }

    pub fn search_prev(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        self.search_focus_requested = true;
        let prev = match self.search_current {
            Some(index) => (index + self.search_matches.len() - 1) % self.search_matches.len(),
            None => self.search_matches.len() - 1,
        };
        self.search_current = Some(prev);
        self.scroll_to_search_match(prev);
    }

    fn scroll_to_search_match(&mut self, index: usize) {
        if let Some(search_match) = self.search_matches.get(index) {
            let offset = crate::render::scroll_offset_for_line(search_match.line);
            self.session.scroll_to_display_offset(offset);
        }
    }

    /// 拉取增量快照并绘制终端区域。
    pub fn draw(
        &mut self,
        ui: &mut egui::Ui,
        font_id: &FontId,
        cell_size: Vec2,
        cursor_blink_on: bool,
        theme: &crate::render::TerminalTheme,
    ) -> Response {
        self.snapshot = self.session.snapshot_incremental(&self.snapshot);

        let search_highlights =
            viewport_highlights(&self.snapshot, &self.search_matches, self.search_current);
        let response = render::terminal_ui(
            ui,
            &self.snapshot,
            font_id,
            cell_size,
            cursor_blink_on && self.focused,
            theme,
            self.selection.as_ref(),
            if self.search_open {
                Some(&search_highlights)
            } else {
                None
            },
        );
        self.last_rect = Some(response.rect);
        self.last_layer_id = Some(ui.layer_id());
        self.last_response_id = Some(response.id);

        if let Some(command) = render::scrollbar(ui, &self.snapshot, &response) {
            match command {
                ScrollCommand::ToOffset(offset) => self.session.scroll_to_display_offset(offset),
                ScrollCommand::PageUp => self.session.page_up(),
                ScrollCommand::PageDown => self.session.page_down(),
            }
        }

        if response.clicked() && !self.focused {
            self.focused = true;
            let _ = self.session.set_focused(true);
        }
        if response.clicked() {
            response.request_focus();
        }
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
        }

        ui.memory_mut(|mem| {
            mem.set_focus_lock_filter(
                response.id,
                egui::EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            );
        });

        response
    }
}

fn enable_trzsz(mut session: TerminalSession) -> TerminalSession {
    session.set_trzsz_policy(Some(TrzszTransferPolicy::default()));
    session
}
