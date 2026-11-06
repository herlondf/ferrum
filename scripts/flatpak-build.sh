#!/usr/bin/env bash
# Builds the Flatpak into dist/Ferrum.flatpak. Needs flatpak-builder, python3
# (with aiohttp and tomlkit) and the GNOME 49 SDK plus the rust-stable extension:
#   flatpak install flathub org.gnome.Sdk//49 org.freedesktop.Sdk.Extension.rust-stable//25.08
set -euo pipefail
cd "$(dirname "$0")/.."

manifest=packaging/flatpak/io.github.herlondf.Ferrum.yml
generator=target/flatpak-cargo-generator.py
mkdir -p target dist
if [[ ! -f $generator ]]; then
    curl -sSfL -o "$generator" \
        https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py
fi
# The crates of Cargo.lock as Flatpak sources, so the build runs offline.
python3 "$generator" Cargo.lock -o packaging/flatpak/cargo-sources.json

flatpak-builder --user --force-clean --repo=target/flatpak-repo target/flatpak-build "$manifest"
flatpak build-bundle target/flatpak-repo dist/Ferrum.flatpak io.github.herlondf.Ferrum
ls -l dist/Ferrum.flatpak
