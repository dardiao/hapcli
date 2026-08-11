//! 会话收藏：连接配置的本地 JSON 持久化。
//!
//! 刻意不保存任何密钥：密码与密钥口令只在连接表单中临时存在，
//! 收藏只保留主机、端口、用户名、认证方式与密钥路径等非敏感字段。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionKind {
    Ssh,
    Telnet,
    Serial,
}

impl ConnectionKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ssh => "SSH",
            Self::Telnet => "Telnet",
            Self::Serial => "串口",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthChoice {
    Password,
    Key,
    Agent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParityChoice {
    None,
    Odd,
    Even,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlowChoice {
    None,
    Software,
    Hardware,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectionProfile {
    pub name: String,
    pub kind: ConnectionKind,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: AuthChoice,
    pub key_path: String,
    pub post_connect_command: String,
    pub serial_port: String,
    pub baud_rate: u32,
    pub data_bits: u8,
    pub stop_bits: u8,
    pub parity: ParityChoice,
    pub flow_control: FlowChoice,
}

pub fn profiles_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir());
    home.map(|home| home.join(".hapcli").join("profiles.json"))
        .unwrap_or_else(|| PathBuf::from("profiles.json"))
}

pub fn load_profiles() -> Vec<ConnectionProfile> {
    load_profiles_from(&profiles_path())
}

pub fn load_profiles_from(path: &Path) -> Vec<ConnectionProfile> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

pub fn save_profiles(profiles: &[ConnectionProfile]) -> Result<(), String> {
    save_profiles_to(&profiles_path(), profiles)
}

pub fn save_profiles_to(path: &Path, profiles: &[ConnectionProfile]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "无效的配置路径".to_string())?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let json = serde_json::to_string_pretty(profiles).map_err(|error| error.to_string())?;
    std::fs::write(path, json).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_round_trip_preserves_fields() {
        let path = std::env::temp_dir().join(format!(
            "hapcli_profiles_test_{}.json",
            std::process::id()
        ));
        let profiles = vec![ConnectionProfile {
            name: "办公机".to_string(),
            kind: ConnectionKind::Ssh,
            host: "office.example.com".to_string(),
            port: 2222,
            username: "alice".to_string(),
            auth: AuthChoice::Key,
            key_path: "/Users/alice/.ssh/id_ed25519".to_string(),
            post_connect_command: String::new(),
            serial_port: String::new(),
            baud_rate: 0,
            data_bits: 8,
            stop_bits: 1,
            parity: ParityChoice::None,
            flow_control: FlowChoice::None,
        }];

        save_profiles_to(&path, &profiles).unwrap();
        let loaded = load_profiles_from(&path);
        std::fs::remove_file(&path).ok();

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "办公机");
        assert_eq!(loaded[0].kind, ConnectionKind::Ssh);
        assert_eq!(loaded[0].host, "office.example.com");
        assert_eq!(loaded[0].port, 2222);
        assert_eq!(loaded[0].username, "alice");
        assert_eq!(loaded[0].auth, AuthChoice::Key);
        assert_eq!(loaded[0].key_path, "/Users/alice/.ssh/id_ed25519");
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let path = std::env::temp_dir().join("hapcli_profiles_missing.json");
        std::fs::remove_file(&path).ok();
        assert!(load_profiles_from(&path).is_empty());
    }
}
