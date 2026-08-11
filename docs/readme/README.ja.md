# hapcli

Rust + eframe (egui) 製のクロスプラットフォーム端末クライアントです。独自の端末カーネル `hapcli-terminal` を採用し、ローカル PTY・SSH・Telnet・シリアル・SFTP ファイル転送に対応しています。

## ビルド手順

### 必要環境

- Rust ツールチェーン（edition 2024、Rust 1.85 以降の安定版を推奨）
- macOS では Xcode Command Line Tools が必要

### ビルドして実行

```sh
cargo run -p hapcli-egui-app
```

### Release バイナリをビルド

```sh
cargo build --release -p hapcli-egui-app
```

### テストを実行

```sh
cargo test -p hapcli-egui-app
```

### macOS で .app をパッケージング

```sh
./scripts/package_macos.sh
```

生成物は `target/hapcli.app`（アイコンと ad-hoc 署名付き、ダブルクリックで起動）。

### ディレクトリ構成

- `crates/hapcli-terminal`：端末カーネル（PTY / SSH / Telnet / シリアル / ANSI 解析 / スナップショット）
- `crates/hapcli-egui-app`：デスクトップアプリ（egui インターフェース、現在のメインプログラム）
- `crates/hapcli-sftp`：SFTP 転送レイヤー
- `crates/hapcli-ssh`：SSH 転送レイヤー
- `scripts/`：パッケージング・補助スクリプト
