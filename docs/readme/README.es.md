# hapcli

Cliente de terminal multiplataforma basado en Rust + eframe (egui), con núcleo propio `hapcli-terminal`: PTY local, SSH, Telnet, serie y transferencia de archivos SFTP.

## Compilación

### Requisitos

- Cadena de herramientas de Rust (edición 2024, se recomienda Rust 1.85 o superior)
- macOS: requiere Xcode Command Line Tools

### Compilar y ejecutar

```sh
cargo run -p hapcli-egui-app
```

### Compilar el binario Release

```sh
cargo build --release -p hapcli-egui-app
```

### Ejecutar las pruebas

```sh
cargo test -p hapcli-egui-app
```

### Empaquetar la aplicación macOS (.app)

```sh
./scripts/package_macos.sh
```

El resultado es `target/hapcli.app` (con icono y firma ad-hoc, se abre con doble clic).

### Estructura del proyecto

- `crates/hapcli-terminal` : núcleo del terminal (PTY / SSH / Telnet / serie / análisis ANSI / instantáneas)
- `crates/hapcli-egui-app` : aplicación de escritorio (interfaz egui, programa principal actual)
- `crates/hapcli-sftp` : capa de transferencia SFTP
- `crates/hapcli-ssh` : capa de transporte SSH
- `scripts/` : scripts de empaquetado y utilidades
