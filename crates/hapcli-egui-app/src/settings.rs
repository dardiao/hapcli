//! 应用设置：主题、字体、透明度（本地 JSON 持久化）。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeChoice {
    Dark,
    Light,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AppSettings {
    pub font_size: f32,
    pub theme: ThemeChoice,
    /// 终端背景不透明度（配合透明窗口生效）。
    pub background_alpha: f32,
    pub transparent_window: bool,
    /// 自定义终端字体文件路径；None 表示使用默认等宽字体。
    pub terminal_font_path: Option<String>,
    /// 选中完成（松开鼠标 / 双击 / 三击）后自动复制到剪贴板。
    pub copy_on_select: bool,
    /// 鼠标中键点击粘贴剪贴板内容。
    pub middle_click_paste: bool,
    /// SSH 断线后自动重连（最多 3 次）。
    pub ssh_auto_reconnect: bool,
    /// 长命令（前台进程运行超过阈值后结束）完成时发系统通知。
    pub notify_on_long_command: bool,
    /// SSH 远程会话自动启用彩色输出（env 请求 + 远程启动文件注入）。
    #[serde(default = "default_true")]
    pub ssh_shell_colors: bool,
    /// SSH 终端输入 cd 时，自动让 SFTP 面板跟随切换目录。
    #[serde(default = "default_true")]
    pub sftp_sync_cwd: bool,
    /// 启动后自动检查 GitHub 新版本。
    #[serde(default = "default_true")]
    pub check_updates: bool,
    /// 用户选择“忽略此版本”的版本号；再次提示会跳过它。
    #[serde(default)]
    pub ignored_update_version: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            font_size: 13.0,
            theme: ThemeChoice::Light,
            background_alpha: 1.0,
            transparent_window: false,
            terminal_font_path: None,
            copy_on_select: false,
            middle_click_paste: true,
            ssh_auto_reconnect: true,
            notify_on_long_command: true,
            ssh_shell_colors: true,
            sftp_sync_cwd: true,
            check_updates: true,
            ignored_update_version: None,
        }
    }
}

pub fn settings_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir());
    home.map(|home| home.join(".hapcli").join("settings.json"))
        .unwrap_or_else(|| PathBuf::from("settings.json"))
}

pub fn load_settings() -> AppSettings {
    match std::fs::read(settings_path()) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => AppSettings::default(),
    }
}

pub fn save_settings(settings: &AppSettings) -> Result<(), String> {
    save_settings_to(&settings_path(), settings)
}

pub fn save_settings_to(path: &Path, settings: &AppSettings) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "无效的设置路径".to_string())?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let json = serde_json::to_string_pretty(settings).map_err(|error| error.to_string())?;
    std::fs::write(path, json).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_preserves_fields() {
        let path = std::env::temp_dir().join(format!(
            "hapcli_settings_test_{}.json",
            std::process::id()
        ));
        let settings = AppSettings {
            font_size: 16.0,
            theme: ThemeChoice::Light,
            background_alpha: 0.7,
            transparent_window: true,
            terminal_font_path: Some("/tmp/myfont.ttf".to_string()),
            copy_on_select: true,
            middle_click_paste: true,
            ssh_auto_reconnect: true,
            notify_on_long_command: true,
            ssh_shell_colors: false,
            sftp_sync_cwd: true,
            check_updates: true,
            ignored_update_version: None,
        };
        save_settings_to(&path, &settings).unwrap();
        let loaded: AppSettings =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        std::fs::remove_file(&path).ok();

        assert_eq!(loaded.font_size, 16.0);
        assert_eq!(loaded.theme, ThemeChoice::Light);
        assert_eq!(loaded.background_alpha, 0.7);
        assert!(loaded.transparent_window);
        assert_eq!(loaded.terminal_font_path.as_deref(), Some("/tmp/myfont.ttf"));
        assert!(loaded.copy_on_select);
        assert!(loaded.middle_click_paste);
        assert!(loaded.ssh_auto_reconnect);
        assert!(loaded.notify_on_long_command);
        assert!(!loaded.ssh_shell_colors);
    }

    #[test]
    fn load_missing_file_returns_default() {
        let path = std::env::temp_dir().join("hapcli_settings_missing.json");
        std::fs::remove_file(&path).ok();
        assert_eq!(load_settings_from(&path), AppSettings::default());
    }

    fn load_settings_from(path: &Path) -> AppSettings {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            Err(_) => AppSettings::default(),
        }
    }
}
