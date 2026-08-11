<!-- RELEASE_CHANGELOG -->

<!-- RELEASE_DOWNLOADS -->

<details>
<summary>📌 Installation Tips / 安装提示</summary>

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

# Fedora/RHEL-compatible systems
sudo dnf install ./hapcli_*_linux_*.rpm
```

</details>

## 🔗 Links

- Documentation: https://hapcli.app
- GitHub Issues: https://github.com/AnalyseDeCircuit/hapcli/issues
- Changelog: https://github.com/AnalyseDeCircuit/hapcli/blob/main/.github/release-notes/stable-changelog.md
