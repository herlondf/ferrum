//! Image diff: the old and new version of an image side by side, as a swipe,
//! as an onion skin, or with the differing pixels highlighted.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Arc, LazyLock};
use std::time::SystemTime;

use eframe::egui::{
    self, Color32, ColorImage, Id, Rect, Sense, TextureHandle, TextureOptions, pos2, vec2,
};
use ferrum_core::{ChangeKind, DiffSource, FileDiff, Oid, Repo};
use resvg::{tiny_skia, usvg};

use crate::ui::diff_prefs::{self, ImageMode};

/// Larger files are not decoded, so the window never stalls.
const MAX_BYTES: u64 = 50 * 1024 * 1024;
const MAX_PIXELS: u64 = 40_000_000;
/// Pixels are kept for the difference view only up to this size.
const MAX_DIFF_PIXELS: usize = 16_000_000;
/// Images are never scaled up more than this.
const MAX_ZOOM: f32 = 2.0;
/// PDF pages are rendered this large (longer side, in pixels).
const PDF_SIDE: f32 = 1000.0;
/// Small SVGs are rendered at least this large, so they stay sharp when zoomed.
const SVG_MIN_SIDE: f32 = 512.0;
/// Decoded images kept in memory.
const CACHE_SIZE: usize = 16;
const CHECKER_CELL: f32 = 8.0;

/// Whether the file can be shown as an image.
pub fn is_image(path: &str) -> bool {
    crate::util::is_image_path(path)
}

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
    /// Content fetched another way (Git LFS), by its id.
    Other(String),
}

struct Picture {
    texture: TextureHandle,
    /// Original dimensions (the texture may be smaller).
    width: u32,
    height: u32,
    /// Decoded pixels, for the difference view (small enough images only).
    pixels: Option<Arc<ColorImage>>,
}

struct Loaded {
    key: Key,
    bytes: u64,
    picture: Result<Picture, String>,
}

#[derive(Clone, Default)]
struct Cache {
    images: HashMap<Key, Arc<Loaded>>,
    checker: Option<(bool, TextureHandle)>,
    /// Difference textures: (old, new) -> (texture, differing pixels).
    diffs: HashMap<(Key, Key), Arc<(TextureHandle, u64)>>,
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
    let path = &file.path;

    let before = match file.old_id {
        _ if added => Side::Missing(t!("No previous version (file added)")),
        Some(id) => load_blob(ui.ctx(), repo, id, path),
        None => Side::Missing(t!("No previous version")),
    };
    let after = if deleted {
        Side::Missing(t!("File deleted"))
    } else if source == DiffSource::Unstaged {
        load_workdir(ui.ctx(), repo, path)
    } else {
        match file.new_id {
            Some(id) => load_blob(ui.ctx(), repo, id, path),
            None => Side::Missing(t!("No new version")),
        }
    };
    show_sides(ui, &before, &after);
}

/// Shows two versions given as bytes, each with an id for the cache.
pub fn show_bytes(
    ui: &mut egui::Ui,
    path: &str,
    before: Option<(&str, &[u8])>,
    after: Option<(&str, &[u8])>,
) {
    let side = |v: Option<(&str, &[u8])>, missing: String| match v {
        None => Side::Missing(missing),
        Some((id, bytes)) => {
            let key = Key::Other(id.to_owned());
            if let Some(l) = cached(ui.ctx(), &key) {
                return Side::Image(l);
            }
            let loaded = Arc::new(decode_to_texture(ui.ctx(), key.clone(), path, bytes));
            store(ui.ctx(), key, loaded.clone());
            Side::Image(loaded)
        }
    };
    let before = side(before, t!("No previous version"));
    let after = side(after, t!("No new version"));
    show_sides(ui, &before, &after);
}

fn show_sides(ui: &mut egui::Ui, before: &Side, after: &Side) {
    let mut prefs = diff_prefs::get(ui.ctx());
    let both = match (picture(before), picture(after)) {
        (Some(a), Some(b)) => Some((a, b)),
        _ => None,
    };
    ui.horizontal(|ui| {
        for m in ImageMode::ALL {
            let enabled = m == ImageMode::SideBySide || both.is_some();
            let r = ui.add_enabled(
                enabled,
                egui::Button::selectable(prefs.image_mode == m, m.label()),
            );
            if r.clicked() {
                prefs.image_mode = m;
                diff_prefs::set(ui.ctx(), prefs.clone());
            }
        }
        if let Some(((a, _), (b, _))) = both
            && (a.width, a.height) != (b.width, b.height)
        {
            ui.label(
                egui::RichText::new(t!(
                    "Dimensions changed: {}×{} -> {}×{}",
                    a.width,
                    a.height,
                    b.width,
                    b.height
                ))
                .color(ui.visuals().warn_fg_color),
            );
        }
    });
    let Some((old, new)) = both else {
        side_by_side(ui, before, after);
        return;
    };
    match prefs.image_mode {
        ImageMode::SideBySide => side_by_side(ui, before, after),
        ImageMode::Swipe => swipe(ui, old, new),
        ImageMode::OnionSkin => onion(ui, old, new),
        ImageMode::Difference => difference(ui, old, new),
    }
}

fn picture(side: &Side) -> Option<(&Picture, &Loaded)> {
    match side {
        Side::Image(l) => l.picture.as_ref().ok().map(|p| (p, &**l)),
        _ => None,
    }
}

fn side_by_side(ui: &mut egui::Ui, before: &Side, after: &Side) {
    let gap = 12.0;
    let avail = ui.available_size();
    let col_w = ((avail.x - gap) / 2.0).max(50.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (title, side) in [(t!("Before"), before), (t!("After"), after)] {
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
                ui.painter()
                    .image(pic.texture.id(), rect, full_uv(), Color32::WHITE);
                ui.vertical_centered(|ui| caption(ui, pic, loaded, scale));
            }
        },
    }
}

fn caption(ui: &mut egui::Ui, pic: &Picture, loaded: &Loaded, scale: f32) {
    ui.weak(format!(
        "{}×{} · {}",
        pic.width,
        pic.height,
        format_size(loaded.bytes)
    ))
    .on_hover_text(format!(
        "{}\n{}",
        t!("Shown at {}%", format!("{:.0}", scale * 100.0)),
        t!("{} bytes", loaded.bytes)
    ));
}

fn full_uv() -> Rect {
    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))
}

/// Room for both images on top of each other, at one scale, and the rect of each.
fn overlay_rects(ui: &mut egui::Ui, a: &Picture, b: &Picture) -> (egui::Response, Rect, Rect, f32) {
    let avail = ui.available_size() - vec2(0.0, 24.0);
    let w = a.width.max(b.width);
    let h = a.height.max(b.height);
    let scale = fit_scale([w, h], avail.max(vec2(40.0, 40.0)));
    let size = vec2(w as f32, h as f32) * scale;
    let (row, resp) = ui.allocate_exact_size(vec2(avail.x, size.y), Sense::click_and_drag());
    let center = row.center();
    let r =
        |p: &Picture| Rect::from_center_size(center, vec2(p.width as f32, p.height as f32) * scale);
    (resp, r(a), r(b), scale)
}

fn swipe(ui: &mut egui::Ui, (a, la): (&Picture, &Loaded), (b, lb): (&Picture, &Loaded)) {
    let id = Id::unique("image-swipe");
    let mut split = ui.data(|d| d.get_temp::<f32>(id)).unwrap_or(0.5);
    let (resp, ra, rb, scale) = overlay_rects(ui, a, b);
    let both = ra.union(rb);
    if (resp.dragged() || resp.clicked())
        && let Some(p) = resp.interact_pointer_pos()
    {
        split = ((p.x - both.left()) / both.width().max(1.0)).clamp(0.0, 1.0);
        ui.data_mut(|d| d.insert_temp(id, split));
    }
    let x = both.left() + both.width() * split;
    paint_checker(ui, both);
    let painter = ui.painter();
    let left_clip = Rect::from_x_y_ranges(both.left()..=x, both.y_range());
    let right_clip = Rect::from_x_y_ranges(x..=both.right(), both.y_range());
    painter
        .with_clip_rect(left_clip.intersect(painter.clip_rect()))
        .image(a.texture.id(), ra, full_uv(), Color32::WHITE);
    painter
        .with_clip_rect(right_clip.intersect(painter.clip_rect()))
        .image(b.texture.id(), rb, full_uv(), Color32::WHITE);
    let stroke = egui::Stroke::new(2.0, ui.visuals().selection.stroke.color);
    painter.vline(x, both.y_range(), stroke);
    painter.circle_filled(pos2(x, both.center().y), 6.0, stroke.color);
    resp.on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    ui.horizontal(|ui| {
        ui.weak(t!("Before"));
        caption(ui, a, la, scale);
        ui.separator();
        ui.weak(t!("After"));
        caption(ui, b, lb, scale);
        ui.separator();
        ui.weak(t!("Drag to move the divider"));
    });
}

fn onion(ui: &mut egui::Ui, (a, la): (&Picture, &Loaded), (b, lb): (&Picture, &Loaded)) {
    let id = Id::unique("image-onion");
    let mut mix = ui.data(|d| d.get_temp::<f32>(id)).unwrap_or(0.5);
    ui.horizontal(|ui| {
        ui.weak(t!("Before"));
        if ui
            .add(egui::Slider::new(&mut mix, 0.0..=1.0).show_value(false))
            .changed()
        {
            ui.data_mut(|d| d.insert_temp(id, mix));
        }
        ui.weak(t!("After"));
    });
    let (_, ra, rb, scale) = overlay_rects(ui, a, b);
    paint_checker(ui, ra.union(rb));
    let painter = ui.painter();
    painter.image(
        a.texture.id(),
        ra,
        full_uv(),
        Color32::WHITE.gamma_multiply(1.0 - mix),
    );
    painter.image(
        b.texture.id(),
        rb,
        full_uv(),
        Color32::WHITE.gamma_multiply(mix),
    );
    ui.horizontal(|ui| {
        caption(ui, a, la, scale);
        ui.separator();
        caption(ui, b, lb, scale);
    });
}

fn difference(ui: &mut egui::Ui, (a, la): (&Picture, &Loaded), (b, lb): (&Picture, &Loaded)) {
    if (a.width, a.height) != (b.width, b.height) {
        ui.weak(t!(
            "The pixel difference needs both images at the same size."
        ));
        return;
    }
    let (Some(pa), Some(pb)) = (&a.pixels, &b.pixels) else {
        ui.weak(t!("Image too large to compare pixel by pixel."));
        return;
    };
    let ctx = ui.ctx().clone();
    let key = (la.key.clone(), lb.key.clone());
    let found = ctx
        .data(|d| d.get_temp::<Cache>(cache_id()))
        .and_then(|c| c.diffs.get(&key).cloned());
    let diff = found.unwrap_or_else(|| {
        let (image, count) = pixel_difference(pa, pb);
        let t = ctx.load_texture("image-diff-pixels", image, TextureOptions::NEAREST);
        let d = Arc::new((t, count));
        ctx.data_mut(|data| {
            let cache = data.get_temp_mut_or_default::<Cache>(cache_id());
            if cache.diffs.len() >= CACHE_SIZE {
                cache.diffs.clear();
            }
            cache.diffs.insert(key, d.clone());
        });
        d
    });
    let total = u64::from(a.width) * u64::from(a.height);
    let (tex, count) = (&diff.0, diff.1);
    ui.horizontal(|ui| {
        if count == 0 {
            ui.label(t!("The images are identical pixel by pixel."));
        } else {
            ui.label(t!(
                "{} pixels differ ({}%)",
                count,
                format!("{:.2}", count as f64 * 100.0 / total.max(1) as f64)
            ));
        }
    });
    let (_, ra, _, scale) = overlay_rects(ui, a, b);
    paint_checker(ui, ra);
    ui.painter().image(tex.id(), ra, full_uv(), Color32::WHITE);
    caption(ui, b, lb, scale);
}

/// The new image dimmed, with every pixel that differs from the old one in magenta.
fn pixel_difference(a: &ColorImage, b: &ColorImage) -> (ColorImage, u64) {
    let mut count = 0;
    let pixels = a
        .pixels
        .iter()
        .zip(&b.pixels)
        .map(|(pa, pb)| {
            if pa == pb {
                let [r, g, bl, al] = pb.to_array();
                let gray = ((r as u32 + g as u32 + bl as u32) / 3) as u8;
                Color32::from_rgba_premultiplied(gray, gray, gray, al).gamma_multiply(0.35)
            } else {
                count += 1;
                Color32::from_rgb(0xff, 0x00, 0xff)
            }
        })
        .collect();
    (
        ColorImage {
            size: a.size,
            source_size: a.source_size,
            pixels,
        },
        count,
    )
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

fn load_blob(ctx: &egui::Context, repo: &Repo, id: Oid, path: &str) -> Side {
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
            key: key.clone(),
            bytes: size,
            picture: Err(too_big()),
        }
    } else {
        match repo.blob(id) {
            Ok(bytes) => decode_to_texture(ctx, key.clone(), path, &bytes),
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
            key: key.clone(),
            bytes: meta.len(),
            picture: Err(too_big()),
        }
    } else {
        match repo.read_workdir_file(path) {
            Ok(bytes) => decode_to_texture(ctx, key.clone(), path, &bytes),
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

fn decode_to_texture(ctx: &egui::Context, key: Key, path: &str, bytes: &[u8]) -> Loaded {
    let max_side = ctx.input(|i| i.max_texture_side) as u32;
    let name = format!("image-{key:?}");
    let picture = decode(bytes, max_side, path).map(|(image, width, height)| {
        let pixels = (image.pixels.len() <= MAX_DIFF_PIXELS).then(|| Arc::new(image.clone()));
        Picture {
            texture: ctx.load_texture(name, image, TextureOptions::LINEAR),
            width,
            height,
            pixels,
        }
    });
    Loaded {
        key,
        bytes: bytes.len() as u64,
        picture,
    }
}

/// Decodes an image, shrinking it to fit `max_side` for the GPU. Returns the
/// image and its original dimensions. `path` tells formats without a
/// signature (TGA, SVG) apart.
pub(crate) fn decode(
    bytes: &[u8],
    max_side: u32,
    path: &str,
) -> Result<(ColorImage, u32, u32), String> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".svg") || lower.ends_with(".svgz") {
        return render_svg(bytes, max_side);
    }
    if lower.ends_with(".pdf") {
        return render_pdf(bytes, max_side);
    }
    let hint = image::ImageFormat::from_path(&lower).ok();
    let reader = || {
        let mut r = image::ImageReader::new(Cursor::new(bytes));
        match hint {
            // TGA has no signature to guess from.
            Some(f @ image::ImageFormat::Tga) => {
                r.set_format(f);
                Ok(r)
            }
            _ => r.with_guessed_format().map_err(|e| e.to_string()),
        }
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

static FONTS: LazyLock<Arc<usvg::fontdb::Database>> = LazyLock::new(|| {
    let mut db = usvg::fontdb::Database::new();
    db.load_system_fonts();
    Arc::new(db)
});

/// Renders an SVG with resvg. Small drawings are rendered larger so they stay sharp.
fn render_svg(bytes: &[u8], max_side: u32) -> Result<(ColorImage, u32, u32), String> {
    let opt = usvg::Options {
        fontdb: FONTS.clone(),
        ..Default::default()
    };
    let tree =
        usvg::Tree::from_data(bytes, &opt).map_err(|e| t!("Could not decode the image: {}", e))?;
    let size = tree.size();
    let (w, h) = (size.width().max(1.0), size.height().max(1.0));
    let longest = w.max(h);
    let scale = (SVG_MIN_SIDE / longest)
        .max(1.0)
        .min(max_side as f32 / longest);
    let (pw, ph) = ((w * scale).ceil() as u32, (h * scale).ceil() as u32);
    check_pixels(pw, ph)?;
    let mut pixmap = tiny_skia::Pixmap::new(pw.max(1), ph.max(1))
        .ok_or_else(|| t!("Could not decode the image: {}", "size"))?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let image = ColorImage::from_rgba_premultiplied(
        [pixmap.width() as usize, pixmap.height() as usize],
        pixmap.data(),
    );
    Ok((image, w.round() as u32, h.round() as u32))
}

/// Renders the first page of a PDF with hayro, about 1000 pixels on its longer side.
fn render_pdf(bytes: &[u8], max_side: u32) -> Result<(ColorImage, u32, u32), String> {
    use hayro::hayro_interpret::InterpreterSettings;
    let pdf = hayro::hayro_syntax::Pdf::new(bytes.to_vec())
        .map_err(|e| t!("Could not decode the image: {}", format!("{e:?}")))?;
    let page = pdf
        .pages()
        .first()
        .ok_or_else(|| t!("Could not decode the image: {}", "no pages"))?;
    let (w, h) = page.render_dimensions();
    let longest = w.max(h).max(1.0);
    let scale = (PDF_SIDE / longest).min(max_side as f32 / longest);
    check_pixels((w * scale) as u32, (h * scale) as u32)?;
    let settings = hayro::PixmapSettings {
        x_scale: scale,
        y_scale: scale,
        bg_color: hayro::vello_cpu::color::palette::css::WHITE,
    };
    let pixmap = hayro::render(
        page,
        &hayro::RenderCache::new(),
        &InterpreterSettings::default(),
        &hayro::RenderSettings::default(),
        &settings,
    );
    let size = [pixmap.width() as usize, pixmap.height() as usize];
    let image = ColorImage::from_rgba_premultiplied(size, pixmap.data_as_u8_slice());
    Ok((image, w.round() as u32, h.round() as u32))
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
    let dark = ui.visuals().dark_mode;
    let texture = ctx
        .data(|d| d.get_temp::<Cache>(cache_id()))
        .and_then(|c| c.checker)
        .filter(|(d, _)| *d == dark)
        .map(|(_, t)| t);
    let texture = texture.unwrap_or_else(|| {
        let (a, b) = if dark {
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
            d.get_temp_mut_or_default::<Cache>(cache_id()).checker = Some((dark, t.clone()));
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

    fn encode(w: u32, h: u32, format: image::ImageFormat) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([255, 0, 0, 128]));
        let mut out = Vec::new();
        img.write_to(&mut Cursor::new(&mut out), format).unwrap();
        out
    }

    fn png(w: u32, h: u32) -> Vec<u8> {
        encode(w, h, image::ImageFormat::Png)
    }

    #[test]
    fn decodes_and_shrinks_for_the_gpu() {
        let (img, w, h) = decode(&png(40, 20), 2048, "a.png").unwrap();
        assert_eq!((w, h, img.size), (40, 20, [40, 20]));
        let (img, w, h) = decode(&png(400, 100), 100, "a.png").unwrap();
        assert_eq!((w, h), (400, 100));
        assert_eq!(img.size, [100, 25]);
    }

    #[test]
    fn decodes_tga_and_svg() {
        let tga = encode(8, 4, image::ImageFormat::Tga);
        let (_, w, h) = decode(&tga, 2048, "pic.TGA").unwrap();
        assert_eq!((w, h), (8, 4));
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="12"><rect width="24" height="12" fill="red"/></svg>"#;
        let (img, w, h) = decode(svg, 2048, "icon.svg").unwrap();
        assert_eq!((w, h), (24, 12));
        // Rendered larger than its nominal size, keeping the aspect ratio.
        assert_eq!(img.size, [512, 256]);
        assert_eq!(img.pixels[0], Color32::from_rgb(255, 0, 0));
        assert!(decode(b"<svg", 2048, "bad.svg").is_err());
        assert!(is_image("a/b.svg") && is_image("x.tga") && !is_image("x.txt"));
    }

    #[test]
    fn renders_the_first_pdf_page() {
        let pdf = minimal_pdf();
        let (img, w, h) = decode(&pdf, 4096, "doc.PDF").unwrap();
        assert_eq!((w, h), (200, 100));
        assert_eq!(img.size, [1000, 500]);
        // White page with a black box in the middle.
        assert_eq!(img.pixels[0], Color32::WHITE);
        assert_eq!(img.pixels[250 * 1000 + 500], Color32::BLACK);
        assert!(decode(b"%PDF-1.4 broken", 4096, "x.pdf").is_err());
    }

    /// A one-page PDF (200×100 points) with a filled rectangle.
    fn minimal_pdf() -> Vec<u8> {
        let content = "0 0 0 rg 50 25 100 50 re f";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R >>".to_owned(),
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            ),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
        for off in offsets {
            out.extend(format!("{off:010} 00000 n \n").bytes());
        }
        out.extend(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .bytes(),
        );
        out
    }

    #[test]
    fn highlights_differing_pixels() {
        let a = ColorImage::new([2, 2], vec![Color32::RED; 4]);
        let mut b = a.clone();
        b.pixels[3] = Color32::BLUE;
        let (img, count) = pixel_difference(&a, &b);
        assert_eq!(count, 1);
        assert_eq!(img.pixels[3], Color32::from_rgb(0xff, 0x00, 0xff));
        assert_ne!(img.pixels[0], img.pixels[3]);
    }

    #[test]
    fn rejects_garbage_and_huge_images() {
        assert!(decode(b"not an image", 2048, "a.png").is_err());
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
