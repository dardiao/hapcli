use std::time::{Duration, Instant};

use eframe::egui::{self, FontFamily, FontId, PointerButton, RichText, Vec2};
use hapcli_terminal::{
    TerminalEvent, TerminalSessionKind, TrzszTransferDirection, TrzszTransferSelection,
};
use hapcli_trzsz::{TrzszState, TrzszTransferPolicy};

use crate::connect::{ConnectForm, ConnectTarget, DialogOutcome};
use crate::forward;
use crate::render::build_theme;
use crate::quick::QuickCommandsPanel;
use crate::settings::{AppSettings, ThemeChoice, load_settings, save_settings};
use crate::sftp;
use crate::terminal::{TerminalPrefs, TerminalTab};
use crate::trzsz::{TrzszPromptRequest, TrzszPromptSelection, TrzszWorkerEvent, spawn_trzsz_worker};
use crate::update::{UpdateCheckState, UpdateStatus, open_url, parse_proxy_prefixes};

const MIN_FONT_SIZE: f32 = 9.0;
const MAX_FONT_SIZE: f32 = 24.0;
const MAX_RECONNECT_ATTEMPTS: u32 = 3;
const RECONNECT_DELAY: Duration = Duration::from_millis(2500);

/// 释放当前焦点（弹窗关闭后调用，避免键盘焦点滞留导致终端无法输入）。
fn surrender_focus(ctx: &egui::Context) {
    ctx.memory_mut(|memory| {
        if let Some(id) = memory.focused() {
            memory.surrender_focus(id);
        }
    });
}

/// 应用 egui 界面主题（深/浅色），让面板、弹窗等跟随设置。
fn apply_egui_theme(ctx: &egui::Context, choice: ThemeChoice) {
    let visuals = match choice {
        ThemeChoice::Dark => egui::Visuals::dark(),
        ThemeChoice::Light => egui::Visuals::light(),
    };
    ctx.set_visuals(visuals);
}

pub struct HapcliApp {
    tabs: Vec<TerminalTab>,
    active_tab: usize,
    settings: AppSettings,
    custom_font_loaded: bool,
    window_focused: bool,
    show_connect_dialog: bool,
    show_settings: bool,
    settings_error: Option<String>,
    connect_form: ConnectForm,
    trzsz_state: std::sync::Arc<TrzszState>,
    ssh_registry: hapcli_ssh::SshConnectionRegistry,
    last_window_title: String,
    quick_panel: QuickCommandsPanel,
    update_state: UpdateCheckState,
    rename_tab_index: Option<usize>,
    rename_draft: String,
    /// 当前已应用的 egui 界面主题（用于检测切换后重绘）。
    applied_ui_theme: Option<ThemeChoice>,
}

impl HapcliApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> anyhow::Result<Self> {
        let settings = load_settings();
        let custom_font_loaded = install_fonts(&cc.egui_ctx, &settings);
        let initial_theme = settings.theme;
        apply_egui_theme(&cc.egui_ctx, initial_theme);

        let local = TerminalTab::new_local(&cc.egui_ctx, 100, 30)?;
        Ok(Self {
            tabs: vec![local],
            active_tab: 0,
            settings,
            custom_font_loaded,
            window_focused: true,
            show_connect_dialog: false,
            show_settings: false,
            settings_error: None,
            connect_form: ConnectForm::default(),
            trzsz_state: TrzszState::new(),
            ssh_registry: hapcli_ssh::SshConnectionRegistry::new(
                hapcli_ssh::ConnectionPoolConfig::default(),
            ),
            last_window_title: String::new(),
            quick_panel: QuickCommandsPanel::new(),
            update_state: UpdateCheckState::default(),
            rename_tab_index: None,
            rename_draft: String::new(),
            applied_ui_theme: Some(initial_theme),
        })
    }

    fn active_tab(&mut self) -> &mut TerminalTab {
        &mut self.tabs[self.active_tab]
    }

    fn activate_tab(&mut self, index: usize) {
        if index == self.active_tab {
            return;
        }
        self.active_tab = index;
        self.tabs[index].notify_pending = false;
        self.tabs[index].focused = self.window_focused;
        let _ = self.tabs[index].session.set_focused(self.window_focused);
    }

    fn close_tab(&mut self, index: usize) {
        if self.tabs.len() <= 1 {
            return;
        }
        self.tabs[index].session.shutdown();
        if self.rename_tab_index == Some(index) {
            self.rename_tab_index = None;
            self.rename_draft.clear();
        } else if let Some(rename_index) = self.rename_tab_index
            && rename_index > index
        {
            self.rename_tab_index = Some(rename_index - 1);
        }
        self.tabs.remove(index);
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        } else if self.active_tab > index {
            self.active_tab -= 1;
        }
    }

    /// 处理标签右键菜单动作。
    fn handle_tab_menu(&mut self, ctx: &egui::Context, index: usize, action: TabMenuAction) {
        match action {
            TabMenuAction::Rename => {
                let draft = self.tabs[index]
                    .custom_label
                    .clone()
                    .unwrap_or_else(|| self.tabs[index].base_label().to_string());
                self.rename_draft = draft;
                self.rename_tab_index = Some(index);
            }
            TabMenuAction::Duplicate => {
                let (cols, rows) = self.tabs[index].last_terminal_size;
                self.duplicate_tab(ctx, index, cols, rows);
            }
            TabMenuAction::Close => {
                self.close_tab(index);
            }
            TabMenuAction::CloseOthers => {
                self.close_other_tabs(index);
            }
        }
    }

    /// 复制当前标签：按会话类型用保存的配置新建一个同款标签。
    fn duplicate_tab(&mut self, ctx: &egui::Context, index: usize, cols: usize, rows: usize) {
        let Some(source) = self.tabs.get(index) else {
            return;
        };
        let base = source.base_label().to_string();
        let label = format!("{base} (副本)");
        match source.session.status().kind {
            TerminalSessionKind::LocalPty => {
                self.add_local_tab(ctx, cols, rows);
            }
            TerminalSessionKind::SshPty => {
                let Some(config) = source.ssh_reconnect_config.clone() else {
                    return;
                };
                let registry = source.ssh_registry.clone();
                let session_config = crate::connect::build_reconnect_session_config(&config);
                let session_config = if let Some(registry) = &registry {
                    session_config.with_registry(
                        registry.clone(),
                        hapcli_ssh::ConnectionConsumer::Terminal("egui-terminal".to_string()),
                    )
                } else {
                    session_config
                };
                let tab = TerminalTab::new_ssh(
                    ctx,
                    session_config,
                    Some(config),
                    registry,
                    label,
                    cols,
                    rows,
                );
                self.tabs.push(tab);
                self.active_tab = self.tabs.len() - 1;
            }
            TerminalSessionKind::Telnet => {
                let Some(config) = source.telnet_config.clone() else {
                    return;
                };
                let tab = TerminalTab::new_telnet(ctx, config, label, cols, rows);
                self.tabs.push(tab);
                self.active_tab = self.tabs.len() - 1;
            }
            TerminalSessionKind::Serial => {
                let Some(config) = source.serial_config.clone() else {
                    return;
                };
                match TerminalTab::new_serial(ctx, config, label, cols, rows) {
                    Ok(tab) => {
                        self.tabs.push(tab);
                        self.active_tab = self.tabs.len() - 1;
                    }
                    Err(error) => {
                        self.connect_form.show_error(format!("复制串口标签失败: {error}"));
                    }
                }
            }
        }
    }

    /// 关闭除指定标签外的所有标签。
    fn close_other_tabs(&mut self, keep: usize) {
        if self.tabs.len() <= 1 {
            return;
        }
        let keep_tab = self.tabs.remove(keep);
        for tab in &mut self.tabs {
            tab.session.shutdown();
        }
        self.tabs = vec![keep_tab];
        self.active_tab = 0;
        self.rename_tab_index = None;
        self.rename_draft.clear();
    }

    /// 重命名标签弹窗。
    fn rename_tab_window(&mut self, ctx: &egui::Context) {
        let Some(index) = self.rename_tab_index else {
            return;
        };
        if index >= self.tabs.len() {
            self.rename_tab_index = None;
            self.rename_draft.clear();
            return;
        }
        let current = self.tabs[index].display_label();
        let mut confirmed = false;
        let mut cancelled = false;
        egui::Window::new("重命名标签")
            .collapsible(false)
            .resizable(false)
            .default_pos(ctx.screen_rect().center() - egui::vec2(140.0, 45.0))
            .show(ctx, |ui| {
                ui.label(format!("当前名称：{current}"));
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.rename_draft)
                        .desired_width(240.0)
                        .hint_text("留空恢复默认名称"),
                );
                let enter_pressed =
                    response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("确定").clicked() || enter_pressed {
                        confirmed = true;
                    }
                    if ui.button("取消").clicked() {
                        cancelled = true;
                    }
                });
            });
        if confirmed {
            let name = self.rename_draft.trim().to_string();
            if name.is_empty() {
                self.tabs[index].custom_label = None;
            } else {
                self.tabs[index].custom_label = Some(name);
            }
            self.rename_tab_index = None;
            self.rename_draft.clear();
            surrender_focus(ctx);
        } else if cancelled {
            self.rename_tab_index = None;
            self.rename_draft.clear();
            surrender_focus(ctx);
        }
    }

    fn add_local_tab(&mut self, ctx: &egui::Context, cols: usize, rows: usize) {
        if let Ok(tab) = TerminalTab::new_local(ctx, cols, rows) {
            self.tabs.push(tab);
            self.active_tab = self.tabs.len() - 1;
        }
    }

    fn add_connect_tab(
        &mut self,
        ctx: &egui::Context,
        request: crate::connect::ConnectRequest,
        cols: usize,
        rows: usize,
    ) -> bool {
        let label = request.label;
        let save_password = request.save_password;
        let mut tab = match request.target {
            ConnectTarget::Ssh(spec) => {
                let session_config = spec.session_config.with_registry(
                    self.ssh_registry.clone(),
                    hapcli_ssh::ConnectionConsumer::Terminal("egui-terminal".to_string()),
                );
                TerminalTab::new_ssh(
                    ctx,
                    session_config,
                    Some(spec.reconnect_config),
                    Some(self.ssh_registry.clone()),
                    label,
                    cols,
                    rows,
                )
            }
            ConnectTarget::Telnet(config) => {
                TerminalTab::new_telnet(ctx, config, label, cols, rows)
            }
            ConnectTarget::Serial(config) => match TerminalTab::new_serial(
                ctx, config, label, cols, rows,
            ) {
                Ok(tab) => tab,
                Err(error) => {
                    self.connect_form.show_error(format!("串口打开失败: {error}"));
                    return false;
                }
            },
        };
        tab.pending_keychain_save = save_password;
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        true
    }

    fn status_line(&self) -> String {
        let tab = &self.tabs[self.active_tab];
        let status = tab.session.status();
        let kind = match status.kind {
            hapcli_terminal::TerminalSessionKind::LocalPty => "本地",
            hapcli_terminal::TerminalSessionKind::SshPty => "SSH",
            hapcli_terminal::TerminalSessionKind::Telnet => "Telnet",
            hapcli_terminal::TerminalSessionKind::Serial => "串口",
        };
        let lifecycle = match &status.lifecycle {
            hapcli_terminal::TerminalLifecycle::Running => "运行中".to_string(),
            hapcli_terminal::TerminalLifecycle::Exited(code) => {
                format!("已退出（代码 {}）", code.unwrap_or(-1))
            }
            hapcli_terminal::TerminalLifecycle::Closed => "已关闭".to_string(),
        };
        let title = status.title.clone().unwrap_or_else(|| "zsh".to_string());
        let trzsz = if tab.trzsz_active {
            " · Trzsz 传输中…".to_string()
        } else if let Some(status) = &tab.trzsz_status {
            format!(" · {status}")
        } else {
            String::new()
        };
        let keychain = tab
            .keychain_status
            .as_ref()
            .map(|status| format!(" · {status}"))
            .unwrap_or_default();
        let reconnect = tab
            .reconnect_status
            .as_ref()
            .map(|status| format!(" · {status}"))
            .unwrap_or_default();
        format!(
            "{kind} · {title} · {lifecycle} · {}x{} · 滚动 {} · {:.0}pt{trzsz}{keychain}{reconnect}",
            tab.snapshot.cols,
            tab.snapshot.rows,
            tab.snapshot.display_offset,
            self.settings.font_size,
        )
    }

    /// SSH 断线自动重连调度。
    fn handle_reconnects(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        for index in 0..self.tabs.len() {
            let tab = &mut self.tabs[index];
            if tab.ssh_reconnect_config.is_none() {
                continue;
            }
            let status = tab.session.status();
            if status.title.is_some() {
                tab.ever_connected = true;
                if tab.reconnect_attempts > 0 || tab.reconnect_at.is_some() {
                    tab.reconnect_attempts = 0;
                    tab.reconnect_at = None;
                    tab.reconnect_dismissed = false;
                    tab.reconnect_status = Some("SSH 已重连".to_string());
                }
            }
            if status.lifecycle.is_running() {
                continue;
            }

            // 会话已退出：安排自动重连。
            if tab.reconnect_at.is_none() && tab.ever_connected {
                if self.settings.ssh_auto_reconnect
                    && tab.reconnect_attempts < MAX_RECONNECT_ATTEMPTS
                {
                    tab.reconnect_at = Some(now + RECONNECT_DELAY);
                    tab.reconnect_status = Some("连接已断开，即将自动重连…".to_string());
                } else if tab.reconnect_attempts >= MAX_RECONNECT_ATTEMPTS {
                    tab.reconnect_status = Some("自动重连失败，可手动重连".to_string());
                }
            }

            if let Some(deadline) = tab.reconnect_at {
                if now >= deadline {
                    tab.reconnect_at = None;
                    tab.reconnect_attempts += 1;
                    let (cols, rows) = tab.last_terminal_size;
                    tab.reconnect_with(ctx, cols, rows);
                    tab.reconnect_status =
                        Some(format!("正在重连（第 {} 次）…", tab.reconnect_attempts));
                }
            }
        }
    }

    /// 断开提示窗口（仅活动 SSH 会话、自动重连未在进行时显示）。
    fn reconnect_banner(&mut self, ctx: &egui::Context) {
        let index = self.active_tab;
        let tab = &self.tabs[index];
        if tab.ssh_reconnect_config.is_none()
            || tab.session.lifecycle().is_running()
            || !tab.ever_connected
            || tab.reconnect_at.is_some()
            || tab.reconnect_dismissed
        {
            return;
        }

        let mut manual_reconnect = false;
        let mut dismiss = false;
        let default_pos = ctx.screen_rect().center() - egui::vec2(100.0, 40.0);
        egui::Window::new("连接已断开")
            .collapsible(false)
            .resizable(false)
            .default_pos(default_pos)
            .show(ctx, |ui| {
                ui.label("SSH 连接已断开。");
                ui.horizontal(|ui| {
                    if ui.button("立即重连").clicked() {
                        manual_reconnect = true;
                    }
                    if ui.button("关闭提示").clicked() {
                        dismiss = true;
                    }
                });
            });

        if manual_reconnect {
            let tab = &mut self.tabs[index];
            tab.reconnect_attempts = 0;
            tab.reconnect_at = None;
            let (cols, rows) = tab.last_terminal_size;
            tab.reconnect_with(ctx, cols, rows);
            tab.reconnect_status = Some("正在手动重连…".to_string());
        }
        if dismiss {
            self.tabs[index].reconnect_dismissed = true;
            self.tabs[index].reconnect_status = None;
            surrender_focus(ctx);
        }
    }

    /// SSH 连接成功后把待保存密码写入系统钥匙串。
    fn handle_keychain_saves(&mut self) {
        for tab in &mut self.tabs {
            let Some((key, password)) = tab.pending_keychain_save.take() else {
                continue;
            };
            let status = tab.session.status();
            if status.kind != TerminalSessionKind::SshPty || !status.lifecycle.is_running() {
                // 非 SSH 或连接失败：不保存。
                continue;
            }
            if status.title.is_none() {
                // 仍在连接中，下一帧再检查。
                tab.pending_keychain_save = Some((key, password));
                continue;
            }
            tab.keychain_status = Some(
                match hapcli_secret_store::NativeSecretStore::new("hapcli")
                    .store(&key, password.as_str())
                {
                    Ok(()) => "密码已保存到钥匙串".to_string(),
                    Err(error) => format!("钥匙串保存失败: {error}"),
                },
            );
        }
    }

    /// 轮询 SFTP worker 事件并应用；列表变化时自动刷新。
    fn poll_sftp(&mut self) {
        let index = self.active_tab;
        let mut refresh = false;
        if let Some(panel) = &mut self.tabs[index].sftp {
            loop {
                match panel.rx.try_recv() {
                    Ok(event) => {
                        if panel.apply_event(event) {
                            refresh = true;
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                }
            }
        }
        if refresh {
            if let Some(panel) = &self.tabs[index].sftp {
                panel.refresh();
            }
        }
    }

    /// 轮询端口转发 worker 事件。
    fn poll_forward(&mut self) {
        let index = self.active_tab;
        if let Some(panel) = &mut self.tabs[index].forward {
            loop {
                match panel.rx.try_recv() {
                    Ok(event) => panel.apply_event(event),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                }
            }
        }
    }

    /// 长命令完成通知：探测本地会话前台进程，结束且运行超阈值时发系统通知。
    fn poll_notifications(&mut self) {
        const PROBE_INTERVAL: Duration = Duration::from_millis(500);
        const THRESHOLD: Duration = Duration::from_secs(5);

        let enabled = self.settings.notify_on_long_command;
        let active = self.active_tab;
        let window_focused = self.window_focused;
        let now = Instant::now();

        for index in 0..self.tabs.len() {
            let tab = &mut self.tabs[index];
            if tab.session.status().kind != TerminalSessionKind::LocalPty {
                continue;
            }
            if tab
                .last_probe
                .is_some_and(|last| now.duration_since(last) < PROBE_INTERVAL)
            {
                continue;
            }
            tab.last_probe = Some(now);
            tab.session.refresh_process_info();
            let info = tab.session.process_info();
            let foreground = info.foreground_pid;
            let shell = info.shell_pid;

            if foreground.is_some() && foreground != shell {
                if tab.foreground_track.is_none() {
                    tab.foreground_track =
                        Some((foreground.expect("checked above"), now, info.command.clone()));
                }
                continue;
            }

            // 前台回到 shell：命令结束。
            if let Some((pid, started, command)) = tab.foreground_track.take() {
                let duration = now.duration_since(started);
                if duration >= THRESHOLD && enabled && (index != active || !window_focused) {
                    let name = command.unwrap_or_else(|| format!("pid {pid}"));
                    let name: String = name.chars().take(60).collect();
                    system_notify(
                        "hapcli 命令完成",
                        &format!("{name}（{} 秒）", duration.as_secs()),
                    );
                    tab.notify_pending = true;
                }
            }
        }
    }

    fn terminal_font_id(&self) -> FontId {
        if self.custom_font_loaded {
            FontId::new(
                self.settings.font_size,
                FontFamily::Name("hapcli-terminal-font".into()),
            )
        } else {
            FontId::monospace(self.settings.font_size)
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        let mut save = false;
        let mut reset = false;
        let mut pick_font = false;
        let mut clear_font = false;
        let mut toggle_transparent: Option<bool> = None;

        let default_pos = ctx.screen_rect().center() - egui::vec2(150.0, 190.0);
        egui::Window::new("设置")
            .collapsible(false)
            .resizable(false)
            .default_pos(default_pos)
            .show(ctx, |ui| {
                ui.label(egui::RichText::new("外观").strong());
                ui.add_space(4.0);
                egui::Grid::new("settings_grid")
                    .num_columns(2)
                    .spacing([10.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("主题");
                        let theme_label = match self.settings.theme {
                            ThemeChoice::Dark => "深色",
                            ThemeChoice::Light => "浅色",
                        };
                        egui::ComboBox::from_id_salt("theme_choice")
                            .selected_text(theme_label)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut self.settings.theme,
                                    ThemeChoice::Dark,
                                    "深色",
                                );
                                ui.selectable_value(
                                    &mut self.settings.theme,
                                    ThemeChoice::Light,
                                    "浅色",
                                );
                            });
                        ui.end_row();

                        ui.label("字体大小");
                        ui.add(
                            egui::Slider::new(
                                &mut self.settings.font_size,
                                MIN_FONT_SIZE..=MAX_FONT_SIZE,
                            )
                            .suffix(" pt"),
                        );
                        ui.end_row();

                        ui.label("背景不透明度");
                        ui.add(
                            egui::Slider::new(&mut self.settings.background_alpha, 0.3..=1.0),
                        );
                        ui.end_row();

                        ui.label("透明窗口");
                        let before = self.settings.transparent_window;
                        ui.checkbox(&mut self.settings.transparent_window, "启用");
                        if before != self.settings.transparent_window {
                            toggle_transparent = Some(self.settings.transparent_window);
                        }
                        ui.end_row();

                        ui.label("终端字体");
                        ui.horizontal(|ui| {
                            let font_label = if self.custom_font_loaded {
                                "自定义字体已加载".to_string()
                            } else {
                                format!("默认 ({})", platform_default_font_label())
                            };
                            ui.label(font_label);
                            if ui.small_button("选择字体文件…").clicked() {
                                pick_font = true;
                            }
                            if self.custom_font_loaded && ui.small_button("恢复默认").clicked() {
                                clear_font = true;
                            }
                        });
                        ui.end_row();
                    });

                ui.add_space(8.0);
                ui.label(egui::RichText::new("行为").strong());
                ui.add_space(2.0);
                ui.checkbox(
                    &mut self.settings.copy_on_select,
                    "选中即复制（拖选、双击、三击完成后自动复制）",
                );
                ui.checkbox(
                    &mut self.settings.middle_click_paste,
                    "鼠标中键点击粘贴剪贴板内容",
                );
                ui.checkbox(
                    &mut self.settings.ssh_auto_reconnect,
                    "SSH 断线自动重连（最多 3 次，间隔 2.5 秒）",
                );
                ui.checkbox(
                    &mut self.settings.notify_on_long_command,
                    "长命令完成时发系统通知（前台运行超 5 秒的命令结束，且未在查看该标签页）",
                );
                ui.checkbox(
                    &mut self.settings.ssh_shell_colors,
                    "SSH 远程终端彩色输出（连接时自动让 ls 显示彩色，保证生效，无需修改远程文件）",
                );
                ui.checkbox(
                    &mut self.settings.sftp_sync_cwd,
                    "SSH 终端 cd 自动同步到 SFTP 面板（输入 cd 后立即跟随，无需远程配置）",
                );
                ui.horizontal(|ui| {
                    ui.checkbox(
                        &mut self.settings.check_updates,
                        "启动时及每 6 小时自动检查新版本（发现新版可在应用内直接升级）",
                    );
                    if ui.small_button("立即检查").clicked() {
                        self.update_state
                            .check_now(env!("CARGO_PKG_VERSION"), Duration::ZERO);
                    }
                    match &self.update_state.status {
                        UpdateStatus::Checking => {
                            ui.weak("正在检查…");
                        }
                        UpdateStatus::UpToDate => {
                            ui.weak("已是最新版本");
                        }
                        UpdateStatus::Error(message) => {
                            ui.weak(format!("检查失败：{message}"));
                        }
                        _ => {}
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("GitHub 代理");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.settings.github_proxies)
                            .desired_width(360.0)
                            .hint_text("逗号分隔的代理前缀，留空禁用代理回退"),
                    );
                    if ui
                        .small_button("恢复默认")
                        .on_hover_text("来自 github.akams.cn 收集的加速源")
                        .clicked()
                    {
                        self.settings.github_proxies = crate::settings::default_github_proxies();
                    }
                });
                ui.weak("直连 GitHub 失败时自动测速选择最快的代理下载更新包，适用于国内网络被墙的情况。");
                if self.settings.ignored_update_version.is_some() {
                    ui.horizontal(|ui| {
                        ui.weak(format!(
                            "已忽略版本：{}",
                            self.settings.ignored_update_version.as_deref().unwrap_or("")
                        ));
                        if ui.small_button("恢复提示").clicked() {
                            self.settings.ignored_update_version = None;
                        }
                    });
                }

                if let Some(path) = &self.settings.terminal_font_path {
                    ui.label(
                        egui::RichText::new(format!("字体: {path}"))
                            .size(10.0)
                            .weak(),
                    );
                }
                if let Some(error) = &self.settings_error {
                    ui.colored_label(egui::Color32::from_rgb(0xff, 0x77, 0x77), error);
                }

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("保存").clicked() {
                        save = true;
                    }
                    if ui.button("恢复默认设置").clicked() {
                        reset = true;
                    }
                    if ui.button("关闭").clicked() {
                        self.show_settings = false;
                        surrender_focus(ctx);
                    }
                });
            });

        if let Some(transparent) = toggle_transparent {
            ctx.send_viewport_cmd(egui::ViewportCommand::Transparent(transparent));
        }
        if pick_font {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("字体文件", &["ttf", "otf", "ttc"])
                .pick_file()
            {
                let path = path.display().to_string();
                self.settings.terminal_font_path = Some(path);
                self.custom_font_loaded = install_fonts(ctx, &self.settings);
            }
        }
        if clear_font {
            self.settings.terminal_font_path = None;
            self.custom_font_loaded = install_fonts(ctx, &self.settings);
        }
        if reset {
            self.settings = AppSettings::default();
            self.custom_font_loaded = install_fonts(ctx, &self.settings);
            ctx.send_viewport_cmd(egui::ViewportCommand::Transparent(false));
            self.settings_error = None;
        }
        if save {
            self.settings_error = None;
            if let Err(message) = save_settings(&self.settings) {
                self.settings_error = Some(format!("保存失败: {message}"));
            }
        }
    }

    /// 处理终端事件：Trzsz 提示 / 目录变化（SFTP 跟随）→ 提示窗 → worker 轮询。
    fn handle_terminal_events(&mut self, ctx: &egui::Context) {
        let index = self.active_tab;
        let mut cwds: Vec<String> = Vec::new();

        // 1. 内核事件 → 传输提示请求 / 目录变化。
        {
            let tab = &mut self.tabs[index];
            if tab.trzsz_prompt.is_none() && !tab.trzsz_active {
                for event in tab.session.take_events() {
                    match event {
                        TerminalEvent::TrzszTransferPrompt {
                            direction,
                            selection,
                            remote_is_windows,
                        } => {
                            tab.trzsz_prompt = Some(TrzszPromptRequest {
                                direction,
                                selection,
                                remote_is_windows,
                            });
                            break;
                        }
                        TerminalEvent::CwdChanged { cwd, .. } => cwds.push(cwd),
                        _ => {}
                    }
                }
            } else {
                for event in tab.session.take_events() {
                    if let TerminalEvent::CwdChanged { cwd, .. } = event {
                        cwds.push(cwd);
                    }
                }
            }
        }

        // 1.5 目录变化 → SFTP 面板跟随。
        if self.settings.sftp_sync_cwd {
            for cwd in cwds {
                self.sync_sftp_to(&cwd);
            }
        }

        // 2. 轮询 worker 事件。
        self.poll_trzsz_worker();

        // 3. 连接断开时中断传输。
        {
            let tab = &mut self.tabs[index];
            if tab.trzsz_active && !tab.session.lifecycle().is_running() {
                tab.session.interrupt_trzsz_transfer();
                tab.trzsz_status = Some("连接已断开，传输已中断".to_string());
                tab.trzsz_active = false;
                tab.trzsz_rx = None;
                tab.trzsz_prompt = None;
            }
        }

        // 4. 传输提示窗口。
        let request = self.tabs[index].trzsz_prompt;
        if let Some(request) = request {
            let mut action: Option<TrzszPromptSelection> = None;
            let default_pos = ctx.screen_rect().center() - egui::vec2(140.0, 50.0);
            egui::Window::new("Trzsz 文件传输")
                .collapsible(false)
                .resizable(false)
                .default_pos(default_pos)
                .show(ctx, |ui| {
                    let (title, hint) = match (request.direction, request.selection) {
                        (TrzszTransferDirection::Upload, TrzszTransferSelection::File) => {
                            ("上传文件 (trz)", "远程请求上传文件")
                        }
                        (TrzszTransferDirection::Upload, TrzszTransferSelection::Directory) => {
                            ("上传目录 (trz)", "远程请求上传目录")
                        }
                        (TrzszTransferDirection::Download, _) => {
                            ("下载文件 (tsz)", "远程请求下载文件")
                        }
                    };
                    ui.label(egui::RichText::new(title).strong());
                    ui.label(hint);
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        match request.direction {
                            TrzszTransferDirection::Upload => {
                                let directory =
                                    request.selection == TrzszTransferSelection::Directory;
                                if directory {
                                    if ui.button("选择目录…").clicked() {
                                        action = Some(match rfd::FileDialog::new().pick_folder() {
                                            Some(path) => TrzszPromptSelection::Upload(vec![
                                                path.display().to_string(),
                                            ]),
                                            None => TrzszPromptSelection::Cancelled,
                                        });
                                    }
                                } else if ui.button("选择文件…").clicked() {
                                    action = Some(match rfd::FileDialog::new().pick_files() {
                                        Some(paths) => TrzszPromptSelection::Upload(
                                            paths
                                                .iter()
                                                .map(|path| path.display().to_string())
                                                .collect(),
                                        ),
                                        None => TrzszPromptSelection::Cancelled,
                                    });
                                }
                            }
                            TrzszTransferDirection::Download => {
                                if ui.button("选择保存目录…").clicked() {
                                    action = Some(match rfd::FileDialog::new().pick_folder() {
                                        Some(path) => {
                                            TrzszPromptSelection::DownloadRoot(
                                                path.display().to_string(),
                                            )
                                        }
                                        None => TrzszPromptSelection::Cancelled,
                                    });
                                }
                            }
                        }
                        if ui.button("取消").clicked() {
                            action = Some(TrzszPromptSelection::Cancelled);
                        }
                    });
                });

            if let Some(action) = action {
                self.start_trzsz_worker(index, action);
                surrender_focus(ctx);
            }
        }
    }

    /// 让当前标签页的 SFTP 面板切换到指定远程目录（目录变化事件触发）。
    fn sync_sftp_to(&mut self, cwd: &str) {
        let index = self.active_tab;
        let cwd = cwd.to_string();
        let previous = {
            let tab = &mut self.tabs[index];
            if tab.session.status().kind != TerminalSessionKind::SshPty {
                return;
            }
            let Some(panel) = tab.sftp.as_mut() else {
                return;
            };
            if panel.cwd == cwd {
                return;
            }
            panel.cwd.clone()
        };
        let tab = &mut self.tabs[index];
        tab.sftp_prev_cwd = Some(previous);
        if let Some(panel) = tab.sftp.as_ref() {
            panel.send(sftp::SftpCommand::List(cwd));
        }
    }

    fn start_trzsz_worker(&mut self, index: usize, selection: TrzszPromptSelection) {
        let tab = &mut self.tabs[index];
        let Some(request) = tab.trzsz_prompt.take() else {
            return;
        };
        let Some(transfer) = tab.session.take_trzsz_transfer() else {
            tab.trzsz_status = Some("未能取得传输句柄".to_string());
            return;
        };
        tab.trzsz_status = None;
        tab.trzsz_active = true;
        let columns = tab.snapshot.cols.max(1);
        let owner_id = tab.trzsz_owner_id.clone();
        let rx = spawn_trzsz_worker(
            transfer,
            request,
            selection,
            self.trzsz_state.clone(),
            owner_id,
            TrzszTransferPolicy::default(),
            columns,
        );
        tab.trzsz_rx = Some(rx);
    }

    fn poll_trzsz_worker(&mut self) {
        let index = self.active_tab;
        let rx = self.tabs[index].trzsz_rx.take();
        let mut events = Vec::new();
        let mut disconnected = false;
        if let Some(receiver) = rx.as_ref() {
            loop {
                match receiver.try_recv() {
                    Ok(event) => events.push(event),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }

        if events.is_empty() && rx.is_some() && !disconnected {
            self.tabs[index].trzsz_rx = rx;
            return;
        }

        let tab = &mut self.tabs[index];
        let mut finished = false;
        for event in events {
            match event {
                TrzszWorkerEvent::TerminalOutput(bytes) => {
                    tab.session.feed_trzsz_terminal_output(&bytes);
                }
                TrzszWorkerEvent::Completed => {
                    tab.trzsz_status = Some("Trzsz 传输完成".to_string());
                    finished = true;
                }
                TrzszWorkerEvent::Cancelled => {
                    tab.trzsz_status = Some("Trzsz 传输已取消".to_string());
                    finished = true;
                }
                TrzszWorkerEvent::Failed { code, detail, message } => {
                    let detail_suffix =
                        detail.map(|detail| format!(" ({detail})")).unwrap_or_default();
                    tab.trzsz_status =
                        Some(format!("Trzsz 传输失败 [{code}]{detail_suffix}: {message}"));
                    finished = true;
                }
            }
        }

        if finished {
            tab.finish_trzsz();
        } else if disconnected && tab.trzsz_active {
            tab.trzsz_status = Some("Trzsz 传输进程意外退出".to_string());
            tab.finish_trzsz();
        } else {
            tab.trzsz_rx = rx;
        }
    }
}

impl eframe::App for HapcliApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let font_id = self.terminal_font_id();
        let cell_size = ctx.fonts(|fonts| {
            Vec2::new(
                fonts.glyph_width(&font_id, 'W').ceil().max(1.0),
                fonts.row_height(&font_id).ceil().max(1.0),
            )
        });

        // 1. 全局事件：缩放、窗口焦点、SSH 弹窗开关。
        self.process_global_input(ctx);
        // 1.1 主题切换时实时应用 egui 界面配色。
        if self.applied_ui_theme != Some(self.settings.theme) {
            self.applied_ui_theme = Some(self.settings.theme);
            apply_egui_theme(ctx, self.settings.theme);
        }

        // 2. 顶部标签栏。
        let mut clicked_tab: Option<usize> = None;
        let mut close_tab: Option<usize> = None;
        let mut tab_menu: Option<(usize, TabMenuAction)> = None;
        let mut want_connect = false;
        let mut want_local = false;
        let mut want_settings = false;
        let mut want_reconnect = false;
        let mut toggle_sftp = false;
        let mut toggle_quick = false;
        let mut toggle_forward = false;
        let active_is_ssh = self.tabs[self.active_tab].session.status().kind
            == TerminalSessionKind::SshPty;
        let sftp_connected = active_is_ssh
            && self.tabs[self.active_tab]
                .session
                .ssh_connection_handle()
                .is_some();
        let sftp_open = self.tabs[self.active_tab].sftp.is_some();
        let forward_open = self.tabs[self.active_tab]
            .forward
            .as_ref()
            .is_some_and(|panel| panel.show);
        egui::TopBottomPanel::top("tab_bar").show(ctx, |ui| {
            egui::ScrollArea::horizontal().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for index in 0..self.tabs.len() {
                        let selected = index == self.active_tab;
                        let label = self.tabs[index].display_label();
                        let (clicked, close_clicked, menu) =
                            draw_tab(
                                ui,
                                index,
                                &label,
                                selected,
                                self.tabs.len() > 1,
                                self.settings.theme == ThemeChoice::Light,
                            );
                        if clicked {
                            clicked_tab = Some(index);
                        }
                        if close_clicked {
                            close_tab = Some(index);
                        }
                        if let Some(action) = menu {
                            tab_menu = Some((index, action));
                        }
                    }
                    ui.separator();
                    let add_menu = egui::menu::menu_custom_button(
                        ui,
                        egui::Button::new(
                            egui::RichText::new("＋")
                                .color(egui::Color32::from_rgb(0x28, 0x2a, 0x36))
                                .strong(),
                        )
                        .fill(egui::Color32::from_rgb(0xbd, 0x93, 0xf9))
                        .rounding(6.0),
                        |ui| {
                            if ui.button("SSH 连接…").clicked() {
                                want_connect = true;
                                ui.close_menu();
                            }
                            if ui.button("本地终端").clicked() {
                                want_local = true;
                                ui.close_menu();
                            }
                        },
                    );
                    add_menu.response.on_hover_text("添加 SSH 或本地会话");
                    if ui.button("⚙").on_hover_text("设置").clicked() {
                        want_settings = true;
                    }
                    if active_is_ssh
                        && !self.tabs[self.active_tab].session.lifecycle().is_running()
                        && ui
                            .button("↻ 重连")
                            .on_hover_text("重新连接当前 SSH 会话")
                            .clicked()
                    {
                        want_reconnect = true;
                    }
                    if active_is_ssh
                        && ui
                            .add_enabled(
                                sftp_connected,
                                egui::Button::new(if sftp_open { "SFTP ×" } else { "SFTP" }),
                            )
                            .on_hover_text("SFTP 文件传输（需已连接）")
                            .clicked()
                    {
                        toggle_sftp = true;
                    }
                    if active_is_ssh
                        && ui
                            .add_enabled(
                                sftp_connected,
                                egui::Button::new(if forward_open { "转发 ×" } else { "转发" }),
                            )
                            .on_hover_text("SSH 端口转发（需已连接）")
                            .clicked()
                    {
                        toggle_forward = true;
                    }
                    if ui.button("⚡").on_hover_text("快捷命令").clicked() {
                        toggle_quick = true;
                    }
                });
            });
        });

        if let Some(index) = clicked_tab {
            self.activate_tab(index);
        }
        if let Some(index) = close_tab {
            self.close_tab(index);
        }
        if let Some((index, action)) = tab_menu {
            self.handle_tab_menu(ctx, index, action);
        }
        if want_connect {
            self.connect_form.shell_colors = self.settings.ssh_shell_colors;
            self.show_connect_dialog = true;
            self.show_settings = false;
        }
        if want_settings {
            self.show_settings = true;
            self.show_connect_dialog = false;
        }
        if want_reconnect {
            let index = self.active_tab;
            let (cols, rows) = self.tabs[index].last_terminal_size;
            self.tabs[index].reconnect_with(ctx, cols, rows);
            self.tabs[index].reconnect_attempts = 0;
            self.tabs[index].reconnect_at = None;
            self.tabs[index].reconnect_dismissed = false;
            self.tabs[index].reconnect_status = Some("正在重连…".to_string());
        }
        if toggle_sftp {
            let index = self.active_tab;
            if self.tabs[index].sftp.is_some() {
                self.tabs[index].sftp = None;
            } else if let Some(handle) = self.tabs[index].session.ssh_connection_handle() {
                let panel = sftp::spawn_sftp_worker(handle);
                panel.send(sftp::SftpCommand::List(".".to_string()));
                self.tabs[index].sftp = Some(panel);
            }
        }
        if toggle_forward {
            let index = self.active_tab;
            if self.tabs[index].forward.is_some() {
                self.tabs[index].forward = None;
            } else if let Some(handle) = self.tabs[index].session.ssh_connection_handle() {
                let mut panel = forward::spawn_forward_worker(handle);
                panel.show = true;
                panel.tx.send(forward::ForwardCommand::List).ok();
                self.tabs[index].forward = Some(panel);
            }
        }
        if toggle_quick {
            self.quick_panel.show = !self.quick_panel.show;
            surrender_focus(ctx);
        }

        // 2.5 搜索栏（仅活动会话开启搜索时显示）。
        if self.tabs[self.active_tab].search_open {
            let mut query_changed = false;
            let mut next = false;
            let mut prev = false;
            let mut close = false;
            egui::TopBottomPanel::top("search_bar").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("🔍");
                    let tab = &mut self.tabs[self.active_tab];
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut tab.search_query)
                            .hint_text("搜索终端内容 · Enter 下一个 · Shift+Enter 上一个 · Esc 关闭")
                            .desired_width(320.0),
                    );
                    if tab.search_focus_requested {
                        response.request_focus();
                        tab.search_focus_requested = false;
                    }
                    let count = tab.search_matches.len();
                    let current = tab.search_current.map_or(0, |index| index + 1);
                    ui.label(format!("{current}/{count}"));
                    if ui.button("↑").on_hover_text("上一个").clicked() {
                        prev = true;
                    }
                    if ui.button("↓").on_hover_text("下一个").clicked() {
                        next = true;
                    }
                    if ui.button("×").on_hover_text("关闭搜索").clicked() {
                        close = true;
                    }
                    query_changed = response.changed();
                    // 搜索框内按 Enter：提交并导航（egui 单行输入框会在 Enter 时失焦，
                    // 标记为已消费，避免该 Enter 被转发给 shell 执行命令行）。
                    if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter))
                    {
                        if ui.input(|input| input.modifiers.shift) {
                            prev = true;
                        } else {
                            next = true;
                        }
                        tab.search_enter_consumed = true;
                    }
                });
            });
            if query_changed {
                self.tabs[self.active_tab].refresh_search();
            }
            if next {
                self.tabs[self.active_tab].search_next();
            }
            if prev {
                self.tabs[self.active_tab].search_prev();
            }
            if close {
                self.tabs[self.active_tab].search_open = false;
            }
        }

        // 3. SSH 连接弹窗。
        if self.show_connect_dialog {
            let mut outcome = None;
            let default_pos = ctx.screen_rect().center() - egui::vec2(170.0, 130.0);
            egui::Window::new("新建连接")
                .collapsible(false)
                .resizable(false)
                .default_pos(default_pos)
                .show(ctx, |ui| {
                    outcome = self.connect_form.ui(ui);
                });
            match outcome {
                Some(DialogOutcome::Connect(request)) => {
                    let (cols, rows) = self.tabs[self.active_tab].last_terminal_size;
                    if self.add_connect_tab(ctx, request, cols, rows) {
                        self.show_connect_dialog = false;
                        surrender_focus(ctx);
                    }
                }
                Some(DialogOutcome::Cancel) => {
                    self.show_connect_dialog = false;
                    self.connect_form = ConnectForm::default();
                    surrender_focus(ctx);
                }
                None => {}
            }
        }
        if want_local {
            let (cols, rows) = self.tabs[self.active_tab].last_terminal_size;
            self.add_local_tab(ctx, cols, rows);
        }

        // 3.5 设置窗口。
        if self.show_settings {
            self.settings_window(ctx);
        }

        // 3.7 快捷命令窗口。
        if self.quick_panel.show {
            let default_pos = ctx.screen_rect().center() - egui::vec2(170.0, 190.0);
            egui::Window::new("快捷命令")
                .collapsible(false)
                .resizable(false)
                .default_pos(default_pos)
                .show(ctx, |ui| {
                    let panel = &mut self.quick_panel;
                    let session = &mut self.tabs[self.active_tab].session;
                    panel.ui(ui, session);
                });
        }

        // 3.8 端口转发窗口。
        let forward_showing = self.tabs[self.active_tab]
            .forward
            .as_ref()
            .is_some_and(|panel| panel.show);
        if forward_showing {
            self.poll_forward();
            let default_pos = ctx.screen_rect().center() - egui::vec2(200.0, 140.0);
            egui::Window::new("端口转发")
                .collapsible(false)
                .resizable(false)
                .default_pos(default_pos)
                .show(ctx, |ui| {
                    let index = self.active_tab;
                    let panel = self.tabs[index]
                        .forward
                        .as_mut()
                        .expect("forward panel");
                    let commands = forward::forward_window_ui(ui, panel);
                    for command in commands {
                        let _ = panel.tx.send(command);
                    }
                });
        }

        // 3.6 断开提示窗口。
        self.reconnect_banner(ctx);

        // 4. 所有会话读取输出；仅活动会话渲染。
        for tab in &mut self.tabs {
            tab.session.read_pending();
        }

        // 4.5 SSH 连接成功后写入钥匙串。
        self.handle_keychain_saves();

        // 4.6 SSH 断线自动重连。
        self.handle_reconnects(ctx);

        // 4.7 长命令完成通知。
        self.poll_notifications();

        // 5. 活动会话输入事件。
        let prefs = TerminalPrefs {
            copy_on_select: self.settings.copy_on_select,
            middle_click_paste: self.settings.middle_click_paste,
            sftp_sync_cwd: self.settings.sftp_sync_cwd,
            modal_open: self.show_settings
                || self.show_connect_dialog
                || self.rename_tab_index.is_some(),
        };
        self.active_tab().process_input(ctx, cell_size, prefs);

        // 5.5 终端事件（Trzsz / 目录变化）处理。
        self.handle_terminal_events(ctx);

        // 6. 底部状态栏。
        let transparent = self.settings.transparent_window;
        let status_frame = if transparent {
            egui::Frame::default().fill(egui::Color32::TRANSPARENT)
        } else {
            egui::Frame::default()
        };
        egui::TopBottomPanel::bottom("status_bar")
            .frame(status_frame)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(self.status_line())
                            .monospace()
                            .size(11.0)
                            .color(if self.settings.theme == ThemeChoice::Light {
                                egui::Color32::from_rgb(0x4a, 0x50, 0x58)
                            } else {
                                egui::Color32::from_rgb(0x8a, 0x8f, 0x98)
                            }),
                    );
                });
            });

        // 6.5 SFTP：轮询事件 + 右侧面板。
        self.poll_sftp();
        if self.tabs[self.active_tab].sftp.is_some() {
            egui::SidePanel::right("sftp_panel")
                .resizable(true)
                .default_width(360.0)
                .show(ctx, |ui| {
                    ui.add_space(4.0);
                    ui.heading("SFTP");
                    ui.separator();
                    let index = self.active_tab;
                    let panel = self.tabs[index].sftp.as_mut().expect("sftp panel");
                    let mut transfer_started = false;
                    let commands = sftp::sftp_panel_ui(ui, panel);
                    for command in commands {
                        if matches!(
                            command,
                            sftp::SftpCommand::Download { .. }
                                | sftp::SftpCommand::DownloadDir { .. }
                                | sftp::SftpCommand::Upload { .. }
                                | sftp::SftpCommand::UploadDir { .. }
                        ) {
                            transfer_started = true;
                        }
                        panel.send(command);
                    }
                    if transfer_started {
                        panel.busy = true;
                    }
                });
        }

        // 7. 中央终端区：尺寸同步所有会话，渲染活动会话。
        let mut central_frame = egui::Frame::default().inner_margin(0.0);
        if transparent {
            central_frame = central_frame.fill(egui::Color32::TRANSPARENT);
        }
        egui::CentralPanel::default().frame(central_frame).show(ctx, |ui| {
                let avail = ui.available_size();
                let cols = (avail.x / cell_size.x).floor().max(2.0) as usize;
                let rows = (avail.y / cell_size.y).floor().max(2.0) as usize;
                for tab in &mut self.tabs {
                    tab.resize(cols, rows, cell_size);
                }

                let now = ctx.input(|i| i.time);
                if self.window_focused && self.tabs[self.active_tab].focused {
                    ctx.request_repaint_after(Duration::from_millis(400));
                }
                let cursor_blink_on = if self.window_focused && self.tabs[self.active_tab].focused {
                    (now * 1.8).fract() < 0.5
                } else {
                    true
                };

                let theme = build_theme(self.settings.theme, self.settings.background_alpha);
                self.active_tab()
                    .draw(ui, &font_id, cell_size, cursor_blink_on, &theme);
            });

        // 8. 窗口标题跟随活动会话。
        let title = format!(
            "hapcli — {}",
            self.tabs[self.active_tab].display_label()
        );
        if title != self.last_window_title {
            self.last_window_title = title.clone();
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        }

        // 9. 更新检查：轮询后台结果、按计划检查、有新版本时弹窗。
        let proxy_list = parse_proxy_prefixes(&self.settings.github_proxies);
        self.update_state.set_proxies(&proxy_list);
        self.update_state.poll();
        self.update_state
            .maybe_periodic_check(env!("CARGO_PKG_VERSION"), self.settings.check_updates);
        self.update_window(ctx);
        self.rename_tab_window(ctx);
    }
}

impl HapcliApp {
    fn process_global_input(&mut self, ctx: &egui::Context) {
        let mut zoom: f32 = 1.0;
        let mut focused: Option<bool> = None;
        ctx.input(|i| {
            for event in &i.events {
                match event {
                    egui::Event::Zoom(z) => zoom *= z,
                    egui::Event::WindowFocused(f) => focused = Some(*f),
                    _ => {}
                }
            }
        });

        if zoom != 1.0 {
            self.settings.font_size =
                (self.settings.font_size * zoom).clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        }
        if let Some(focused) = focused {
            self.window_focused = focused;
            let _ = self.active_tab().session.set_focused(focused);
        }
    }

    /// 发现新版本时弹出升级提示窗口。
    fn update_window(&mut self, ctx: &egui::Context) {
        // 新版本已就位：替换脚本正在等待本进程退出，关闭窗口即退出。
        if matches!(self.update_state.status, UpdateStatus::ReadyToInstall) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.update_state.dismissed {
            return;
        }

        let current = env!("CARGO_PKG_VERSION");
        match self.update_state.status.clone() {
            UpdateStatus::UpdateAvailable {
                version,
                name,
                notes,
                assets,
                ..
            } => {
                if self.settings.ignored_update_version.as_deref() == Some(version.as_str()) {
                    return;
                }
                let mut start = false;
                let mut toggle_notes = false;
                let mut ignore = false;
                let mut later = false;
                egui::Window::new("软件更新")
                    .collapsible(false)
                    .resizable(false)
                    .default_pos(ctx.screen_rect().center() - egui::vec2(190.0, 90.0))
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("🎉");
                            ui.label(egui::RichText::new(format!("发现新版本 {version}")).strong());
                        });
                        ui.add_space(4.0);
                        ui.label(format!("当前版本 {current} · {name}"));
                        ui.add_space(8.0);
                        if self.update_state.show_notes {
                            ui.label(egui::RichText::new("更新内容").strong());
                            let notes = if notes.trim().is_empty() {
                                "（本版本没有提供更新说明）".to_string()
                            } else {
                                notes.clone()
                            };
                            egui::ScrollArea::vertical()
                                .max_height(180.0)
                                .auto_shrink([false, true])
                                .show(ui, |ui| {
                                    ui.label(notes);
                                });
                            ui.add_space(6.0);
                        }
                        ui.horizontal(|ui| {
                            if ui
                                .button(if self.update_state.show_notes {
                                    "收起更新内容"
                                } else {
                                    "更新了什么"
                                })
                                .clicked()
                            {
                                toggle_notes = true;
                            }
                            if ui.button("升级并安装").clicked() {
                                start = true;
                            }
                            if ui.small_button("忽略此版本").clicked() {
                                ignore = true;
                            }
                            if ui.small_button("稍后再说").clicked() {
                                later = true;
                            }
                        });
                    });

                if toggle_notes {
                    self.update_state.show_notes = !self.update_state.show_notes;
                }
                if start {
                    self.update_state.start_download(&version, &assets);
                }
                if ignore {
                    self.settings.ignored_update_version = Some(version.clone());
                    self.update_state.dismissed = true;
                    let _ = save_settings(&self.settings);
                }
                if later {
                    self.update_state.dismissed = true;
                }
            }
            UpdateStatus::Downloading {
                version,
                transferred,
                total,
                speed_bps,
            } => {
                let mut cancel = false;
                egui::Window::new("软件更新")
                    .collapsible(false)
                    .resizable(false)
                    .default_pos(ctx.screen_rect().center() - egui::vec2(190.0, 70.0))
                    .show(ctx, |ui| {
                        ui.label(
                            egui::RichText::new(format!("正在下载 v{version} …")).strong(),
                        );
                        ui.add_space(6.0);
                        let fraction = if total > 0 {
                            (transferred as f64 / total as f64).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        let mut progress = egui::ProgressBar::new(fraction as f32)
                            .desired_width(320.0)
                            .show_percentage();
                        if total == 0 {
                            progress = progress.animate(true);
                        }
                        ui.add(progress);
                        ui.add_space(4.0);
                        ui.weak(if total > 0 {
                            format!(
                                "{} / {} · {}",
                                format_bytes(transferred),
                                format_bytes(total),
                                format_speed(speed_bps)
                            )
                        } else {
                            format!("{} · {}", format_bytes(transferred), format_speed(speed_bps))
                        });
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if ui.button("取消").clicked() {
                                cancel = true;
                            }
                        });
                    });
                if cancel {
                    self.update_state.cancel_download_now();
                    self.update_state.dismissed = true;
                }
            }
            UpdateStatus::DownloadFailed { version, message } => {
                let mut retry = false;
                let mut open_page = false;
                let mut later = false;
                egui::Window::new("软件更新")
                    .collapsible(false)
                    .resizable(false)
                    .default_pos(ctx.screen_rect().center() - egui::vec2(190.0, 80.0))
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("⚠️");
                            ui.label(
                                egui::RichText::new(format!("更新 v{version} 失败")).strong(),
                            );
                        });
                        ui.add_space(4.0);
                        ui.label(message);
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("重试").clicked() {
                                retry = true;
                            }
                            if ui.button("打开下载页").clicked() {
                                open_page = true;
                            }
                            if ui.small_button("稍后再说").clicked() {
                                later = true;
                            }
                        });
                    });
                if retry {
                    self.update_state.retry_download();
                }
                if open_page {
                    let _ = open_url(&format!(
                        "https://github.com/dardiao/hapcli/releases/tag/v{version}"
                    ));
                    self.update_state.dismissed = true;
                }
                if later {
                    self.update_state.dismissed = true;
                }
            }
            _ => {}
        }
    }
}

impl Drop for HapcliApp {
    fn drop(&mut self) {
        for tab in &mut self.tabs {
            tab.session.shutdown();
        }
    }
}

/// 标签右键菜单动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TabMenuAction {
    Rename,
    Duplicate,
    Close,
    CloseOthers,
}

/// 自绘标签页：圆角胶囊 + 当前标签绿色指示灯 + 内嵌关闭按钮（×）+ 右键菜单。
/// 返回 (是否点击标签, 是否点击关闭, 右键菜单动作)。
fn draw_tab(
    ui: &mut egui::Ui,
    index: usize,
    label: &str,
    selected: bool,
    can_close: bool,
    light: bool,
) -> (bool, bool, Option<TabMenuAction>) {
    const TAB_HEIGHT: f32 = 26.0;
    const PADDING_X: f32 = 12.0;
    const CLOSE_SIZE: f32 = 16.0;
    const GAP: f32 = 6.0;

    let font_id = egui::FontId::proportional(13.5);
    let text_color = if light {
        if selected {
            egui::Color32::from_rgb(0x1c, 0x1e, 0x21)
        } else {
            egui::Color32::from_rgb(0x55, 0x5c, 0x66)
        }
    } else {
        if selected {
            egui::Color32::from_rgb(0xec, 0xf0, 0xf4)
        } else {
            egui::Color32::from_rgb(0x9a, 0xa2, 0xab)
        }
    };
    let text_width = ui
        .fonts(|fonts| fonts.layout_no_wrap(label.to_owned(), font_id.clone(), egui::Color32::WHITE))
        .size()
        .x;
    let dot_space = if selected { 18.0 } else { 0.0 };
    let width = PADDING_X + dot_space + text_width + GAP + CLOSE_SIZE + PADDING_X;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, TAB_HEIGHT), egui::Sense::click());

    let painter = ui.painter();
    let background = if light {
        if selected {
            egui::Color32::from_rgb(0xd6, 0xe2, 0xee)
        } else {
            egui::Color32::from_rgb(0xf2, 0xf4, 0xf7)
        }
    } else {
        if selected {
            egui::Color32::from_rgb(0x2a, 0x3b, 0x4f)
        } else {
            egui::Color32::from_rgb(0x16, 0x1b, 0x22)
        }
    };
    painter.rect_filled(rect, 7.0, background);
    if selected {
        painter.rect_stroke(
            rect,
            7.0,
            egui::Stroke::new(
                1.0_f32,
                if light {
                    egui::Color32::from_rgb(0x9f, 0xb4, 0xc8)
                } else {
                    egui::Color32::from_rgb(0x3f, 0x5f, 0x7f)
                },
            ),
        );
    }

    let mut text_x = rect.left() + PADDING_X;
    if selected {
        painter.circle_filled(
            egui::pos2(text_x + 5.0, rect.center().y),
            4.0,
            egui::Color32::from_rgb(0x3f, 0xca, 0x6b),
        );
        text_x += 14.0;
    }
    painter.text(
        egui::pos2(text_x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        font_id.clone(),
        text_color,
    );

    let close_center =
        egui::pos2(rect.right() - PADDING_X - CLOSE_SIZE / 2.0, rect.center().y);
    let close_rect =
        egui::Rect::from_center_size(close_center, egui::vec2(CLOSE_SIZE, CLOSE_SIZE));
    let close_response = ui.interact(
        close_rect,
        ui.make_persistent_id(("tab_close", index)),
        egui::Sense::click(),
    );
    if close_response.hovered() {
        painter.rect_filled(
            close_rect,
            4.0,
            if light {
                egui::Color32::from_rgba_unmultiplied(0, 0, 0, 18)
            } else {
                egui::Color32::from_rgba_unmultiplied(255, 255, 255, 30)
            },
        );
    }
    painter.text(
        close_center,
        egui::Align2::CENTER_CENTER,
        "×",
        font_id,
        if light {
            egui::Color32::from_rgb(0x3a, 0x40, 0x48)
        } else {
            if close_response.hovered() {
                egui::Color32::WHITE
            } else {
                egui::Color32::from_rgb(0x8a, 0x92, 0x9a)
            }
        },
    );

    // 右键菜单：重命名 / 复制标签 / 关闭 / 关闭其他。
    let mut menu_action: Option<TabMenuAction> = None;
    response.context_menu(|ui| {
        ui.set_min_width(150.0);
        if ui.button("重命名…").clicked() {
            menu_action = Some(TabMenuAction::Rename);
            ui.close_menu();
        }
        if ui.button("复制标签").clicked() {
            menu_action = Some(TabMenuAction::Duplicate);
            ui.close_menu();
        }
        ui.separator();
        if ui
            .add_enabled(can_close, egui::Button::new("关闭标签"))
            .clicked()
        {
            menu_action = Some(TabMenuAction::Close);
            ui.close_menu();
        }
        if ui
            .add_enabled(can_close, egui::Button::new("关闭其他标签"))
            .clicked()
        {
            menu_action = Some(TabMenuAction::CloseOthers);
            ui.close_menu();
        }
    });

    (
        response.clicked_by(PointerButton::Primary),
        close_response.clicked_by(PointerButton::Primary),
        menu_action,
    )
}

/// 发送 macOS 系统通知；其他平台暂为 no-op。
fn system_notify(title: &str, message: &str) {
    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "display notification {} with title {}",
            apple_quote(message),
            apple_quote(title)
        );
        let _ = std::process::Command::new("osascript")
            .arg("-e")
            .arg(script)
            .spawn();
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (title, message);
    }
}

/// 字节数的人类可读格式。
fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let bytes = bytes as f64;
    if bytes >= GB {
        format!("{:.2} GB", bytes / GB)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes / MB)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes / KB)
    } else {
        format!("{bytes:.0} B")
    }
}

/// 下载速度的人类可读格式。
fn format_speed(bytes_per_second: f64) -> String {
    format!("{}/s", format_bytes(bytes_per_second.max(0.0) as u64))
}

#[cfg(target_os = "macos")]
fn apple_quote(text: &str) -> String {
    format!("'{}'", text.replace('\\', "\\\\").replace('\'', "'\\''"))
}

/// 安装 CJK 兜底字体与（可选的）自定义终端字体。
fn install_fonts(ctx: &egui::Context, settings: &AppSettings) -> bool {
    const CANDIDATES: &[&str] = &[
        // macOS
        "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        "/System/Library/Fonts/Supplemental/Songti.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
        "/System/Library/Fonts/STHeiti Medium.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/System/Library/Fonts/PingFang.ttc",
        // Windows
        "C:\\Windows\\Fonts\\simhei.ttf",
        "C:\\Windows\\Fonts\\Deng.ttf",
        "C:\\Windows\\Fonts\\simkai.ttf",
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\simsun.ttc",
        "C:\\Windows\\Fonts\\msyhl.ttc",
        // Linux
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        "/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc",
    ];

    let mut fonts = egui::FontDefinitions::default();
    let mut custom_loaded = false;

    if let Some(path) = &settings.terminal_font_path {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert(
                "hapcli-terminal-font".to_owned(),
                egui::FontData::from_owned(bytes),
            );
            custom_loaded = true;
        }
    }

    let mut cjk_name = None;
    for path in CANDIDATES {
        if let Ok(bytes) = std::fs::read(path) {
            // 先验证字体可解析（ab_glyph），避免个别 .ttc 集合解析失败
            // 触发 epaint 的 panic，或悄悄让中文变成方块。
            if ab_glyph::FontArc::try_from_vec(bytes.clone()).is_ok() {
                fonts.font_data.insert(
                    "hapcli-cjk".to_owned(),
                    egui::FontData::from_owned(bytes),
                );
                cjk_name = Some("hapcli-cjk".to_owned());
                break;
            }
        }
    }

    // 平台默认等宽字体：比内置 Hack 有更好的符号与本地化字形覆盖
    // （macOS Monaco / Windows Consolas / Linux DejaVu Sans Mono）。
    const SYSTEM_MONO_CANDIDATES: &[&str] = &[
        "/System/Library/Fonts/Monaco.ttf",
        "C:\\Windows\\Fonts\\consola.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    ];
    for path in SYSTEM_MONO_CANDIDATES {
        if let Ok(bytes) = std::fs::read(path)
            && ab_glyph::FontArc::try_from_vec(bytes.clone()).is_ok()
        {
            fonts.font_data.insert(
                "hapcli-system-mono".to_owned(),
                egui::FontData::from_owned(bytes),
            );
            if let Some(mono) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
                mono.insert(0, "hapcli-system-mono".to_owned());
            }
            break;
        }
    }

    // 符号字体兜底（盲文等 npm 旋转动画字符）：放在中文字体之后，
    // 保证 ⠋⠙⠹ 等 spinner 字符正常显示而不是方块。
    const SYMBOL_CANDIDATES: &[&str] = &[
        "/System/Library/Fonts/Apple Symbols.ttf",
        "C:\\Windows\\Fonts\\seguisym.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ];
    for path in SYMBOL_CANDIDATES {
        if let Ok(bytes) = std::fs::read(path)
            && ab_glyph::FontArc::try_from_vec(bytes.clone()).is_ok()
        {
            fonts.font_data.insert(
                "hapcli-symbol".to_owned(),
                egui::FontData::from_owned(bytes),
            );
            for family in [egui::FontFamily::Monospace, egui::FontFamily::Proportional] {
                fonts
                    .families
                    .entry(family)
                    .or_default()
                    .push("hapcli-symbol".to_owned());
            }
            break;
        }
    }

    if custom_loaded {
        let mut family = vec!["hapcli-terminal-font".to_owned(), "Hack".to_owned()];
        if let Some(cjk) = &cjk_name {
            family.push(cjk.clone());
        }
        fonts.families.insert(
            egui::FontFamily::Name("hapcli-terminal-font".into()),
            family,
        );
        if let Some(mono) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
            mono.insert(0, "hapcli-terminal-font".to_owned());
        }
    }

    if let Some(cjk) = cjk_name {
        for family in [egui::FontFamily::Monospace, egui::FontFamily::Proportional] {
            fonts.families.entry(family).or_default().push(cjk.clone());
        }
    }

    ctx.set_fonts(fonts);
    custom_loaded
}

/// 平台默认等宽字体的显示名（与 install_fonts 的候选顺序一致）。
fn platform_default_font_label() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Monaco"
    }
    #[cfg(target_os = "windows")]
    {
        "Consolas"
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        "DejaVu Sans Mono"
    }
}
