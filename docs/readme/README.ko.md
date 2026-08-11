# hapcli

Rust + eframe (egui) 기반의 크로스플랫폼 터미널 클라이언트입니다. 자체 개발 터미널 커널 `hapcli-terminal`을 사용하며 로컬 PTY, SSH, Telnet, 시리얼, SFTP 파일 전송을 지원합니다.

## 빌드 방법

### 요구 사항

- Rust 도구 체인 (edition 2024, Rust 1.85 이상 권장)
- macOS는 Xcode Command Line Tools 필요

### 빌드 및 실행

```sh
cargo run -p hapcli-egui-app
```

### Release 바이너리 빌드

```sh
cargo build --release -p hapcli-egui-app
```

### 테스트 실행

```sh
cargo test -p hapcli-egui-app
```

### macOS .app 패키징

```sh
./scripts/package_macos.sh
```

결과물은 `target/hapcli.app` (아이콘 및 ad-hoc 서명 포함, 더블 클릭으로 실행).

### 디렉터리 구조

- `crates/hapcli-terminal`: 터미널 커널 (PTY / SSH / Telnet / 시리얼 / ANSI 파싱 / 스냅샷)
- `crates/hapcli-egui-app`: 데스크톱 앱 (egui 인터페이스, 현재 메인 프로그램)
- `crates/hapcli-sftp`: SFTP 전송 레이어
- `crates/hapcli-ssh`: SSH 전송 레이어
- `scripts/`: 패키징 및 보조 스크립트
