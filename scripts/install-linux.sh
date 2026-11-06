#!/usr/bin/env bash
# Builds Ferrum and installs it for the current user (no root needed):
#   ~/.local/bin/ferrum, a menu entry and an icon.
# Uses the `dev` container helper when available, plain cargo otherwise.
set -euo pipefail
cd "$(dirname "$0")/.."

if command -v dev >/dev/null; then
    dev rust cargo build --release -p ferrum
else
    cargo build --release -p ferrum
fi

install -Dm755 target/release/ferrum "$HOME/.local/bin/ferrum"
install -Dm644 packaging/linux/ferrum.png "$HOME/.local/share/icons/hicolor/256x256/apps/ferrum.png"
install -Dm644 packaging/linux/ferrum.desktop "$HOME/.local/share/applications/ferrum.desktop"
update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
gtk-update-icon-cache "$HOME/.local/share/icons/hicolor" 2>/dev/null || true

echo "Ferrum installed. Open it from the app menu or run: ferrum [repository]"
