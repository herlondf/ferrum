# Ferrum

A fast, cross-platform Git client written in Rust, inspired by Fork and GitKraken.

![icon](packaging/linux/ferrum.png)

## Features

- **History** with a colored branch graph, branch/tag labels, search by
  message/author/hash or inside changes (`git log -S`), keyboard navigation.
- **Commit details**: message, files and diff; side-by-side **image diff**.
- **Local changes**: stage/unstage/discard whole files, hunks or single lines;
  commit and amend; prepared messages for merges, reverts and cherry-picks.
- **Branches, remotes, tags, stashes, submodules and worktrees** in the sidebar,
  each with a context menu (checkout, merge, rebase, rename, delete, push, track…).
- **Fetch, pull and push** with live progress, force-with-lease and upstream setup.
- **Interactive rebase** planner: reorder by drag and drop, reword, squash,
  fixup, edit, drop.
- **Three-way conflict resolver** with per-conflict choices and an editable result.
- **Cherry-pick, revert, reset (soft/mixed/hard), move branch** from the commit menu.
- **Undo** for commits, checkouts, branch/tag changes, merges, rebases and more.
- **File history and blame** (follows renames).
- **GitHub and GitLab**: list, check out and create pull/merge requests
  (uses your GitHub CLI login automatically, or a saved token).
- **Profiles**: per-repository name, email, SSH key and commit signing.
- **Command palette** (Ctrl+P), keyboard shortcuts, light/dark theme, zoom,
  tabs that reopen with the app.
- **Status and activity**: the toolbar shows the repository, branch,
  ahead/behind and the running operation with its progress; a click opens the
  **Activity** window with every operation, the exact git commands, their
  output and result (user and background operations apart). Running
  operations (fetch, pull, push, clone…) can be cancelled.
- **Live refresh** without freezing: the working folder is watched for
  changes (or polled at a set interval), reloads run in the background, and
  long operations that finish while the window is in the background raise a
  desktop notification. Window size and position, panel widths and scroll
  positions are remembered.
- **Update check** once a day against GitHub Releases ("Version X available";
  nothing is downloaded or replaced). Each of these can be turned off in
  Settings.

Writes go through the `git` command line, so hooks, credential helpers, SSH
config and Git LFS behave exactly as in the terminal. Reads use libgit2 for speed.
Git must be installed and on the `PATH`.

## Keyboard shortcuts

| Action | Shortcut |
|---|---|
| Command palette | Ctrl+P |
| Open repository | Ctrl+O |
| History / Local changes | Ctrl+1 / Ctrl+2 |
| Search commits | Ctrl+F |
| Refresh | F5 or Ctrl+R |
| Commit (in the message box) | Ctrl+Enter |
| New branch | Ctrl+B |
| Stash | Ctrl+Shift+S |
| Fetch / Pull / Push | Ctrl+Shift+F / Ctrl+Shift+D / Ctrl+Shift+U |
| Undo | Ctrl+Z |
| Next / previous tab, close tab | Ctrl+Tab / Ctrl+Shift+Tab, Ctrl+W |
| Settings | Ctrl+, |

On macOS, Cmd replaces Ctrl.

## Structure

- `crates/ferrum-core` — Git logic (libgit2 reads, git CLI writes), no UI code
- `crates/ferrum-app` — desktop app (`egui`/`eframe`), binary `ferrum`

## Building

The toolchain runs in a container; nothing needs to be installed on the host
besides Podman. The `dev` helper lives in `~/containers` (see its README).

```sh
dev rust cargo build            # build
dev rust cargo test             # unit and UI tests
dev rust cargo clippy --all-targets
./target/debug/ferrum [path...] # run on the host
./scripts/install-linux.sh      # release build + app menu entry (Linux)
```

Without the container, plain `cargo build` / `cargo test` work too.

CI builds and tests on Linux, Windows and macOS and uploads the installers as
artifacts; pushing a `v*` tag publishes them as a GitHub release:

- Linux: `.deb`, `.rpm`, AppImage and a tarball (`scripts/package-linux.sh`,
  needs `cargo-deb`, `cargo-generate-rpm` and `appimagetool`)
- macOS: `Ferrum.app` in a zip (`scripts/package-macos.sh`, unsigned)
- Windows: zip with `ferrum.exe`
- Flatpak: `packaging/flatpak/io.github.herlondf.Ferrum.yml`
  (`scripts/flatpak-build.sh`; AppStream metadata in `packaging/linux`)

### Development aids

- `FERRUM_SCREENSHOT=out.ppm ./target/debug/ferrum <repo>` saves a screenshot
  and quits (`FERRUM_SCREENSHOT_FRAMES=N` waits longer).
- `FERRUM_START="changes;select:unstaged:src/main.rs"` opens a specific screen;
  the steps are listed in `startup_commands` in `app.rs`.
- `FERRUM_TIMING=1` prints how long reloading a repository takes.
- `ferrum --benchmark <repo> [runs]` times each reload step (branches, commits,
  graph layout, status…) and prints the best and average of each.

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT), at your option.
