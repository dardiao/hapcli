# hapcli

Plattformübergreifender Terminal-Client auf Basis von Rust + eframe (egui) mit eigenem Terminal-Kern `hapcli-terminal`. Unterstützt lokale PTYs, SSH, Telnet, serielle Schnittstellen und SFTP-Dateiübertragung.

## Kompilieren

### Voraussetzungen

- Rust-Toolchain (Edition 2024, Rust 1.85 oder neuer empfohlen)
- macOS: Xcode Command Line Tools erforderlich

### Kompilieren und ausführen

```sh
cargo run -p hapcli-egui-app
```

### Release-Binary bauen

```sh
cargo build --release -p hapcli-egui-app
```

### Tests ausführen

```sh
cargo test -p hapcli-egui-app
```

### macOS-`.app`-Paket erstellen

```sh
./scripts/package_macos.sh
```

Das Ergebnis liegt unter `target/hapcli.app` (mit Icon und Ad-hoc-Signatur, per Doppelklick startbar).

### Verzeichnisstruktur

- `crates/hapcli-terminal` : Terminal-Kern (PTY / SSH / Telnet / seriell / ANSI-Parsing / Snapshots)
- `crates/hapcli-egui-app` : Desktop-Anwendung (egui-Oberfläche, aktuelles Hauptprogramm)
- `crates/hapcli-sftp` : SFTP-Übertragungsschicht
- `crates/hapcli-ssh` : SSH-Transportschicht
- `scripts/` : Paketierungs- und Hilfsskripte
