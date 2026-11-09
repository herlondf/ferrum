#!/usr/bin/env bash
# Builds a universal (Apple silicon + Intel) Ferrum.app into dist/:
#   ferrum-macos-universal.dmg   drag-to-Applications disk image
#   ferrum-macos-universal.zip   the app zipped (Homebrew cask, in-app update)
# Runs on macOS (uses lipo, sips, iconutil, ditto, hdiutil). Builds both
# targets with cargo unless they are already built.
#
# Signing: without MACOS_SIGN_IDENTITY the app is signed ad hoc (it runs,
# but Gatekeeper warns on first open). With a "Developer ID Application"
# identity in the keychain it is signed for distribution, and with
# APPLE_ID, APPLE_TEAM_ID and APPLE_APP_PASSWORD it is also notarized.
# See docs/RELEASING.md.
set -euo pipefail
cd "$(dirname "$0")/.."

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
name=ferrum-macos-universal
mkdir -p dist

bins=()
for target in aarch64-apple-darwin x86_64-apple-darwin; do
    bin="target/$target/release/ferrum"
    if [[ ! -x $bin ]]; then
        rustup target add "$target" >/dev/null 2>&1 || true
        MACOSX_DEPLOYMENT_TARGET=11.0 cargo build --release --locked -p ferrum --target "$target"
    fi
    bins+=("$bin")
done

app=target/Ferrum.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
lipo -create -output "$app/Contents/MacOS/ferrum" "${bins[@]}"
lipo -info "$app/Contents/MacOS/ferrum"

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

if [[ -n ${MACOS_SIGN_IDENTITY:-} ]]; then
    codesign --force --options runtime --timestamp --sign "$MACOS_SIGN_IDENTITY" "$app"
else
    codesign --force --sign - "$app"
fi
codesign --verify --verbose "$app"

can_notarize() {
    [[ -n ${MACOS_SIGN_IDENTITY:-} && -n ${APPLE_ID:-} && -n ${APPLE_TEAM_ID:-} && -n ${APPLE_APP_PASSWORD:-} ]]
}
notarize() {
    xcrun notarytool submit "$1" --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" \
        --password "$APPLE_APP_PASSWORD" --wait
}

# Zip (notarized and stapled first, so the app inside carries its ticket).
rm -f "dist/$name.zip"
ditto -c -k --keepParent "$app" "dist/$name.zip"
if can_notarize; then
    notarize "dist/$name.zip"
    xcrun stapler staple "$app"
    rm -f "dist/$name.zip"
    ditto -c -k --keepParent "$app" "dist/$name.zip"
fi

# Disk image with an Applications link to drag the app onto.
dmgdir=target/dmg
rm -rf "$dmgdir" "dist/$name.dmg"
mkdir -p "$dmgdir"
cp -R "$app" "$dmgdir/"
ln -s /Applications "$dmgdir/Applications"
hdiutil create -volname "Ferrum $version" -srcfolder "$dmgdir" -ov -format UDZO "dist/$name.dmg"
if [[ -n ${MACOS_SIGN_IDENTITY:-} ]]; then
    codesign --force --timestamp --sign "$MACOS_SIGN_IDENTITY" "dist/$name.dmg"
fi
if can_notarize; then
    notarize "dist/$name.dmg"
    xcrun stapler staple "dist/$name.dmg"
fi

ls -l dist
