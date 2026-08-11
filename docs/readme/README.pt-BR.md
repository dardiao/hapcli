# hapcli

Cliente de terminal multiplataforma baseado em Rust + eframe (egui), com núcleo próprio `hapcli-terminal`: PTY local, SSH, Telnet, serial e transferência de arquivos SFTP.

## Compilação

### Requisitos

- Toolchain Rust (edition 2024, recomendado Rust 1.85 ou superior)
- macOS: requer Xcode Command Line Tools

### Compilar e executar

```sh
cargo run -p hapcli-egui-app
```

### Compilar o binário Release

```sh
cargo build --release -p hapcli-egui-app
```

### Executar os testes

```sh
cargo test -p hapcli-egui-app
```

### Empacotar o aplicativo macOS (.app)

```sh
./scripts/package_macos.sh
```

O resultado é `target/hapcli.app` (com ícone e assinatura ad-hoc, abre com duplo clique).

### Estrutura do projeto

- `crates/hapcli-terminal` : núcleo do terminal (PTY / SSH / Telnet / serial / parsing ANSI / snapshots)
- `crates/hapcli-egui-app` : aplicativo de desktop (interface egui, programa principal atual)
- `crates/hapcli-sftp` : camada de transferência SFTP
- `crates/hapcli-ssh` : camada de transporte SSH
- `scripts/` : scripts de empacotamento e utilitários
