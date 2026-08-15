//! 单个终端会话标签页：持有内核会话、快照与交互状态。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use eframe::egui::{self, FontId, PointerButton, Pos2, Rect, Response, Vec2};
use hapcli_sftp::join_remote_path;
use hapcli_terminal::{
    GraphicsOptions, SerialSessionConfig, TerminalEncoding, TerminalSession, TerminalSessionKind,
    TerminalSearchMatch, TerminalSnapshot, TelnetSessionConfig, TrzszTransferPolicy,
};

use crate::keys;
use crate::render::{
    self, ImageTextureCache, ScrollCommand, TextSelection, cell_at, scrollbar_track_rect,
    select_line, select_word_at, selected_text, viewport_highlights,
};
use crate::trzsz::{TrzszPromptRequest, TrzszWorkerEvent};
use zeroize::Zeroizing;

static TRZSZ_OWNER_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug)]
pub struct TerminalPrefs {
    pub copy_on_select: bool,
    pub middle_click_paste: bool,
    /// 终端输入 `cd` 后自动让 SFTP 面板跟随目录。
    pub sftp_sync_cwd: bool,
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
    /// SSH 远程颜色环境注入：是否已尝试、结果接收器、显示用状态。
    pub color_env_attempted: bool,
    pub color_env_rx: Option<Receiver<String>>,
    pub color_env_status: Option<String>,
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
    /// 上一次同步给 SFTP 面板的目录（用于 `cd -`）。
    pub sftp_prev_cwd: Option<String>,
    /// 当前输入行缓冲（用于识别 `cd` 命令）。
    input_line: String,
    /// 输入行经过编辑/补全/粘贴后无法可靠解析。
    input_line_unreliable: bool,
    /// 按回车时提交的 (输入行, 是否不可靠)。
    pending_input_line: Option<(String, bool)>,
    /// SSH 远程 shell 集成自动安装：是否已尝试、结果接收器、显示用状态。
    pub shell_integration_attempted: bool,
    pub shell_integration_rx: Option<Receiver<String>>,
    pub shell_integration_status: Option<String>,
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
    ) -> anyhow::Result<Self> {
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
            color_env_attempted: false,
            color_env_rx: None,
            color_env_status: None,
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
            sftp_prev_cwd: None,
            input_line: String::new(),
            input_line_unreliable: false,
            pending_input_line: None,
            shell_integration_attempted: false,
            shell_integration_rx: None,
            shell_integration_status: None,
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
            color_env_attempted: false,
            color_env_rx: None,
            color_env_status: None,
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
            sftp_prev_cwd: None,
            input_line: String::new(),
            input_line_unreliable: false,
            pending_input_line: None,
            shell_integration_attempted: false,
            shell_integration_rx: None,
            shell_integration_status: None,
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
    ) -> Self {
        let stored_config = config.clone();
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
            color_env_attempted: false,
            color_env_rx: None,
            color_env_status: None,
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
            sftp_prev_cwd: None,
            input_line: String::new(),
            input_line_unreliable: false,
            pending_input_line: None,
            shell_integration_attempted: false,
            shell_integration_rx: None,
            shell_integration_status: None,
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
    ) -> Result<Self, hapcli_terminal::SerialError> {
        let stored_config = config.clone();
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
            color_env_attempted: false,
            color_env_rx: None,
            color_env_status: None,
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
            sftp_prev_cwd: None,
            input_line: String::new(),
            input_line_unreliable: false,
            pending_input_line: None,
            shell_integration_attempted: false,
            shell_integration_rx: None,
            shell_integration_status: None,
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
        // 新连接需要重新执行远程颜色环境注入。
        self.color_env_attempted = false;
        self.color_env_rx = None;
        self.color_env_status = None;
        // 新连接需要重新安装目录同步集成，并清空输入行跟踪。
        self.shell_integration_attempted = false;
        self.shell_integration_rx = None;
        self.shell_integration_status = None;
        self.sftp_prev_cwd = None;
        self.input_line.clear();
        self.input_line_unreliable = false;
        self.pending_input_line = None;
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

    /// 后台线程：等待远程环境探测完成后，通过 SFTP 把 hapcli 的颜色环境块
    /// 写入远程启动文件（幂等）。结果通过 `tx` 回传，由 app 轮询显示。
    pub fn spawn_color_env_worker(handle: hapcli_ssh::SshConnectionHandle, tx: Sender<String>) {
        std::thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = tx.send(format!("颜色环境注入失败（运行时）: {error}"));
                    return;
                }
            };
            let deadline = Instant::now() + Duration::from_secs(12);
            let remote_env = loop {
                if let Some(env) = handle.remote_env() {
                    break Some(env);
                }
                if Instant::now() >= deadline {
                    break None;
                }
                std::thread::sleep(Duration::from_millis(400));
            };
            let Some(remote_env) = remote_env else {
                let _ = tx.send("远程环境探测超时，未自动启用彩色".to_string());
                return;
            };
            let session = match runtime.block_on(handle.acquire_sftp()) {
                Ok(session) => session,
                Err(error) => {
                    let _ = tx.send(format!("SFTP 不可用，未自动启用彩色: {error}"));
                    return;
                }
            };
            let result = runtime.block_on(async {
                let sftp = session.lock().await;
                match hapcli_terminal::inspect_remote_color_environment(
                    &sftp,
                    Some(&remote_env),
                )
                .await
                {
                    Ok(status)
                        if status.state == hapcli_terminal::RemoteColorEnvState::Installed =>
                    {
                        Ok("远程彩色已启用".to_string())
                    }
                    _ => {
                        hapcli_terminal::install_remote_color_environment(
                            &sftp,
                            Some(&remote_env),
                        )
                        .await
                        .map(|status| {
                            format!("已写入 {}，重连后生效", status.startup_file)
                        })
                    }
                }
            });
            let _ = tx.send(match result {
                Ok(message) => message,
                Err(error) => format!("远程彩色启用失败: {error}"),
            });
        });
    }

    /// 后台线程：通过 SFTP 安装远程 shell 集成（OSC 7 目录上报）。
    /// 安装后重连 SSH，终端每次提示符都会上报真实目录，SFTP 面板可精确跟随。
    pub fn spawn_shell_integration_worker(handle: hapcli_ssh::SshConnectionHandle, tx: Sender<String>) {
        std::thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = tx.send(format!("目录同步集成安装失败（运行时）: {error}"));
                    return;
                }
            };
            let deadline = Instant::now() + Duration::from_secs(12);
            let remote_env = loop {
                if let Some(env) = handle.remote_env() {
                    break Some(env);
                }
                if Instant::now() >= deadline {
                    break None;
                }
                std::thread::sleep(Duration::from_millis(400));
            };
            let Some(remote_env) = remote_env else {
                let _ = tx.send("远程环境探测超时，未安装目录同步集成".to_string());
                return;
            };
            let session = match runtime.block_on(handle.acquire_sftp()) {
                Ok(session) => session,
                Err(error) => {
                    let _ = tx.send(format!("SFTP 不可用，未安装目录同步集成: {error}"));
                    return;
                }
            };
            let result = runtime.block_on(async {
                let sftp = session.lock().await;
                match hapcli_terminal::inspect_remote_shell_integration(&sftp, Some(&remote_env)).await
                {
                    Ok(status)
                        if status.state
                            == hapcli_terminal::RemoteShellIntegrationState::Installed =>
                    {
                        Ok("目录同步集成已就绪".to_string())
                    }
                    _ => hapcli_terminal::install_remote_shell_integration(&sftp, Some(&remote_env))
                        .await
                        .map(|_| "目录同步集成已写入，重连后精确生效".to_string()),
                }
            });
            let _ = tx.send(match result {
                Ok(message) => message,
                Err(error) => format!("目录同步集成安装失败: {error}"),
            });
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
                self.session.clear_buffer();
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
}
