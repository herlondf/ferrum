//! Images of the README in the overview: relative paths (`docs/shot.png`,
//! `./logo.svg`, `/assets/x.png`) are read from the repository, like GitHub
//! does, through an egui image loader for our own URI scheme.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use eframe::egui::load::{ImageLoadResult, ImageLoader, ImagePoll, LoadError, SizeHint};
use eframe::egui::{self, ColorImage};

const SCHEME: &str = "ferrum-readme://";
/// Separates the repository root, the README's folder and the image path.
const SEP: char = '\u{1f}';
/// Largest side of a decoded image, in pixels.
const MAX_SIDE: u32 = 2048;

/// The prefix the markdown viewer puts in front of relative image paths
/// (`CommonMarkViewer::default_implicit_uri_scheme`).
pub fn uri_prefix(root: &Path, readme_dir: &str) -> String {
    format!("{SCHEME}{}{SEP}{readme_dir}{SEP}", root.display())
}

/// The file an image URI points at, if it stays inside the repository.
/// `/x` is relative to the repository root, anything else to the README's folder.
pub fn resolve(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix(SCHEME)?;
    let mut parts = rest.splitn(3, SEP);
    let root = PathBuf::from(parts.next()?);
    let dir = parts.next()?;
    let link = parts.next()?;
    // `?raw=true` and `#anchor` are for the web.
    let link = link.split(['?', '#']).next().unwrap_or_default();
    let link = percent_decode(link);
    let rel = match link.strip_prefix('/') {
        Some(from_root) => PathBuf::from(from_root),
        None => Path::new(dir).join(link),
    };
    // Normalize without touching the disk; `..` must not leave the root.
    let mut clean = PathBuf::new();
    for c in rel.components() {
        match c {
            Component::Normal(p) => clean.push(p),
            Component::ParentDir => {
                if !clean.pop() {
                    return None;
                }
            }
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!clean.as_os_str().is_empty()).then(|| root.join(clean))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        if bytes[i] == b'%'
            && let Some(b) = hex
        {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

type Cache = HashMap<String, Result<Arc<ColorImage>, String>>;

#[derive(Default)]
struct Loader {
    cache: Mutex<Cache>,
}

impl ImageLoader for Loader {
    fn id(&self) -> &str {
        concat!(module_path!(), "::Loader")
    }

    fn load(&self, _ctx: &egui::Context, uri: &str, _: SizeHint) -> ImageLoadResult {
        if !uri.starts_with(SCHEME) {
            return Err(LoadError::NotSupported);
        }
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let entry = cache.entry(uri.to_owned()).or_insert_with(|| load(uri));
        match entry {
            Ok(image) => Ok(ImagePoll::Ready {
                image: image.clone(),
            }),
            Err(e) => Err(LoadError::Loading(e.clone())),
        }
    }

    fn forget(&self, uri: &str) {
        self.cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(uri);
    }

    fn forget_all(&self) {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    fn byte_size(&self) -> usize {
        self.cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .map(|r| r.as_ref().map_or(0, |i| i.pixels.len() * 4))
            .sum()
    }
}

fn load(uri: &str) -> Result<Arc<ColorImage>, String> {
    let path = resolve(uri).ok_or_else(|| t!("The image is outside the repository."))?;
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let name = path.to_string_lossy();
    let (mut image, w, h) = crate::ui::image_diff::decode(&bytes, MAX_SIDE, &name)?;
    // SVGs are rendered larger for sharpness: show them at their own size.
    image.source_size = egui::vec2(w as f32, h as f32);
    Ok(Arc::new(image))
}

/// Installs the loader once.
pub fn install(ctx: &egui::Context) {
    let id = egui::Id::unique("ferrum-readme-loader");
    if ctx.data(|d| d.get_temp::<bool>(id)).is_none() {
        ctx.add_image_loader(Arc::new(Loader::default()));
        ctx.data_mut(|d| d.insert_temp(id, true));
    }
}

/// Forgets the decoded images, e.g. when the README is reloaded.
pub fn forget(ctx: &egui::Context) {
    ctx.forget_all_images();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_relative_paths_inside_the_repository() {
        let root = Path::new("/repo");
        let top = uri_prefix(root, "");
        let docs = uri_prefix(root, "docs");
        let r = |prefix: &str, link: &str| resolve(&format!("{prefix}{link}"));
        let img = Some(root.join("img").join("a.png"));
        assert_eq!(r(&top, "img/a.png"), img);
        assert_eq!(r(&top, "./img/a.png"), img);
        assert_eq!(r(&docs, "a.png"), Some(root.join("docs").join("a.png")));
        assert_eq!(r(&docs, "../a.png"), Some(root.join("a.png")));
        assert_eq!(r(&docs, "/x/a.png"), Some(root.join("x").join("a.png")));
        assert_eq!(r(&top, "a%20b.png?raw=true"), Some(root.join("a b.png")));
        assert_eq!(r(&top, "../outside.png"), None);
        assert_eq!(resolve("https://example.com/a.png"), None);
    }
}
