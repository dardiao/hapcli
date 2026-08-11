# hapcli Native Beta

This is a beta hapcli native desktop release.

Use this channel when you want to test changes before they reach the stable channel, but do not need the faster-moving GPUI preview track. Beta builds may include packaging, updater, compatibility, and workflow changes that still need wider validation.

## What This Beta Is

- A pre-stable hapcli desktop build.
- Intended for users who can tolerate small regressions and report issues with enough detail to reproduce them.
- Published with updater metadata for the beta channel.
- More conservative than GPUI preview, but not as stable as the stable channel.

<!-- RELEASE_CHANGELOG -->

## Beta Notes

- Keep a stable build installed if hapcli is business-critical for your daily work.
- Report regressions with OS, CPU architecture, installed asset name, and steps to reproduce.
- If a beta workflow behaves differently from the stable release, include both results in the issue.

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

## Links

- Documentation: https://hapcli.app
- GitHub Issues: https://github.com/AnalyseDeCircuit/hapcli/issues
- Changelog: https://github.com/AnalyseDeCircuit/hapcli/tree/main/docs/changelog
