#!/usr/bin/env bash
# Builds the Linux packages into dist/: a .deb, an .rpm, an AppImage and a
# tarball. Needs cargo-deb, cargo-generate-rpm and appimagetool on the PATH
# (CI installs them). Run after `cargo build --release -p ferrum`, or let
# this script build.
set -euo pipefail
cd "$(dirname "$0")/.."

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
arch=$(uname -m)
mkdir -p dist
[[ -x target/release/ferrum ]] || cargo build --release -p ferrum

# Debian / Ubuntu
cargo deb -p ferrum --no-build --output dist/

# Fedora / openSUSE
cargo generate-rpm -p crates/ferrum-app -o dist/

# AppImage: an AppDir with the binary, desktop entry, icon and metainfo.
appdir=target/AppDir
rm -rf "$appdir"
install -Dm755 target/release/ferrum "$appdir/usr/bin/ferrum"
install -Dm644 packaging/linux/ferrum.desktop "$appdir/usr/share/applications/ferrum.desktop"
install -Dm644 packaging/linux/ferrum.png "$appdir/usr/share/icons/hicolor/256x256/apps/ferrum.png"
install -Dm644 packaging/linux/io.github.herlondf.Ferrum.metainfo.xml \
    "$appdir/usr/share/metainfo/io.github.herlondf.Ferrum.appdata.xml"
cp packaging/linux/ferrum.desktop "$appdir/ferrum.desktop"
cp packaging/linux/ferrum.png "$appdir/ferrum.png"
ln -sf usr/bin/ferrum "$appdir/AppRun"
# appimagetool is itself an AppImage; extract-and-run avoids needing FUSE.
APPIMAGE_EXTRACT_AND_RUN=1 ARCH="$arch" appimagetool --no-appstream "$appdir" \
    "dist/Ferrum-${version}-${arch}.AppImage"

# Plain binary
tar -czf "dist/ferrum-${version}-linux-${arch}.tar.gz" \
    -C target/release ferrum \
    -C ../../packaging/linux ferrum.desktop ferrum.png

ls -l dist
