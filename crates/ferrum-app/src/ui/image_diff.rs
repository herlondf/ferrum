//! Side-by-side image diff: the old and new version of an image file.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;
use std::time::SystemTime;

use eframe::egui::{self, Color32, ColorImage, Rect, Sense, TextureHandle, TextureOptions, vec2};
use ferrum_core::{ChangeKind, DiffSource, FileDiff, Oid, Repo};

/// Larger files are not decoded, so the window never stalls.
const MAX_BYTES: u64 = 50 * 1024 * 1024;
const MAX_PIXELS: u64 = 40_000_000;
/// Images are never scaled up more than this.
const MAX_ZOOM: f32 = 2.0;
/// Decoded images kept in memory.
const CACHE_SIZE: usize = 16;
const CHECKER_CELL: f32 = 8.0;

/// Identifies one version of an image, so it is decoded only once.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Key {
    Blob(Oid),
    /// A working-copy file changes without a new id; size and mtime tell.
    File {
        path: String,
        size: u64,
        modified: Option<SystemTime>,
    },
}

struct Picture {
    texture: TextureHandle,
    /// Original dimensions (the texture may be smaller).
    width: u32,
    height: u32,
}

struct Loaded {
    bytes: u64,
    picture: Result<Picture, String>,
}

#[derive(Clone, Default)]
struct Cache {
    images: HashMap<Key, Arc<Loaded>>,
    checker: Option<TextureHandle>,
}

/// What one side of the diff shows.
enum Side {
    /// The file does not exist on this side; the text says why.
    Missing(String),
    Error(String),
    Image(Arc<Loaded>),
}

pub fn show(ui: &mut egui::Ui, repo: &Repo, file: &FileDiff, source: DiffSource) {
    let added = matches!(file.kind, ChangeKind::Added | ChangeKind::Untracked);
    let deleted = file.kind == ChangeKind::Deleted;

    let before = match file.old_id {
        _ if added => Side::Missing(t!("No previous version (file added)")),
        Some(id) => load_blob(ui.ctx(), repo, id),
        None => Side::Missing(t!("No previous version")),
    };
    let after = if deleted {
        Side::Missing(t!("File deleted"))
    } else if source == DiffSource::Unstaged {
        load_workdir(ui.ctx(), repo, &file.path)
    } else {
        match file.new_id {
            Some(id) => load_blob(ui.ctx(), repo, id),
            None => Side::Missing(t!("No new version")),
        }
    };

    if let (Some(a), Some(b)) = (dimensions(&before), dimensions(&after))
        && a != b
    {
        ui.label(
            egui::RichText::new(t!("Dimensions changed: {}×{} -> {}×{}", a.0, a.1, b.0, b.1))
                .color(ui.visuals().warn_fg_color),
        );
    }

    let gap = 12.0;
    let avail = ui.available_size();
    let col_w = ((avail.x - gap) / 2.0).max(50.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (title, side) in [(t!("Before"), &before), (t!("After"), &after)] {
            ui.allocate_ui(vec2(col_w, avail.y), |ui| {
                ui.set_width(col_w);
                ui.vertical(|ui| column(ui, &title, side, vec2(col_w, avail.y)));
            });
        }
    });
}

fn column(ui: &mut egui::Ui, title: &str, side: &Side, size: egui::Vec2) {
    ui.strong(title);
    match side {
        Side::Missing(text) => {
            ui.weak(text.as_str());
        }
        Side::Error(e) => {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
        Side::Image(loaded) => match &loaded.picture {
            Err(e) => {
                ui.colored_label(ui.visuals().error_fg_color, e);
                ui.weak(format_size(loaded.bytes));
            }
            Ok(pic) => {
                // Room for the title above and the caption below.
                let area = vec2(size.x, (size.y - 56.0).max(40.0));
                let scale = fit_scale([pic.width, pic.height], area);
                let shown = vec2(pic.width as f32, pic.height as f32) * scale;
                let (row, _) = ui.allocate_exact_size(vec2(size.x, shown.y), Sense::hover());
                let rect = Rect::from_center_size(row.center(), shown);
                paint_checker(ui, rect);
                let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                ui.painter()
                    .image(pic.texture.id(), rect, uv, Color32::WHITE);
                ui.vertical_centered(|ui| {
                    ui.weak(format!(
                        "{}×{} · {}",
                        pic.width,
                        pic.height,
                        format_size(loaded.bytes)
                    ))
                    .on_hover_text(t!("Shown at {}%", format!("{:.0}", scale * 100.0)));
                });
            }
        },
    }
}

fn dimensions(side: &Side) -> Option<(u32, u32)> {
    match side {
        Side::Image(l) => l.picture.as_ref().ok().map(|p| (p.width, p.height)),
        _ => None,
    }
}

fn cache_id() -> egui::Id {
    egui::Id::unique("image-diff-cache")
}

fn cached(ctx: &egui::Context, key: &Key) -> Option<Arc<Loaded>> {
    ctx.data(|d| d.get_temp::<Cache>(cache_id()))
        .and_then(|c| c.images.get(key).cloned())
}

fn store(ctx: &egui::Context, key: Key, loaded: Arc<Loaded>) {
    ctx.data_mut(|d| {
        let cache = d.get_temp_mut_or_default::<Cache>(cache_id());
        if cache.images.len() >= CACHE_SIZE {
            cache.images.clear();
        }
        cache.images.insert(key, loaded);
    });
}

fn load_blob(ctx: &egui::Context, repo: &Repo, id: Oid) -> Side {
    let key = Key::Blob(id);
    if let Some(l) = cached(ctx, &key) {
        return Side::Image(l);
    }
    let size = match repo.blob_size(id) {
        Ok(s) => s as u64,
        Err(e) => return Side::Error(t!("Could not read the image: {}", e)),
    };
    let loaded = if size > MAX_BYTES {
        Loaded {
            bytes: size,
            picture: Err(too_big()),
        }
    } else {
        match repo.blob(id) {
            Ok(bytes) => decode_to_texture(ctx, &format!("image-{id}"), &bytes),
            Err(e) => return Side::Error(t!("Could not read the image: {}", e)),
        }
    };
    let loaded = Arc::new(loaded);
    store(ctx, key, loaded.clone());
    Side::Image(loaded)
}

fn load_workdir(ctx: &egui::Context, repo: &Repo, path: &str) -> Side {
    let Ok(meta) = std::fs::metadata(repo.path().join(path)) else {
        return Side::Missing(t!("File not found in the working copy"));
    };
    let key = Key::File {
        path: path.to_owned(),
        size: meta.len(),
        modified: meta.modified().ok(),
    };
    if let Some(l) = cached(ctx, &key) {
        return Side::Image(l);
    }
    let loaded = if meta.len() > MAX_BYTES {
        Loaded {
            bytes: meta.len(),
            picture: Err(too_big()),
        }
    } else {
        match repo.read_workdir_file(path) {
            Ok(bytes) => decode_to_texture(ctx, &format!("image-wd-{path}"), &bytes),
            Err(e) => return Side::Error(t!("Could not read the file: {}", e)),
        }
    };
    let loaded = Arc::new(loaded);
    store(ctx, key, loaded.clone());
    Side::Image(loaded)
}

fn too_big() -> String {
    t!("Image too large to preview.")
}

fn decode_to_texture(ctx: &egui::Context, name: &str, bytes: &[u8]) -> Loaded {
    let max_side = ctx.input(|i| i.max_texture_side) as u32;
    let picture = decode(bytes, max_side).map(|(image, width, height)| Picture {
        texture: ctx.load_texture(name, image, TextureOptions::LINEAR),
        width,
        height,
    });
    Loaded {
        bytes: bytes.len() as u64,
        picture,
    }
}

/// Decodes an image, shrinking it to fit `max_side` for the GPU. Returns the
/// image and its original dimensions.
fn decode(bytes: &[u8], max_side: u32) -> Result<(ColorImage, u32, u32), String> {
    let reader = || {
        image::ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|e| e.to_string())
    };
    // Read the header first so huge images are rejected before decoding.
    let (w, h) = reader()?
        .into_dimensions()
        .map_err(|e| t!("Could not decode the image: {}", e))?;
    check_pixels(w, h)?;
    let mut img = reader()?
        .decode()
        .map_err(|e| t!("Could not decode the image: {}", e))?;
    if w > max_side || h > max_side {
        img = img.thumbnail(max_side, max_side);
    }
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Ok((ColorImage::from_rgba_unmultiplied(size, &rgba), w, h))
}

fn check_pixels(w: u32, h: u32) -> Result<(), String> {
    if u64::from(w) * u64::from(h) > MAX_PIXELS {
        Err(t!("Image too large to preview ({}×{}).", w, h))
    } else {
        Ok(())
    }
}

/// Scale that fits an image into `area`, keeping its aspect ratio.
fn fit_scale(size: [u32; 2], area: egui::Vec2) -> f32 {
    let (w, h) = (size[0].max(1) as f32, size[1].max(1) as f32);
    (area.x / w).min(area.y / h).min(MAX_ZOOM)
}

fn format_size(bytes: u64) -> String {
    match bytes {
        b if b < 1024 => format!("{b} B"),
        b if b < 1024 * 1024 => format!("{:.1} KB", b as f64 / 1024.0),
        b => format!("{:.1} MB", b as f64 / (1024.0 * 1024.0)),
    }
}

/// Paints a checkerboard behind `rect`, so transparent pixels are visible.
fn paint_checker(ui: &egui::Ui, rect: Rect) {
    let ctx = ui.ctx();
    let texture = ctx
        .data(|d| d.get_temp::<Cache>(cache_id()))
        .and_then(|c| c.checker);
    let texture = texture.unwrap_or_else(|| {
        let (a, b) = if ui.visuals().dark_mode {
            (Color32::from_gray(0x3a), Color32::from_gray(0x2a))
        } else {
            (Color32::from_gray(0xff), Color32::from_gray(0xdd))
        };
        let image = ColorImage::new([2, 2], vec![a, b, b, a]);
        let options = TextureOptions {
            wrap_mode: egui::TextureWrapMode::Repeat,
            ..TextureOptions::NEAREST
        };
        let t = ctx.load_texture("image-diff-checker", image, options);
        ctx.data_mut(|d| {
            d.get_temp_mut_or_default::<Cache>(cache_id()).checker = Some(t.clone());
        });
        t
    });
    // One texel per cell, repeated across the rect.
    let uv = Rect::from_min_size(egui::Pos2::ZERO, rect.size() / (CHECKER_CELL * 2.0));
    ui.painter().image(texture.id(), rect, uv, Color32::WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([255, 0, 0, 128]));
        let mut out = Vec::new();
        img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn decodes_and_shrinks_for_the_gpu() {
        let (img, w, h) = decode(&png(40, 20), 2048).unwrap();
        assert_eq!((w, h, img.size), (40, 20, [40, 20]));
        let (img, w, h) = decode(&png(400, 100), 100).unwrap();
        assert_eq!((w, h), (400, 100));
        assert_eq!(img.size, [100, 25]);
    }

    #[test]
    fn rejects_garbage_and_huge_images() {
        assert!(decode(b"not an image", 2048).is_err());
        assert!(check_pixels(8000, 6000).is_err());
        assert!(check_pixels(6000, 6000).is_ok());
    }

    #[test]
    fn fits_without_upscaling_too_much() {
        assert_eq!(fit_scale([100, 50], vec2(1000.0, 1000.0)), 2.0);
        assert_eq!(fit_scale([1000, 500], vec2(500.0, 1000.0)), 0.5);
        assert_eq!(fit_scale([1000, 2000], vec2(1000.0, 500.0)), 0.25);
    }

    #[test]
    fn formats_file_sizes() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(24_678), "24.1 KB");
        assert_eq!(format_size(3 * 1024 * 1024), "3.0 MB");
    }
}
