# Ferrum

A fast, cross-platform Git client written in Rust, inspired by Fork and GitKraken.

![icon](packaging/linux/ferrum.png)

## Features

- **History** with a colored branch graph, branch/tag labels, search by
  message/author/hash, inside changes (`git log -S`) or by file path (current
  branch or all, recent searches, Enter/F3 for the next match), keyboard
  navigation, and more commits loaded as you scroll.
- **Graph options**: collapse/expand merges (with a tooltip of what a merge
  brought in), topological order, first parent, reflog with unreachable commits
  greyed out, hide remote branches/tags/stashes, show only or hide branches,
  folders and remotes (saved per repository, shown in the status bar),
  unpushed/unpulled markers, author avatars (Gravatar/GitHub, can be turned
  off) with `.mailmap`, author highlight, conventional commit prefixes, issue
  tracker links (regex rules and `#123`), compact labels, details beside the
  list, and Shift+wheel for wide graphs.
- **Drag commits** on the current branch to reorder or squash them.
- **Repository statistics**: commits per weekday, hour and author, and a
  treemap of changed folders.
- **Commit details**: message, files and diff.
- **Diff viewer**: unified or side by side, syntax highlighting, changed words
  highlighted, find (Ctrl+F), map of changes, next/previous change, ignore
  whitespace, word wrap, font size, whole file or N context lines, visible
  whitespace and tab width, optional `-`/`+` markers, a large window (Space),
  rename and file mode changes, scroll position kept per file.
  **Images** (PNG, JPEG, GIF, BMP, WebP, ICO, TGA, SVG, first page of PDF) side
  by side, as a swipe, onion skin or pixel difference; binary size changes; Git
  LFS pointers with a download/preview; commits between two submodule
  revisions; large untracked files load on request.
- **Local changes**: stage/unstage/discard whole files, hunks or single lines;
  flat, tree or single-list view; filter; drag files between lists; hide
  untracked / show ignored files; `.gitignore` by file, folder, extension or
  pattern (optionally untracking) and built-in templates; large-file warning for
  files outside Git LFS; copy or save changes as a patch; reset a file to a revision.
- **Commit box**: subject and description fields with length limits and a
  ruler (wrap at ruler), recent messages, drafts kept per worktree,
  `commit.template`, `Co-authored-by:` completion, sign-off, amend (shows the
  last commit's changes, warns if already pushed), commit and push, live hook
  output with "commit without hooks", detached HEAD warning, and an optional
  AI-written message from a configurable CLI (`claude -p`, `codex exec -`).
- **Branches, remotes, tags, stashes, submodules and worktrees** in the sidebar,
  each with a context menu (checkout, merge, rebase, rename, delete, push, track…).
- **Sidebar**: branch folders by `/` (with counts, drag a branch onto a folder to
  move it), pinned branches and tags (also in the palette), quick filter, sorting
  (alphabetical, most recent commit, folders first; `v2` before `v10`), upstream
  icons (○ local only, ☁ published, ⚠ upstream gone), drag a branch onto another
  to merge or rebase, double-click to check out / track / open, "Select stale
  local branches" for bulk delete, annotated tag messages in tooltips, and a "+"
  per section. Open folders and sort order are kept per worktree; pins and
  issue link rules are shared by all worktrees of a repository.
- **Branches**: quick checkout with fuzzy search (Ctrl+B); new branch with
  folder prefix completion (Tab), validation (`git check-ref-format`, already
  exists), space replacement and the last prefix remembered; rename (F2) also
  on the remote; delete several at once with their remote branches and the
  worktrees that have them checked out; copy the name from the sidebar or the
  commit menu.
- **Tags**: multiline messages, signed tags (`-s`, when a signing key is set),
  push from the create dialog, a details window (tagger, date, message,
  commit), delete locally and/or on a remote.
- **Stashes**: stash only some files, snapshots (keep the working copy),
  rename, apply with the staged state restored (`--index`), save as a patch,
  drop several; a stashes dropdown in the toolbar and quick stash
  (Ctrl/Alt+click on Stash).
- **Git LFS** (toolbar ☰ > Git LFS, when `git lfs` is installed): tracked
  patterns (track / untrack, from `.gitattributes`), status, fetch / pull / prune
  with progress, install, lock / unlock from the local changes file menu, and a
  locks dialog with a filter and multi-unlock (optionally forced). LFS content in
  diffs can be downloaded, previewed and saved.
- **Submodules**: add (URL, path, branch), remove (deinit, `git rm` and the clone
  in `.git/modules`), move, change URL (with `git submodule sync`), update the
  selected ones (init / recursive), discard their changes, open in a tab next to
  the parent; icons for a moved commit (⟳), uncommitted changes (📝) and
  untracked files (+). Optional update after checkout, cherry-pick and revert
  (Settings > Git: never / ask / always).
- **Worktrees**: linked worktrees nested under the main one, "Check out as
  worktree…" for local and remote branches (with a suggested folder), a 📂 icon on
  branches checked out in another worktree; double-clicking one opens that
  worktree in a tab.
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
Git must be installed and on the `PATH` (or chosen in Settings > Git).

## Keyboard shortcuts

| Action | Shortcut |
|---|---|
| Command palette | Ctrl+P |
| Open repository | Ctrl+O |
| History / Local changes | Ctrl+1 / Ctrl+2 |
| Search commits (or find in the diff under the pointer) | Ctrl+F |
| Next / previous match | Enter or F3 / Shift+Enter or Shift+F3 |
| Jump to HEAD | Ctrl+0 (or Ctrl+1 in the history) |
| Go to commit | Ctrl+G |
| Only the current branch in the graph | Ctrl+Shift+A |
| Show reflog | Ctrl+Shift+. |
| Collapse / expand the selected merge | Left / Right |
| Previous / next hunk in the diff | Ctrl+Up / Ctrl+Down |
| Large diff window | Space |
| Refresh | F5 or Ctrl+R |
| Commit (in the message box) | Ctrl+Enter |
| Commit and push | Ctrl+Shift+Enter |
| Stage / unstage selected files | Enter (Ctrl/Shift-click to select several) |
| Check out a branch (search) | Ctrl+B |
| New branch | Ctrl+Shift+B |
| Rename the selected sidebar branch | F2 |
| Stash (Ctrl/Alt+click on the Stash button: quick stash) | Ctrl+Shift+S |
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
  the steps are listed in `startup_commands` in `app.rs` (diff options:
  `diff:sbs,wrap,find=TEXT,large,...`, see `diff_prefs::dev_step`).

- `FERRUM_TIMING=1` prints how long reloading a repository takes.
- `ferrum --benchmark <repo> [runs]` times each reload step (branches, commits,
  graph layout, status…) and prints the best and average of each.

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT), at your option.
