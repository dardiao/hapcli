//! 新建连接弹窗：统一支持 SSH / Telnet / 串口三类会话，并管理本地收藏。

use std::{future::Future, pin::Pin, sync::Arc};

use eframe::egui;
use hapcli_ssh::{
    AuthMethod, KeyboardInteractivePromptRequest, KeyboardInteractiveResponses, SshConfig,
    SshPromptError, SshPromptHandler,
};
use hapcli_terminal::{
    SerialFlowControl, SerialParity, SerialPortInfo, SerialSessionConfig, SshSessionConfig,
    TelnetSessionConfig, serial_list_ports,
};
use hapcli_secret_store::NativeSecretStore;
use zeroize::Zeroizing;

use crate::profiles::{
    AuthChoice, ConnectionKind, ConnectionProfile, FlowChoice, ParityChoice, load_profiles,
    save_profiles,
};

pub enum ConnectTarget {
    Ssh(SshConnectSpec),
    Telnet(TelnetSessionConfig),
    Serial(SerialSessionConfig),
}

pub struct SshConnectSpec {
    pub session_config: SshSessionConfig,
    /// 用于断线重连的原始配置（可 Clone）。
    pub reconnect_config: hapcli_ssh::SshConfig,
}

pub struct ConnectRequest {
    pub target: ConnectTarget,
    pub label: String,
    /// 连接成功后需要写入钥匙串的 (key, 密码)。
    pub save_password: Option<(String, Zeroizing<String>)>,
}

pub enum DialogOutcome {
    Connect(ConnectRequest),
    Cancel,
}

pub struct ConnectForm {
    pub kind: ConnectionKind,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: AuthChoice,
    pub password: String,
    pub key_path: String,
    pub key_passphrase: String,
    pub post_connect_command: String,
    pub save_password: bool,
    /// 本次 SSH 连接是否自动启用远程彩色输出（来自全局设置）。
    pub shell_colors: bool,
    pub serial_port: String,
    pub baud_rate: u32,
    pub data_bits: u8,
    pub stop_bits: u8,
    pub parity: ParityChoice,
    pub flow_control: FlowChoice,
    profiles: Vec<ConnectionProfile>,
    selected_profile: Option<String>,
    serial_ports: Vec<SerialPortInfo>,
    error: Option<String>,
}

impl Default for ConnectForm {
    fn default() -> Self {
        let serial_ports = serial_list_ports().unwrap_or_default();
        Self {
            kind: ConnectionKind::Ssh,
            name: String::new(),
            host: String::new(),
            port: 22,
            username: std::env::var("USER").unwrap_or_default(),
            auth: AuthChoice::Password,
            password: String::new(),
            key_path: default_key_path(),
            key_passphrase: String::new(),
            post_connect_command: String::new(),
            save_password: false,
            shell_colors: true,
            serial_port: String::new(),
            baud_rate: 115_200,
            data_bits: 8,
            stop_bits: 1,
            parity: ParityChoice::None,
            flow_control: FlowChoice::None,
            profiles: load_profiles(),
            selected_profile: None,
            serial_ports,
            error: None,
        }
    }
}

impl ConnectForm {
    pub fn show_error(&mut self, message: String) {
        self.error = Some(message);
    }

    /// 渲染表单；「连接」返回配置请求，「取消」返回 Cancel，其余返回 None。
    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<DialogOutcome> {
        let mut outcome = None;

        if !self.profiles.is_empty() {
            ui.horizontal(|ui| {
                ui.label("已保存配置");
                let selected_text = self
                    .selected_profile
                    .clone()
                    .unwrap_or_else(|| "选择…".to_string());
                let mut to_load: Option<ConnectionProfile> = None;
                egui::ComboBox::from_id_salt("saved_profiles")
                    .selected_text(selected_text)
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        for profile in &self.profiles {
                            let selected = self.selected_profile.as_deref() == Some(profile.name.as_str());
                            if ui.selectable_label(selected, profile.name.clone()).clicked() {
                                self.selected_profile = Some(profile.name.clone());
                                to_load = Some(profile.clone());
                            }
                        }
                    });
                if let Some(profile) = to_load {
                    self.apply_profile(profile);
                }
            });
            ui.add_space(6.0);
        }

        egui::Grid::new("connect_form")
            .num_columns(2)
            .spacing([10.0, 8.0])
            .show(ui, |ui| {
                ui.label("会话类型");
                let kind_label = self.kind.label();
                egui::ComboBox::from_id_salt("connect_kind")
                    .selected_text(kind_label)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.kind,
                            ConnectionKind::Ssh,
                            "SSH",
                        );
                        ui.selectable_value(
                            &mut self.kind,
                            ConnectionKind::Telnet,
                            "Telnet",
                        );
                        ui.selectable_value(
                            &mut self.kind,
                            ConnectionKind::Serial,
                            "串口",
                        );
                    });
                ui.end_row();

                ui.label("配置名称（收藏）");
                ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(240.0));
                ui.end_row();

                if self.kind == ConnectionKind::Serial {
                    self.serial_fields(ui);
                } else {
                    ui.label("主机");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.host)
                            .hint_text("example.com")
                            .desired_width(240.0),
                    );
                    ui.end_row();

                    ui.label("端口");
                    ui.add(egui::DragValue::new(&mut self.port).range(1.0..=65535.0));
                    ui.end_row();

                    if self.kind == ConnectionKind::Ssh {
                        ui.label("用户名");
                        ui.add(egui::TextEdit::singleline(&mut self.username).desired_width(240.0));
                        ui.end_row();
                        self.ssh_auth_fields(ui);
                    }
                }
            });

        if let Some(error) = &self.error {
            ui.add_space(4.0);
            ui.colored_label(egui::Color32::from_rgb(0xff, 0x77, 0x77), error);
        }

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("保存配置").clicked() {
                if let Err(message) = self.save_current_profile() {
                    self.error = Some(message);
                }
            }
            let can_delete = self.selected_profile.is_some();
            if ui
                .add_enabled(can_delete, egui::Button::new("删除配置"))
                .clicked()
            {
                if let Err(message) = self.delete_selected_profile() {
                    self.error = Some(message);
                }
            }
            ui.separator();
            if ui.button("连接").clicked() {
                match self.build_request() {
                    Ok(request) => {
                        self.error = None;
                        outcome = Some(DialogOutcome::Connect(request));
                    }
                    Err(message) => self.error = Some(message),
                }
            }
            if ui.button("取消").clicked() {
                outcome = Some(DialogOutcome::Cancel);
            }
        });

        outcome
    }

    fn ssh_auth_fields(&mut self, ui: &mut egui::Ui) {
        ui.label("认证方式");
        let selected = match self.auth {
            AuthChoice::Password => "密码",
            AuthChoice::Key => "私钥",
            AuthChoice::Agent => "SSH Agent",
        };
        egui::ComboBox::from_id_salt("ssh_auth_method")
            .selected_text(selected)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.auth, AuthChoice::Password, "密码");
                ui.selectable_value(&mut self.auth, AuthChoice::Key, "私钥");
                ui.selectable_value(&mut self.auth, AuthChoice::Agent, "SSH Agent");
            });
        ui.end_row();

        match self.auth {
            AuthChoice::Password => {
                ui.label("密码");
                ui.add(
                    egui::TextEdit::singleline(&mut self.password)
                        .password(true)
                        .desired_width(240.0),
                );
                ui.end_row();
                ui.label("");
                ui.checkbox(
                    &mut self.save_password,
                    "保存密码到 macOS 钥匙串（连接成功后自动填入）",
                );
                ui.end_row();
            }
            AuthChoice::Key => {
                ui.label("私钥路径");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.key_path).desired_width(210.0));
                    if ui.button("浏览…").clicked() {
                        if let Some(path) = pick_key_file() {
                            self.key_path = path.display().to_string();
                        }
                    }
                });
                ui.end_row();
                ui.label("密钥口令（可选）");
                ui.add(
                    egui::TextEdit::singleline(&mut self.key_passphrase)
                        .password(true)
                        .desired_width(240.0),
                );
                ui.end_row();
            }
            AuthChoice::Agent => {}
        }

        ui.label("连接后命令（可选）");
        ui.add(
            egui::TextEdit::singleline(&mut self.post_connect_command)
                .hint_text("例如 htop")
                .desired_width(240.0),
        );
        ui.end_row();
    }

    fn serial_fields(&mut self, ui: &mut egui::Ui) {
        ui.label("端口");
        ui.horizontal(|ui| {
            let selected = if self.serial_port.is_empty() {
                "选择端口…".to_string()
            } else {
                self.serial_port.clone()
            };
            egui::ComboBox::from_id_salt("serial_port")
                .selected_text(selected)
                .width(190.0)
                .show_ui(ui, |ui| {
                    for port in &self.serial_ports {
                        let label = format!("{} ({})", port.display_name, port.port_path);
                        if ui
                            .selectable_label(self.serial_port == port.port_path, label)
                            .clicked()
                        {
                            self.serial_port = port.port_path.clone();
                        }
                    }
                });
            if ui.small_button("刷新").clicked() {
                self.refresh_serial_ports();
            }
        });
        ui.end_row();

        ui.label("端口路径（手动）");
        ui.add(
            egui::TextEdit::singleline(&mut self.serial_port)
                .hint_text("/dev/cu.usbserial-…")
                .desired_width(240.0),
        );
        ui.end_row();

        ui.label("波特率");
        ui.add(egui::DragValue::new(&mut self.baud_rate).range(1.0..=4_000_000.0));
        ui.end_row();

        ui.label("数据位");
        ui.add(egui::DragValue::new(&mut self.data_bits).range(5.0..=8.0));
        ui.end_row();

        ui.label("停止位");
        ui.add(egui::DragValue::new(&mut self.stop_bits).range(1.0..=2.0));
        ui.end_row();

        ui.label("校验");
        let parity_label = match self.parity {
            ParityChoice::None => "无",
            ParityChoice::Odd => "奇校验",
            ParityChoice::Even => "偶校验",
        };
        egui::ComboBox::from_id_salt("serial_parity")
            .selected_text(parity_label)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.parity, ParityChoice::None, "无");
                ui.selectable_value(&mut self.parity, ParityChoice::Odd, "奇校验");
                ui.selectable_value(&mut self.parity, ParityChoice::Even, "偶校验");
            });
        ui.end_row();

        ui.label("流控");
        let flow_label = match self.flow_control {
            FlowChoice::None => "无",
            FlowChoice::Software => "软件 (XON/XOFF)",
            FlowChoice::Hardware => "硬件 (RTS/CTS)",
        };
        egui::ComboBox::from_id_salt("serial_flow")
            .selected_text(flow_label)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.flow_control, FlowChoice::None, "无");
                ui.selectable_value(&mut self.flow_control, FlowChoice::Software, "软件 (XON/XOFF)");
                ui.selectable_value(&mut self.flow_control, FlowChoice::Hardware, "硬件 (RTS/CTS)");
            });
        ui.end_row();
    }

    fn refresh_serial_ports(&mut self) {
        self.serial_ports = serial_list_ports().unwrap_or_default();
        if self.serial_ports.is_empty() {
            self.error = Some("未检测到串口设备，可手动填写端口路径".to_string());
        }
    }

    fn apply_profile(&mut self, profile: ConnectionProfile) {
        // 在移动字段前先从钥匙串读取密码（若已保存过）。
        let keychain_secret = if profile.kind == ConnectionKind::Ssh
            && profile.auth == AuthChoice::Password
        {
            let key = ssh_password_key(&profile.username, &profile.host, profile.port);
            load_keychain_password(&key)
        } else {
            None
        };

        self.kind = profile.kind;
        self.name = profile.name;
        self.host = profile.host;
        self.port = profile.port;
        self.username = profile.username;
        self.auth = profile.auth;
        self.key_path = profile.key_path;
        self.post_connect_command = profile.post_connect_command;
        self.save_password = false;
        self.serial_port = profile.serial_port;
        self.baud_rate = profile.baud_rate;
        self.data_bits = profile.data_bits;
        self.stop_bits = profile.stop_bits;
        self.parity = profile.parity;
        self.flow_control = profile.flow_control;
        // 密码 / 密钥口令永不持久化，也不回填。
        self.password.clear();
        self.key_passphrase.clear();
        if let Some(secret) = keychain_secret {
            self.password = secret.to_string();
            self.save_password = true;
        }
        self.error = None;
    }

    fn current_profile(&self) -> Result<ConnectionProfile, String> {
        let name = {
            let name = self.name.trim();
            if !name.is_empty() {
                name.to_string()
            } else if self.kind == ConnectionKind::Serial {
                self.serial_port.trim().to_string()
            } else {
                self.host.trim().to_string()
            }
        };
        if name.is_empty() {
            return Err("配置名称或主机不能为空".to_string());
        }

        Ok(ConnectionProfile {
            name,
            kind: self.kind,
            host: self.host.trim().to_string(),
            port: self.port,
            username: self.username.trim().to_string(),
            auth: self.auth,
            key_path: self.key_path.trim().to_string(),
            post_connect_command: self.post_connect_command.trim().to_string(),
            serial_port: self.serial_port.trim().to_string(),
            baud_rate: self.baud_rate,
            data_bits: self.data_bits,
            stop_bits: self.stop_bits,
            parity: self.parity,
            flow_control: self.flow_control,
        })
    }

    fn save_current_profile(&mut self) -> Result<(), String> {
        let profile = self.current_profile()?;
        let name = profile.name.clone();
        if let Some(existing) = self.profiles.iter_mut().find(|p| p.name == profile.name) {
            *existing = profile;
        } else {
            self.profiles.push(profile);
        }
        save_profiles(&self.profiles)?;
        self.selected_profile = Some(name);
        self.error = None;
        Ok(())
    }

    fn delete_selected_profile(&mut self) -> Result<(), String> {
        let Some(name) = self.selected_profile.clone() else {
            return Ok(());
        };
        if let Some(profile) = self.profiles.iter().find(|profile| profile.name == name) {
            if profile.kind == ConnectionKind::Ssh && profile.auth == AuthChoice::Password {
                let key = ssh_password_key(&profile.username, &profile.host, profile.port);
                let _ = NativeSecretStore::new("hapcli").delete(&key);
            }
        }
        self.profiles.retain(|profile| profile.name != name);
        save_profiles(&self.profiles)?;
        self.selected_profile = None;
        self.error = None;
        Ok(())
    }

    fn build_request(&self) -> Result<ConnectRequest, String> {
        match self.kind {
            ConnectionKind::Ssh => self.build_ssh_request(),
            ConnectionKind::Telnet => self.build_telnet_request(),
            ConnectionKind::Serial => self.build_serial_request(),
        }
    }

    fn build_ssh_request(&self) -> Result<ConnectRequest, String> {
        let host = self.host.trim();
        let username = self.username.trim();
        if host.is_empty() {
            return Err("主机不能为空".to_string());
        }
        if username.is_empty() {
            return Err("用户名不能为空".to_string());
        }

        let auth = match self.auth {
            AuthChoice::Password => AuthMethod::password(self.password.clone()),
            AuthChoice::Key => {
                let key_path = self.key_path.trim();
                if key_path.is_empty() {
                    return Err("私钥路径不能为空".to_string());
                }
                let passphrase =
                    (!self.key_passphrase.is_empty()).then(|| self.key_passphrase.clone());
                AuthMethod::key(key_path.to_string(), passphrase)
            }
            AuthChoice::Agent => AuthMethod::Agent,
        };

        let mut config = SshConfig {
            host: host.to_string(),
            port: self.port,
            username: username.to_string(),
            auth,
            timeout_secs: 20,
            shell_colors: self.shell_colors,
            ..Default::default()
        };
        let post_connect_command = self.post_connect_command.trim().to_string();
        if !post_connect_command.is_empty() {
            config.post_connect_command = Some(post_connect_command);
        }

        let prompt_handler = PasswordPromptHandler {
            password: Zeroizing::new(self.password.clone()),
        };
        let reconnect_config = config.clone();
        let session_config =
            SshSessionConfig::from(config).with_prompt_handler(Arc::new(prompt_handler));

        Ok(ConnectRequest {
            target: ConnectTarget::Ssh(SshConnectSpec {
                session_config,
                reconnect_config,
            }),
            label: format!("{username}@{host}:{}", self.port),
            save_password: (self.save_password && !self.password.is_empty())
                .then(|| {
                    (
                        ssh_password_key(username, host, self.port),
                        Zeroizing::new(self.password.clone()),
                    )
                }),
        })
    }

    fn build_telnet_request(&self) -> Result<ConnectRequest, String> {
        let host = self.host.trim();
        if host.is_empty() {
            return Err("主机不能为空".to_string());
        }
        Ok(ConnectRequest {
            target: ConnectTarget::Telnet(TelnetSessionConfig {
                host: host.to_string(),
                port: self.port,
            }),
            label: format!("{host}:{}", self.port),
            save_password: None,
        })
    }

    fn build_serial_request(&self) -> Result<ConnectRequest, String> {
        let port_path = self.serial_port.trim();
        if port_path.is_empty() {
            return Err("串口端口不能为空".to_string());
        }
        if self.baud_rate == 0 {
            return Err("波特率必须大于 0".to_string());
        }
        if !(5..=8).contains(&self.data_bits) {
            return Err("数据位必须在 5..=8 之间".to_string());
        }
        if !matches!(self.stop_bits, 1 | 2) {
            return Err("停止位必须为 1 或 2".to_string());
        }

        let config = SerialSessionConfig {
            port_path: port_path.to_string(),
            baud_rate: self.baud_rate,
            data_bits: self.data_bits,
            stop_bits: self.stop_bits,
            parity: match self.parity {
                ParityChoice::None => SerialParity::None,
                ParityChoice::Odd => SerialParity::Odd,
                ParityChoice::Even => SerialParity::Even,
            },
            flow_control: match self.flow_control {
                FlowChoice::None => SerialFlowControl::None,
                FlowChoice::Software => SerialFlowControl::Software,
                FlowChoice::Hardware => SerialFlowControl::Hardware,
            },
        };
        config.validate().map_err(|error| error.to_string())?;

        Ok(ConnectRequest {
            target: ConnectTarget::Serial(config),
            label: format!("串口 {}", port_path),
            save_password: None,
        })
    }
}

/// 用原始配置重建会话配置（断线重连用），保留 keyboard-interactive 密码兜底。
pub(crate) fn build_reconnect_session_config(
    config: &hapcli_ssh::SshConfig,
) -> SshSessionConfig {
    let password = match &config.auth {
        AuthMethod::Password { password } => password.to_string(),
        _ => String::new(),
    };
    SshSessionConfig::from(config.clone())
        .with_prompt_handler(Arc::new(PasswordPromptHandler {
            password: Zeroizing::new(password),
        }))
}

fn default_key_path() -> String {
    std::env::var("HOME")
        .map(|home| format!("{home}/.ssh/id_ed25519"))
        .unwrap_or_else(|_| "~/.ssh/id_ed25519".to_string())
}

fn ssh_password_key(username: &str, host: &str, port: u16) -> String {
    format!("ssh://{username}@{host}:{port}")
}

fn load_keychain_password(key: &str) -> Option<Zeroizing<String>> {
    NativeSecretStore::new("hapcli").get(key).ok().flatten()
}

/// 弹出系统原生文件选择框，默认定位到 ~/.ssh。
fn pick_key_file() -> Option<std::path::PathBuf> {
    let mut dialog = rfd::FileDialog::new()
        .add_filter("私钥文件", &["pem", "key", "cer", "crt", "p12", "pfx", "ppk"])
        .add_filter("所有文件", &["*"]);

    if let Some(home) = std::env::var_os("HOME") {
        let ssh_dir = std::path::PathBuf::from(home).join(".ssh");
        if ssh_dir.is_dir() {
            dialog = dialog.set_directory(&ssh_dir);
        }
    }

    dialog.pick_file()
}

/// keyboard-interactive 密码提示兜底：仅应答密码/口令类提示，其余留空。
struct PasswordPromptHandler {
    password: Zeroizing<String>,
}

impl SshPromptHandler for PasswordPromptHandler {
    fn keyboard_interactive(
        &self,
        request: KeyboardInteractivePromptRequest,
    ) -> Pin<
        Box<dyn Future<Output = Result<KeyboardInteractiveResponses, SshPromptError>> + Send + '_>,
    > {
        let password = self.password.clone();
        Box::pin(async move {
            let mut responses = Vec::with_capacity(request.prompts.len());
            for prompt in &request.prompts {
                let lower = prompt.prompt.to_lowercase();
                if lower.contains("password") || lower.contains("passphrase") {
                    responses.push(password.to_string());
                } else {
                    responses.push(String::new());
                }
            }
            Ok(Zeroizing::new(responses))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form() -> ConnectForm {
        ConnectForm {
            host: String::new(),
            username: String::new(),
            ..Default::default()
        }
    }

    #[test]
    fn ssh_request_rejects_empty_host() {
        let form = form();
        assert!(form.build_request().is_err());
    }

    #[test]
    fn ssh_request_rejects_empty_username() {
        let mut form = form();
        form.host = "example.com".to_string();
        assert!(form.build_request().is_err());
    }

    #[test]
    fn ssh_request_password_auth() {
        let mut form = form();
        form.host = "example.com".to_string();
        form.username = "alice".to_string();
        form.password = "secret".to_string();
        form.save_password = true;
        let request = form.build_request().unwrap();
        assert_eq!(request.label, "alice@example.com:22");
        let (key, secret) = request.save_password.unwrap();
        assert_eq!(key, "ssh://alice@example.com:22");
        assert_eq!(secret.as_str(), "secret");
    }

    #[test]
    fn ssh_request_does_not_save_password_by_default() {
        let mut form = form();
        form.host = "example.com".to_string();
        form.username = "alice".to_string();
        form.password = "secret".to_string();
        let request = form.build_request().unwrap();
        assert!(request.save_password.is_none());
    }

    #[test]
    fn ssh_request_key_auth_requires_path() {
        let mut form = form();
        form.host = "example.com".to_string();
        form.username = "alice".to_string();
        form.auth = AuthChoice::Key;
        form.key_path.clear();
        assert!(form.build_request().is_err());
    }

    #[test]
    fn telnet_request_builds_config() {
        let mut form = form();
        form.kind = ConnectionKind::Telnet;
        form.host = "router.local".to_string();
        form.port = 23;
        let request = form.build_request().unwrap();
        assert_eq!(request.label, "router.local:23");
        match request.target {
            ConnectTarget::Telnet(config) => {
                assert_eq!(config.host, "router.local");
                assert_eq!(config.port, 23);
            }
            _ => panic!("expected telnet target"),
        }
    }

    #[test]
    fn serial_request_validates_port() {
        let mut form = form();
        form.kind = ConnectionKind::Serial;
        assert!(form.build_request().is_err());
        form.serial_port = "/dev/cu.usbserial".to_string();
        assert!(form.build_request().is_ok());
    }

    #[test]
    fn current_profile_never_contains_secrets() {
        let mut form = form();
        form.host = "example.com".to_string();
        form.username = "alice".to_string();
        form.password = "hunter2".to_string();
        form.key_passphrase = "passphrase".to_string();
        let profile = form.current_profile().unwrap();
        assert_eq!(profile.name, "example.com");
        // ConnectionProfile 结构本身没有密码/口令字段。
        assert_eq!(profile.host, "example.com");
    }
}
