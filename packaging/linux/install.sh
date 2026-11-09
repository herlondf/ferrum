#!/usr/bin/env bash
# Installs Ferrum from this folder (the ferrum-linux-x86_64.tar.gz release)
# for the current user, no root needed:
#   ~/.local/bin/ferrum, a menu entry, an icon and the AppStream metadata.
# get.sh downloads the release and runs this. `install.sh --uninstall` removes it.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
bin="$HOME/.local/bin"

files=(
    "$bin/ferrum"
    "$data/applications/ferrum.desktop"
    "$data/icons/hicolor/256x256/apps/ferrum.png"
    "$data/metainfo/io.github.herlondf.Ferrum.metainfo.xml"
)

if [[ "${1:-}" == "--uninstall" ]]; then
    rm -f "${files[@]}"
    echo "Ferrum removed. Your settings were kept."
    exit 0
fi

install -Dm755 "$here/ferrum" "${files[0]}"
mkdir -p "$(dirname "${files[1]}")"
# The full path: ~/.local/bin is not always on the desktop session's PATH.
sed "s|^Exec=ferrum|Exec=$bin/ferrum|" "$here/ferrum.desktop" > "${files[1]}"
install -Dm644 "$here/ferrum.png" "${files[2]}"
install -Dm644 "$here/io.github.herlondf.Ferrum.metainfo.xml" "${files[3]}"
update-desktop-database "$data/applications" 2>/dev/null || true
gtk-update-icon-cache "$data/icons/hicolor" 2>/dev/null || true

echo "Ferrum installed in $bin."
case ":$PATH:" in
    *":$bin:"*) ;;
    *) echo "Add $bin to your PATH to run 'ferrum' from a terminal." ;;
esac
echo "Open it from the app menu or run: ferrum [repository]"
