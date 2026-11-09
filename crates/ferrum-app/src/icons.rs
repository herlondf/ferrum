//! Icons: Phosphor (regular weight, MIT), embedded as a font (see `fonts`).
//!
//! An icon is a `&str` holding one private-use character, so it goes
//! anywhere text goes: `ui.button(icons::text(icons::BRANCH, "Branch"))`.
//!
//! - `text(icon, label)`: "icon label" for buttons, labels and menu items.
//! - `button(icon, label)`: an `egui::Button` with an icon and text.
//! - `icon_button(icon)`: an icon-only button; add `on_hover_text`.
//! - `tinted(icon, color, label)`: icon in its own color, text in the widget's.
//!
//! The constants are named after Phosphor (<https://phosphoricons.com>); the
//! aliases at the end say which icon means what in Ferrum, so views stay
//! consistent: prefer them. The font has all 1530 Phosphor icons: to add one,
//! look its codepoint up with `scripts/build-fonts.py SRC --lookup NAME` (or
//! in Phosphor's `style.css`) and add it to `ALL` too.
//!
//! The older Unicode symbols in strings (⚠ ✔ 🌿 📁 ⬆ …) are drawn with the
//! matching Phosphor icon (the font maps them, see `LEGACY` in
//! `scripts/build-fonts.py`), so translated texts keep the same look; prefer
//! the constants in new code.

// A palette for views: not every icon is used yet.
#![allow(dead_code)]

use eframe::egui::{self, Color32, RichText, WidgetText, text::LayoutJob};

pub const ARCHIVE: &str = "\u{e00c}";
pub const ARCHIVE_BOX: &str = "\u{e00e}";
pub const ARROWS_OUT_LINE_VERTICAL: &str = "\u{e536}";
pub const ARROW_BEND_LEFT_UP: &str = "\u{e01e}";
pub const ARROW_CIRCLE_UP: &str = "\u{e030}";
pub const ARROW_CLOCKWISE: &str = "\u{e036}";
pub const ARROW_COUNTER_CLOCKWISE: &str = "\u{e038}";
pub const ARROW_DOWN: &str = "\u{e03e}";
pub const ARROW_ELBOW_DOWN_LEFT: &str = "\u{e044}";
pub const ARROW_ELBOW_RIGHT_UP: &str = "\u{e052}";
pub const ARROW_LEFT: &str = "\u{e058}";
pub const ARROW_LINE_DOWN: &str = "\u{e05c}";
pub const ARROW_LINE_UP: &str = "\u{e066}";
pub const ARROW_RIGHT: &str = "\u{e06c}";
pub const ARROW_SQUARE_OUT: &str = "\u{e5de}";
pub const ARROW_U_UP_LEFT: &str = "\u{e08a}";
pub const ARROW_U_UP_RIGHT: &str = "\u{e08c}";
pub const ARROW_UP: &str = "\u{e08e}";
pub const ARROWS_CLOCKWISE: &str = "\u{e094}";
pub const ARROWS_DOWN_UP: &str = "\u{e098}";
pub const ARROWS_LEFT_RIGHT: &str = "\u{e0a0}";
pub const ARROWS_IN_SIMPLE: &str = "\u{e09e}";
pub const ARROWS_MERGE: &str = "\u{ed3e}";
pub const ARROWS_OUT_SIMPLE: &str = "\u{e0a6}";
pub const BELL: &str = "\u{e0ce}";
pub const BOOKMARK_SIMPLE: &str = "\u{e0ea}";
pub const BROWSERS: &str = "\u{e0f6}";
pub const BUG: &str = "\u{e5f4}";
pub const CALENDAR: &str = "\u{e108}";
pub const CARET_DOWN: &str = "\u{e136}";
pub const CARET_LEFT: &str = "\u{e138}";
pub const CARET_RIGHT: &str = "\u{e13a}";
pub const CARET_UP: &str = "\u{e13c}";
pub const CARET_UP_DOWN: &str = "\u{e140}";
pub const CHART_BAR: &str = "\u{e150}";
pub const CHART_LINE: &str = "\u{e154}";
pub const CHAT_TEXT: &str = "\u{e17a}";
pub const CHECK: &str = "\u{e182}";
pub const CHECK_CIRCLE: &str = "\u{e184}";
pub const CHECK_SQUARE: &str = "\u{e186}";
pub const CHERRIES: &str = "\u{e830}";
pub const CIRCLE: &str = "\u{e18a}";
pub const CIRCLE_DASHED: &str = "\u{e602}";
pub const CLIPBOARD_TEXT: &str = "\u{e198}";
pub const CLOCK: &str = "\u{e19a}";
pub const CLOCK_COUNTER_CLOCKWISE: &str = "\u{e1a0}";
pub const CLOUD: &str = "\u{e1aa}";
pub const CLOUD_ARROW_DOWN: &str = "\u{e1ac}";
pub const CLOUD_ARROW_UP: &str = "\u{e1ae}";
pub const CLOUD_CHECK: &str = "\u{e1b0}";
pub const CODE: &str = "\u{e1bc}";
pub const COLUMNS: &str = "\u{e546}";
pub const COMMAND: &str = "\u{e1c4}";
pub const COPY: &str = "\u{e1ca}";
pub const CORNERS_IN: &str = "\u{e1ce}";
pub const CORNERS_OUT: &str = "\u{e1d0}";
pub const CUBE: &str = "\u{e1da}";
pub const CURSOR_CLICK: &str = "\u{e7c8}";
pub const DATABASE: &str = "\u{e1de}";
pub const DESKTOP: &str = "\u{e560}";
pub const DOT_OUTLINE: &str = "\u{ece0}";
pub const DOTS_SIX_VERTICAL: &str = "\u{eae2}";
pub const DOTS_THREE: &str = "\u{e1fe}";
pub const DOTS_THREE_VERTICAL: &str = "\u{e208}";
pub const DOWNLOAD_SIMPLE: &str = "\u{e20c}";
pub const ENVELOPE: &str = "\u{e214}";
pub const EYE: &str = "\u{e220}";
pub const EYE_SLASH: &str = "\u{e224}";
pub const FILE: &str = "\u{e230}";
pub const FILE_CODE: &str = "\u{e914}";
pub const FILE_DASHED: &str = "\u{e704}";
pub const FILE_MINUS: &str = "\u{e234}";
pub const FILE_PLUS: &str = "\u{e236}";
pub const FILE_TEXT: &str = "\u{e23a}";
pub const FILES: &str = "\u{e710}";
pub const FLAG: &str = "\u{e244}";
pub const FLAME: &str = "\u{e624}";
pub const FOLDER: &str = "\u{e24a}";
pub const FOLDER_OPEN: &str = "\u{e256}";
pub const FOLDER_PLUS: &str = "\u{e258}";
pub const FOLDER_SIMPLE: &str = "\u{e25a}";
pub const FOLDER_SIMPLE_PLUS: &str = "\u{e25e}";
pub const FUNNEL: &str = "\u{e266}";
pub const GEAR_SIX: &str = "\u{e272}";
pub const GIT_BRANCH: &str = "\u{e278}";
pub const GIT_COMMIT: &str = "\u{e27a}";
pub const GIT_DIFF: &str = "\u{e27c}";
pub const GIT_FORK: &str = "\u{e27e}";
pub const GIT_MERGE: &str = "\u{e280}";
pub const GIT_PULL_REQUEST: &str = "\u{e282}";
pub const GITHUB_LOGO: &str = "\u{e576}";
pub const GITLAB_LOGO: &str = "\u{e694}";
pub const GLOBE: &str = "\u{e288}";
pub const HAND_GRABBING: &str = "\u{e57c}";
pub const HARD_DRIVES: &str = "\u{e2a0}";
pub const HASH: &str = "\u{e2a2}";
pub const HOURGLASS: &str = "\u{e2b2}";
pub const HOUSE: &str = "\u{e2c2}";
pub const IMAGE: &str = "\u{e2ca}";
pub const IMAGE_BROKEN: &str = "\u{e7a8}";
pub const INFO: &str = "\u{e2ce}";
pub const KEY: &str = "\u{e2d6}";
pub const KEYBOARD: &str = "\u{e2d8}";
pub const LIGHTBULB: &str = "\u{e2dc}";
pub const LINK: &str = "\u{e2e2}";
pub const LIST: &str = "\u{e2f0}";
pub const LOCK: &str = "\u{e2fa}";
pub const LOCK_OPEN: &str = "\u{e306}";
pub const MAGIC_WAND: &str = "\u{e6b6}";
pub const MAGNIFYING_GLASS: &str = "\u{e30c}";
pub const MINUS: &str = "\u{e32a}";
pub const MONITOR: &str = "\u{e32e}";
pub const MOON: &str = "\u{e330}";
pub const NOTE_PENCIL: &str = "\u{e34c}";
pub const PACKAGE: &str = "\u{e390}";
pub const PALETTE: &str = "\u{e6c8}";
pub const PARAGRAPH: &str = "\u{e960}";
pub const PAUSE: &str = "\u{e39e}";
pub const PENCIL_SIMPLE: &str = "\u{e3b4}";
pub const PLAY: &str = "\u{e3d0}";
pub const PLUS: &str = "\u{e3d4}";
pub const PLUS_CIRCLE: &str = "\u{e3d6}";
pub const PROHIBIT: &str = "\u{e3de}";
pub const PULSE: &str = "\u{e000}";
pub const PUSH_PIN: &str = "\u{e3e2}";
pub const PUSH_PIN_SLASH: &str = "\u{e3e4}";
pub const QUESTION: &str = "\u{e3e8}";
pub const REPEAT: &str = "\u{e3f6}";
pub const ROBOT: &str = "\u{e762}";
pub const ROWS: &str = "\u{e5a2}";
pub const SIDEBAR_SIMPLE: &str = "\u{ec24}";
pub const SIGN_OUT: &str = "\u{e42a}";
pub const SLIDERS_HORIZONTAL: &str = "\u{e434}";
pub const SPARKLE: &str = "\u{e6a2}";
pub const SQUARE: &str = "\u{e45e}";
pub const SQUARES_FOUR: &str = "\u{e464}";
pub const STACK: &str = "\u{e466}";
pub const STAR: &str = "\u{e46a}";
pub const SUN: &str = "\u{e472}";
pub const TAG: &str = "\u{e478}";
pub const TERMINAL_WINDOW: &str = "\u{eae8}";
pub const TEXT_AA: &str = "\u{e6ee}";
pub const TEXT_ALIGN_LEFT: &str = "\u{e484}";
pub const TOOLBOX: &str = "\u{eca0}";
pub const TREE_STRUCTURE: &str = "\u{e67c}";
pub const TRASH: &str = "\u{e4a6}";
pub const TRAY_ARROW_DOWN: &str = "\u{e010}";
pub const TRAY_ARROW_UP: &str = "\u{ee52}";
pub const UPLOAD_SIMPLE: &str = "\u{e4c0}";
pub const USER: &str = "\u{e4c2}";
pub const USER_CIRCLE: &str = "\u{e4c4}";
pub const USERS: &str = "\u{e4d6}";
pub const WARNING: &str = "\u{e4e0}";
pub const WARNING_CIRCLE: &str = "\u{e4e2}";
pub const WRENCH: &str = "\u{e5d4}";
pub const X: &str = "\u{e4f6}";
pub const X_CIRCLE: &str = "\u{e4f8}";

// ----- What each icon means in Ferrum -----

pub const BRANCH: &str = GIT_BRANCH;
pub const REMOTE: &str = CLOUD;
pub const STASH: &str = ARCHIVE;
pub const SUBMODULE: &str = CUBE;
pub const WORKTREE: &str = FOLDER_OPEN;
pub const FETCH: &str = ARROWS_CLOCKWISE;
pub const PULL: &str = ARROW_DOWN;
pub const PUSH: &str = ARROW_UP;
pub const POP: &str = TRAY_ARROW_UP;
pub const UNDO: &str = ARROW_U_UP_LEFT;
pub const REFRESH: &str = ARROW_CLOCKWISE;
pub const TERMINAL: &str = TERMINAL_WINDOW;
pub const FILE_MANAGER: &str = FOLDER_OPEN;
pub const SETTINGS: &str = GEAR_SIX;
pub const COMMAND_PALETTE: &str = COMMAND;
pub const SEARCH: &str = MAGNIFYING_GLASS;
pub const MENU: &str = LIST;
pub const MORE: &str = DOTS_THREE;
pub const DROPDOWN: &str = CARET_DOWN;
pub const EXPANDED: &str = CARET_DOWN;
pub const COLLAPSED: &str = CARET_RIGHT;
pub const CLOSE: &str = X;
pub const ADD: &str = PLUS;
pub const LOCAL_CHANGES: &str = NOTE_PENCIL;
pub const HISTORY: &str = CLOCK_COUNTER_CLOCKWISE;
pub const OVERVIEW: &str = CHART_BAR;
pub const PULL_REQUEST: &str = GIT_PULL_REQUEST;
pub const ISSUES: &str = CLIPBOARD_TEXT;
pub const PIN: &str = PUSH_PIN;
pub const IDENTITY: &str = USER;
pub const REPOSITORIES: &str = SQUARES_FOUR;
pub const CURRENT: &str = CHECK;
pub const AI: &str = SPARKLE;

/// Size of icon-only buttons.
pub const ICON_SIZE: f32 = 15.0;

/// "icon label", for buttons, labels and menu items.
pub fn text(icon: &str, label: impl AsRef<str>) -> String {
    format!("{icon} {}", label.as_ref())
}

/// A button with an icon and text.
pub fn button(icon: &str, label: impl AsRef<str>) -> egui::Button<'static> {
    egui::Button::new(text(icon, label))
}

/// An icon-only button, a bit larger than text. Give it a tooltip.
pub fn icon_button(icon: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(icon).size(ICON_SIZE))
}

/// An icon in `color` followed by text in the widget's own text color.
pub fn tinted(icon: &str, color: Color32, label: impl AsRef<str>) -> WidgetText {
    let style = egui::Style::default();
    let mut job = LayoutJob::default();
    RichText::new(format!("{icon} ")).color(color).append_to(
        &mut job,
        &style,
        egui::FontSelection::Default,
        egui::Align::Center,
    );
    RichText::new(label.as_ref()).append_to(
        &mut job,
        &style,
        egui::FontSelection::Default,
        egui::Align::Center,
    );
    job.into()
}

/// Every named icon, for the glyph test.
#[cfg(test)]
pub const ALL: &[&str] = &[
    ARCHIVE,
    ARCHIVE_BOX,
    ARROWS_OUT_LINE_VERTICAL,
    ARROW_BEND_LEFT_UP,
    ARROW_CIRCLE_UP,
    ARROW_CLOCKWISE,
    ARROW_COUNTER_CLOCKWISE,
    ARROW_DOWN,
    ARROW_ELBOW_DOWN_LEFT,
    ARROW_ELBOW_RIGHT_UP,
    ARROW_LEFT,
    ARROW_LINE_DOWN,
    ARROW_LINE_UP,
    ARROW_RIGHT,
    ARROW_SQUARE_OUT,
    ARROW_U_UP_LEFT,
    ARROW_U_UP_RIGHT,
    ARROW_UP,
    ARROWS_CLOCKWISE,
    ARROWS_DOWN_UP,
    ARROWS_LEFT_RIGHT,
    ARROWS_IN_SIMPLE,
    ARROWS_MERGE,
    ARROWS_OUT_SIMPLE,
    BELL,
    BOOKMARK_SIMPLE,
    BROWSERS,
    BUG,
    CALENDAR,
    CARET_DOWN,
    CARET_LEFT,
    CARET_RIGHT,
    CARET_UP,
    CARET_UP_DOWN,
    CHART_BAR,
    CHART_LINE,
    CHAT_TEXT,
    CHECK,
    CHECK_CIRCLE,
    CHECK_SQUARE,
    CHERRIES,
    CIRCLE,
    CIRCLE_DASHED,
    CLIPBOARD_TEXT,
    CLOCK,
    CLOCK_COUNTER_CLOCKWISE,
    CLOUD,
    CLOUD_ARROW_DOWN,
    CLOUD_ARROW_UP,
    CLOUD_CHECK,
    CODE,
    COLUMNS,
    COMMAND,
    COPY,
    CORNERS_IN,
    CORNERS_OUT,
    CUBE,
    CURSOR_CLICK,
    DATABASE,
    DESKTOP,
    DOT_OUTLINE,
    DOTS_SIX_VERTICAL,
    DOTS_THREE,
    DOTS_THREE_VERTICAL,
    DOWNLOAD_SIMPLE,
    ENVELOPE,
    EYE,
    EYE_SLASH,
    FILE,
    FILE_CODE,
    FILE_DASHED,
    FILE_MINUS,
    FILE_PLUS,
    FILE_TEXT,
    FILES,
    FLAG,
    FLAME,
    FOLDER,
    FOLDER_OPEN,
    FOLDER_PLUS,
    FOLDER_SIMPLE,
    FOLDER_SIMPLE_PLUS,
    FUNNEL,
    GEAR_SIX,
    GIT_BRANCH,
    GIT_COMMIT,
    GIT_DIFF,
    GIT_FORK,
    GIT_MERGE,
    GIT_PULL_REQUEST,
    GITHUB_LOGO,
    GITLAB_LOGO,
    GLOBE,
    HAND_GRABBING,
    HARD_DRIVES,
    HASH,
    HOURGLASS,
    HOUSE,
    IMAGE,
    IMAGE_BROKEN,
    INFO,
    KEY,
    KEYBOARD,
    LIGHTBULB,
    LINK,
    LIST,
    LOCK,
    LOCK_OPEN,
    MAGIC_WAND,
    MAGNIFYING_GLASS,
    MINUS,
    MONITOR,
    MOON,
    NOTE_PENCIL,
    PACKAGE,
    PALETTE,
    PARAGRAPH,
    PAUSE,
    PENCIL_SIMPLE,
    PLAY,
    PLUS,
    PLUS_CIRCLE,
    PROHIBIT,
    PULSE,
    PUSH_PIN,
    PUSH_PIN_SLASH,
    QUESTION,
    REPEAT,
    ROBOT,
    ROWS,
    SIDEBAR_SIMPLE,
    SIGN_OUT,
    SLIDERS_HORIZONTAL,
    SPARKLE,
    SQUARE,
    SQUARES_FOUR,
    STACK,
    STAR,
    SUN,
    TAG,
    TERMINAL_WINDOW,
    TEXT_AA,
    TEXT_ALIGN_LEFT,
    TOOLBOX,
    TREE_STRUCTURE,
    TRASH,
    TRAY_ARROW_DOWN,
    TRAY_ARROW_UP,
    UPLOAD_SIMPLE,
    USER,
    USER_CIRCLE,
    USERS,
    WARNING,
    WARNING_CIRCLE,
    WRENCH,
    X,
    X_CIRCLE,
];
