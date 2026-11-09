# Releasing Ferrum

Everything that can be automated is in the repository; the steps below need
the owner's accounts and were deliberately **not** done by CI or by an agent.
Nothing here is published until someone follows this list.

## What the repository produces

The packaging scripts are the building blocks; any release workflow calls them.

| Script | Runs on | Writes to `dist/` |
| --- | --- | --- |
| `scripts/package-linux.sh` (`STRICT=1` in CI) | Linux | `ferrum-linux-x86_64.tar.gz`, `.AppImage`, `.deb`, `.rpm` |
| `scripts/package-windows.ps1 -Strict` | Windows | `ferrum-windows-x86_64.zip`, `ferrum-windows-x86_64-setup.exe` |
| `scripts/package-macos.sh` | macOS | `ferrum-macos-universal.dmg`, `ferrum-macos-universal.zip` |
| `scripts/checksums.sh dist` | anywhere | `SHA256SUMS` |
| `scripts/release-manifests.py VERSION dist OUT` | anywhere | winget, Scoop, Homebrew and Flathub files in `OUT` |

- The file names carry no version, so
  `https://github.com/herlondf/ferrum/releases/latest/download/<name>` always
  points at the newest release. The website's download buttons, `get.sh`,
  `get.ps1` and the in-app updater rely on these exact names and on
  `SHA256SUMS`. Keep them.
- `get.sh` (Linux) and `get.ps1` (Windows) install per user, no root/admin,
  from the latest release and verify `SHA256SUMS`:

  ```sh
  curl -fsSL https://raw.githubusercontent.com/herlondf/ferrum/main/get.sh | sh
  ```
  ```powershell
  irm https://raw.githubusercontent.com/herlondf/ferrum/main/get.ps1 | iex
  ```
- The Windows build links the C runtime statically (`.cargo/config.toml`);
  `package-windows.ps1` fails when `dumpbin` shows a `VCRUNTIME`/`MSVCP`
  dependency.
- CI (`.github/workflows/ci.yml`) runs every script on each push and uploads
  the files as workflow artifacts. It also builds the Flatpak in the
  `flathub-infra` container and validates the desktop file and metainfo.
  A `v*` tag publishes a GitHub release with the files, `SHA256SUMS` and a
  `manifests` artifact.

## Before each release

1. Bump `version` in the root `Cargo.toml` (and `Cargo.lock`, via `cargo build`).
2. Add a `<release version="X.Y.Z" date="YYYY-MM-DD">` entry with a short
   description at the top of `<releases>` in
   `packaging/linux/io.github.herlondf.Ferrum.metainfo.xml` (Flathub and GNOME
   Software show it as release notes).
3. Refresh the screenshots if the UI changed (see "Website" below); the
   metainfo points at `site/assets/screenshots/*.png` on `main`.
4. Run `scripts/benchmark.sh` and update `site/benchmarks.md` (and the numbers
   on the site) if performance changed.
5. Publish. Today a pushed `vX.Y.Z` tag makes `ci.yml` publish the release.
   A push-to-main release workflow (version from `Cargo.toml`, like the
   owner's other projects) can replace that; it should call the same scripts
   and keep the file names and `SHA256SUMS`.

## One-time steps that need the owner

### GitHub Pages (website)

- [ ] Repository Settings → Pages → Source: **GitHub Actions**.
- [ ] Run the **Website** workflow (`.github/workflows/pages.yml`) by hand
  (Actions → Website → Run workflow). It only runs on manual dispatch, so
  nothing is deployed until you do this. Once Pages is on, you can add a
  `push` trigger for `site/**` if you want automatic deploys.
- [ ] Optional: a custom domain (Settings → Pages), then update `HOMEPAGE` in
  `scripts/release-manifests.py` and the `homepage` URL in the metainfo.

### Flathub (`io.github.herlondf.Ferrum`)

The app id uses the `io.github.<user>` prefix, so Flathub verifies it through
the GitHub account, no domain needed.

- [ ] After the first tagged release, run
  `scripts/release-manifests.py X.Y.Z dist out` on a checkout that has the tag
  (or take the `manifests` artifact of the release run). `out/flathub/` has
  the manifest with a `git` source pinned to the tag and commit. Generate
  `cargo-sources.json` next to it with `scripts/flatpak-build.sh` (or the
  `flatpak-cargo-sources` CI artifact).
- [ ] Fork `flathub/flathub`, branch off `new-pr`, add those two files, open a
  pull request against `new-pr` titled "Add io.github.herlondf.Ferrum", and
  follow the reviewers. Flathub's linter usually asks about these
  `finish-args`; the reasons are:
  - `--filesystem=home`: repositories live anywhere in the home folder and git
    reads `~/.gitconfig` and `~/.ssh`.
  - `--socket=ssh-auth`: pushing and fetching over SSH.
  - `--share=network`: fetch, pull, push, clone and the forge APIs.
  - `--talk-name=org.freedesktop.Notifications`: desktop notifications when a
    long operation finishes (they may ask to use the notification portal
    instead).
- [ ] Once accepted, you get `flathub/io.github.herlondf.Ferrum`; later
  releases are a PR there bumping `tag`, `commit` and `cargo-sources.json`.
- [ ] Verify the app on flathub.org (Developer portal → verify with GitHub).
- [ ] Inside the Flatpak the in-app updater only says Flathub updates it (it
  never replaces files there).
- [ ] Known sandbox limits to check before submitting: opening an external
  terminal, editor or diff/merge tool runs host programs, which the sandbox
  does not allow without `flatpak-spawn --host` (and Flathub rarely grants
  `--talk-name=org.freedesktop.Flatpak`). Test those menu items in the
  Flatpak build from CI and decide whether to hide them there.

### winget (`herlondf.Ferrum`)

- [ ] Take `out/winget/manifests/h/herlondf/Ferrum/X.Y.Z/` (3 YAML files).
- [ ] Validate on Windows: `winget validate --manifest <folder>` and test with
  `winget install --manifest <folder>`.
- [ ] Open a PR adding the folder to `microsoft/winget-pkgs` (or use
  `wingetcreate submit <folder>` with a GitHub token). Later versions:
  `wingetcreate update herlondf.Ferrum -v X.Y.Z -u <setup.exe URL> --submit`.
- [ ] The installer is not code-signed. winget accepts unsigned installers, but
  SmartScreen warns on first run until the file gains reputation. To remove
  the warning, sign `ferrum.exe` and the setup with an Authenticode
  certificate (or Azure Trusted Signing) in `package-windows.ps1`.

### Scoop

- [ ] Create a bucket repository (e.g. `herlondf/scoop-bucket`) with
  `out/scoop/ferrum.json` in `bucket/`. Users then run
  `scoop bucket add herlondf https://github.com/herlondf/scoop-bucket` and
  `scoop install ferrum`. The manifest has `checkver`/`autoupdate`, so the
  bucket's Excavator action can update it by itself.
- [ ] Later, optionally submit to the `extras` bucket (needs some popularity).

### Homebrew (cask)

- [ ] Create a tap `herlondf/homebrew-tap` with `out/homebrew/ferrum.rb` in
  `Casks/`. Users: `brew install --cask herlondf/tap/ferrum`.
- [ ] The official `homebrew/cask` only accepts apps that pass Gatekeeper, so
  submit there only after notarization (below).

### macOS signing and notarization (needs an Apple Developer account, 99 USD/year)

Without it the app is signed ad hoc: it runs, but the first open needs
right-click → Open (or `xattr -dr com.apple.quarantine /Applications/Ferrum.app`),
and Homebrew's official cask will not take it.

- [ ] Create a "Developer ID Application" certificate, export it as `.p12`.
- [ ] Add repository secrets: the `.p12` (base64) and its password, `APPLE_ID`,
  `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD` (an app-specific password).
- [ ] In the macOS build job, import the certificate into a temporary keychain
  (e.g. `apple-actions/import-codesign-certs`) and set `MACOS_SIGN_IDENTITY`
  to "Developer ID Application: <name> (<team>)". `scripts/package-macos.sh`
  then signs with the hardened runtime, notarizes the zip and the dmg with
  `notarytool` and staples the tickets.

### Windows code signing (optional, paid)

- [ ] An Authenticode certificate (OV/EV) or Azure Trusted Signing; add a
  `signtool sign` step for `ferrum.exe` before zipping and for the setup after
  `iscc` in `scripts/package-windows.ps1`.

## In-app updates

The toolbar's **Update** button (after the daily check finds a newer release)
opens a window that, for the formats that can update themselves, downloads the
matching file, checks it against `SHA256SUMS` and installs it:

| Installed as | What happens |
| --- | --- |
| AppImage (`$APPIMAGE`) | `ferrum-linux-x86_64.AppImage` replaces the file, then Ferrum restarts |
| Windows installer | `ferrum-windows-x86_64-setup.exe /SILENT /CLOSEAPPLICATIONS /RELAUNCH=1` |
| macOS `Ferrum.app` | `ferrum-macos-universal.zip` is unpacked with `ditto` and swapped in (old copy kept until the swap worked) |
| `get.sh` / `get.ps1` | shows the install command to run again |
| Flatpak, .deb/.rpm, Homebrew, Scoop | says which package manager updates it |

Nothing is downloaded without the user clicking **Install and restart**.
