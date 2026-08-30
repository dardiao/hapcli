//! 应用设置：主题、字体、透明度（本地 JSON 持久化）。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeChoice {
    Dark,
    Light,
}

/// 设置窗口左侧的分类页面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsPage {
    /// 常规设置：通用行为（复制粘贴、重连、通知、更新、代理等）。
    General,
    /// 外观设置：应用界面 UI（深浅色主题、透明窗口等）。
    Appearance,
    /// 终端设置：终端仿真器内容的外观与行为（字体、字号、光标、滚动等），
    /// 只作用于连上 SSH / zsh 之后的终端画面，与 app 界面 UI 无关。
    Terminal,
}

/// 终端默认光标样式（Oxideterm 终端设置里的 CursorStyle）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CursorStyleChoice {
    #[default]
    Block,
    Underline,
    Beam,
}

impl CursorStyleChoice {
    pub fn label(self) -> &'static str {
        match self {
            Self::Block => "方块 ▮",
            Self::Underline => "下划线 _",
            Self::Beam => "竖线 |",
        }
    }

    pub fn to_kernel(self) -> hapcli_terminal::TerminalCursorStyle {
        match self {
            Self::Block => hapcli_terminal::TerminalCursorStyle::Block,
            Self::Underline => hapcli_terminal::TerminalCursorStyle::Underline,
            Self::Beam => hapcli_terminal::TerminalCursorStyle::Beam,
        }
    }
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
    /// 终端滚动历史行数（新会话生效）。
    #[serde(default = "default_scrollback")]
    pub scrollback_lines: usize,
    /// 终端行高倍率（1.0 = 默认）。
    #[serde(default = "default_line_height")]
    pub line_height: f32,
    /// 终端默认光标样式（新会话生效）。
    #[serde(default)]
    pub cursor_style: CursorStyleChoice,
    /// 选中完成（松开鼠标 / 双击 / 三击）后自动复制到剪贴板。
    pub copy_on_select: bool,
    /// 鼠标中键点击粘贴剪贴板内容。
    pub middle_click_paste: bool,
    /// SSH 断线后自动重连（最多 3 次）。
    pub ssh_auto_reconnect: bool,
    /// 长命令（前台进程运行超过阈值后结束）完成时发系统通知。
    pub notify_on_long_command: bool,
    /// SSH 终端输入 cd 时，自动让 SFTP 面板跟随切换目录。
    #[serde(default = "default_true")]
    pub sftp_sync_cwd: bool,
    /// 启动后自动检查 GitHub 新版本。
    #[serde(default = "default_true")]
    pub check_updates: bool,
    /// 用户选择“忽略此版本”的版本号；再次提示会跳过它。
    #[serde(default)]
    pub ignored_update_version: Option<String>,
    /// GitHub 代理前缀列表（逗号分隔，用于更新下载被墙时自动回退 / 测速选择最快节点）。
    /// 留空表示禁用代理回退。默认来自 github.akams.cn 收集的加速源。
    #[serde(default = "default_github_proxies")]
    pub github_proxies: String,
}

fn default_true() -> bool {
    true
}

fn default_scrollback() -> usize {
    1000
}

fn default_line_height() -> f32 {
    1.0
}

/// 默认 GitHub 代理前缀（来自 github.akams.cn 站点前端收集的加速源）。
pub fn default_github_proxies() -> String {
    [
        "https://gh.dpik.top/",
        "https://ghfast.top/",
        "https://gh-proxy.com/",
        "https://ghproxy.net/",
        "https://gh-proxy.net/",
        "https://github.tbap.top/",
        "https://gh.ddlc.top/",
        "https://gitproxy.click/",
    ]
    .join(",")
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            font_size: 13.0,
            theme: ThemeChoice::Light,
            background_alpha: 1.0,
            transparent_window: false,
            terminal_font_path: None,
            scrollback_lines: default_scrollback(),
            line_height: default_line_height(),
            cursor_style: CursorStyleChoice::default(),
            copy_on_select: false,
            middle_click_paste: true,
            ssh_auto_reconnect: true,
            notify_on_long_command: true,
            sftp_sync_cwd: true,
            check_updates: true,
            ignored_update_version: None,
            github_proxies: default_github_proxies(),
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
            scrollback_lines: 5000,
            line_height: 1.2,
            cursor_style: CursorStyleChoice::Beam,
            copy_on_select: true,
            middle_click_paste: true,
            ssh_auto_reconnect: true,
            notify_on_long_command: true,
            sftp_sync_cwd: true,
            check_updates: true,
            ignored_update_version: None,
            github_proxies: default_github_proxies(),
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
        assert_eq!(loaded.scrollback_lines, 5000);
        assert_eq!(loaded.line_height, 1.2);
        assert_eq!(loaded.cursor_style, CursorStyleChoice::Beam);
        assert!(loaded.copy_on_select);
        assert!(loaded.middle_click_paste);
        assert!(loaded.ssh_auto_reconnect);
        assert!(loaded.notify_on_long_command);
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
