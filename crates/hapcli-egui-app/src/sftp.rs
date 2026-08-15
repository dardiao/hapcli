//! SFTP 文件传输面板：后台线程持有 tokio 运行时与 SFTP 会话，
//! UI 通过命令/事件通道交互，避免阻塞界面。

use std::{
    future::Future,
    path::Path,
    pin::Pin,
    sync::{Arc, mpsc::Receiver, mpsc::Sender, mpsc::channel},
};

use eframe::egui;
use hapcli_ssh::SshConnectionHandle;
use hapcli_sftp::{
    FileInfo, FileType, SftpError, SftpSession, TransferProgress, TransferState,
    join_remote_path, remote_parent_path,
};
use tokio::runtime::Runtime;
use tokio::sync::Mutex;

pub enum SftpCommand {
    List(String),
    Download { remote: String, local: String },
    DownloadDir { remote: String, local: String },
    Upload { local: String, remote: String },
    UploadDir { local: String, remote: String },
    Mkdir(String),
    Delete { path: String, recursive: bool },
}

pub enum SftpEvent {
    Listing {
        cwd: String,
        entries: Vec<FileInfo>,
        error: Option<String>,
    },
    Progress {
        id: String,
        transferred: u64,
        total: u64,
        done: bool,
        message: Option<String>,
    },
    Finished {
        ok: bool,
        message: String,
    },
}

pub struct SftpPanelState {
    pub tx: Sender<SftpCommand>,
    pub rx: Receiver<SftpEvent>,
    pub cwd: String,
    pub entries: Vec<FileInfo>,
    pub selected: Option<usize>,
    pub error: Option<String>,
    pub transfer_id: Option<String>,
    pub transfer_progress: Option<(u64, u64)>,
    pub busy: bool,
    pub new_dir_name: String,
    /// 是否有本地文件正拖拽悬停在面板上（用于显示“松开上传”提示）。
    drop_hovering: bool,
    confirm_delete: Option<FileInfo>,
}

impl SftpPanelState {
    pub fn send(&self, command: SftpCommand) {
        let _ = self.tx.send(command);
    }

    pub fn refresh(&self) {
        self.send(SftpCommand::List(self.cwd.clone()));
    }

    pub fn apply_event(&mut self, event: SftpEvent) -> bool {
        // 返回 true 表示列表内容可能变化，需要刷新。
        match event {
            SftpEvent::Listing {
                cwd,
                entries,
                error,
            } => {
                self.cwd = cwd;
                self.entries = entries;
                self.selected = None;
                self.error = error;
                self.busy = false;
                false
            }
            SftpEvent::Progress {
                id,
                transferred,
                total,
                done,
                message,
            } => {
                if self.transfer_id.as_deref() != Some(id.as_str()) {
                    self.transfer_id = Some(id);
                }
                if done {
                    self.transfer_id = None;
                    self.transfer_progress = None;
                    self.busy = false;
                    if let Some(message) = message {
                        self.error = Some(message);
                    }
                    true
                } else {
                    self.transfer_progress = Some((transferred, total));
                    false
                }
            }
            SftpEvent::Finished { ok, message } => {
                self.busy = false;
                self.error = Some(message);
                ok
            }
        }
    }
}

/// 启动 SFTP 工作线程（内部创建 tokio 运行时并获取会话）。
pub fn spawn_sftp_worker(handle: SshConnectionHandle) -> SftpPanelState {
    let (cmd_tx, cmd_rx) = channel::<SftpCommand>();
    let (evt_tx, evt_rx) = channel::<SftpEvent>();

    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                let _ = evt_tx.send(SftpEvent::Listing {
                    cwd: String::new(),
                    entries: Vec::new(),
                    error: Some(format!("SFTP 运行时创建失败: {error}")),
                });
                return;
            }
        };
        let mut session = match runtime.block_on(handle.acquire_sftp()) {
            Ok(session) => session,
            Err(error) if error.is_channel_recoverable() => {
                // 旧会话已失效（如远端关闭了 SFTP 通道）：重建一次再重试。
                match runtime.block_on(async {
                    handle.invalidate_sftp().await;
                    handle.acquire_sftp().await
                }) {
                    Ok(session) => session,
                    Err(error) => {
                        let _ = evt_tx.send(SftpEvent::Listing {
                            cwd: String::new(),
                            entries: Vec::new(),
                            error: Some(format!("SFTP 连接失败: {error}")),
                        });
                        return;
                    }
                }
            }
            Err(error) => {
                let _ = evt_tx.send(SftpEvent::Listing {
                    cwd: String::new(),
                    entries: Vec::new(),
                    error: Some(format!("SFTP 连接失败: {error}")),
                });
                return;
            }
        };

        while let Ok(command) = cmd_rx.recv() {
            match command {
                SftpCommand::List(path) => {
                    let result = runtime.block_on(async {
                        let sftp = session.lock().await;
                        sftp.list_dir_with_cwd(&path, None).await
                    });
                    let event = match result {
                        Ok((cwd, entries)) => SftpEvent::Listing {
                            cwd,
                            entries,
                            error: None,
                        },
                        Err(error) if error.is_channel_recoverable() => {
                            // 会话失效：重建并重试一次，避免一次性的 “session closed” 卡死面板。
                            let retried = runtime.block_on(async {
                                handle.invalidate_sftp().await;
                                match handle.acquire_sftp().await {
                                    Ok(new_session) => {
                                        session = new_session;
                                        let sftp = session.lock().await;
                                        sftp.list_dir_with_cwd(&path, None).await
                                    }
                                    Err(error) => Err(error),
                                }
                            });
                            match retried {
                                Ok((cwd, entries)) => SftpEvent::Listing {
                                    cwd,
                                    entries,
                                    error: None,
                                },
                                Err(error) => SftpEvent::Listing {
                                    cwd: String::new(),
                                    entries: Vec::new(),
                                    error: Some(error.to_string()),
                                },
                            }
                        }
                        Err(error) => SftpEvent::Listing {
                            cwd: String::new(),
                            entries: Vec::new(),
                            error: Some(error.to_string()),
                        },
                    };
                    let _ = evt_tx.send(event);
                }
                SftpCommand::Mkdir(path) => {
                    let result = runtime.block_on(async {
                        let sftp = session.lock().await;
                        sftp.mkdir(&path).await
                    });
                    let _ = evt_tx.send(finished_event(result, "新建目录"));
                }
                SftpCommand::Delete { path, recursive } => {
                    let result = runtime.block_on(async {
                        let sftp = session.lock().await;
                        if recursive {
                            sftp.delete_recursive(&path).await.map(|_| ())
                        } else {
                            sftp.delete(&path).await
                        }
                    });
                    let _ = evt_tx.send(finished_event(result, "删除"));
                }
                SftpCommand::Download { remote, local } => {
                    let id = format!("dl-{:x}", fast_id());
                    run_transfer(
                        &runtime,
                        &session,
                        &evt_tx,
                        id.clone(),
                        move |sftp, progress_tx| {
                            Box::pin(async move {
                                sftp
                                    .download_file(
                                        &remote,
                                        &local,
                                        &id,
                                        Some(progress_tx),
                                        None,
                                    )
                                    .await
                            })
                        },
                    );
                }
                SftpCommand::DownloadDir { remote, local } => {
                    let id = format!("dldir-{:x}", fast_id());
                    run_transfer(
                        &runtime,
                        &session,
                        &evt_tx,
                        id.clone(),
                        move |sftp, progress_tx| {
                            Box::pin(async move {
                                sftp
                                    .download_dir(
                                        &remote,
                                        &local,
                                        &id,
                                        Some(progress_tx),
                                        None,
                                    )
                                    .await
                            })
                        },
                    );
                }
                SftpCommand::Upload { local, remote } => {
                    let id = format!("ul-{:x}", fast_id());
                    run_transfer(
                        &runtime,
                        &session,
                        &evt_tx,
                        id.clone(),
                        move |sftp, progress_tx| {
                            Box::pin(async move {
                                sftp
                                    .upload_file(
                                        &local,
                                        &remote,
                                        &id,
                                        Some(progress_tx),
                                        None,
                                    )
                                    .await
                            })
                        },
                    );
                }
                SftpCommand::UploadDir { local, remote } => {
                    let id = format!("uldir-{:x}", fast_id());
                    run_transfer(
                        &runtime,
                        &session,
                        &evt_tx,
                        id.clone(),
                        move |sftp, progress_tx| {
                            Box::pin(async move {
                                sftp
                                    .upload_dir(
                                        &local,
                                        &remote,
                                        &id,
                                        Some(progress_tx),
                                        None,
                                    )
                                    .await
                            })
                        },
                    );
                }
            }
        }
    });

    SftpPanelState {
        tx: cmd_tx,
        rx: evt_rx,
        cwd: String::new(),
        entries: Vec::new(),
        selected: None,
        error: None,
        transfer_id: None,
        transfer_progress: None,
        busy: false,
        new_dir_name: String::new(),
        drop_hovering: false,
        confirm_delete: None,
    }
}

type TransferFuture<'s> =
    Pin<Box<dyn Future<Output = Result<u64, SftpError>> + Send + 's>>;

fn run_transfer(
    runtime: &Runtime,
    session: &Arc<Mutex<SftpSession>>,
    event_tx: &Sender<SftpEvent>,
    id: String,
    run: impl for<'s> FnOnce(
        &'s SftpSession,
        tokio::sync::mpsc::Sender<TransferProgress>,
    ) -> TransferFuture<'s>,
) {
    let result = runtime.block_on(async {
        let (progress_tx, mut progress_rx) =
            tokio::sync::mpsc::channel::<TransferProgress>(16);
        let forward_event_tx = event_tx.clone();
        let forward_id = id.clone();
        let forwarder = tokio::spawn(async move {
            while let Some(progress) = progress_rx.recv().await {
                let done = matches!(
                    progress.state,
                    TransferState::Completed
                        | TransferState::Failed
                        | TransferState::Cancelled
                );
                let _ = forward_event_tx.send(SftpEvent::Progress {
                    id: forward_id.clone(),
                    transferred: progress.transferred_bytes,
                    total: progress.total_bytes,
                    done,
                    message: progress.error.clone(),
                });
            }
        });
        let sftp = session.lock().await;
        let result = run(&sftp, progress_tx).await;
        forwarder.abort();
        result
    });

    let message = match &result {
        Ok(bytes) => format!("传输完成，共 {bytes} 字节"),
        Err(error) => format!("传输失败: {error}"),
    };
    let _ = event_tx.send(SftpEvent::Progress {
        id,
        transferred: 0,
        total: 0,
        done: true,
        message: Some(message),
    });
}

fn finished_event(
    result: Result<(), SftpError>,
    action: &str,
) -> SftpEvent {
    match result {
        Ok(()) => SftpEvent::Finished {
            ok: true,
            message: format!("{action}成功"),
        },
        Err(error) => SftpEvent::Finished {
            ok: false,
            message: format!("{action}失败: {error}"),
        },
    }
}

fn fast_id() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0)
        ^ std::process::id() as u64
}

/// 为拖拽落下的本地路径生成上传命令：目录 → UploadDir，文件 → Upload。
fn upload_command_for_drop(path: &Path, cwd: &str) -> Option<SftpCommand> {
    let metadata = std::fs::metadata(path).ok()?;
    let name = path.file_name()?.to_string_lossy().to_string();
    let local = path.to_string_lossy().to_string();
    let remote = join_remote_path(cwd, &name);
    if metadata.is_dir() {
        Some(SftpCommand::UploadDir { local, remote })
    } else {
        Some(SftpCommand::Upload { local, remote })
    }
}

/// 渲染 SFTP 面板，返回需要发送给 worker 的命令。
pub fn sftp_panel_ui(ui: &mut egui::Ui, panel: &mut SftpPanelState) -> Vec<SftpCommand> {
    let mut commands = Vec::new();

    // 拖拽上传：检测系统文件拖入面板，松手时对每个文件/目录生成上传命令。
    let panel_rect = ui.max_rect();
    let (over_panel, hovering, dropped_paths) = ui.input(|input| {
        let over_panel = input
            .pointer
            .hover_pos()
            .is_some_and(|pos| panel_rect.contains(pos));
        let hovering = over_panel && !input.raw.hovered_files.is_empty();
        let dropped_paths = input
            .raw
            .dropped_files
            .iter()
            .filter_map(|file| file.path.clone())
            .collect::<Vec<_>>();
        (over_panel, hovering, dropped_paths)
    });
    let was_hovering = panel.drop_hovering;
    panel.drop_hovering = hovering;
    if (was_hovering || over_panel) && !dropped_paths.is_empty() {
        for path in dropped_paths {
            if let Some(command) = upload_command_for_drop(&path, &panel.cwd) {
                commands.push(command);
            }
        }
    }

    if panel.drop_hovering {
        egui::Frame::none()
            .fill(egui::Color32::from_rgba_unmultiplied(0xbd, 0x93, 0xf9, 36))
            .stroke(egui::Stroke::new(
                1.5_f32,
                egui::Color32::from_rgb(0xbd, 0x93, 0xf9),
            ))
            .rounding(6.0)
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new("松开以上传到当前目录")
                        .strong()
                        .color(egui::Color32::from_rgb(0xd8, 0xcf, 0xf5)),
                );
            });
        ui.add_space(4.0);
    }

    ui.horizontal(|ui| {
        ui.label("路径");
        ui.add(
            egui::TextEdit::singleline(&mut panel.cwd)
                .desired_width(170.0),
        );
        if ui.button("进入").clicked() {
            commands.push(SftpCommand::List(panel.cwd.clone()));
        }
        if ui.button("上级").clicked() {
            commands.push(SftpCommand::List(remote_parent_path(&panel.cwd)));
        }
        if ui.button("刷新").clicked() {
            commands.push(SftpCommand::List(panel.cwd.clone()));
        }
    });

    if let Some(error) = &panel.error {
        ui.colored_label(egui::Color32::from_rgb(0xff, 0x77, 0x77), error);
    }

    ui.separator();
    egui::ScrollArea::vertical()
        .max_height(340.0)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (index, entry) in panel.entries.iter().enumerate() {
                let is_dir = entry.file_type == FileType::Directory;
                let icon = if is_dir { "📁" } else { "📄" };
                let size = if is_dir {
                    String::new()
                } else {
                    format_size(entry.size)
                };
                let selected = panel.selected == Some(index);
                let label = format!("{icon} {}  {size}", entry.name);
                let response = ui.selectable_label(selected, label);
                if response.clicked() {
                    panel.selected = Some(index);
                }
                if is_dir && response.double_clicked() {
                    commands.push(SftpCommand::List(entry.path.clone()));
                }
            }
        });
    ui.separator();

    let selected_is_dir = panel
        .selected
        .and_then(|index| panel.entries.get(index))
        .is_some_and(|entry| entry.file_type == FileType::Directory);
    let selected_is_file = panel
        .selected
        .and_then(|index| panel.entries.get(index))
        .is_some_and(|entry| entry.file_type != FileType::Directory);

    ui.horizontal(|ui| {
        if ui
            .add_enabled(!panel.busy, egui::Button::new("上传文件"))
            .clicked()
        {
            if let Some(paths) = rfd::FileDialog::new().pick_files() {
                for path in paths {
                    let local = path.display().to_string();
                    let name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let remote = join_remote_path(&panel.cwd, &name);
                    commands.push(SftpCommand::Upload { local, remote });
                }
            }
        }
        if ui
            .add_enabled(!panel.busy, egui::Button::new("上传目录"))
            .clicked()
        {
            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                let local = path.display().to_string();
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| "folder".to_string());
                let remote = join_remote_path(&panel.cwd, &name);
                commands.push(SftpCommand::UploadDir { local, remote });
            }
        }
        if ui
            .add_enabled(
                panel.selected.is_some() && !panel.busy && selected_is_file,
                egui::Button::new("下载"),
            )
            .clicked()
            && let Some(index) = panel.selected
        {
            let entry = &panel.entries[index];
            if let Some(path) = rfd::FileDialog::new()
                .set_file_name(&entry.name)
                .save_file()
            {
                commands.push(SftpCommand::Download {
                    remote: entry.path.clone(),
                    local: path.display().to_string(),
                });
            }
        }
        if ui
            .add_enabled(
                panel.selected.is_some() && !panel.busy && selected_is_dir,
                egui::Button::new("下载目录"),
            )
            .clicked()
            && let Some(index) = panel.selected
        {
            let entry = &panel.entries[index];
            if let Some(root) = rfd::FileDialog::new().pick_folder() {
                let local = root.join(&entry.name);
                commands.push(SftpCommand::DownloadDir {
                    remote: entry.path.clone(),
                    local: local.display().to_string(),
                });
            }
        }
    });

    ui.horizontal(|ui| {
        ui.label("新目录名");
        ui.add(
            egui::TextEdit::singleline(&mut panel.new_dir_name).desired_width(110.0),
        );
        if ui
            .add_enabled(!panel.busy, egui::Button::new("新建目录"))
            .clicked()
        {
            let name = panel.new_dir_name.trim().to_string();
            if !name.is_empty() {
                commands.push(SftpCommand::Mkdir(join_remote_path(&panel.cwd, &name)));
                panel.new_dir_name.clear();
            }
        }
        if ui
            .add_enabled(
                panel.selected.is_some() && !panel.busy,
                egui::Button::new("删除"),
            )
            .clicked()
            && let Some(index) = panel.selected
        {
            panel.confirm_delete = Some(panel.entries[index].clone());
        }
    });

    if let Some((transferred, total)) = panel.transfer_progress {
        let fraction = if total > 0 {
            transferred as f32 / total as f32
        } else {
            0.0
        };
        ui.add(
            egui::ProgressBar::new(fraction)
                .text(format!("{} / {}", format_size(transferred), format_size(total))),
        );
    }

    if let Some(entry) = &panel.confirm_delete {
        let mut confirm = false;
        let mut cancel = false;
        egui::Window::new("确认删除")
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.label(format!("确定删除「{}」？", entry.path));
                ui.horizontal(|ui| {
                    if ui.button("删除").clicked() {
                        confirm = true;
                    }
                    if ui.button("取消").clicked() {
                        cancel = true;
                    }
                });
            });
        if confirm {
            commands.push(SftpCommand::Delete {
                path: entry.path.clone(),
                recursive: entry.file_type == FileType::Directory,
            });
            panel.confirm_delete = None;
        }
        if cancel {
            panel.confirm_delete = None;
        }
    }

    commands
}

fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_size_uses_human_units() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn apply_event_finishes_transfer_and_requests_refresh() {
        let (_tx, rx) = channel::<SftpCommand>();
        let (_evt_tx, evt_rx) = channel::<SftpEvent>();
        let mut panel = SftpPanelState {
            tx: _tx,
            rx: evt_rx,
            cwd: "/".to_string(),
            entries: Vec::new(),
            selected: None,
            error: None,
            transfer_id: Some("dl-1".to_string()),
            transfer_progress: Some((100, 200)),
            busy: true,
            new_dir_name: String::new(),
            drop_hovering: false,
            confirm_delete: None,
        };
        let refresh = panel.apply_event(SftpEvent::Progress {
            id: "dl-1".to_string(),
            transferred: 200,
            total: 200,
            done: true,
            message: Some("传输完成".to_string()),
        });
        assert!(refresh);
        assert!(panel.transfer_id.is_none());
        assert!(panel.transfer_progress.is_none());
        assert!(!panel.busy);
    }

    #[test]
    fn drop_file_builds_upload_command_into_cwd() {
        let mut path = std::env::temp_dir();
        path.push(format!("hapcli-drop-file-{}.txt", std::process::id()));
        std::fs::write(&path, b"content").unwrap();

        let command = upload_command_for_drop(&path, "/home/user").unwrap();
        let SftpCommand::Upload { local, remote } = command else {
            panic!("expected Upload command");
        };
        let expected_name = format!("hapcli-drop-file-{}.txt", std::process::id());
        assert!(local.ends_with(&expected_name));
        assert_eq!(remote, format!("/home/user/{expected_name}"));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn drop_directory_builds_upload_dir_command() {
        let mut path = std::env::temp_dir();
        path.push(format!("hapcli-drop-dir-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();

        let command = upload_command_for_drop(&path, "/").unwrap();
        let SftpCommand::UploadDir { remote, .. } = command else {
            panic!("expected UploadDir command");
        };
        assert_eq!(remote, format!("/hapcli-drop-dir-{}", std::process::id()));
        std::fs::remove_dir_all(&path).ok();
    }

    #[test]
    fn drop_missing_path_returns_none() {
        let path = Path::new("/nonexistent/hapcli-sftp-drop-missing");
        assert!(upload_command_for_drop(path, "/").is_none());
    }
}
