# hapcli

基於 Rust + eframe (egui) 的跨平台終端用戶端，自研終端核心 `hapcli-terminal`，支援本機 PTY、SSH、Telnet、序列埠與 SFTP 檔案傳輸。

## 編譯說明

### 環境需求

- Rust 工具鏈（edition 2024，建議 Rust 1.85 以上穩定版）
- macOS 需安裝 Xcode Command Line Tools

### 編譯並執行

```sh
cargo run -p hapcli-egui-app
```

### 建置 Release 二進位檔

```sh
cargo build --release -p hapcli-egui-app
```

### 執行測試

```sh
cargo test -p hapcli-egui-app
```

### macOS 打包 .app

```sh
./scripts/package_macos.sh
```

打包產物為 `target/hapcli.app`（含圖示與 ad-hoc 簽名，雙擊即可執行）。

### 目錄結構

- `crates/hapcli-terminal`：終端核心（PTY / SSH / Telnet / 序列埠 / ANSI 解析 / 快照）
- `crates/hapcli-egui-app`：桌面應用（egui 介面，目前主程式）
- `crates/hapcli-sftp`：SFTP 傳輸層
- `crates/hapcli-ssh`：SSH 傳輸層
- `scripts/`：打包與輔助腳本
