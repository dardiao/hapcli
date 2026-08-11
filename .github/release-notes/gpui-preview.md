# hapcli Native GPUI Preview

This is a GPUI/native preview build for testing the next-generation hapcli desktop app.

If you just want the most stable daily-use hapcli release, do not download this preview yet. Use the current Tauri/WebView release instead. This preview exists so users can try the native UI early and report parity, packaging, terminal, SFTP, port forwarding, SSH, and AI workflow issues before it becomes the default app.

> **Final Preview notice:** This is the last GPUI Preview release. The supported upgrade path to the stable hapcli 2.0 release starts from an installed hapcli 1.x client. Preview builds will not be upgraded in place to stable 2.0. Keep or install the current 1.x client and use its upgrade flow when hapcli 2.0 is released.

## What This Preview Is

- A native GPUI desktop build of hapcli.
- A preview channel, not the recommended stable channel.
- Built from the same product direction as hapcli: local-first remote server work over SSH, with terminal, SFTP, port forwarding, connection management, file workflows, and OxideSens AI context.
- Intended for users who are comfortable testing early builds and filing detailed issues.

<!-- RELEASE_CHANGELOG -->

## What To Test

- SSH connection creation, saved connections, reconnect, known-host prompts, and jump/proxy routes.
- Telnet and serial connection creation, saved profiles, and session manager editing.
- Terminal tabs, split panes, command bar, quick commands, broadcast input, recording, and local shell behavior.
- SFTP browsing, transfers, previews, and remote file workflows.
- Local, remote, and dynamic port forwarding.
- Cloud sync setup, scope selection, upload/pull preview, conflict handling, rollback backups, and provider-specific OAuth flows.
- OxideSens AI sidebar, model/provider setup, context capture, tool calls, approval behavior, and credential redaction.
- Settings, i18n, theme rendering, keyboard shortcuts, and visual parity with the Tauri version.

## Known Preview Caveats

- Some workflows may be incomplete or rough around edge cases.
- macOS builds are not notarized unless explicitly stated in a later release.
- Windows SmartScreen and macOS Gatekeeper may warn because this is an unsigned/early preview build.
- If a workflow is business-critical, keep the stable Tauri release installed as your fallback.

<details>
<summary>Installation Tips / 安装提示</summary>

### macOS

Downloaded `.dmg` files may be quarantined by Gatekeeper. Run in Terminal:

```bash
xattr -cr ~/Downloads/hapcli_*.dmg
# or after install / 或安装后
xattr -cr /Applications/hapcli.app
```

### Windows

If SmartScreen warns, click **More info** -> **Run anyway**.

若 SmartScreen 弹出警告，点击 **更多信息** -> **仍要运行**。

### Linux

```bash
# AppImage
chmod +x hapcli_*_linux_*.AppImage && ./hapcli_*_linux_*.AppImage

# Debian/Ubuntu
sudo dpkg -i hapcli_*_linux_*.deb && sudo apt-get install -f
```

</details>

## Reporting Issues

Please include:

- OS and CPU architecture.
- The exact asset you installed.
- Whether this is a fresh install or an update from another hapcli build.
- Steps to reproduce.
- Screenshots or logs when useful.
- Whether the same workflow works in the Tauri version.

GitHub Issues: https://github.com/AnalyseDeCircuit/hapcli/issues
