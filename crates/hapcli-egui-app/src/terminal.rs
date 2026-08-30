//! 单个终端会话标签页：持有内核会话、快照与交互状态。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::time::Instant;

use eframe::egui::{self, FontId, PointerButton, Pos2, Rect, Response, Vec2};
use hapcli_sftp::join_remote_path;
use hapcli_terminal::{
    GraphicsOptions, SerialSessionConfig, TerminalCursorStyle, TerminalEncoding, TerminalSession,
    TerminalSessionKind, TerminalSearchMatch, TerminalSnapshot, TelnetSessionConfig,
    TrzszTransferPolicy,
};

use crate::keys;
use crate::render::{
    self, ImageTextureCache, ScrollCommand, TextSelection, cell_at, scrollbar_track_rect,
    select_line, select_word_at, selected_text, viewport_highlights,
};
use crate::trzsz::{TrzszPromptRequest, TrzszWorkerEvent};
use zeroize::Zeroizing;

static TRZSZ_OWNER_COUNTER: AtomicU64 = AtomicU64::new(0);

/// 右侧面板当前展示的标签页。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RightPanelTab {
    Files,
    Quick,
    Forward,
}

#[derive(Clone, Copy, Debug)]
pub struct TerminalPrefs {
    pub copy_on_select: bool,
    pub middle_click_paste: bool,
    /// 终端输入 `cd` 后自动让 SFTP 面板跟随目录。
    pub sftp_sync_cwd: bool,
    /// 是否有模态弹窗（设置 / 新建连接 / 重命名）打开，此时不应把控制键转发给终端。
    pub modal_open: bool,
}

/// 终端右键菜单动作（菜单关闭后统一处理，避免在菜单闭包内借用会话）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TerminalMenuAction {
    Copy,
    Paste,
    SelectAll,
    Search,
    Clear,
}

fn new_trzsz_owner_id() -> String {
    let sequence = TRZSZ_OWNER_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("egui-{}-{sequence}", std::process::id())
}

pub struct TerminalTab {
    pub session: TerminalSession,
    pub snapshot: TerminalSnapshot,
    pub last_terminal_size: (usize, usize),
    /// 会话滚动历史行数（新建 / 重连时传给内核）。
    scrollback_lines: usize,
    /// 会话默认光标样式（新建 / 重连时传给内核）。
    cursor_style: TerminalCursorStyle,
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
    /// 搜索框已消费 Enter（用于导航），终端不应再把该 Enter 转发给 shell。
    pub search_enter_consumed: bool,
    /// 右侧面板是否打开及当前标签（SFTP / 快捷命令）。
    pub right_panel: Option<RightPanelTab>,
    pub sftp: Option<crate::sftp::SftpPanelState>,
    /// 本地终端使用的本地目录浏览器状态。
    pub local_browser: Option<crate::sftp::LocalBrowserState>,
    /// 上一次同步给 SFTP 面板的目录（用于 `cd -`）。
    pub sftp_prev_cwd: Option<String>,
    /// 当前输入行缓冲（用于识别 `cd` 命令）。
    input_line: String,
    /// 输入行经过编辑/补全/粘贴后无法可靠解析。
    input_line_unreliable: bool,
    /// 按回车时提交的 (输入行, 是否不可靠)。
    pending_input_line: Option<(String, bool)>,
    /// 拖选自动滚动的累积量（跨帧，保证越过边界时平滑持续滚动）。
    drag_scroll_accum: f32,
    /// 是否有本地文件正拖拽悬停在终端上（用于识别拖放落点）。
    drop_hovering: bool,
    /// 用户自定义标签名；None 表示自动显示（会话标题 / 连接名）。
    pub custom_label: Option<String>,
    /// Telnet / 串口配置（用于“复制标签”）。
    pub telnet_config: Option<TelnetSessionConfig>,
    pub serial_config: Option<SerialSessionConfig>,
    pub forward: Option<crate::forward::ForwardPanel>,
    pub image_textures: ImageTextureCache,
    /// 长命令完成提醒：标签页显示 🔔，激活后清除。
    pub notify_pending: bool,
    pub foreground_track: Option<(u32, Instant, Option<String>)>,
    pub last_probe: Option<Instant>,
    /// 静态标签：本地会话或 `user@host` 基础标签。
    base_label: String,
}

impl TerminalTab {
    pub fn new_local(
        ctx: &egui::Context,
        cols: usize,
        rows: usize,
        scrollback_lines: usize,
        cursor_style: TerminalCursorStyle,
    ) -> anyhow::Result<Self> {
        let session = enable_trzsz(TerminalSession::local_with_graphics_and_encoding(
            cols,
            rows,
            GraphicsOptions::default(),
            TerminalEncoding::Utf8,
            scrollback_lines,
            cursor_style,
        )?);
        let snapshot = session.snapshot();
        Self::spawn_activity_thread(&session, ctx);
        Ok(Self {
            session,
            snapshot,
            last_terminal_size: (cols, rows),
            scrollback_lines,
            cursor_style,
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
            search_enter_consumed: false,
            right_panel: None,
            sftp: None,
            local_browser: None,
            sftp_prev_cwd: None,
            input_line: String::new(),
            input_line_unreliable: false,
            pending_input_line: None,
            drag_scroll_accum: 0.0,
            drop_hovering: false,
            custom_label: None,
            telnet_config: None,
            serial_config: None,
            forward: None,
            notify_pending: false,
            foreground_track: None,
            last_probe: None,
            image_textures: ImageTextureCache::default(),
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
        scrollback_lines: usize,
        cursor_style: TerminalCursorStyle,
    ) -> Self {
        let session = enable_trzsz(TerminalSession::ssh_with_graphics_and_encoding(
            config,
            cols,
            rows,
            GraphicsOptions::default(),
            TerminalEncoding::Utf8,
            scrollback_lines,
            cursor_style,
        ));
        let snapshot = session.snapshot();
        Self::spawn_activity_thread(&session, ctx);
        Self {
            session,
            snapshot,
            last_terminal_size: (cols, rows),
            scrollback_lines,
            cursor_style,
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
            search_enter_consumed: false,
            right_panel: None,
            sftp: None,
            local_browser: None,
            sftp_prev_cwd: None,
            input_line: String::new(),
            input_line_unreliable: false,
            pending_input_line: None,
            drag_scroll_accum: 0.0,
            drop_hovering: false,
            custom_label: None,
            telnet_config: None,
            serial_config: None,
            forward: None,
            notify_pending: false,
            foreground_track: None,
            last_probe: None,
            image_textures: ImageTextureCache::default(),
            base_label,
        }
    }

    pub fn new_telnet(
        ctx: &egui::Context,
        config: TelnetSessionConfig,
        base_label: String,
        cols: usize,
        rows: usize,
        scrollback_lines: usize,
        cursor_style: TerminalCursorStyle,
    ) -> Self {
        let stored_config = config.clone();
        let session = enable_trzsz(TerminalSession::telnet_with_graphics_and_encoding(
            config,
            cols,
            rows,
            GraphicsOptions::default(),
            TerminalEncoding::Utf8,
            scrollback_lines,
            cursor_style,
        ));
        let snapshot = session.snapshot();
        Self::spawn_activity_thread(&session, ctx);
        Self {
            session,
            snapshot,
            last_terminal_size: (cols, rows),
            scrollback_lines,
            cursor_style,
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
            search_enter_consumed: false,
            right_panel: None,
            sftp: None,
            local_browser: None,
            sftp_prev_cwd: None,
            input_line: String::new(),
            input_line_unreliable: false,
            pending_input_line: None,
            drag_scroll_accum: 0.0,
            drop_hovering: false,
            custom_label: None,
            telnet_config: Some(stored_config),
            serial_config: None,
            forward: None,
            notify_pending: false,
            foreground_track: None,
            last_probe: None,
            image_textures: ImageTextureCache::default(),
            base_label,
        }
    }

    pub fn new_serial(
        ctx: &egui::Context,
        config: SerialSessionConfig,
        base_label: String,
        cols: usize,
        rows: usize,
        scrollback_lines: usize,
        cursor_style: TerminalCursorStyle,
    ) -> Result<Self, hapcli_terminal::SerialError> {
        let stored_config = config.clone();
        let session = TerminalSession::serial_with_graphics_and_encoding(
            config,
            cols,
            rows,
            GraphicsOptions::default(),
            TerminalEncoding::Utf8,
            scrollback_lines,
            cursor_style,
        )?;
        let snapshot = session.snapshot();
        Self::spawn_activity_thread(&session, ctx);
        Ok(Self {
            session,
            snapshot,
            last_terminal_size: (cols, rows),
            scrollback_lines,
            cursor_style,
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
            search_enter_consumed: false,
            right_panel: None,
            sftp: None,
            local_browser: None,
            sftp_prev_cwd: None,
            input_line: String::new(),
            input_line_unreliable: false,
            pending_input_line: None,
            drag_scroll_accum: 0.0,
            drop_hovering: false,
            custom_label: None,
            telnet_config: None,
            serial_config: Some(stored_config),
            forward: None,
            notify_pending: false,
            foreground_track: None,
            last_probe: None,
            image_textures: ImageTextureCache::default(),
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
        let mut session = TerminalSession::ssh_with_graphics_and_encoding(
            session_config,
            cols,
            rows,
            GraphicsOptions::default(),
            TerminalEncoding::Utf8,
            self.scrollback_lines,
            self.cursor_style,
        );
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
        // 新连接需要重新执行远程颜色环境注入。
        self.sftp_prev_cwd = None;
        self.input_line.clear();
        self.input_line_unreliable = false;
        self.pending_input_line = None;
        self.drag_scroll_accum = 0.0;
        self.drop_hovering = false;
        self.search_enter_consumed = false;
        self.right_panel = None;
        self.local_browser = None;
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
        let label = self
            .custom_label
            .clone()
            .unwrap_or_else(|| self.display_label_inner());
        if self.notify_pending {
            format!("🔔 {label}")
        } else {
            label
        }
    }

    /// 会话的基础标签（连接名 / “本地”）。
    pub fn base_label(&self) -> &str {
        &self.base_label
    }

    fn display_label_inner(&self) -> String {
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
        // 拖选自动滚动的累积量（与滚轮分开，滚动时不清除选区）。
        let mut drag_scroll: i32 = 0;
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
        // 拖到终端区域内的本地文件路径（写入 shell 输入行）。
        let mut terminal_drop_paths: Vec<String> = Vec::new();
        ctx.input(|i| {
            if !i.raw.dropped_files.is_empty() {
                let over_terminal = i.pointer.hover_pos().is_some_and(|pos| {
                    self.last_rect
                        .is_some_and(|rect| rect.contains(pos))
                });
                if over_terminal || self.drop_hovering {
                    terminal_drop_paths = i
                        .raw
                        .dropped_files
                        .iter()
                        .filter_map(|file| file.path.clone())
                        .map(|path| path.display().to_string())
                        .collect();
                }
            }
            let over_terminal = i.pointer.hover_pos().is_some_and(|pos| {
                self.last_rect
                    .is_some_and(|rect| rect.contains(pos))
            });
            self.drop_hovering = over_terminal && !i.raw.hovered_files.is_empty();
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
                                self.input_line_unreliable = true;
                                bytes.insert(0, 0x1b);
                            } else {
                                self.input_line.push_str(text);
                            }
                            writes.push(bytes);
                        }
                    }
                    egui::Event::Paste(text) => {
                        if terminal_owns_keys && !self.search_open {
                            self.input_line_unreliable = true;
                            let _ = self.session.paste_text(text);
                        }
                    }
                    egui::Event::Ime(egui::ImeEvent::Commit(text)) => {
                        if terminal_owns_keys
                            && !self.search_open
                            && !text.is_empty()
                            && !i.modifiers.mac_cmd
                        {
                            self.input_line.push_str(text);
                            writes.push(text.as_bytes().to_vec());
                        }
                    }
                    egui::Event::Key {
                        key,
                        pressed,
                        modifiers,
                        ..
                    } => {
                        // 搜索栏打开时处理导航键。egui 的单行输入框在 Esc/Enter
                        // 时都会先释放焦点，因此不能依赖“搜索框持有焦点”判断。
                        if self.search_open && *pressed {
                            // 搜索框已消费 Enter（导航到下一个/上一个匹配）：
                            // 该 Enter 不再转发给 shell。
                            if self.search_enter_consumed {
                                self.search_enter_consumed = false;
                                continue;
                            }
                            if *key == egui::Key::Escape {
                                self.search_open = false;
                                continue;
                            }
                        }
                        // 搜索框持有键盘时，其余按键不转发给终端（搜索框自身处理）。
                        if self.search_open && !terminal_owns_keys {
                            continue;
                        }
                        // 焦点在其他控件（弹窗输入框等）时不转发键盘。
                        if !terminal_owns_keys {
                            // 例外：普通控件（如标签栏/工具栏按钮）持有焦点时，
                            // 控制键（Ctrl+字母）仍转发给终端，保证 npm 等前台
                            // 进程可被 Ctrl+C 中断；模态弹窗/搜索框打开时不转发。
                            let forward_control = !self.search_open
                                && !prefs.modal_open
                                && *pressed
                                && modifiers.ctrl
                                && key.name().len() == 1
                                && key.name().as_bytes()[0].is_ascii_alphabetic();
                            if !forward_control {
                                continue;
                            }
                        }
                        if *pressed && modifiers.command && *key == egui::Key::F {
                            self.search_open = true;
                            self.search_focus_requested = true;
                            self.refresh_search();
                            continue;
                        }
                        // 输入行跟踪：识别 `cd` 命令，供 SFTP 面板目录同步使用。
                        if *pressed {
                            match key {
                                egui::Key::Enter => {
                                    let line = std::mem::take(&mut self.input_line);
                                    let unreliable = self.input_line_unreliable;
                                    self.input_line_unreliable = false;
                                    self.pending_input_line = Some((line, unreliable));
                                }
                                egui::Key::Backspace => {
                                    self.input_line.pop();
                                }
                                egui::Key::Tab => {
                                    self.input_line_unreliable = true;
                                }
                                egui::Key::ArrowLeft
                                | egui::Key::ArrowRight
                                | egui::Key::ArrowUp
                                | egui::Key::ArrowDown
                                | egui::Key::Home
                                | egui::Key::End
                                | egui::Key::Delete => {
                                    self.input_line_unreliable = true;
                                }
                                _ => {}
                            }
                            if modifiers.ctrl {
                                match key {
                                    egui::Key::C | egui::Key::U => {
                                        self.input_line.clear();
                                        self.input_line_unreliable = false;
                                    }
                                    egui::Key::W => {
                                        delete_last_word(&mut self.input_line);
                                    }
                                    _ => {}
                                }
                            }
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

        // 阶段二：退出输入锁后处理拖放、指针事件与剪贴板。
        if !terminal_drop_paths.is_empty() {
            let inserted: String = terminal_drop_paths
                .iter()
                .map(|path| shell_quote_path(path))
                .collect::<Vec<_>>()
                .join(" ")
                + " ";
            let _ = self.session.write_text(&inserted);
            // 同步更新输入行跟踪（保证后续 `cd` 路径解析仍准确）。
            self.input_line.push_str(&inserted);
        }

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
                // 指针越过终端边界时自动滚动视口：越过下边界向下翻页，
                // 越过上边界向上翻页，选区同步平移/扩展以便连续复制。
                if let Some(rect) = self.last_rect {
                    let rows = self.snapshot.rows;
                    let cols = self.snapshot.cols.max(1);
                    let edge_col = |x: f32| {
                        (((x - rect.left()) / cell_size.x).floor() as i32)
                            .clamp(0, cols.saturating_sub(1) as i32)
                            as usize
                    };
                    if pos.y > rect.bottom() {
                        let depth = ((pos.y - rect.bottom()) / cell_size.y).clamp(0.0, 8.0);
                        self.drag_scroll_accum += depth * 0.25;
                        let whole = self.drag_scroll_accum.trunc() as i32;
                        self.drag_scroll_accum -= whole as f32;
                        if whole > 0 {
                            let delta = -whole;
                            if let Some(mut selection) = self.selection {
                                // anchor 跟随视口平移（保持同一行内容），
                                // active 钉在底部行，让新滚入的内容继续进入选区。
                                selection.anchor.0 =
                                    selection.anchor.0.saturating_add_signed(delta as isize);
                                selection.active = (rows.saturating_sub(1), edge_col(pos.x));
                                self.selection = Some(selection);
                                self.selection_dragged = true;
                            }
                            drag_scroll += delta;
                        }
                    } else if pos.y < rect.top() {
                        let depth = ((rect.top() - pos.y) / cell_size.y).clamp(0.0, 8.0);
                        self.drag_scroll_accum += depth * 0.25;
                        let whole = self.drag_scroll_accum.trunc() as i32;
                        self.drag_scroll_accum -= whole as f32;
                        if whole > 0 {
                            let delta = whole;
                            if let Some(mut selection) = self.selection {
                                // anchor 钉在顶部行，active 跟随视口平移。
                                selection.active.0 = selection
                                    .active
                                    .0
                                    .saturating_add_signed(delta as isize)
                                    .min(rows.saturating_sub(1));
                                selection.anchor = (0, edge_col(pos.x));
                                self.selection = Some(selection);
                                self.selection_dragged = true;
                            }
                            drag_scroll += delta;
                        }
                    } else {
                        self.drag_scroll_accum = 0.0;
                    }
                }
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
            } else {
                self.drag_scroll_accum = 0.0;
            }
        } else {
            self.drag_scroll_accum = 0.0;
        }

        if copy_requested {
            self.copy_selection(ctx);
        }
        if focus_changed {
            let _ = self.session.set_focused(self.focused);
        }
        if drag_scroll != 0 {
            self.session.scroll_lines(drag_scroll);
        }
        if scroll_lines != 0 {
            self.selection = None;
            self.session.scroll_lines(scroll_lines);
        }
        for bytes in writes {
            self.selection = None;
            let _ = self.session.write_input(&bytes);
        }
        // 回车提交的命令行：尝试让 SFTP 面板跟随 `cd` 目录。
        if let Some((line, unreliable)) = self.pending_input_line.take()
            && !unreliable
        {
            self.sync_sftp_after_submit(&line, &prefs);
        }
    }

    /// 提交的输入行以 `cd` 开头时，把 SFTP 面板切换到解析出的目录。
    fn sync_sftp_after_submit(&mut self, line: &str, prefs: &TerminalPrefs) {
        if !prefs.sftp_sync_cwd {
            return;
        }
        if self.session.status().kind != TerminalSessionKind::SshPty {
            return;
        }
        let Some(panel_cwd) = self.sftp.as_ref().map(|panel| panel.cwd.clone()) else {
            return;
        };
        let Some(target) = resolve_cd_target(line, &panel_cwd, self.sftp_prev_cwd.as_deref()) else {
            return;
        };
        self.sftp_prev_cwd = Some(panel_cwd);
        if let Some(panel) = self.sftp.as_ref() {
            panel.send(crate::sftp::SftpCommand::List(target));
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

    /// 全选当前可见屏幕（与三击选行同一套选区坐标）。
    fn select_all(&mut self) {
        self.selection = Some(TextSelection {
            anchor: (0, 0),
            active: (
                self.snapshot.rows.saturating_sub(1),
                self.snapshot.cols.saturating_sub(1),
            ),
        });
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
            self.snapshot.images.as_slice(),
            &mut self.image_textures,
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
        // 无任何 egui 控件持有焦点时，把焦点固定在终端上。
        // 否则按 Tab 完成路径补全时，egui 会把焦点挪到标签栏按钮上，
        // 连续 Tab 就会在多个标签之间跳动而不是继续补全。
        if ui.ctx().memory(|memory| memory.focused().is_none()) {
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

        // 终端右键菜单：复制 / 粘贴 / 全选 / 搜索 / 清屏。
        let copy_enabled = self.selection.is_some();
        let mut menu_action: Option<TerminalMenuAction> = None;
        response.context_menu(|ui| {
            ui.set_min_width(150.0);
            if ui
                .add_enabled(copy_enabled, egui::Button::new("复制"))
                .on_hover_text("复制选中内容（⌘C）")
                .clicked()
            {
                menu_action = Some(TerminalMenuAction::Copy);
                ui.close_menu();
            }
            if ui
                .button("粘贴")
                .on_hover_text("粘贴剪贴板内容（⌘V）")
                .clicked()
            {
                menu_action = Some(TerminalMenuAction::Paste);
                ui.close_menu();
            }
            ui.separator();
            if ui.button("全选").clicked() {
                menu_action = Some(TerminalMenuAction::SelectAll);
                ui.close_menu();
            }
            if ui.button("搜索…").clicked() {
                menu_action = Some(TerminalMenuAction::Search);
                ui.close_menu();
            }
            ui.separator();
            if ui.button("清屏").clicked() {
                menu_action = Some(TerminalMenuAction::Clear);
                ui.close_menu();
            }
        });
        match menu_action {
            Some(TerminalMenuAction::Copy) => self.copy_selection(ui.ctx()),
            Some(TerminalMenuAction::Paste) => self.paste_clipboard(),
            Some(TerminalMenuAction::SelectAll) => self.select_all(),
            Some(TerminalMenuAction::Search) => {
                self.search_open = true;
                self.search_focus_requested = true;
                self.refresh_search();
            }
            Some(TerminalMenuAction::Clear) => {
                // 标准清屏：先清空本地缓冲（屏幕 + 滚动历史），
                // 再发 Ctrl+L 让 shell 把提示符（含未提交的输入）重绘到顶部。
                self.session.clear_buffer();
                let _ = self.session.write_input(&[0x0c]);
                self.selection = None;
                self.search_matches.clear();
                self.search_current = None;
            }
            None => {}
        }

        response
    }
}

fn enable_trzsz(mut session: TerminalSession) -> TerminalSession {
    session.set_trzsz_policy(Some(TrzszTransferPolicy::default()));
    session
}

/// Ctrl+W：删除输入行里最后一个词（含其前导空白）。
fn delete_last_word(line: &mut String) {
    let end = line.trim_end().len();
    if end == 0 {
        line.clear();
        return;
    }
    let bytes = line.as_bytes();
    let mut start = end;
    while start > 0 && !bytes[start - 1].is_ascii_whitespace() {
        start -= 1;
    }
    let mut after_space = start;
    while after_space > 0 && bytes[after_space - 1].is_ascii_whitespace() {
        after_space -= 1;
    }
    line.truncate(after_space);
}

/// 把本地路径转成 shell 可用的引号形式（拖拽插入用）：
/// Unix 用单引号（空格/特殊字符安全），Windows 用双引号。
fn shell_quote_path(path: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        format!("\"{}\"", path.replace('"', "\"\""))
    }
    #[cfg(not(target_os = "windows"))]
    {
        format!("'{}'", path.replace('\'', "'\\''"))
    }
}

/// 判断远程路径是否为绝对路径（Unix `/` 或 Windows 盘符 `C:/`）。
fn looks_absolute_remote(path: &str) -> bool {
    if path.starts_with('/') {
        return true;
    }
    let bytes = path.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\')
}

/// 从已提交的命令行中解析 `cd` 目标目录。
///
/// 返回可以直接交给 SFTP 面板的路径；遇到无法可靠解析的形式返回 `None`
/// （此时留给远程 shell 集成（OSC 7）在后续提示符精确上报）。
fn resolve_cd_target(line: &str, panel_cwd: &str, prev_cwd: Option<&str>) -> Option<String> {
    let mut parts = line.split_whitespace();
    if parts.next()? != "cd" {
        return None;
    }
    let Some(target) = parts.next() else {
        // `cd` 单独输入：回到用户主目录。
        return Some("~".to_string());
    };
    if target == "-" {
        return prev_cwd.map(str::to_string);
    }
    if target.starts_with('-') {
        return None;
    }
    if target == "$HOME" {
        return Some("~".to_string());
    }
    if let Some(rest) = target.strip_prefix("$HOME/") {
        return Some(join_remote_path("~", rest));
    }
    if target.starts_with('$') {
        return None;
    }
    if target.contains(['&', '|', ';', '<', '>', '"', '\'', '(', ')', '`']) {
        return None;
    }
    if target.starts_with('~') {
        return Some(target.to_string());
    }
    if looks_absolute_remote(target) {
        return Some(target.to_string());
    }
    Some(join_remote_path(panel_cwd, target))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cd_resolves_relative_paths_against_panel_cwd() {
        assert_eq!(
            resolve_cd_target("cd cert", "/home/user", None),
            Some("/home/user/cert".to_string())
        );
        assert_eq!(
            resolve_cd_target("cd ../www", "/home/user", None),
            Some("/home/user/../www".to_string())
        );
        assert_eq!(
            resolve_cd_target("cd cert && make", "/home/user", None),
            Some("/home/user/cert".to_string())
        );
    }

    #[test]
    fn cd_resolves_absolute_home_and_previous() {
        assert_eq!(
            resolve_cd_target("cd /etc", "/home/user", None),
            Some("/etc".to_string())
        );
        assert_eq!(
            resolve_cd_target("cd ~/projects", "/home/user", None),
            Some("~/projects".to_string())
        );
        assert_eq!(
            resolve_cd_target("cd", "/var/www", None),
            Some("~".to_string())
        );
        assert_eq!(
            resolve_cd_target("cd $HOME/code", "/var/www", None),
            Some("~/code".to_string())
        );
        assert_eq!(
            resolve_cd_target("cd -", "/var/www", Some("/var/log")),
            Some("/var/log".to_string())
        );
    }

    #[test]
    fn cd_skips_unreliable_or_non_cd_commands() {
        assert_eq!(resolve_cd_target("ls cert", "/home/user", None), None);
        assert_eq!(resolve_cd_target("git cd x", "/home/user", None), None);
        assert_eq!(resolve_cd_target("cd && pwd", "/home/user", None), None);
        assert_eq!(resolve_cd_target("cd $MYDIR", "/home/user", None), None);
        assert_eq!(resolve_cd_target("cd --", "/home/user", None), None);
        assert_eq!(resolve_cd_target("cd -", "/home/user", None), None);
        assert_eq!(
            resolve_cd_target("cd \"my dir\"", "/home/user", None),
            None
        );
    }

    #[test]
    fn windows_cd_paths_are_absolute() {
        assert_eq!(
            resolve_cd_target("cd C:\\Users\\demo", "/home/user", None),
            Some("C:\\Users\\demo".to_string())
        );
        assert_eq!(
            resolve_cd_target("cd D:/Data", "/home/user", None),
            Some("D:/Data".to_string())
        );
    }

    #[test]
    fn ctrl_w_deletes_last_word() {
        let mut line = "cd cert ".to_string();
        delete_last_word(&mut line);
        assert_eq!(line, "cd");
        let mut line = "cd cert".to_string();
        delete_last_word(&mut line);
        assert_eq!(line, "cd");
        let mut line = "cd".to_string();
        delete_last_word(&mut line);
        assert_eq!(line, "");
    }

    #[test]
    fn shell_quote_path_handles_spaces_and_quotes() {
        let quoted = shell_quote_path("/Users/me/My Documents/a.txt");
        assert_eq!(quoted, "'/Users/me/My Documents/a.txt'");
        let quoted = shell_quote_path("/it's/a/path");
        assert_eq!(quoted, "'/it'\\''s/a/path'");
    }

}
