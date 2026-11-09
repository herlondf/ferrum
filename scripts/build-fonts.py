#!/usr/bin/env python3
"""Builds the fonts embedded in Ferrum (crates/ferrum-app/assets/fonts).

Needs fontTools (`pip install fonttools`) and the official releases, unpacked
into one folder:

  phosphor/   npm package @phosphor-icons/web 2.1.2
              (https://registry.npmjs.org/@phosphor-icons/web/-/web-2.1.2.tgz)
  inter/      Inter 4.1 (https://github.com/rsms/inter/releases/tag/v4.1)
  jetbrains/  JetBrains Mono 2.304
              (https://github.com/JetBrains/JetBrainsMono/releases/tag/v2.304)

Usage:
  build-fonts.py SRC_DIR OUT_DIR       build the fonts
  build-fonts.py SRC_DIR --lookup NAME  print an icon's codepoint

What it does:
- Inter (Regular, SemiBold) and JetBrains Mono (Regular, no ligatures) are
  cut down to Latin, Greek, Cyrillic and common punctuation, without hinting.
  Other characters fall back to egui's built-in fonts.
- Phosphor (regular weight) keeps every icon at its private-use codepoint, and
  also gets the Unicode symbols Ferrum used before (see LEGACY) mapped to the
  matching icon, so text like "⚠ Conflicts" shows the same icon set.
"""

import re
import sys
from pathlib import Path

LATIN = "U+0020-007E,U+00A0-024F,U+0259,U+02B0-02FF,U+0300-036F,U+0370-03FF,U+0400-04FF,U+1E00-1EFF,U+2000-206F,U+20A0-20CF,U+2100-214F,U+2190-2195,U+2200-22FF"
# Code also needs the arrows and box drawing.
CODE = LATIN + ",U+2190-21FF,U+2500-259F"

# Symbols used as icons in Ferrum's strings -> Phosphor icon name.
LEGACY = {
    "⚠": "warning",
    "✔": "check",
    "✖": "x",
    "✕": "x",
    "✗": "x",
    "🗙": "x",
    "❌": "x-circle",
    "⬆": "arrow-up",
    "⬇": "arrow-down",
    "⬅": "arrow-left",
    "➡": "arrow-right",
    "⏶": "caret-up",
    "⏷": "caret-down",
    "⏵": "caret-right",
    "⏴": "caret-left",
    "⬍": "caret-up-down",
    "☁": "cloud",
    "🏷": "tag",
    "🔖": "bookmark-simple",
    "📦": "archive",
    "🗃": "archive",
    "📁": "folder",
    "📂": "folder-open",
    "📄": "file",
    "🌿": "git-branch",
    "⎇": "git-branch",
    "⑂": "git-fork",
    "🔀": "git-pull-request",
    "⟳": "arrows-clockwise",
    "🔄": "arrows-clockwise",
    "↻": "arrow-clockwise",
    "↺": "arrow-counter-clockwise",
    "🔃": "arrows-down-up",
    "🔁": "repeat",
    "↩": "arrow-u-up-left",
    "↪": "arrow-u-up-right",
    "⤴": "arrow-bend-left-up",
    "⏎": "arrow-elbow-down-left",
    "🔍": "magnifying-glass",
    "⚙": "gear-six",
    "⌘": "command",
    "☰": "list",
    "⋯": "dots-three",
    "⋮": "dots-three-vertical",
    "✚": "plus",
    "➕": "plus",
    "📝": "note-pencil",
    "📌": "push-pin",
    "👤": "user",
    "🕓": "clock-counter-clockwise",
    "🔒": "lock",
    "🔐": "key",
    "📋": "clipboard-text",
    "📤": "tray-arrow-up",
    "📥": "tray-arrow-down",
    "⏳": "hourglass",
    "⌛": "hourglass",
    "🛠": "toolbox",
    "🔧": "wrench",
    "⛶": "corners-out",
    "🎨": "palette",
    "✉": "envelope",
    "💡": "lightbulb",
    "▶": "play",
    "📊": "chart-bar",
    "☐": "square",
    "☑": "check-square",
    "▣": "cube",
}


def icon_codes(src: Path) -> dict:
    css = (src / "phosphor/src/regular/style.css").read_text()
    return {
        m[1]: int(m[2], 16)
        for m in re.finditer(r'\.ph-([a-z0-9-]+):before \{\s*content: "\\([0-9a-f]+)"', css)
    }


def subset(path: Path, out: Path, unicodes: str, extra=()):
    from fontTools import subset as fs

    opts = fs.Options()
    opts.hinting = False
    opts.layout_features = ["kern"]
    opts.name_IDs = [0, 1, 2, 3, 4, 5, 6, 13, 14]
    opts.notdef_outline = True
    font = fs.load_font(str(path), opts)
    sub = fs.Subsetter(opts)
    sub.populate(unicodes=fs.parse_unicodes(unicodes) + list(extra))
    sub.subset(font)
    fs.save_font(font, str(out), opts)
    print(f"{out.name}: {out.stat().st_size // 1024} KiB")


def icons(src: Path, out: Path):
    from fontTools import subset as fs
    from fontTools.ttLib import TTFont
    from fontTools.ttLib.tables._c_m_a_p import CmapSubtable

    codes = icon_codes(src)
    font = TTFont(str(src / "phosphor/src/regular/Phosphor.ttf"))
    best = font.getBestCmap()
    mapping = dict(best)
    for ch, name in LEGACY.items():
        mapping[ord(ch)] = best[codes[name]]
    # Format 4 for the BMP, format 12 for everything (emoji are above it).
    bmp = CmapSubtable.newSubtable(4)
    bmp.platformID, bmp.platEncID, bmp.language = 3, 1, 0
    bmp.cmap = {c: g for c, g in mapping.items() if c <= 0xFFFF}
    full = CmapSubtable.newSubtable(12)
    full.platformID, full.platEncID, full.language = 3, 10, 0
    full.cmap = mapping
    font["cmap"].tables = [bmp, full]
    # Drop the ligatures (icon names typed as text) and glyph names.
    del font["GSUB"]
    font["post"].formatType = 3.0
    tmp = out.with_suffix(".tmp.ttf")
    font.save(str(tmp))

    opts = fs.Options()
    opts.hinting = False
    opts.layout_features = []
    opts.glyph_names = False
    font = fs.load_font(str(tmp), opts)
    sub = fs.Subsetter(opts)
    sub.populate(unicodes=list(mapping))
    sub.subset(font)
    fs.save_font(font, str(out), opts)
    tmp.unlink()
    print(f"{out.name}: {out.stat().st_size // 1024} KiB, {len(codes)} icons")


def main():
    src = Path(sys.argv[1])
    if sys.argv[2] == "--lookup":
        codes = icon_codes(src)
        for name in sys.argv[3:]:
            code = codes.get(name)
            print(name, f"\\u{{{code:x}}}" if code else "MISSING")
        return
    out = Path(sys.argv[2])
    out.mkdir(parents=True, exist_ok=True)
    inter = src / "inter/extras/ttf"
    subset(inter / "Inter-Regular.ttf", out / "Inter-Regular.ttf", LATIN)
    subset(inter / "Inter-SemiBold.ttf", out / "Inter-SemiBold.ttf", LATIN)
    mono = src / "jetbrains/fonts/ttf/JetBrainsMonoNL-Regular.ttf"
    subset(mono, out / "JetBrainsMono-Regular.ttf", CODE)
    icons(src, out / "Phosphor.ttf")


if __name__ == "__main__":
    main()
