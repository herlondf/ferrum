//! Reads the operating system's accent color.
//!
//! Linux: the XDG desktop portal (GNOME 47+, KDE Plasma 6), then KDE's
//! `kdeglobals`. Windows: the DWM accent in the registry. macOS: the
//! `AppleAccentColor` default. Blocking; call it off the UI thread.

use eframe::egui::Color32;

pub fn query() -> Option<Color32> {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        portal().or_else(kdeglobals)
    }
    #[cfg(windows)]
    {
        windows()
    }
    #[cfg(target_os = "macos")]
    {
        macos()
    }
    #[cfg(not(any(unix, windows)))]
    {
        None
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn portal() -> Option<Color32> {
    use zbus::zvariant::Value;
    let conn = zbus::blocking::Connection::session().ok()?;
    let reply = conn
        .call_method(
            Some("org.freedesktop.portal.Desktop"),
            "/org/freedesktop/portal/desktop",
            Some("org.freedesktop.portal.Settings"),
            "ReadOne",
            &("org.freedesktop.appearance", "accent-color"),
        )
        .ok()?;
    let body = reply.body();
    let value: zbus::zvariant::OwnedValue = body.deserialize().ok()?;
    let mut v: &Value = &value;
    while let Value::Value(inner) = v {
        v = inner;
    }
    let Value::Structure(s) = v else { return None };
    let rgb: Vec<f64> = s
        .fields()
        .iter()
        .filter_map(|f| match f {
            Value::F64(x) => Some(*x),
            _ => None,
        })
        .collect();
    from_unit_rgb(&rgb)
}

/// The portal gives 0..1 floats; anything outside means "no accent set".
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
fn from_unit_rgb(rgb: &[f64]) -> Option<Color32> {
    if rgb.len() != 3 || rgb.iter().any(|c| !(0.0..=1.0).contains(c)) {
        return None;
    }
    let b = |c: f64| (c * 255.0).round() as u8;
    Some(Color32::from_rgb(b(rgb[0]), b(rgb[1]), b(rgb[2])))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn kdeglobals() -> Option<Color32> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config"))
        })?;
    parse_kdeglobals(&std::fs::read_to_string(config.join("kdeglobals")).ok()?)
}

/// `AccentColor=61,174,233` in the `[General]` group.
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
fn parse_kdeglobals(text: &str) -> Option<Color32> {
    let mut general = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            general = line == "[General]";
        } else if general && let Some(v) = line.strip_prefix("AccentColor=") {
            let parts: Vec<u8> = v.split(',').filter_map(|p| p.trim().parse().ok()).collect();
            if let [r, g, b, ..] = parts[..] {
                return Some(Color32::from_rgb(r, g, b));
            }
        }
    }
    None
}

#[cfg(windows)]
fn windows() -> Option<Color32> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\DWM",
            "/v",
            "AccentColor",
        ])
        .creation_flags(0x0800_0000)
        .output()
        .ok()?;
    parse_reg_dword(&String::from_utf8_lossy(&out.stdout))
}

/// `AccentColor    REG_DWORD    0xffd77800` (ABGR).
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_reg_dword(text: &str) -> Option<Color32> {
    let hex = text.split_whitespace().find_map(|w| w.strip_prefix("0x"))?;
    let v = u32::from_str_radix(hex, 16).ok()?;
    Some(Color32::from_rgb(
        (v & 0xff) as u8,
        (v >> 8 & 0xff) as u8,
        (v >> 16 & 0xff) as u8,
    ))
}

#[cfg(target_os = "macos")]
fn macos() -> Option<Color32> {
    let out = std::process::Command::new("defaults")
        .args(["read", "-g", "AppleAccentColor"])
        .output()
        .ok()?;
    // Not set means the default, blue.
    let n = if out.status.success() {
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()?
    } else {
        4
    };
    macos_color(n)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn macos_color(n: i32) -> Option<Color32> {
    let (r, g, b) = match n {
        -1 => (0x8c, 0x8c, 0x8c),
        0 => (0xff, 0x52, 0x57),
        1 => (0xf7, 0x82, 0x1b),
        2 => (0xff, 0xc6, 0x00),
        3 => (0x62, 0xba, 0x46),
        4 => (0x00, 0x7a, 0xff),
        5 => (0xa5, 0x50, 0xa7),
        6 => (0xf7, 0x4f, 0x9e),
        _ => return None,
    };
    Some(Color32::from_rgb(r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_os_answers() {
        assert_eq!(
            from_unit_rgb(&[0.0, 0.5, 1.0]),
            Some(Color32::from_rgb(0, 128, 255))
        );
        assert_eq!(from_unit_rgb(&[-1.0, 0.5, 1.0]), None);
        let kde = "[Colors:View]\nAccentColor=1,2,3\n[General]\nAccentColor=61,174,233\n";
        assert_eq!(parse_kdeglobals(kde), Some(Color32::from_rgb(61, 174, 233)));
        assert_eq!(parse_kdeglobals("[General]\nName=x\n"), None);
        let reg = "\r\nHKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\DWM\r\n    AccentColor    REG_DWORD    0xffd77800\r\n";
        assert_eq!(
            parse_reg_dword(reg),
            Some(Color32::from_rgb(0x00, 0x78, 0xd7))
        );
        assert_eq!(macos_color(4), Some(Color32::from_rgb(0, 0x7a, 0xff)));
        assert_eq!(macos_color(9), None);
    }

    /// `cargo test -- --ignored prints_system_accent` on a desktop.
    #[test]
    #[ignore]
    fn prints_system_accent() {
        println!("system accent: {:?}", query());
    }
}
