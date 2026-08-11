# hapcli

Ứng dụng terminal đa nền tảng dựa trên Rust + eframe (egui), sử dụng lõi terminal tự phát triển `hapcli-terminal`, hỗ trợ PTY cục bộ, SSH, Telnet, cổng nối tiếp và truyền tệp SFTP.

## Hướng dẫn biên dịch

### Yêu cầu

- Bộ công cụ Rust (edition 2024, khuyến nghị Rust 1.85 trở lên)
- macOS: cần Xcode Command Line Tools

### Biên dịch và chạy

```sh
cargo run -p hapcli-egui-app
```

### Biên dịch bản Release

```sh
cargo build --release -p hapcli-egui-app
```

### Chạy kiểm thử

```sh
cargo test -p hapcli-egui-app
```

### Đóng gói .app trên macOS

```sh
./scripts/package_macos.sh
```

Sản phẩm tạo ra là `target/hapcli.app` (có biểu tượng và chữ ký ad-hoc, nhấp đúp để chạy).

### Cấu trúc thư mục

- `crates/hapcli-terminal` : lõi terminal (PTY / SSH / Telnet / cổng nối tiếp / phân tích ANSI / snapshot)
- `crates/hapcli-egui-app` : ứng dụng desktop (giao diện egui, chương trình chính hiện tại)
- `crates/hapcli-sftp` : lớp truyền tệp SFTP
- `crates/hapcli-ssh` : lớp truyền tải SSH
- `scripts/` : tập lệnh đóng gói và tiện ích
