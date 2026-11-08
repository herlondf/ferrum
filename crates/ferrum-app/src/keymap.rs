//! Keyboard shortcuts: every action with its default keys, the user's
//! overrides (Settings > Shortcuts) and the lookups the views use.
//!
//! To add an action, add one line to `actions!` below (variant, settings
//! id, label, default keys), then check it with `keymap::pressed(ctx,
//! Action::Name)` where it applies. "Mod" is Ctrl, or Cmd on macOS.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

use eframe::egui::{self, Event, Key, Modifiers};

/// Saved overrides: action id to its keys (an empty list unbinds it).
pub type Overrides = BTreeMap<String, Vec<String>>;

macro_rules! actions {
    ($($name:ident = $id:literal, $label:expr, [$($key:literal),*];)*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Action { $($name),* }

        impl Action {
            pub const ALL: &[Action] = &[$(Action::$name),*];

            /// Name used in the settings file.
            pub fn id(self) -> &'static str {
                match self { $(Action::$name => $id),* }
            }

            pub fn label(self) -> String {
                match self { $(Action::$name => $label),* }
            }

            pub fn defaults(self) -> &'static [&'static str] {
                match self { $(Action::$name => &[$($key),*]),* }
            }
        }
    };
}

#[rustfmt::skip]
actions! {
    Palette = "palette", t!("Command palette"), ["Mod+P", "Mod+Shift+P"];
    OpenRepo = "open_repository", t!("Open repository"), ["Mod+O"];
    Settings = "settings", t!("Settings"), ["Mod+Comma"];
    Refresh = "refresh", t!("Refresh"), ["F5", "Mod+R"];
    Find = "find", t!("Search commits (or find in the diff under the pointer)"), ["Mod+F"];
    NextMatch = "next_match", t!("Next match"), ["F3"];
    PrevMatch = "previous_match", t!("Previous match"), ["Shift+F3"];
    ShowHistory = "show_history", t!("History (again: jump to HEAD)"), ["Mod+1"];
    ShowChanges = "show_changes", t!("Local changes"), ["Mod+2"];
    JumpToHead = "jump_to_head", t!("Jump to HEAD"), ["Mod+0"];
    GoTo = "go_to", t!("Go to commit (Go to line in the blame under the pointer)"), ["Mod+G"];
    ActiveBranch = "active_branch_only", t!("Only the current branch in the graph"), ["Mod+Shift+A"];
    Reflog = "reflog", t!("Show reflog"), ["Mod+Shift+Period"];
    ToggleDetails = "toggle_details", t!("Show / hide the commit details"), ["Mod+Alt+D"];
    LargeView = "large_view", t!("Large diff window / large commit details"), ["Space"];
    CopyCommitInfo = "copy_commit_info", t!("Copy commit info"), ["Mod+Shift+C"];
    ExternalDiff = "external_diff", t!("External diff of the selected file"), ["Mod+D"];
    NextHunk = "next_hunk", t!("Next hunk in the diff"), ["Mod+Down"];
    PrevHunk = "previous_hunk", t!("Previous hunk in the diff"), ["Mod+Up"];
    BlameBack = "blame_back", t!("Blame: back"), ["Alt+Left"];
    BlameForward = "blame_forward", t!("Blame: forward"), ["Alt+Right"];
    Commit = "commit", t!("Commit (in the message box)"), ["Mod+Enter"];
    CommitPush = "commit_and_push", t!("Commit and push"), ["Mod+Shift+Enter"];
    QuickCheckout = "quick_checkout", t!("Check out a branch (search)"), ["Mod+B"];
    NewBranch = "new_branch", t!("New branch"), ["Mod+Shift+B"];
    NewTag = "new_tag", t!("New tag"), ["Mod+Shift+G"];
    Rename = "rename", t!("Rename the selected sidebar branch"), ["F2"];
    DeleteRef = "delete_ref", t!("Delete the selected sidebar branch, tag or stash"), ["Delete"];
    Stash = "stash", t!("Stash changes"), ["Mod+Shift+S"];
    Fetch = "fetch", t!("Fetch all remotes"), ["Mod+Shift+F"];
    Pull = "pull", t!("Pull"), ["Mod+Shift+D"];
    Push = "push", t!("Push (dialog)"), ["Mod+Shift+U"];
    Undo = "undo", t!("Undo the last operation"), ["Mod+Z"];
    Terminal = "open_terminal", t!("Open in terminal"), ["Mod+Alt+T"];
    FileManager = "open_file_manager", t!("Show in file manager"), ["Mod+Alt+O"];
    CloseTab = "close_tab", t!("Close tab"), ["Mod+W"];
    NewTab = "new_tab", t!("New tab (Repository Manager)"), ["Mod+T"];
    ReopenTab = "reopen_tab", t!("Reopen closed tab"), ["Mod+Shift+T"];
    OpenQuickly = "open_quickly", t!("Open Quickly (search known repositories)"), ["Mod+Shift+O"];
    NextTab = "next_tab", t!("Next tab"), ["Mod+Tab"];
    PrevTab = "previous_tab", t!("Previous tab"), ["Mod+Shift+Tab"];
    Quit = "quit", t!("Quit"), ["Mod+Q"];
}

/// A key with modifiers. `command` is Ctrl, or Cmd on macOS; `ctrl` is the
/// Control key on macOS only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Combo {
    pub command: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: Key,
}

const MAC: bool = cfg!(target_os = "macos");

impl Combo {
    pub fn parse(s: &str) -> Option<Combo> {
        let s = s.trim();
        // "Mod++" ends with the Plus key.
        let (mods, key) = match s.strip_suffix("++") {
            Some(m) => (m, "Plus"),
            None => s.rsplit_once('+').unwrap_or(("", s)),
        };
        let mut c = Combo {
            command: false,
            ctrl: false,
            alt: false,
            shift: false,
            key: Key::from_name(key.trim())?,
        };
        for m in mods.split('+').map(str::trim).filter(|m| !m.is_empty()) {
            match m.to_lowercase().as_str() {
                "mod" | "cmd" | "command" => c.command = true,
                "ctrl" | "control" if MAC => c.ctrl = true,
                "ctrl" | "control" => c.command = true,
                "alt" | "option" => c.alt = true,
                "shift" => c.shift = true,
                _ => return None,
            }
        }
        Some(c)
    }

    /// The form saved in the settings file, e.g. `Mod+Shift+P`.
    pub fn id(&self) -> String {
        let mut s = String::new();
        for (on, name) in [
            (self.command, "Mod+"),
            (self.ctrl, "Ctrl+"),
            (self.alt, "Alt+"),
            (self.shift, "Shift+"),
        ] {
            if on {
                s.push_str(name);
            }
        }
        s.push_str(self.key.name());
        s
    }

    /// As shown to the user, e.g. `Ctrl+Shift+P` (`Cmd+Shift+P` on macOS).
    pub fn text(&self) -> String {
        let mut s = String::new();
        for (on, name) in [
            (self.command, if MAC { "Cmd+" } else { "Ctrl+" }),
            (self.ctrl, "Ctrl+"),
            (self.alt, if MAC { "Option+" } else { "Alt+" }),
            (self.shift, "Shift+"),
        ] {
            if on {
                s.push_str(name);
            }
        }
        s.push_str(key_text(self.key));
        s
    }

    fn from_event(key: Key, m: Modifiers) -> Combo {
        Combo {
            command: m.command,
            ctrl: MAC && m.ctrl,
            alt: m.alt,
            shift: m.shift,
            key,
        }
    }

    /// Whether a key event is this combination (modifiers must match exactly).
    fn matches(&self, key: Key, physical: Option<Key>, m: Modifiers) -> bool {
        // Shifted symbols (Shift+. is `>` on many layouts): use the physical key.
        let key_ok = key == self.key || (m.shift && physical == Some(self.key));
        let cmd_ok = if self.command || self.ctrl {
            (!self.command || m.command) && (!self.ctrl || m.ctrl) && (!MAC || self.ctrl == m.ctrl)
        } else {
            !m.ctrl && !m.command && !m.mac_cmd
        };
        key_ok && cmd_ok && m.alt == self.alt && m.shift == self.shift
    }

    /// No Ctrl/Cmd/Alt and not a function key: text fields need it.
    fn types_text(&self) -> bool {
        let name = self.key.name();
        let function_key = name.len() > 1
            && name.starts_with('F')
            && name[1..].chars().all(|c| c.is_ascii_digit());
        !self.command && !self.ctrl && !self.alt && !function_key
    }
}

fn key_text(key: Key) -> &'static str {
    match key {
        Key::ArrowUp | Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight => key.name(),
        Key::Comma | Key::Period | Key::Minus | Key::Equals | Key::Slash | Key::Backslash => {
            key.symbol_or_name()
        }
        Key::Semicolon | Key::OpenBracket | Key::CloseBracket | Key::Backtick | Key::Quote => {
            key.symbol_or_name()
        }
        _ => key.name(),
    }
}

/// The keys of every action, with the user's overrides applied.
#[derive(Debug, Clone, Default)]
pub struct Keymap {
    keys: HashMap<Action, Vec<Combo>>,
}

impl Keymap {
    pub fn new(overrides: &Overrides) -> Self {
        let keys = Action::ALL
            .iter()
            .map(|&a| {
                let list: Vec<Combo> = match overrides.get(a.id()) {
                    Some(list) => list.iter().filter_map(|s| Combo::parse(s)).collect(),
                    None => a
                        .defaults()
                        .iter()
                        .filter_map(|s| Combo::parse(s))
                        .collect(),
                };
                (a, list)
            })
            .collect();
        Self { keys }
    }

    pub fn keys(&self, a: Action) -> &[Combo] {
        self.keys.get(&a).map_or(&[], Vec::as_slice)
    }

    /// Combinations bound to more than one action.
    pub fn conflicts(&self) -> Vec<(Combo, Vec<Action>)> {
        let mut by_combo: BTreeMap<String, (Combo, Vec<Action>)> = BTreeMap::new();
        for &a in Action::ALL {
            for c in self.keys(a) {
                by_combo.entry(c.id()).or_insert((*c, Vec::new())).1.push(a);
            }
        }
        by_combo
            .into_values()
            .filter(|(_, list)| list.len() > 1)
            .collect()
    }

    /// Other actions that use `combo`.
    pub fn users_of(&self, combo: &Combo, except: Action) -> Vec<Action> {
        Action::ALL
            .iter()
            .copied()
            .filter(|&a| a != except && self.keys(a).contains(combo))
            .collect()
    }
}

/// Overrides with `a` set to `keys` (dropped again when they are the defaults).
pub fn set_keys(overrides: &mut Overrides, a: Action, keys: &[Combo]) {
    let ids: Vec<String> = keys.iter().map(Combo::id).collect();
    let defaults: Vec<String> = a
        .defaults()
        .iter()
        .filter_map(|s| Combo::parse(s))
        .map(|c| c.id())
        .collect();
    if ids == defaults {
        overrides.remove(a.id());
    } else {
        overrides.insert(a.id().to_owned(), ids);
    }
}

struct State {
    overrides: Option<Overrides>,
    map: Keymap,
    /// Settings > Shortcuts waits for a key: `Some(None)` while waiting.
    capture: Option<Option<Combo>>,
    /// The recording editor was drawn last frame (else recording stops).
    alive: bool,
    /// Actions picked in a menu, run next frame as if their key was pressed.
    queued: Vec<Action>,
    triggered: Vec<Action>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State {
        overrides: None,
        map: Keymap::new(&Overrides::new()),
        capture: None,
        alive: false,
        queued: Vec::new(),
        triggered: Vec::new(),
    });
}

/// Called at the start of each frame with the saved overrides. While a
/// shortcut is being recorded, takes the keys before anything else sees them.
pub fn begin_frame(ctx: &egui::Context, overrides: &Overrides) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if s.overrides.as_ref() != Some(overrides) {
            s.map = Keymap::new(overrides);
            s.overrides = Some(overrides.clone());
        }
        s.triggered = std::mem::take(&mut s.queued);
        if !std::mem::take(&mut s.alive) {
            s.capture = None;
        }
        if s.capture == Some(None) {
            let hit = ctx.input_mut(|i| {
                let hit = i.events.iter().find_map(|e| match e {
                    Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => Some(Combo::from_event(*key, *modifiers)),
                    _ => None,
                });
                i.events
                    .retain(|e| !matches!(e, Event::Key { .. } | Event::Text(_)));
                hit
            });
            if let Some(c) = hit {
                let plain_escape =
                    c.key == Key::Escape && c == Combo::from_event(c.key, Modifiers::NONE);
                s.capture = if plain_escape { None } else { Some(Some(c)) };
            }
        }
    });
}

/// Runs `a` next frame as if one of its keys was pressed (menus use it).
pub fn trigger(ctx: &egui::Context, a: Action) {
    STATE.with(|s| s.borrow_mut().queued.push(a));
    ctx.request_repaint();
}

/// Starts recording the next key combination (Escape cancels).
pub fn start_capture() {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.capture = Some(None);
        s.alive = true;
    });
}

/// Called every frame by the editor while it records.
pub fn keep_capture() {
    STATE.with(|s| s.borrow_mut().alive = true);
}

pub fn stop_capture() {
    STATE.with(|s| s.borrow_mut().capture = None);
}

pub fn capturing() -> bool {
    STATE.with(|s| s.borrow().capture.is_some())
}

/// The recorded combination, once a key was pressed.
pub fn take_captured() -> Option<Combo> {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        match s.capture {
            Some(Some(c)) => {
                s.capture = None;
                Some(c)
            }
            _ => None,
        }
    })
}

/// Whether a key of `a` was pressed this frame; consumes the key.
pub fn pressed(ctx: &egui::Context, a: Action) -> bool {
    check(ctx, a, true)
}

/// Like `pressed`, without consuming the key.
pub fn peek(ctx: &egui::Context, a: Action) -> bool {
    check(ctx, a, false)
}

fn check(ctx: &egui::Context, a: Action, consume: bool) -> bool {
    let picked = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let pos = s.triggered.iter().position(|&t| t == a);
        if consume && let Some(i) = pos {
            s.triggered.remove(i);
        }
        pos.is_some()
    });
    if picked {
        return true;
    }
    let keys: Vec<Combo> = STATE.with(|s| {
        let s = s.borrow();
        if s.capture.is_some() {
            Vec::new()
        } else {
            s.map.keys(a).to_vec()
        }
    });
    if keys.is_empty() {
        return false;
    }
    let typing = ctx.text_edit_focused();
    let keys: Vec<Combo> = keys
        .into_iter()
        .filter(|c| !(typing && c.types_text()))
        .collect();
    ctx.input_mut(|i| {
        let hit = |e: &Event| match e {
            Event::Key {
                key,
                physical_key,
                pressed: true,
                modifiers,
                ..
            } => keys
                .iter()
                .any(|c| c.matches(*key, *physical_key, *modifiers)),
            _ => false,
        };
        let found = i.events.iter().any(hit);
        if found && consume {
            i.events.retain(|e| !hit(e));
        }
        found
    })
}

/// The first key of `a` as text ("" when unbound), e.g. for menus.
pub fn text(a: Action) -> String {
    STATE.with(|s| {
        s.borrow()
            .map
            .keys(a)
            .first()
            .map(Combo::text)
            .unwrap_or_default()
    })
}

/// All keys of `a` as text, e.g. "F5 / Ctrl+R".
pub fn all_text(map: &Keymap, a: Action) -> String {
    let list: Vec<String> = map.keys(a).iter().map(Combo::text).collect();
    list.join(" / ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_parse() {
        for &a in Action::ALL {
            for k in a.defaults() {
                assert!(Combo::parse(k).is_some(), "{k} of {a:?}");
            }
        }
    }

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<&str> = Action::ALL.iter().map(|a| a.id()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), Action::ALL.len());
    }

    #[test]
    fn defaults_do_not_conflict() {
        let map = Keymap::new(&Overrides::new());
        assert!(map.conflicts().is_empty(), "{:?}", map.conflicts());
    }

    #[test]
    fn parses_and_formats() {
        let c = Combo::parse("Mod+Shift+P").unwrap();
        assert!(c.command && c.shift && !c.alt && c.key == Key::P);
        assert_eq!(c.id(), "Mod+Shift+P");
        assert_eq!(Combo::parse("Mod+Comma").unwrap().id(), "Mod+Comma");
        assert_eq!(Combo::parse("Mod+,").unwrap().key, Key::Comma);
        assert_eq!(Combo::parse("Mod++").unwrap().key, Key::Plus);
        assert!(Combo::parse("Hyper+P").is_none());
        assert!(Combo::parse("Mod+Nothing").is_none());
        if !MAC {
            assert_eq!(Combo::parse("Ctrl+Shift+.").unwrap().text(), "Ctrl+Shift+.");
            assert_eq!(Combo::parse("Ctrl+B"), Combo::parse("Mod+B"));
        }
    }

    #[test]
    fn modifiers_match_exactly() {
        let b = Combo::parse("Mod+B").unwrap();
        let cmd = Modifiers::COMMAND;
        assert!(b.matches(Key::B, None, cmd));
        assert!(!b.matches(Key::B, None, cmd | Modifiers::SHIFT));
        assert!(!b.matches(Key::B, None, Modifiers::NONE));
        let f2 = Combo::parse("F2").unwrap();
        assert!(f2.matches(Key::F2, None, Modifiers::NONE));
        assert!(!f2.matches(Key::F2, None, cmd));
        // Shift+. may arrive as another logical key.
        let reflog = Combo::parse("Mod+Shift+Period").unwrap();
        assert!(reflog.matches(Key::Questionmark, Some(Key::Period), cmd | Modifiers::SHIFT));
    }

    #[test]
    fn overrides_replace_defaults_and_conflicts_are_found() {
        let mut o = Overrides::new();
        let ctrl_b = Combo::parse("Mod+B").unwrap();
        set_keys(&mut o, Action::Fetch, &[ctrl_b]);
        let map = Keymap::new(&o);
        assert_eq!(map.keys(Action::Fetch), &[ctrl_b]);
        let conflicts = map.conflicts();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].1, [Action::QuickCheckout, Action::Fetch]);
        assert_eq!(
            map.users_of(&ctrl_b, Action::Fetch),
            [Action::QuickCheckout]
        );
        // Back to the defaults: the override is dropped.
        let f = Combo::parse("Mod+Shift+F").unwrap();
        set_keys(&mut o, Action::Fetch, &[f]);
        assert!(o.is_empty());
        // Unbound.
        set_keys(&mut o, Action::Fetch, &[]);
        assert!(Keymap::new(&o).keys(Action::Fetch).is_empty());
    }

    #[test]
    fn text_fields_keep_plain_keys() {
        assert!(Combo::parse("Space").unwrap().types_text());
        assert!(Combo::parse("Delete").unwrap().types_text());
        assert!(Combo::parse("N").unwrap().types_text());
        assert!(!Combo::parse("F2").unwrap().types_text());
        assert!(Combo::parse("F").unwrap().types_text());
        assert!(!Combo::parse("Mod+Z").unwrap().types_text());
    }
}
