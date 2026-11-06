#!/usr/bin/env bash
# Builds dist/Ferrum-<version>-macos-<arch>.zip holding Ferrum.app.
# Runs on macOS (uses sips, iconutil and ditto). The app is not signed.
set -euo pipefail
cd "$(dirname "$0")/.."

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
arch=$(uname -m)
mkdir -p dist
[[ -x target/release/ferrum ]] || cargo build --release -p ferrum

app=target/Ferrum.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/ferrum "$app/Contents/MacOS/ferrum"

# Icon set from the 256 px PNG.
iconset=target/ferrum.iconset
rm -rf "$iconset"
mkdir -p "$iconset"
for size in 16 32 128 256; do
    sips -z $size $size packaging/linux/ferrum.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z $double $double packaging/linux/ferrum.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/ferrum.icns"

cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Ferrum</string>
    <key>CFBundleDisplayName</key><string>Ferrum</string>
    <key>CFBundleIdentifier</key><string>io.github.herlondf.Ferrum</string>
    <key>CFBundleVersion</key><string>${version}</string>
    <key>CFBundleShortVersionString</key><string>${version}</string>
    <key>CFBundleExecutable</key><string>ferrum</string>
    <key>CFBundleIconFile</key><string>ferrum</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF

ditto -c -k --keepParent "$app" "dist/Ferrum-${version}-macos-${arch}.zip"
ls -l dist
