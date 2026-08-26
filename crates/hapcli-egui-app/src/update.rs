//! 新版本检测与自动更新：
//! 检查 GitHub Releases → 弹窗提示 → 应用内下载（进度条）→ 解压 → 替换安装 → 重启。

use std::cmp::Ordering;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
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
/// 下载分块大小（用于刷新进度条）。
const DOWNLOAD_CHUNK: usize = 256 * 1024;
/// 下载总体超时。
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);

/// GitHub Release 资产（下载安装包用）。
#[derive(Debug, Clone)]
pub struct UpdateAsset {
    pub name: String,
    pub browser_download_url: String,
}

/// 当前更新状态（GUI 展示与弹窗判断用）。
#[derive(Debug, Clone)]
pub enum UpdateStatus {
    Idle,
    Checking,
    UpToDate,
    UpdateAvailable {
        version: String,
        name: String,
        notes: String,
        assets: Vec<UpdateAsset>,
    },
    Downloading {
        version: String,
        transferred: u64,
        total: u64,
        speed_bps: f64,
    },
    DownloadFailed {
        version: String,
        message: String,
    },
    /// 新版本已就位、替换脚本已启动，等待本应用退出并重启。
    ReadyToInstall,
    Error(String),
}

enum UpdateCheckEvent {
    Result(UpdateStatus),
    Download(DownloadEvent),
}

enum DownloadEvent {
    Progress {
        transferred: u64,
        total: u64,
        speed_bps: f64,
    },
    Finished,
    Failed(String),
}

/// 更新检查与下载状态机：持有后台线程的结果通道。
pub struct UpdateCheckState {
    tx: Sender<UpdateCheckEvent>,
    rx: Receiver<UpdateCheckEvent>,
    pub status: UpdateStatus,
    /// 弹窗内“更新了什么”是否展开。
    pub show_notes: bool,
    /// 本次运行内已“稍后再说”，不再重复弹出。
    pub dismissed: bool,
    pub last_check_at: Option<Instant>,
    /// 下载取消标记。
    cancel_download: Arc<AtomicBool>,
    /// 最近一次可更新的版本与资产（下载失败后“重试”用）。
    last_version: Option<String>,
    last_assets: Vec<UpdateAsset>,
    /// GitHub 代理前缀列表（更新被墙时自动回退 / 测速选择最快节点）。
    proxies: Vec<String>,
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
            cancel_download: Arc::new(AtomicBool::new(false)),
            last_version: None,
            last_assets: Vec::new(),
            proxies: Vec::new(),
        }
    }
}

impl UpdateCheckState {
    /// 设置代理前缀列表（来自设置里的“GitHub 代理”）。
    pub fn set_proxies(&mut self, proxies: &[String]) {
        self.proxies = proxies.to_vec();
    }

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
        let proxies = self.proxies.clone();
        thread::spawn(move || {
            if !delay.is_zero() {
                thread::sleep(delay);
            }
            let status = fetch_latest_release(&current_version, &proxies);
            let _ = tx.send(UpdateCheckEvent::Result(status));
        });
    }

    /// 开始下载并安装新版本。
    pub fn start_download(&mut self, version: &str, assets: &[UpdateAsset]) {
        let Some(asset) = pick_platform_asset(assets) else {
            self.status = UpdateStatus::DownloadFailed {
                version: version.to_string(),
                message: "当前平台没有可用的自动更新安装包，请打开下载页手动安装".to_string(),
            };
            return;
        };
        self.last_version = Some(version.to_string());
        self.last_assets = assets.to_vec();
        self.cancel_download.store(false, AtomicOrdering::Relaxed);
        self.dismissed = false;
        self.status = UpdateStatus::Downloading {
            version: version.to_string(),
            transferred: 0,
            total: 0,
            speed_bps: 0.0,
        };
        let tx = self.tx.clone();
        let cancel = self.cancel_download.clone();
        let proxies = self.proxies.clone();
        thread::spawn(move || {
            match download_and_stage(&asset, &cancel, &tx, &proxies) {
                Ok(()) => {
                    let _ = tx.send(UpdateCheckEvent::Download(DownloadEvent::Finished));
                }
                Err(message) => {
                    let _ = tx.send(UpdateCheckEvent::Download(DownloadEvent::Failed(message)));
                }
            }
        });
    }

    /// 取消进行中的下载。
    pub fn cancel_download_now(&mut self) {
        self.cancel_download.store(true, AtomicOrdering::Relaxed);
    }

    /// 下载失败后使用相同的版本与资产重试。
    pub fn retry_download(&mut self) {
        let Some(version) = self.last_version.clone() else {
            return;
        };
        let assets = self.last_assets.clone();
        self.start_download(&version, &assets);
    }

    /// 每帧轮询后台结果。
    pub fn poll(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                UpdateCheckEvent::Result(status) => {
                    self.status = status;
                    self.last_check_at = Some(Instant::now());
                }
                UpdateCheckEvent::Download(DownloadEvent::Progress {
                    transferred,
                    total,
                    speed_bps,
                }) => {
                    if let UpdateStatus::Downloading { version, .. } = &self.status {
                        self.status = UpdateStatus::Downloading {
                            version: version.clone(),
                            transferred,
                            total,
                            speed_bps,
                        };
                    }
                }
                UpdateCheckEvent::Download(DownloadEvent::Finished) => {
                    self.status = UpdateStatus::ReadyToInstall;
                }
                UpdateCheckEvent::Download(DownloadEvent::Failed(message)) => {
                    let version = self.last_version.clone().unwrap_or_default();
                    self.status = UpdateStatus::DownloadFailed { version, message };
                }
            }
        }
    }

    /// 到点（6 小时）且用户开启自动检查时，触发新一轮检查。
    pub fn maybe_periodic_check(&mut self, current_version: &str, enabled: bool) {
        if !enabled || matches!(self.status, UpdateStatus::Downloading { .. }) {
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
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

fn fetch_latest_release(current_version: &str, proxies: &[String]) -> UpdateStatus {
    let client = match reqwest::blocking::Client::builder()
        .user_agent(format!("hapcli-updater/{current_version}"))
        .timeout(Duration::from_secs(20))
        .build()
    {
        Ok(client) => client,
        Err(error) => return UpdateStatus::Error(format!("HTTP 客户端创建失败: {error}")),
    };
    let mut last_error = String::new();
    // 直连优先；失败时依次尝试代理。
    let mut candidates = vec![UPDATE_API_URL.to_string()];
    candidates.extend(proxies.iter().map(|prefix| proxied(UPDATE_API_URL, prefix)));
    for url in candidates {
        match fetch_release_once(&client, &url, current_version) {
            Ok(Some(status)) => return status,
            Ok(None) => return UpdateStatus::UpToDate,
            Err(error) => last_error = error,
        }
    }
    UpdateStatus::Error(format!("无法连接更新服务器（直连与代理均失败）: {last_error}"))
}

/// 请求单个 release 端点；Ok(None) 表示“无新版本”，Err 表示该端点不可用。
fn fetch_release_once(
    client: &reqwest::blocking::Client,
    url: &str,
    current_version: &str,
) -> Result<Option<UpdateStatus>, String> {
    let response = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|error| error.to_string())?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        // 仓库还没有任何 Release。
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    let release: GithubRelease = response
        .json()
        .map_err(|error| format!("解析失败: {error}"))?;
    if release.draft || release.prerelease {
        return Ok(None);
    }
    let version = release.tag_name.trim_start_matches('v').to_string();
    if !is_update_newer(&version, current_version) {
        return Ok(None);
    }
    Ok(Some(UpdateStatus::UpdateAvailable {
        name: release.name.unwrap_or_else(|| release.tag_name.clone()),
        version,
        notes: release.body.unwrap_or_default(),
        assets: release
            .assets
            .into_iter()
            .map(|asset| UpdateAsset {
                name: asset.name,
                browser_download_url: asset.browser_download_url,
            })
            .collect(),
    }))
}

/// 当前平台对应的安装包文件名。
fn platform_asset_name() -> Option<&'static str> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        Some("hapcli-macos-arm64.zip")
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        Some("hapcli-windows-x86_64.zip")
    }
    #[cfg(not(any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "windows", target_arch = "x86_64")
    )))]
    {
        None
    }
}

fn pick_platform_asset(assets: &[UpdateAsset]) -> Option<UpdateAsset> {
    let expected = platform_asset_name()?;
    assets
        .iter()
        .find(|asset| asset.name == expected)
        .cloned()
}

/// 安装目标：macOS 打包的 .app 整体替换，或裸二进制直接替换。
#[derive(Debug, Clone, PartialEq, Eq)]
enum InstallTarget {
    AppBundle(PathBuf),
    Executable(PathBuf),
}

/// 从当前可执行文件路径推断安装目标。
fn detect_install_target(exe: &Path) -> InstallTarget {
    if let Some(bundle) = macos_bundle_root(exe) {
        return InstallTarget::AppBundle(bundle);
    }
    InstallTarget::Executable(exe.to_path_buf())
}

/// 若可执行文件位于 …/xxx.app/Contents/MacOS/ 内，返回 xxx.app 路径。
fn macos_bundle_root(exe: &Path) -> Option<PathBuf> {
    let mac_os = exe.parent()?;
    if mac_os.file_name().and_then(|name| name.to_str()) != Some("MacOS") {
        return None;
    }
    let contents = mac_os.parent()?;
    if contents.file_name().and_then(|name| name.to_str()) != Some("Contents") {
        return None;
    }
    let bundle = contents.parent()?;
    if bundle.extension().and_then(|ext| ext.to_str()) != Some("app") {
        return None;
    }
    Some(bundle.to_path_buf())
}

/// 下载安装包、解压、生成替换脚本并启动；全部完成后由脚本负责替换与重启。
fn download_and_stage(
    asset: &UpdateAsset,
    cancel: &AtomicBool,
    tx: &Sender<UpdateCheckEvent>,
    proxies: &[String],
) -> Result<(), String> {
    let current_exe = std::env::current_exe().map_err(|error| format!("无法定位当前程序: {error}"))?;
    let target = detect_install_target(&current_exe);
    // 安装目录不可写（如 /Applications 需要管理员权限）时提前失败，
    // 避免替换脚本静默失败。
    let target_dir = match &target {
        InstallTarget::AppBundle(bundle) => bundle.parent(),
        InstallTarget::Executable(exe) => exe.parent(),
    };
    if !target_dir.is_some_and(parent_dir_writable) {
        return Err(
            "安装目录没有写入权限（例如安装到了 /Applications），请改用下载页手动安装".to_string(),
        );
    }
    let staging = staging_dir();
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|error| format!("无法创建临时目录: {error}"))?;

    // 1. 下载 zip。
    let zip_path = staging.join(&asset.name);
    let client = reqwest::blocking::Client::builder()
        .user_agent(format!("hapcli-updater/{}", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(20))
        .timeout(DOWNLOAD_TIMEOUT)
        .build()
        .map_err(|error| format!("HTTP 客户端创建失败: {error}"))?;
    // 直连下载；失败时自动挑选最快的代理重试（国内网络常见 github.com 被墙）。
    let download_result =
        download_to(&client, &asset.browser_download_url, &zip_path, cancel, tx);
    if download_result.is_err() && !proxies.is_empty() {
        if let Some(prefix) = fastest_proxy(&client, &asset.browser_download_url, proxies) {
            let proxied_url = proxied(&asset.browser_download_url, &prefix);
            download_to(&client, &proxied_url, &zip_path, cancel, tx)?;
        } else {
            download_result?;
        }
    } else {
        download_result?;
    }

    // 2. 解压。
    extract_archive(&zip_path, &staging)?;

    // 3. 定位新程序。
    let payload = locate_payload(&staging)?;

    // 4. 生成并启动替换脚本（脱离本进程，等本应用退出后替换并重启）。
    write_and_launch_helper(&staging, &target, &payload, std::process::id())?;
    Ok(())
}

/// 把安装包下载到指定路径（带进度与取消支持）。
fn download_to(
    client: &reqwest::blocking::Client,
    url: &str,
    zip_path: &Path,
    cancel: &AtomicBool,
    tx: &Sender<UpdateCheckEvent>,
) -> Result<(), String> {
    let mut response = client
        .get(url)
        .send()
        .map_err(|error| format!("下载失败: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("下载失败：服务器返回 {}", response.status()));
    }
    let total = response.content_length().unwrap_or(0);
    let mut file = std::fs::File::create(zip_path)
        .map_err(|error| format!("无法写入临时文件: {error}"))?;
    let mut transferred: u64 = 0;
    let mut buffer = vec![0u8; DOWNLOAD_CHUNK];
    let started = Instant::now();
    let mut last_send = Instant::now();
    loop {
        if cancel.load(AtomicOrdering::Relaxed) {
            return Err("下载已取消".to_string());
        }
        let read = response
            .read(&mut buffer)
            .map_err(|error| format!("下载中断: {error}"))?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .map_err(|error| format!("写入临时文件失败: {error}"))?;
        transferred += read as u64;
        if last_send.elapsed() >= Duration::from_millis(80) {
            let speed_bps = transferred as f64 / started.elapsed().as_secs_f64().max(0.001);
            let _ = tx.send(UpdateCheckEvent::Download(DownloadEvent::Progress {
                transferred,
                total,
                speed_bps,
            }));
            last_send = Instant::now();
        }
    }
    file.flush().map_err(|error| format!("写入临时文件失败: {error}"))?;
    drop(file);
    if total > 0 && transferred != total {
        return Err(format!("下载不完整（{transferred}/{total} 字节）"));
    }
    if cancel.load(AtomicOrdering::Relaxed) {
        return Err("下载已取消".to_string());
    }
    Ok(())
}

/// 解析设置里的代理前缀列表（逗号分隔）。
pub fn parse_proxy_prefixes(list: &str) -> Vec<String> {
    list.split(',')
        .map(|item| item.trim().trim_end_matches('/'))
        .filter(|item| !item.is_empty())
        .map(|item| {
            if item.starts_with("http://") || item.starts_with("https://") {
                format!("{item}/")
            } else {
                // 只填域名时自动补 https://。
                format!("https://{item}/")
            }
        })
        .collect()
}

/// 通过代理前缀访问原始 GitHub 链接（格式：代理域名 + / + 完整 URL）。
fn proxied(url: &str, prefix: &str) -> String {
    format!("{prefix}{url}")
}

/// 对候选代理做 HEAD 测速，返回最快的前缀（全部失败返回 None）。
fn fastest_proxy(
    client: &reqwest::blocking::Client,
    url: &str,
    prefixes: &[String],
) -> Option<String> {
    let mut best: Option<(Duration, String)> = None;
    for prefix in prefixes {
        let started = Instant::now();
        let ok = client
            .head(proxied(url, prefix))
            .timeout(Duration::from_secs(5))
            .send()
            .map(|response| response.status().is_success())
            .unwrap_or(false);
        if ok {
            let elapsed = started.elapsed();
            if best.as_ref().is_none_or(|(best_elapsed, _)| elapsed < *best_elapsed) {
                best = Some((elapsed, prefix.clone()));
            }
        }
    }
    best.map(|(_, prefix)| prefix)
}

/// 临时目录：系统临时目录下的固定命名目录（脚本结束后自行清理）。
fn staging_dir() -> PathBuf {
    std::env::temp_dir().join(format!("hapcli-update-{}", std::process::id()))
}

/// 探测目录是否可写（创建并删除一个临时文件）。
fn parent_dir_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".hapcli-write-test-{}", std::process::id()));
    match std::fs::File::create(&probe) {
        Ok(file) => {
            drop(file);
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

fn extract_archive(zip_path: &Path, staging: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        // ditto 保留 .app 内权限与符号链接。
        let output = Command::new("ditto")
            .args(["-x", "-k"])
            .arg(zip_path)
            .arg(staging)
            .output()
            .map_err(|error| format!("解压失败: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "解压失败：{}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
    }
    #[cfg(target_os = "windows")]
    {
        let script = format!(
            "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
            zip_path.display(),
            staging.display()
        );
        let output = Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .output()
            .map_err(|error| format!("解压失败: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "解压失败：{}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (zip_path, staging);
        return Err("当前平台暂不支持自动更新".to_string());
    }
    Ok(())
}

/// 在解压目录中定位新版本程序：
/// macOS 为 hapcli.app/Contents/MacOS/hapcli，Windows 为 hapcli.exe。
fn locate_payload(staging: &Path) -> Result<PathBuf, String> {
    #[cfg(target_os = "macos")]
    {
        let app = staging.join("hapcli.app");
        let exe = app.join("Contents/MacOS/hapcli");
        if exe.is_file() {
            return Ok(exe);
        }
        // 兼容 zip 内多一层目录的情况。
        let Ok(entries) = std::fs::read_dir(staging) else {
            return Err("解压后未找到 hapcli.app".to_string());
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.extension().and_then(|ext| ext.to_str()) == Some("app") {
                let nested = path.join("Contents/MacOS/hapcli");
                if nested.is_file() {
                    return Ok(nested);
                }
            }
        }
        Err("解压后未找到 hapcli.app".to_string())
    }
    #[cfg(target_os = "windows")]
    {
        let exe = staging.join("hapcli.exe");
        if exe.is_file() {
            return Ok(exe);
        }
        Err("解压后未找到 hapcli.exe".to_string())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = staging;
        Err("当前平台暂不支持自动更新".to_string())
    }
}

/// 把路径转成 shell 单引号形式（macOS）。
fn sh_quote(path: &str) -> String {
    format!("'{}'", path.replace('\'', "'\\''"))
}

/// 把路径转成 Windows 批处理可用的双引号形式。
#[cfg(target_os = "windows")]
fn bat_quote(path: &str) -> String {
    format!("\"{}\"", path.replace('"', "\"\""))
}

/// 生成替换脚本并脱离启动；脚本等待本进程退出后替换文件并重新启动。
fn write_and_launch_helper(
    staging: &Path,
    target: &InstallTarget,
    payload: &Path,
    pid: u32,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let script_path = staging.join("hapcli-update.sh");
        let script = match target {
            InstallTarget::AppBundle(bundle) => {
                // 整包替换必须用新的 .app 目录（payload 是可执行文件路径）。
                // 找不到 .app 目录说明安装包结构异常：取消更新，绝不把可执行
                // 文件当作整包源（曾因此把 .app 替换成普通文件导致应用损坏）。
                let Some(payload_bundle) = macos_bundle_root(payload) else {
                    return Err("安装包结构异常（未找到 .app 目录），已取消更新".to_string());
                };
                let new_bundle = PathBuf::from(format!("{}.new", bundle.display()));
                let (bundle_q, new_bundle_q, payload_bundle_q, staging_q) = (
                    sh_quote(&bundle.to_string_lossy()),
                    sh_quote(&new_bundle.to_string_lossy()),
                    sh_quote(&payload_bundle.to_string_lossy()),
                    sh_quote(&staging.to_string_lossy()),
                );
                let log_q = sh_quote("/tmp/hapcli-update.log");
                format!(
                    "#!/bin/sh\n\
                     LOG={log_q}\n\
                     {{\n\
                     \x20 echo \"== hapcli update $(date) ==\"\n\
                     \x20 PID={pid}\n\
                     \x20 i=0\n\
                     \x20 while kill -0 \"$PID\" 2>/dev/null; do\n\
                     \x20\x20 i=$((i+1))\n\
                     \x20\x20 [ \"$i\" -ge 120 ] && break\n\
                     \x20\x20 sleep 0.3\n\
                     \x20 done\n\
                     \x20 sleep 1\n\
                     \x20 rm -rf {new_bundle_q}\n\
                     \x20 ditto {payload_bundle_q} {new_bundle_q} || {{ echo \"ditto failed\"; exit 1; }}\n\
                     \x20 test -x {new_bundle_q}/Contents/MacOS/hapcli || {{ echo \"verify failed\"; rm -rf {new_bundle_q}; exit 1; }}\n\
                     \x20 rm -rf {bundle_q}\n\
                     \x20 mv {new_bundle_q} {bundle_q} || {{ echo \"mv failed\"; exit 1; }}\n\
                     \x20 xattr -dr com.apple.quarantine {bundle_q} 2>/dev/null || true\n\
                     \x20 if ! open -n {bundle_q} 2>/dev/null; then\n\
                     \x20\x20 sleep 1\n\
                     \x20\x20 open -n {bundle_q} 2>/dev/null || true\n\
                     \x20 fi\n\
                     \x20 rm -rf {staging_q}\n\
                     \x20 echo \"done\"\n\
                     }} >> \"$LOG\" 2>&1\n",
                )
            }
            InstallTarget::Executable(exe) => {
                let new_exe = PathBuf::from(format!("{}.new", exe.display()));
                let (exe_q, new_exe_q, payload_q, staging_q) = (
                    sh_quote(&exe.to_string_lossy()),
                    sh_quote(&new_exe.to_string_lossy()),
                    sh_quote(&payload.to_string_lossy()),
                    sh_quote(&staging.to_string_lossy()),
                );
                format!(
                    "#!/bin/sh\n\
                     PID={pid}\n\
                     i=0\n\
                     while kill -0 \"$PID\" 2>/dev/null; do\n\
                     \x20 i=$((i+1))\n\
                     \x20 [ \"$i\" -ge 120 ] && break\n\
                     \x20 sleep 0.3\n\
                     done\n\
                     sleep 1\n\
                     rm -f {new_exe_q}\n\
                     cp {payload_q} {new_exe_q}\n\
                     chmod +x {new_exe_q}\n\
                     mv {new_exe_q} {exe_q}\n\
                     {exe_q}\n\
                     rm -rf {staging_q}\n",
                )
            }
        };
        std::fs::write(&script_path, script).map_err(|error| format!("写入更新脚本失败: {error}"))?;
        let output = Command::new("nohup")
            .args(["sh", script_path.to_str().unwrap_or_default()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|error| format!("启动更新脚本失败: {error}"))?;
        let _ = output;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        let script_path = staging.join("hapcli-update.bat");
        let (target_exe, payload_exe) = match target {
            InstallTarget::Executable(exe) => (exe.to_path_buf(), payload.to_path_buf()),
            InstallTarget::AppBundle(_) => {
                return Err("Windows 不支持 .app 安装目标".to_string());
            }
        };
        let script = format!(
            "@echo off\r\n\
             timeout /t 2 /nobreak >nul\r\n\
             :wait\r\n\
             tasklist /FI \"PID eq {pid}\" | findstr \"{pid}\" >nul\r\n\
             if not errorlevel 1 (\r\n\
             \x20 timeout /t 1 /nobreak >nul\r\n\
             \x20 goto wait\r\n\
             )\r\n\
             timeout /t 1 /nobreak >nul\r\n\
             copy /y {} {} >nul\r\n\
             start \"\" {}\r\n\
             rmdir /s /q {}\r\n",
            bat_quote(&payload_exe.to_string_lossy()),
            bat_quote(&target_exe.to_string_lossy()),
            bat_quote(&target_exe.to_string_lossy()),
            bat_quote(&staging.to_string_lossy()),
        );
        std::fs::write(&script_path, script).map_err(|error| format!("写入更新脚本失败: {error}"))?;
        use std::os::windows::process::CommandExt;
        let _ = Command::new("cmd")
            .args(["/C", "start", "", "/b", script_path.to_str().unwrap_or_default()])
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
            .spawn()
            .map_err(|error| format!("启动更新脚本失败: {error}"))?;
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (staging, target, payload, pid);
        Err("当前平台暂不支持自动更新".to_string())
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

/// 用系统默认浏览器打开链接（下载失败时的兜底方案）。
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
            "draft": false,
            "assets": [
                {
                    "name": "hapcli-macos-arm64.zip",
                    "browser_download_url": "https://github.com/dardiao/hapcli/releases/download/v2.0.24/hapcli-macos-arm64.zip"
                }
            ]
        }"#;
        let release: GithubRelease = serde_json::from_str(json).unwrap();
        assert_eq!(release.tag_name, "v2.0.24");
        assert_eq!(release.assets.len(), 1);
        assert_eq!(release.assets[0].name, "hapcli-macos-arm64.zip");
    }

    #[test]
    fn platform_asset_picks_matching_name() {
        let assets = vec![
            UpdateAsset {
                name: "hapcli-windows-x86_64.zip".to_string(),
                browser_download_url: "https://example.invalid/win.zip".to_string(),
            },
            UpdateAsset {
                name: "hapcli-macos-arm64.zip".to_string(),
                browser_download_url: "https://example.invalid/mac.zip".to_string(),
            },
        ];
        if let Some(expected) = platform_asset_name() {
            let picked = pick_platform_asset(&assets).expect("asset should match");
            assert_eq!(picked.name, expected);
        } else {
            assert!(pick_platform_asset(&assets).is_none());
        }
    }

    #[test]
    fn macos_bundle_root_detects_app_bundle() {
        let exe = Path::new("/Applications/hapcli.app/Contents/MacOS/hapcli");
        assert_eq!(
            macos_bundle_root(exe),
            Some(PathBuf::from("/Applications/hapcli.app"))
        );
        let bare = Path::new("/usr/local/bin/hapcli");
        assert_eq!(macos_bundle_root(bare), None);
    }

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(sh_quote("/a b/c"), "'/a b/c'");
        assert_eq!(sh_quote("/it's"), "'/it'\\''s'");
    }

    #[test]
    fn app_bundle_payload_resolves_to_bundle_dir() {
        let payload = Path::new("/tmp/hapcli-update-123/hapcli.app/Contents/MacOS/hapcli");
        assert_eq!(
            macos_bundle_root(payload),
            Some(PathBuf::from("/tmp/hapcli-update-123/hapcli.app"))
        );
    }

    #[test]
    fn proxy_prefixes_are_parsed_and_normalized() {
        let list = "https://gh.dpik.top/, ghfast.top , ,http://localhost:8080,not-a-url";
        let proxies = parse_proxy_prefixes(list);
        assert_eq!(
            proxies,
            vec![
                "https://gh.dpik.top/".to_string(),
                "https://ghfast.top/".to_string(),
                "http://localhost:8080/".to_string(),
                "https://not-a-url/".to_string(),
            ]
        );
    }

    #[test]
    fn proxied_url_keeps_full_github_url() {
        let url = "https://github.com/dardiao/hapcli/releases/download/v3.0.0/hapcli-macos-arm64.zip";
        assert_eq!(
            proxied(url, "https://gh.dpik.top/"),
            "https://gh.dpik.top/https://github.com/dardiao/hapcli/releases/download/v3.0.0/hapcli-macos-arm64.zip"
        );
    }
}
