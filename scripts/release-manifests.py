#!/usr/bin/env python3
"""Writes the package manager manifests for a release from its SHA256SUMS.

    scripts/release-manifests.py VERSION [DIST] [OUT]

DIST (default dist/) holds the release files and SHA256SUMS; OUT (default
DIST/manifests) receives:

    winget/manifests/h/herlondf/Ferrum/VERSION/*.yaml   PR to microsoft/winget-pkgs
    scoop/ferrum.json                                    a Scoop bucket
    homebrew/ferrum.rb                                   a Homebrew tap (Casks/)
    flathub/io.github.herlondf.Ferrum.yml                PR to flathub/flathub
    flathub/cargo-sources.json (when DIST has it)

Nothing is published: docs/RELEASING.md says where each file goes. Only the
standard library is used, so it runs anywhere python3 does.
"""

import re
import subprocess
import sys
from datetime import date
from pathlib import Path

REPO = "herlondf/ferrum"
HOMEPAGE = "https://herlondf.github.io/ferrum/"
SUMMARY = "A fast, cross-platform Git client"
DESCRIPTION = (
    "Ferrum is a free and open source Git client for Linux, Windows and macOS: "
    "history graph, staging of files, hunks and lines, interactive rebase, "
    "conflict resolution and GitHub/GitLab pull requests."
)
# The Inno Setup AppId (packaging/windows/ferrum.iss) plus "_is1".
PRODUCT_CODE = "{6E1B0F5A-3C2D-4F7B-9A4E-F3E2A1C0B7D9}_is1"
ROOT = Path(__file__).resolve().parent.parent


def sums(dist: Path) -> dict:
    out = {}
    for line in (dist / "SHA256SUMS").read_text().splitlines():
        parts = line.split(None, 1)
        if len(parts) == 2 and len(parts[0]) == 64:
            out[parts[1].strip().lstrip("*")] = parts[0].lower()
    return out


def url(version: str, name: str) -> str:
    return f"https://github.com/{REPO}/releases/download/v{version}/{name}"


def write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
    print(path)


def winget(version: str, hashes: dict, out: Path) -> None:
    name = "ferrum-windows-x86_64-setup.exe"
    if name not in hashes:
        print(f"skipping winget: no {name}", file=sys.stderr)
        return
    base = out / "winget/manifests/h/herlondf/Ferrum" / version
    head = "# yaml-language-server: $schema=https://aka.ms/winget-manifest.{kind}.1.9.0.schema.json\n\n"
    common = f"PackageIdentifier: herlondf.Ferrum\nPackageVersion: {version}\n"
    write(
        base / "herlondf.Ferrum.yaml",
        head.format(kind="version")
        + common
        + "DefaultLocale: en-US\nManifestType: version\nManifestVersion: 1.9.0\n",
    )
    write(
        base / "herlondf.Ferrum.installer.yaml",
        head.format(kind="installer")
        + common
        + "InstallerType: inno\n"
        + "Scope: user\n"
        + "UpgradeBehavior: install\n"
        + f"ProductCode: '{PRODUCT_CODE}'\n"
        + f"ReleaseDate: {date.today().isoformat()}\n"
        + "Installers:\n"
        + "  - Architecture: x64\n"
        + f"    InstallerUrl: {url(version, name)}\n"
        + f"    InstallerSha256: {hashes[name].upper()}\n"
        + "ManifestType: installer\nManifestVersion: 1.9.0\n",
    )
    write(
        base / "herlondf.Ferrum.locale.en-US.yaml",
        head.format(kind="defaultLocale")
        + common
        + "PackageLocale: en-US\n"
        + "Publisher: Herlon D. Filgueira\n"
        + f"PublisherUrl: https://github.com/{REPO.split('/')[0]}\n"
        + f"PublisherSupportUrl: https://github.com/{REPO}/issues\n"
        + "PackageName: Ferrum\n"
        + f"PackageUrl: {HOMEPAGE}\n"
        + "License: MIT OR Apache-2.0\n"
        + f"LicenseUrl: https://github.com/{REPO}/blob/main/LICENSE-MIT\n"
        + f"ShortDescription: {SUMMARY}\n"
        + f"Description: {DESCRIPTION}\n"
        + "Moniker: ferrum\n"
        + "Tags:\n  - git\n  - git-client\n  - version-control\n  - gui\n"
        + f"ReleaseNotesUrl: https://github.com/{REPO}/releases/tag/v{version}\n"
        + "ManifestType: defaultLocale\nManifestVersion: 1.9.0\n",
    )


def scoop(version: str, hashes: dict, out: Path) -> None:
    name = "ferrum-windows-x86_64.zip"
    if name not in hashes:
        print(f"skipping scoop: no {name}", file=sys.stderr)
        return
    dl = f"https://github.com/{REPO}/releases/download/v$version/{name}"
    write(
        out / "scoop/ferrum.json",
        f"""{{
    "version": "{version}",
    "description": "{SUMMARY}",
    "homepage": "{HOMEPAGE}",
    "license": "MIT|Apache-2.0",
    "architecture": {{
        "64bit": {{
            "url": "{url(version, name)}",
            "hash": "{hashes[name]}"
        }}
    }},
    "bin": "ferrum.exe",
    "shortcuts": [["ferrum.exe", "Ferrum"]],
    "checkver": "github",
    "autoupdate": {{
        "architecture": {{
            "64bit": {{
                "url": "{dl}"
            }}
        }},
        "hash": {{
            "url": "$baseurl/SHA256SUMS"
        }}
    }}
}}
""",
    )


def homebrew(version: str, hashes: dict, out: Path) -> None:
    name = "ferrum-macos-universal.dmg"
    if name not in hashes:
        print(f"skipping homebrew: no {name}", file=sys.stderr)
        return
    write(
        out / "homebrew/ferrum.rb",
        f'''cask "ferrum" do
  version "{version}"
  sha256 "{hashes[name]}"

  url "https://github.com/{REPO}/releases/download/v#{{version}}/{name}"
  name "Ferrum"
  desc "{SUMMARY}"
  homepage "{HOMEPAGE}"

  livecheck do
    url :url
    strategy :github_latest
  end

  # Ferrum installs its own updates (the toolbar's Update button).
  auto_updates true
  depends_on macos: ">= :big_sur"

  app "Ferrum.app"

  zap trash: [
    "~/Library/Application Support/Ferrum",
    "~/Library/Saved Application State/io.github.herlondf.Ferrum.savedState",
  ]
end
''',
    )


def flathub(version: str, dist: Path, out: Path) -> None:
    manifest = (ROOT / "packaging/flatpak/io.github.herlondf.Ferrum.yml").read_text()
    try:
        commit = subprocess.run(
            ["git", "rev-list", "-n", "1", f"v{version}"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=True,
        ).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        commit = ""
    if not commit:
        print(f"note: no tag v{version} yet; fill in the commit by hand", file=sys.stderr)
        commit = "COMMIT-OF-THE-TAG"
    # The local folder source becomes the tagged commit on GitHub.
    git_source = (
        "      - type: git\n"
        f"        url: https://github.com/{REPO}.git\n"
        f"        tag: v{version}\n"
        f"        commit: {commit}\n"
    )
    new, count = re.subn(
        r"      - type: dir\n        path: \.\./\.\.\n        skip:\n(?:          - .*\n)+",
        git_source,
        manifest,
    )
    if count != 1:
        sys.exit("could not find the dir source in the Flatpak manifest")
    new = re.sub(r"^# Flatpak manifest.*?(?=^app-id)", "", new, flags=re.S | re.M)
    write(out / "flathub/io.github.herlondf.Ferrum.yml", new)
    sources = dist / "cargo-sources.json"
    if not sources.exists():
        sources = ROOT / "packaging/flatpak/cargo-sources.json"
    if sources.exists():
        write(out / "flathub/cargo-sources.json", sources.read_text())
    else:
        print("note: run scripts/flatpak-build.sh (or flatpak-cargo-generator) for cargo-sources.json",
              file=sys.stderr)


def main() -> None:
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    version = sys.argv[1].lstrip("v")
    dist = Path(sys.argv[2]) if len(sys.argv) > 2 else ROOT / "dist"
    out = Path(sys.argv[3]) if len(sys.argv) > 3 else dist / "manifests"
    hashes = sums(dist)
    winget(version, hashes, out)
    scoop(version, hashes, out)
    homebrew(version, hashes, out)
    flathub(version, dist, out)


if __name__ == "__main__":
    main()
