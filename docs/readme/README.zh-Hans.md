# hapcli

基于 Rust + eframe (egui) 的跨平台终端客户端，自研终端内核 `hapcli-terminal`，支持本地 PTY、SSH、Telnet、串口与 SFTP 文件传输。

## 编译说明

### 环境要求

- Rust 工具链（edition 2024，建议 Rust 1.85 及以上稳定版）
- macOS 需要安装 Xcode Command Line Tools

### 编译并运行

```sh
cargo run -p hapcli-egui-app
```

### 构建 Release 二进制

```sh
cargo build --release -p hapcli-egui-app
```

### 运行测试

```sh
cargo test -p hapcli-egui-app
```

### macOS 打包 .app

```sh
./scripts/package_macos.sh
```

打包产物为 `target/hapcli.app`（带图标与 ad-hoc 签名，双击即可运行）。

### 目录结构

- `crates/hapcli-terminal`：终端内核（PTY / SSH / Telnet / 串口 / ANSI 解析 / 快照）
- `crates/hapcli-egui-app`：桌面应用（egui 界面，当前主程序）
- `crates/hapcli-sftp`：SFTP 传输层
- `crates/hapcli-ssh`：SSH 传输层
- `scripts/`：打包与辅助脚本
