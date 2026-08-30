#!/usr/bin/env bash
# 打包 macOS .app：构建 release 二进制、生成图标、写 Info.plist、ad-hoc 签名。
set -euo pipefail

cd "$(dirname "$0")/.."

APP_NAME="HapCLI"
BIN_NAME="hapcli-egui-app"
VERSION="$(cargo metadata --no-deps --format-version 1 \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["packages"][0]["version"])')"

echo "==> 构建 release 二进制 (${VERSION})"
cargo build --release -p hapcli-egui-app

APP_DIR="target/${APP_NAME}.app"
echo "==> 组装 ${APP_DIR}"
rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS" "$APP_DIR/Contents/Resources"
cp "target/release/${BIN_NAME}" "$APP_DIR/Contents/MacOS/${APP_NAME}"

cat > "$APP_DIR/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>HapCLI</string>
    <key>CFBundleDisplayName</key>
    <string>HapCLI</string>
    <key>NSHumanReadableCopyright</key>
    <string>HapX.tm 的商标。</string>
    <key>CFBundleIdentifier</key>
    <string>com.hapcli.app</string>
    <key>CFBundleExecutable</key>
    <string>HapCLI</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>${VERSION}</string>
    <key>CFBundleVersion</key>
    <string>${VERSION}</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.developer-tools</string>
    <key>CFBundleIconFile</key>
    <string>hapcli</string>
</dict>
</plist>
PLIST

if command -v python3 >/dev/null 2>&1; then
    echo "==> 生成图标 (.icns)"
    python3 scripts/make_icon_png.py "$APP_DIR/Contents/Resources/hapcli.icns"
fi

echo "==> 签名"
SIGN_IDENTITY="$({
  security find-identity -v -p codesigning 2>/dev/null \
    | grep 'Developer ID Application' \
    | sed -E 's/.*"([^"]+)".*/\1/' \
    | head -1
} || true)"
if [ -n "$SIGN_IDENTITY" ]; then
  echo "使用 Developer ID 签名: ${SIGN_IDENTITY}"
  codesign --force --deep --sign "$SIGN_IDENTITY" "$APP_DIR"
else
  echo "未找到 Developer ID 证书，使用 ad-hoc 签名（仅本机/测试用）。"
  codesign --force --deep --sign - "$APP_DIR"
fi
codesign --verify --deep --strict "$APP_DIR"
plutil -lint "$APP_DIR/Contents/Info.plist" >/dev/null

echo "完成: $(pwd)/${APP_DIR}"
echo "双击即可运行，或执行: open $(pwd)/${APP_DIR}"
if [ -z "$SIGN_IDENTITY" ]; then
  echo ""
  echo "提示：ad-hoc 包首次打开会提示“无法验证开发者”。"
  echo "本机免提示方法：解压后执行"
  echo "  xattr -dr com.apple.quarantine \"$(pwd)/${APP_DIR}\""
  echo "或首次右键点击 App → 打开。"
fi
