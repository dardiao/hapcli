# hapcli

Client terminal multiplateforme basé sur Rust + eframe (egui), avec son propre noyau terminal `hapcli-terminal` : PTY local, SSH, Telnet, série et transfert de fichiers SFTP.

## Compilation

### Prérequis

- Chaîne d'outils Rust (édition 2024, Rust 1.85 ou plus recommandé)
- macOS : Xcode Command Line Tools requis

### Compiler et exécuter

```sh
cargo run -p hapcli-egui-app
```

### Compiler la binaire Release

```sh
cargo build --release -p hapcli-egui-app
```

### Exécuter les tests

```sh
cargo test -p hapcli-egui-app
```

### Empaqueter l'application macOS (.app)

```sh
./scripts/package_macos.sh
```

Le résultat est `target/hapcli.app` (avec icône et signature ad-hoc, lancement en double-clic).

### Structure du projet

- `crates/hapcli-terminal` : noyau terminal (PTY / SSH / Telnet / série / analyse ANSI / instantanés)
- `crates/hapcli-egui-app` : application de bureau (interface egui, programme principal actuel)
- `crates/hapcli-sftp` : couche de transfert SFTP
- `crates/hapcli-ssh` : couche de transport SSH
- `scripts/` : scripts d'empaquetage et utilitaires
