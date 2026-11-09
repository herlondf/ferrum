#!/usr/bin/env bash
# Builds the Linux release files into dist/ (names without the version, so
# `releases/latest/download/NAME` links keep working):
#   ferrum-linux-x86_64.tar.gz   binary + desktop entry + install.sh (used by get.sh)
#   ferrum-linux-x86_64.AppImage updates itself from the app
#   ferrum-linux-x86_64.deb      Debian / Ubuntu
#   ferrum-linux-x86_64.rpm      Fedora / openSUSE
# The .deb, .rpm and AppImage need cargo-deb, cargo-generate-rpm and
# appimagetool on the PATH; each one is skipped (with a warning) when its tool
# is missing, unless STRICT=1 (CI). Run after `cargo build --release --locked
# -p ferrum`, or let this script build.
set -euo pipefail
cd "$(dirname "$0")/.."

arch=$(uname -m)
name="ferrum-linux-${arch}"
strict="${STRICT:-0}"
mkdir -p dist
[[ -x target/release/ferrum ]] || cargo build --release --locked -p ferrum

missing() {
    if [[ $strict == 1 ]]; then
        echo "error: $1 is not installed" >&2
        exit 1
    fi
    echo "warning: $1 is not installed; skipping $2" >&2
}

# Tarball: one folder, as get.sh expects.
stage="target/$name"
rm -rf "$stage"
install -Dm755 target/release/ferrum "$stage/ferrum"
install -Dm755 packaging/linux/install.sh "$stage/install.sh"
cp packaging/linux/ferrum.desktop packaging/linux/ferrum.png \
    packaging/linux/io.github.herlondf.Ferrum.metainfo.xml \
    README.md LICENSE-MIT LICENSE-APACHE "$stage/"
tar -czf "dist/$name.tar.gz" -C target "$name"

# Debian / Ubuntu
if command -v cargo-deb >/dev/null; then
    cargo deb -p ferrum --no-build --output "dist/$name.deb"
else
    missing cargo-deb "the .deb"
fi

# Fedora / openSUSE
if command -v cargo-generate-rpm >/dev/null; then
    cargo generate-rpm -p crates/ferrum-app -o "dist/$name.rpm"
else
    missing cargo-generate-rpm "the .rpm"
fi

# AppImage: an AppDir with the binary, desktop entry, icon and metainfo.
if command -v appimagetool >/dev/null; then
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
        "dist/$name.AppImage"
else
    missing appimagetool "the AppImage"
fi

ls -l dist
