//! SSH 端口转发面板：后台线程持有 tokio 运行时与 ForwardingManager，
//! UI 通过命令/事件通道交互。

use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui;
use hapcli_forwarding::{
    ForwardRule, ForwardStatus, ForwardType, ForwardingManager,
};
use hapcli_ssh::SshConnectionHandle;
use tokio::runtime::Runtime;

pub enum ForwardCommand {
    Create(ForwardRule),
    Stop(String),
    List,
}

pub enum ForwardEvent {
    Rules(Vec<ForwardRule>),
    Error(String),
}

pub struct ForwardPanel {
    pub show: bool,
    pub tx: Sender<ForwardCommand>,
    pub rx: Receiver<ForwardEvent>,
    pub rules: Vec<ForwardRule>,
    pub error: Option<String>,
    pub busy: bool,
    pub bind_port: u16,
    pub target_host: String,
    pub target_port: u16,
    pub description: String,
}

impl ForwardPanel {
    pub fn apply_event(&mut self, event: ForwardEvent) {
        match event {
            ForwardEvent::Rules(rules) => {
                self.rules = rules;
                self.busy = false;
            }
            ForwardEvent::Error(error) => {
                self.error = Some(error);
                self.busy = false;
            }
        }
    }
}

/// 启动端口转发工作线程。
pub fn spawn_forward_worker(handle: SshConnectionHandle) -> ForwardPanel {
    let (cmd_tx, cmd_rx) = channel::<ForwardCommand>();
    let (evt_tx, evt_rx) = channel::<ForwardEvent>();

    std::thread::spawn(move || {
        let runtime = match Runtime::new() {
            Ok(runtime) => runtime,
            Err(error) => {
                let _ = evt_tx.send(ForwardEvent::Error(format!(
                    "转发运行时创建失败: {error}"
                )));
                return;
            }
        };
        let manager = ForwardingManager::new("egui-forward", handle);

        let refresh = |evt_tx: &Sender<ForwardEvent>, manager: &ForwardingManager| {
            let _ = evt_tx.send(ForwardEvent::Rules(manager.list_forwards()));
        };

        while let Ok(command) = cmd_rx.recv() {
            match command {
                ForwardCommand::Create(rule) => {
                    match runtime.block_on(manager.create_forward(rule)) {
                        Ok(_) => {}
                        Err(error) => {
                            let _ = evt_tx.send(ForwardEvent::Error(error.to_string()));
                        }
                    }
                    refresh(&evt_tx, &manager);
                }
                ForwardCommand::Stop(id) => {
                    match runtime.block_on(manager.stop_forward(&id)) {
                        Ok(_) => {}
                        Err(error) => {
                            let _ = evt_tx.send(ForwardEvent::Error(error.to_string()));
                        }
                    }
                    refresh(&evt_tx, &manager);
                }
                ForwardCommand::List => refresh(&evt_tx, &manager),
            }
        }
    });

    ForwardPanel {
        show: false,
        tx: cmd_tx,
        rx: evt_rx,
        rules: Vec::new(),
        error: None,
        busy: false,
        bind_port: 8080,
        target_host: "localhost".to_string(),
        target_port: 80,
        description: String::new(),
    }
}

/// 渲染转发窗口，返回需要发送给 worker 的命令。
pub fn forward_window_ui(ui: &mut egui::Ui, panel: &mut ForwardPanel) -> Vec<ForwardCommand> {
    let mut commands = Vec::new();

    ui.horizontal(|ui| {
        ui.label(format!("{} 条规则", panel.rules.len()));
        if ui.button("刷新").clicked() {
            commands.push(ForwardCommand::List);
        }
    });
    if let Some(error) = &panel.error {
        ui.colored_label(egui::Color32::from_rgb(0xff, 0x77, 0x77), error);
    }
    ui.separator();

    egui::ScrollArea::vertical()
        .max_height(220.0)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for rule in panel.rules.clone() {
                ui.horizontal(|ui| {
                    let kind = match rule.forward_type {
                        ForwardType::Local => "本地",
                        ForwardType::Remote => "远程",
                        ForwardType::Dynamic => "动态",
                    };
                    let status = match rule.status {
                        ForwardStatus::Active => "运行中",
                        ForwardStatus::Starting => "启动中",
                        ForwardStatus::Stopped => "已停止",
                        ForwardStatus::Error => "错误",
                        ForwardStatus::Suspended => "挂起",
                    };
                    ui.label(format!(
                        "[{kind}] {}:{} → {}:{} ({status})",
                        rule.bind_address, rule.bind_port, rule.target_host, rule.target_port
                    ));
                    if !rule.description.is_empty() {
                        ui.weak(format!(" {}", rule.description));
                    }
                    if ui.small_button("停止").clicked() {
                        commands.push(ForwardCommand::Stop(rule.id));
                    }
                });
            }
            if panel.rules.is_empty() {
                ui.weak("还没有转发规则。");
            }
        });

    ui.separator();
    ui.label("添加本地转发");
    ui.horizontal(|ui| {
        ui.label("本地端口");
        ui.add(
            egui::DragValue::new(&mut panel.bind_port).range(1.0..=65535.0),
        );
        ui.label("目标主机");
        ui.add(
            egui::TextEdit::singleline(&mut panel.target_host).desired_width(120.0),
        );
        ui.label("目标端口");
        ui.add(
            egui::DragValue::new(&mut panel.target_port).range(1.0..=65535.0),
        );
    });
    ui.horizontal(|ui| {
        ui.label("描述");
        ui.add(
            egui::TextEdit::singleline(&mut panel.description).desired_width(220.0),
        );
        if ui
            .add_enabled(!panel.busy, egui::Button::new("添加"))
            .clicked()
        {
            let target_host = panel.target_host.trim().to_string();
            if target_host.is_empty() {
                panel.error = Some("目标主机不能为空".to_string());
            } else {
                commands.push(ForwardCommand::Create(ForwardRule {
                    id: format!("fwd-{:x}", fast_id()),
                    forward_type: ForwardType::Local,
                    bind_address: "127.0.0.1".to_string(),
                    bind_port: panel.bind_port,
                    target_host,
                    target_port: panel.target_port,
                    status: ForwardStatus::Starting,
                    description: panel.description.trim().to_string(),
                }));
                panel.busy = true;
                panel.description.clear();
            }
        }
    });

    commands
}

fn fast_id() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0)
        ^ std::process::id() as u64
}
