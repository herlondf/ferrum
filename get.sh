#!/bin/sh
# Installs or updates Ferrum for the current user from the latest release
# (no root, nothing to build): ~/.local/bin/ferrum plus a menu entry and icon.
#
#   curl -fsSL https://raw.githubusercontent.com/herlondf/ferrum/main/get.sh | sh
#   curl -fsSL .../get.sh | sh -s -- --uninstall        (remove it again)
#   FERRUM_VERSION=v0.2.0 sh get.sh                    (a specific version)
set -eu

repo="${FERRUM_REPO:-herlondf/ferrum}"
case "$(uname -s)-$(uname -m)" in
    Linux-x86_64 | Linux-amd64) name=ferrum-linux-x86_64 ;;
    Darwin-*)
        echo "On macOS use Homebrew (brew install --cask ferrum) or the .dmg from" >&2
        echo "https://github.com/$repo/releases/latest" >&2
        exit 1 ;;
    *)
        echo "No prebuilt Ferrum for $(uname -s) $(uname -m) yet." >&2
        echo "Build it from source: https://github.com/$repo#building" >&2
        exit 1 ;;
esac
if [ -n "${FERRUM_VERSION:-}" ]; then
    base="https://github.com/$repo/releases/download/$FERRUM_VERSION"
else
    base="https://github.com/$repo/releases/latest/download"
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM
echo "Downloading $base/$name.tar.gz"
curl -fsSL -o "$tmp/$name.tar.gz" "$base/$name.tar.gz"
curl -fsSL -o "$tmp/SHA256SUMS" "$base/SHA256SUMS"
(cd "$tmp" && grep " [* ]\{0,1\}$name.tar.gz\$" SHA256SUMS | sha256sum -c --quiet -) || {
    echo "Checksum mismatch for $name.tar.gz; nothing was installed." >&2
    exit 1
}
tar xzf "$tmp/$name.tar.gz" -C "$tmp"
bash "$tmp/$name/install.sh" "$@"
