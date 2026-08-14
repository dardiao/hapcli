//! 新版本检测：后台线程查询 GitHub Releases API，GUI 弹出升级提示。

use std::cmp::Ordering;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;

/// 应用发布仓库（公开仓库，匿名 API 可直接访问）。
const UPDATE_API_URL: &str = "https://api.github.com/repos/dardiao/hapcli/releases/latest";
/// 自动检查间隔。
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// 启动后的首次检查延迟（等窗口先显示出来）。
const INITIAL_DELAY: Duration = Duration::from_secs(4);

/// 当前更新检查状态（GUI 展示与弹窗判断用）。
#[derive(Debug, Clone)]
pub enum UpdateStatus {
    Idle,
    Checking,
    UpToDate,
    UpdateAvailable {
        version: String,
        name: String,
        notes: String,
        html_url: String,
    },
    Error(String),
}

enum UpdateCheckEvent {
    Result(UpdateStatus),
}

/// 更新检查状态机：持有后台检查线程的结果通道。
pub struct UpdateCheckState {
    tx: Sender<UpdateCheckEvent>,
    rx: Receiver<UpdateCheckEvent>,
    pub status: UpdateStatus,
    /// 弹窗内“更新了什么”是否展开。
    pub show_notes: bool,
    /// 本次运行内已“稍后再说”，不再重复弹出。
    pub dismissed: bool,
    pub last_check_at: Option<Instant>,
}

impl Default for UpdateCheckState {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            tx,
            rx,
            status: UpdateStatus::Idle,
            show_notes: false,
            dismissed: false,
            last_check_at: None,
        }
    }
}

impl UpdateCheckState {
    /// 启动后台线程检查一次；delay 用于启动时等窗口先出现。
    pub fn check_now(&mut self, current_version: &str, delay: Duration) {
        if matches!(self.status, UpdateStatus::Checking) {
            return;
        }
        self.status = UpdateStatus::Checking;
        self.dismissed = false;
        self.show_notes = false;
        let tx = self.tx.clone();
        let current_version = current_version.to_string();
        thread::spawn(move || {
            if !delay.is_zero() {
                thread::sleep(delay);
            }
            let status = fetch_latest_release(&current_version);
            let _ = tx.send(UpdateCheckEvent::Result(status));
        });
    }

    /// 每帧轮询后台结果。
    pub fn poll(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                UpdateCheckEvent::Result(status) => {
                    self.status = status;
                    self.last_check_at = Some(Instant::now());
                }
            }
        }
    }

    /// 到点（6 小时）且用户开启自动检查时，触发新一轮检查。
    pub fn maybe_periodic_check(&mut self, current_version: &str, enabled: bool) {
        if !enabled {
            return;
        }
        let due = match self.last_check_at {
            None => true,
            Some(at) => at.elapsed() >= CHECK_INTERVAL,
        };
        if due {
            let delay = if self.last_check_at.is_none() {
                INITIAL_DELAY
            } else {
                Duration::ZERO
            };
            self.check_now(current_version, delay);
        }
    }
}

/// GitHub Releases API 的 latest release 字段（只取需要的部分）。
#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    body: Option<String>,
    html_url: String,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    draft: bool,
}

fn fetch_latest_release(current_version: &str) -> UpdateStatus {
    let client = match reqwest::blocking::Client::builder()
        .user_agent(format!("hapcli-updater/{current_version}"))
        .timeout(Duration::from_secs(20))
        .build()
    {
        Ok(client) => client,
        Err(error) => return UpdateStatus::Error(format!("HTTP 客户端创建失败: {error}")),
    };
    let response = match client
        .get(UPDATE_API_URL)
        .header("Accept", "application/vnd.github+json")
        .send()
    {
        Ok(response) => response,
        Err(error) => return UpdateStatus::Error(format!("无法连接更新服务器: {error}")),
    };
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        // 仓库还没有任何 Release。
        return UpdateStatus::UpToDate;
    }
    if !response.status().is_success() {
        return UpdateStatus::Error(format!("更新服务器返回 {}", response.status()));
    }
    let release: GithubRelease = match response.json() {
        Ok(release) => release,
        Err(error) => return UpdateStatus::Error(format!("更新信息解析失败: {error}")),
    };
    if release.draft || release.prerelease {
        return UpdateStatus::UpToDate;
    }
    let version = release.tag_name.trim_start_matches('v').to_string();
    if !is_update_newer(&version, current_version) {
        return UpdateStatus::UpToDate;
    }
    UpdateStatus::UpdateAvailable {
        name: release.name.unwrap_or_else(|| release.tag_name.clone()),
        version,
        notes: release.body.unwrap_or_default(),
        html_url: release.html_url,
    }
}

/// 比较两个版本号（忽略前导 v；预发布版本小于正式版）。
pub fn compare_versions(candidate: &str, current: &str) -> Ordering {
    let numeric = |version: &str| -> Vec<u64> {
        version
            .trim_start_matches('v')
            .split(['-', '+'])
            .next()
            .unwrap_or("")
            .split('.')
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    let candidate_parts = numeric(candidate);
    let current_parts = numeric(current);
    for index in 0..candidate_parts.len().max(current_parts.len()) {
        let left = candidate_parts.get(index).copied().unwrap_or(0);
        let right = current_parts.get(index).copied().unwrap_or(0);
        if left != right {
            return left.cmp(&right);
        }
    }
    let candidate_prerelease = candidate.contains('-');
    let current_prerelease = current.contains('-');
    match (candidate_prerelease, current_prerelease) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => Ordering::Equal,
    }
}

pub fn is_update_newer(candidate: &str, current: &str) -> bool {
    compare_versions(candidate, current) == Ordering::Greater
}

/// 用系统默认浏览器打开链接。
pub fn open_url(url: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .is_ok()
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .is_ok()
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .is_ok()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
    {
        let _ = url;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare_handles_numeric_segments() {
        assert_eq!(compare_versions("2.0.24", "2.0.23"), Ordering::Greater);
        assert_eq!(compare_versions("2.0.9", "2.0.10"), Ordering::Less);
        assert_eq!(compare_versions("2.1.0", "2.0.99"), Ordering::Greater);
        assert_eq!(compare_versions("2.0.23", "2.0.23"), Ordering::Equal);
        assert_eq!(compare_versions("v2.0.24", "2.0.23"), Ordering::Greater);
        assert_eq!(compare_versions("2.0.24-beta", "2.0.24"), Ordering::Less);
        assert_eq!(compare_versions("2.0.24", "2.0.24-beta"), Ordering::Greater);
    }

    #[test]
    fn update_newer_ignores_older_or_same() {
        assert!(is_update_newer("2.0.24", "2.0.23"));
        assert!(!is_update_newer("2.0.22", "2.0.23"));
        assert!(!is_update_newer("2.0.23", "2.0.23"));
    }

    #[test]
    fn parses_github_release_payload() {
        let json = r#"{
            "tag_name": "v2.0.24",
            "name": "hapcli 2.0.24",
            "body": "修复了若干问题",
            "html_url": "https://github.com/dardiao/hapcli/releases/tag/v2.0.24",
            "prerelease": false,
            "draft": false
        }"#;
        let release: GithubRelease = serde_json::from_str(json).unwrap();
        assert_eq!(release.tag_name, "v2.0.24");
        assert_eq!(release.name.as_deref(), Some("hapcli 2.0.24"));
        assert_eq!(release.body.as_deref(), Some("修复了若干问题"));
        assert!(!release.prerelease);
        assert!(!release.draft);
    }
}
