#!/usr/bin/env bash
# Removes what install-linux.sh installed. Settings are kept in ~/.local/share/ferrum.
set -euo pipefail
rm -f "$HOME/.local/bin/ferrum" \
      "$HOME/.local/share/icons/hicolor/256x256/apps/ferrum.png" \
      "$HOME/.local/share/applications/ferrum.desktop"
update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
echo "Ferrum removed."
