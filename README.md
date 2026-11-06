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
- **Sidebar**: branch folders by `/` (with counts, drag a branch onto a folder to
  move it), pinned branches and tags (also in the palette), quick filter, sorting
  (alphabetical, most recent commit, folders first; `v2` before `v10`), upstream
  icons (○ local only, ☁ published, ⚠ upstream gone), drag a branch onto another
  to merge or rebase, double-click to check out / track / open, "Select stale
  local branches" for bulk delete, annotated tag messages in tooltips, and a "+"
  per section. Open folders, pins and sort order are kept per repository.
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
- **External tools**: open the repository in a terminal, the file manager or
  your editor/IDE (VS Code, Zed, Sublime, JetBrains, Neovim, Cursor or a custom
  command); external diff and merge tools (Meld, KDiff3, Beyond Compare,
  P4Merge, VS Code, vimdiff, git's difftool/mergetool or a custom command).
- **Preferences**: global git identity, git executable and verbose git output,
  code font and size, tab width, relative/absolute/long dates, system accent
  color, toolbar buttons, branch name space replacement, default merge type.
- **Command palette** (Ctrl+P), keyboard shortcuts, light/dark theme, zoom,
  tabs that reopen with the app.

Writes go through the `git` command line, so hooks, credential helpers, SSH
config and Git LFS behave exactly as in the terminal. Reads use libgit2 for speed.
Git must be installed and on the `PATH` (or chosen in Settings > Git).

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
| Move between sidebar, commit list and details | Tab / Shift+Tab |
| Sidebar: move, open/close, check out, select | Arrows, Home/End, Enter, Space |

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

CI builds and tests on Linux, Windows and macOS, and uploads release binaries
as artifacts.

### Development aids

- `FERRUM_SCREENSHOT=out.ppm ./target/debug/ferrum <repo>` saves a screenshot
  and quits (`FERRUM_SCREENSHOT_FRAMES=N` waits longer).
- `FERRUM_START="changes;select:unstaged:src/main.rs"` opens a specific screen;
  the steps are listed in `startup_commands` in `app.rs`.
- `FERRUM_TIMING=1` prints how long reloading a repository takes.

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT), at your option.
