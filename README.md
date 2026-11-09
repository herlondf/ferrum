# Ferrum

A fast, cross-platform Git client written in Rust, inspired by Fork and GitKraken.

![icon](packaging/linux/ferrum.png)

## Features

- **Look**: Ferrum's own dark, light and high-contrast themes (or follow the
  system), with a rust-orange accent (or the system accent color), rounded
  corners, accent-colored main buttons in dialogs, and Compact / Comfortable
  density (Settings > Appearance). Inter for the interface, JetBrains Mono for
  code and Phosphor icons, all embedded (see `crates/ferrum-app/assets/fonts`).
- **Main menu bar**: File, Edit, View, Repository, Window and Help, with every
  item's shortcut (from Settings > Shortcuts). F10 opens the first menu,
  Alt + the underlined letter opens a menu, Left / Right switch menus. It can be
  hidden in Settings > Appearance > Window.
- **Repository Manager** (start screen, and Ctrl+T in a new tab): known
  repositories in groups, rename (F2), remove from the list (Delete), open
  (Enter or double-click), Ctrl/Shift+click to select several, missing folders
  flagged with "Remove missing"; **source folders** scanned for repositories to
  a chosen depth (Rescan picks up new ones); **workspaces** (nested with `/`),
  each remembering its own tabs, switched from the manager or the command
  palette ("Workspace: …"), shown in the window title. **Open Quickly**
  (Ctrl+Shift+O) searches every known, recent and scanned repository.
- **Toolbar** like Fork's: large buttons with the label underneath, in groups
  (Fetch / Pull / Push, Branch / Stash / Pop, Undo, Terminal / Folder), with
  the commits to pull and to push as badges on Pull and Push and a spinner on
  the button whose operation runs. When the window is too narrow the labels
  hide and only the icons stay. Buttons can be hidden in Settings.
- **Tabs**: real tabs attached to the view, showing the repository color, the
  current branch and a dot for uncommitted changes; × on hover (always on the
  active tab); the strip scrolls when there are many tabs; drag a tab to move
  it. Reopen closed tabs (Ctrl+Shift+T), middle-click to close, same-named tabs
  show the folder that tells them apart (a worktree shows "repo (worktree)"),
  and a menu with Close Others/All, Rename, Color, Open in terminal, Show in
  file manager, Open in new window, Repository settings and Move to workspace;
  optionally a vertical tab list (name and branch per row).
- **Repository overview** (sidebar > Overview): the rendered README, remotes and
  counts of commits, contributors, branches and tags, and the size on disk.
- **Opening**: drop a folder on the window (`git init` is offered when it is not
  a repository); `ferrum PATH` and `ferrum log -- FILE` reuse the running window;
  "Open in Desktop" links (`x-github-client://openRepo/…`,
  `sourcetree://cloneRepo/…`) open the clone dialog. A stale `index.lock` can
  be deleted from the error, and folders git does not trust can be added to
  `safe.directory`. Repository settings (pull request remote, commit template,
  issue link rules) are shared by all worktrees.
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
- **Commit details** in tabs (Commit / Changes / File Tree, remembered):
  author and committer, parent links, the branches and tags that contain the
  commit, commit ids in the message with a preview on hover, copy id or info,
  open the commit on GitHub, GitLab, Bitbucket or Gitea-style hosts; changed
  files as a tree or a list; a large popup (Space in the commit list), a
  separate window per commit, and a collapsible pane (Ctrl+Alt+D).
- **File Tree** of any commit: browse the repository at that revision, view
  text (with syntax colors), images and Git LFS pointers, save a file to disk.
- **Compare**: Ctrl+click two commits, pick two branches in the sidebar, or
  compare a commit or stash with the working copy; swap the two sides.
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
  A plain click on the toolbar button runs with the remembered options;
  Alt/Ctrl+click, right-click or the ⏷ next to it opens the dialog. Ahead/behind
  counts on the Pull and Push buttons. Fetch options (prune, all tags), pull
  with stash and reapply (`--autostash`), push to a remote branch with another
  name, push several branches at once (atomic), push one, several or all tags,
  warning about submodule commits that are not pushed. Searchable branch lists.
- **Automatic background fetch** of open repositories every 10 minutes (with
  `--prune`, never on top of a running operation; interval in Settings > General,
  0 turns it off), can be turned off per remote from its sidebar menu.
- **Fast-forward branches without checking them out** (branch menu, or several
  at once from the sidebar selection), check out a remote branch whose local
  branch is behind or has diverged (fast-forward, merge, rebase or reset),
  change the tracking branch (and pull, with stash and reapply), edit and rename
  remotes, copy their URL and test the connection.
- **Merge, rebase, cherry-pick and revert dialogs** that check for conflicts
  before running ("Will conflict in: …", via `git merge-tree`, git 2.38+), with
  `--no-commit`, `--allow-unrelated-histories`, `--autostash`, `--update-refs`,
  rebase onto any branch, remote branch or tag, cherry-pick `-x`, and the parent
  of a merge commit. Cherry-pick, squash or drop several commits selected with
  Ctrl/Shift+click in the history.
- **Interactive rebase** planner: reorder by drag and drop or Alt+Up/Down,
  reword, squash, fixup, edit, drop (keys P R E S F D on the selected rows),
  avatars, an optional backup branch and moving dependent branches along; asks
  before throwing away a changed plan. Quick actions in the commit menu (reword,
  edit, squash or fixup into parent, drop) run without the planner.
- **Three-way conflict resolver** with per-conflict choices and an editable
  result, sides named after the branches, markers next to the scroll bar,
  resolving several files with one side, images, LFS files and submodules (pick
  a side), and a diff of files resolved by hand or in a merge tool. A banner
  shows what is being merged or rebased (with links to the commits and rebase
  progress), Continue / Skip / Abort, and undoes a resolution.
- **Local changes on checkout**: when a checkout would overwrite them (or
  always, in Settings), choose to stash and reapply, discard or keep them.
- **Cherry-pick, revert, reset (soft/mixed/hard), move branch** from the commit
  menu; double-click a branch or tag label in the graph to check it out.
- **Undo** for commits, checkouts, branch/tag changes, merges, rebases and more.
- **File history and blame** (follows renames): history of files and folders
  on any branch, compare two revisions (Ctrl+click), open them in the external
  diff tool, reset a file to a revision (with undo); blame at any revision with
  back/forward, a timeline of the file's commits, "blame before this change"
  and Go to line; history of a range of lines (`git log -L`) from the blame or
  any diff line's menu; a separate window per file; "File history…" in the
  command palette finds files and folders by name.
- **Hosted services**: accounts for GitHub (and Enterprise), GitLab (and
  self-hosted), Bitbucket Cloud and Server/Data Center, Azure DevOps and
  Gitea/Forgejo, several per service (uses your GitHub CLI login
  automatically). List, check out and create pull/merge requests (with the
  repository's PR templates); an **Issues** view with state, author,
  assignee, label and text filters, recent searches and "New issue"; GitHub
  notifications in the toolbar; "Open website" in the Repository menu; the
  remote used for pull requests and issues can be picked when a repository
  has several. The clone dialog lists the accounts' repositories (SSH or
  HTTPS URL).
- **Authentication**: HTTPS usernames/passwords and SSH passphrases asked in
  a window (Ferrum is git's `GIT_ASKPASS`/`SSH_ASKPASS`; answers can be
  remembered until it closes), unknown SSH hosts shown with their
  fingerprints before trusting them. Settings > SSH keys lists the keys in
  `~/.ssh` and other locations, generates ED25519 keys, adds them to the
  ssh-agent (which remembers the passphrase) or to a GitHub/GitLab/Gitea
  account; a first-time SSH assistant does it in one go.
- **AI code review** of a branch (branch menus) or of two compared commits,
  with the AI command from Settings > AI and an editable prompt.
- **Profiles**: per-repository name, email, SSH key and commit signing.
- **External tools**: open the repository in a terminal, the file manager or
  your editor/IDE (VS Code, Zed, Sublime, JetBrains, Neovim, Cursor or a custom
  command); external diff and merge tools (Meld, KDiff3, Beyond Compare,
  P4Merge, VS Code, vimdiff, git's difftool/mergetool or a custom command).
  Files in local changes, commit details, the compare view and the file tree
  open in the editor or the file manager; a diff line opens in the editor at
  that line (right-click); Ctrl+D compares a file in the external diff tool.
- **Preferences**: global git identity, git executable and verbose git output,
  code font and size, tab width, relative/absolute/long dates, system accent
  color, toolbar buttons, branch name space replacement, default merge type.
- **Command palette** (Ctrl+P): commands, branches to check out (local and
  remote), pinned refs, files (opens their history), tabs, "Create pull
  request", "Add worktree" and custom commands, grouped by kind with their
  shortcuts. Light/dark theme, zoom, tabs that reopen with the app.
- **Customizable keyboard shortcuts** (Settings > Shortcuts): click a shortcut
  and press the new keys, several keys per action, reset one or all, conflicts
  flagged.
- **Custom commands** (Settings > Custom commands): shell commands, programs
  with arguments or URLs for the repository, a commit, a branch, a file or a
  submodule, in the context menus (submenus with `/` in the name) and the
  palette. Variables `$SHA`, `$SHORT_SHA`, `$branch`, `$file`, `$path`,
  `$remote`, `$submodule`, `$selected_shas` (shell-quoted when needed); an
  optional form before running (text, branch, file, checkbox, choice, each
  value becoming `$name`); per-system commands; confirm, show output (live
  window, also in Activity) and refresh after. Teams can share them in
  `.ferrum/custom-commands.json` (same format as "Copy as JSON"); each shared
  command shows its text and asks for approval before its first run, and again
  when it changes.
- **Status bar**: the current branch (a click opens quick checkout) with its
  upstream and ahead/behind, the repository state (merging, rebasing,
  bisecting…), the running operation with a spinner, its progress and a cancel
  button, the graph filter, the last fetch time and the author identity (a
  click edits it). The pulse button and the running operation open the
  **Activity** window with every operation, the exact git commands, their
  output and result (user and background operations apart). Running
  operations (fetch, pull, push, clone…) can be cancelled.
- **Toasts**: finished operations say what they did in the bottom-right corner
  ("Pushed 3 commits to origin/main", "Fetched 2 remotes") and fade out (not
  while hovered); errors stay until dismissed, with Copy (and Delete the lock
  file when git left one behind).
- **Live refresh** without freezing: the working folder is watched for
  changes (or polled at a set interval), reloads run in the background, and
  long operations that finish while the window is in the background raise a
  desktop notification. Window size and position, panel widths and scroll
  positions are remembered.
- **Git-flow** without the git-flow extension (toolbar ☰ > Git-flow): init
  with custom branch names and prefixes (stored in the same config keys as
  git-flow AVH), start and finish features, releases and hotfixes (no-ff,
  tag with a message, merge back into develop, delete the branch, push), and
  remove the settings. Flow branches get "Finish…" in their sidebar menu.
- **Bisect**: start from a commit's menu (or two selected commits), mark
  commits good / bad / skipped from the history menu or the banner; marked
  commits are colored in the graph, and the banner shows the commit being
  tested, a timeline of the marks, the revisions and steps left, the first bad
  commit once found (all clickable) and Reset.
- **Patches**: save or copy a commit, several selected commits or a range as
  one patch (`format-patch`), the local changes or a stash. Apply a patch
  from a file, by dropping a `.patch`/`.diff`/`.eml` file on the window or
  by pasting it (Ctrl+V in the history or local changes): a preview lists the
  commits and files and checks whether it applies cleanly, is already applied
  or needs a 3-way merge; then commit it (`am`), or only change (and
  optionally stage) the files (`apply`). A stopped `am` shows the banner
  with Continue, Skip and Abort.
- **Update check** once a day against GitHub Releases ("Version X available";
  nothing is downloaded or replaced). Each of these can be turned off in
  Settings.

Writes go through the `git` command line, so hooks, credential helpers, SSH
config and Git LFS behave exactly as in the terminal. Reads use libgit2 for speed.
Git must be installed and on the `PATH` (or chosen in Settings > Git).

## Keyboard shortcuts

Every shortcut except the context keys at the end of the table can be changed
in Settings > Shortcuts. Defaults:

| Action | Shortcut |
|---|---|
| Command palette | Ctrl+P or Ctrl+Shift+P |
| Open repository | Ctrl+O |
| Settings | Ctrl+, |
| Refresh | F5 or Ctrl+R |
| Open Quickly (search known repositories) | Ctrl+Shift+O |
| New tab (Repository Manager) / reopen closed tab | Ctrl+T / Ctrl+Shift+T |
| History (again: jump to HEAD) / Local changes | Ctrl+1 / Ctrl+2 |
| Search commits (or find in the diff under the pointer) | Ctrl+F |
| Next / previous match | F3 / Shift+F3 (Enter / Shift+Enter in the search field) |
| Jump to HEAD | Ctrl+0 |
| Go to commit (Go to line in the blame under the pointer) | Ctrl+G |
| Only the current branch in the graph | Ctrl+Shift+A |
| Show reflog | Ctrl+Shift+. |
| Show / hide the commit details | Ctrl+Alt+D |
| Large diff window (diff focused) / large commit details (commit list focused) | Space |
| Copy commit info (commit list focused) | Ctrl+Shift+C |
| External diff of the selected file | Ctrl+D |
| Previous / next hunk in the diff | Ctrl+Up / Ctrl+Down |
| Blame: back / forward | Alt+Left / Alt+Right |
| Commit (in the message box) | Ctrl+Enter |
| Commit and push | Ctrl+Shift+Enter |
| Check out a branch (search) | Ctrl+B |
| New branch | Ctrl+Shift+B |
| New tag (at the selected commit, else HEAD) | Ctrl+Shift+G |
| Rename the selected sidebar branch | F2 |
| Delete the selected sidebar branch, tag or stash (asks first) | Delete |
| Stash (Ctrl/Alt+click on the Stash button: quick stash) | Ctrl+Shift+S |
| Fetch / Pull / Push (dialog) | Ctrl+Shift+F / Ctrl+Shift+D / Ctrl+Shift+U |
| Undo | Ctrl+Z |
| Open in terminal / show in file manager | Ctrl+Alt+T / Ctrl+Alt+O |
| Close tab, next / previous tab | Ctrl+W, Ctrl+Tab / Ctrl+Shift+Tab |
| Quit | Ctrl+Q |
| *Context keys (fixed):* | |
| Copy commit id (commit list focused) | Ctrl+C |
| Apply the patch on the clipboard (history or local changes focused) | Ctrl+V |
| Repository Manager: open, rename, remove from the list | Enter, F2, Delete |
| Collapse / expand the selected merge | Left / Right |
| Main menu: open the first menu / a menu / switch menus | F10 / Alt+letter / Left, Right |
| Stage / unstage selected files | Enter (Ctrl/Shift-click to select several) |
| Fetch / Pull / Push with options | Alt+click, right-click or ⏷ on the toolbar button |
| Quick stash (no dialog) | Ctrl/Alt+click Stash in the toolbar |
| Move a tab / close a tab | Drag it / middle-click or × |
| Compare two commits / select several | Ctrl+click / Shift+click |
| Move between sidebar, commit list and details | Tab / Shift+Tab |
| Sidebar: move, open/close, check out, select | Arrows, Home/End, Enter, Space |
| Interactive rebase: pick, reword, edit, squash, fixup, drop | P, R, E, S, F, D |
| Interactive rebase: move the selected commits | Alt+Up / Alt+Down |

On macOS, Cmd replaces Ctrl.

### Single instance and links

`ferrum PATH` and `ferrum log -- FILE` hand the request to a running Ferrum
(through a socket in its data folder) and exit; set
`FERRUM_NO_SINGLE_INSTANCE=1` to always start a new window. To clone from the
browser's "Open in Desktop" buttons, register Ferrum for the
`x-github-client` and `sourcetree` URL schemes: the Linux `.desktop` file does
it already; on Windows add `HKEY_CURRENT_USER\Software\Classes\x-github-client`
(with an empty `URL Protocol` value and `shell\open\command` =
`"C:\path\to\ferrum.exe" "%1"`), and on macOS add the schemes to
`CFBundleURLTypes` in `Ferrum.app/Contents/Info.plist`.

## Structure

- `crates/ferrum-core` — Git logic (libgit2 reads, git CLI writes), no UI code
- `crates/ferrum-app` — desktop app (`egui`/`eframe`), binary `ferrum`
- `crates/ferrum-app/assets/fonts` — embedded fonts (Inter and JetBrains Mono
  under the OFL, Phosphor icons under MIT; licenses next to them), cut down by
  `scripts/build-fonts.py`

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
  `diff:sbs,wrap,find=TEXT,large,...`, see `diff_prefs::dev_step`; details:
  `details:tree;details:open=src;details:select=src/main.rs`,
  `details:compare=0,3`, `details:workdir=1`, `details:popup`, `details:window`,
  see `details::dev_step`;
  merge, rebase and cherry-pick dialogs: `flow:merge=REV`, `flow:rebase=REV`,
  `flow:pick=REV,REV`, `flow:select=REV,REV`, see `flow::dev_steps`;
  `gitflow:init|start=feature|finish=BRANCH`, `bisect:dialog|start=BAD,GOOD`,
  `patch:FILE`;
  repositories: `manager`, `quick:TEXT`, `overview`, `repo-settings`,
  `workspace:NAME`, `vertical-tabs`, see `app/repos.rs`;
  hosted services: `issues`, `settings:accounts`, `settings:ssh`,
  `hosted:askpass[=PROMPT]`, `hosted:review=BASE..HEAD`, `hosted:ssh-setup`,
  `hosted:trust=HOST`, `hosted:notifications`, `hosted:new-issue`, see
  `app/hosted.rs`; look: `theme:dark|light|contrast`, `density:compact`;
  sample toasts: `toast:success|info|error`).
- `FERRUM_WINDOW_SIZE=1000x700` sets the first window size (narrow layouts).

- `FERRUM_TIMING=1` prints how long reloading a repository takes.
- `ferrum --benchmark <repo> [runs]` times each reload step (branches, commits,
  graph layout, status…) and prints the best and average of each.

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT), at your option.
