# hapcli

Client terminale multipiattaforma basato su Rust + eframe (egui), con kernel terminale proprietario `hapcli-terminal`: PTY locale, SSH, Telnet, seriale e trasferimento file SFTP.

## Compilazione

### Requisiti

- Toolchain Rust (edition 2024, consigliato Rust 1.85 o superiore)
- macOS: richiesto Xcode Command Line Tools

### Compilare ed eseguire

```sh
cargo run -p hapcli-egui-app
```

### Compilare il binario Release

```sh
cargo build --release -p hapcli-egui-app
```

### Eseguire i test

```sh
cargo test -p hapcli-egui-app
```

### Creare il pacchetto macOS (.app)

```sh
./scripts/package_macos.sh
```

Il risultato è `target/hapcli.app` (con icona e firma ad-hoc, avviabile con doppio clic).

### Struttura del progetto

- `crates/hapcli-terminal` : kernel terminale (PTY / SSH / Telnet / seriale / parsing ANSI / snapshot)
- `crates/hapcli-egui-app` : applicazione desktop (interfaccia egui, programma principale attuale)
- `crates/hapcli-sftp` : livello di trasferimento SFTP
- `crates/hapcli-ssh` : livello di trasporto SSH
- `scripts/` : script di impacchettamento e utilità
