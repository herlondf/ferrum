//! The app icon, drawn in code so it needs no image files: a rust-colored
//! rounded square with a small branch graph.

/// Icon pixels as RGBA, `size` x `size`.
pub fn rgba(size: u32) -> Vec<u8> {
    let s = size as f32;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    // 4x4 supersampling for smooth edges.
    const N: u32 = 4;
    for y in 0..size {
        for x in 0..size {
            let (mut bg, mut fg) = (0.0, 0.0);
            for sy in 0..N {
                for sx in 0..N {
                    let px = (x as f32 + (sx as f32 + 0.5) / N as f32) / s;
                    let py = (y as f32 + (sy as f32 + 0.5) / N as f32) / s;
                    if rounded_square(px, py) <= 0.0 {
                        bg += 1.0;
                        if graph(px, py) <= 0.0 {
                            fg += 1.0;
                        }
                    }
                }
            }
            let total = (N * N) as f32;
            let (bg, fg) = (bg / total, fg / total);
            // Vertical gradient from a bright to a deep rust tone.
            let t = y as f32 / s;
            let base = [
                lerp(0xd9, 0x8a, t),
                lerp(0x55, 0x2c, t),
                lerp(0x1e, 0x0f, t),
            ];
            let light = [0xff, 0xf3, 0xe6];
            let mix = if bg > 0.0 { fg / bg } else { 0.0 };
            for c in 0..3 {
                out.push((base[c] * (1.0 - mix) + light[c] as f32 * mix).round() as u8);
            }
            out.push((bg * 255.0).round() as u8);
        }
    }
    out
}

fn lerp(a: u8, b: u8, t: f32) -> f32 {
    a as f32 + (b as f32 - a as f32) * t
}

/// Signed distance to the rounded square background (negative inside).
fn rounded_square(x: f32, y: f32) -> f32 {
    let (half, r) = (0.46, 0.2);
    let dx = (x - 0.5).abs() - (half - r);
    let dy = (y - 0.5).abs() - (half - r);
    let outside = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt();
    outside + dx.max(dy).min(0.0) - r
}

fn segment(px: f32, py: f32, a: (f32, f32), b: (f32, f32), width: f32) -> f32 {
    let (vx, vy) = (b.0 - a.0, b.1 - a.1);
    let t = (((px - a.0) * vx + (py - a.1) * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
    let (cx, cy) = (a.0 + vx * t, a.1 + vy * t);
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt() - width / 2.0
}

fn circle(px: f32, py: f32, c: (f32, f32), r: f32) -> f32 {
    ((px - c.0).powi(2) + (py - c.1).powi(2)).sqrt() - r
}

/// A main line with a branch going off to the right.
fn graph(x: f32, y: f32) -> f32 {
    let w = 0.075;
    [
        segment(x, y, (0.37, 0.22), (0.37, 0.78), w),
        segment(x, y, (0.37, 0.64), (0.64, 0.44), w),
        segment(x, y, (0.64, 0.44), (0.64, 0.3), w),
        circle(x, y, (0.37, 0.78), 0.085),
        circle(x, y, (0.37, 0.24), 0.085),
        circle(x, y, (0.64, 0.28), 0.085),
    ]
    .into_iter()
    .fold(f32::MAX, f32::min)
}

#[cfg(test)]
mod tests {
    #[test]
    fn has_transparent_corners_and_opaque_center() {
        let size = 64;
        let px = super::rgba(size);
        assert_eq!(px.len(), (size * size * 4) as usize);
        assert_eq!(px[3], 0, "corner is transparent");
        let center = ((size / 2 * size + size / 2) * 4 + 3) as usize;
        assert_eq!(px[center], 255);
    }

    /// Writes the PNG used by the Linux desktop entry:
    /// `cargo test -p ferrum export_icon -- --ignored`.
    #[test]
    #[ignore]
    fn export_icon() {
        let size = 256;
        let img = image::RgbaImage::from_raw(size, size, super::rgba(size)).unwrap();
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../packaging/linux/ferrum.png"
        );
        img.save(path).unwrap();
    }
}
