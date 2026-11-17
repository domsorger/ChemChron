// SPDX-License-Identifier: GPL-3.0-or-later
//! The program icon: the ChemChron logo from the top-left corner of the
//! window (a ring, two dots and a line), drawn large on a dark rounded square.
//! An own icon replaces it: put a square PNG at `assets/icon.png` (1024 × 1024
//! pixels is best) and build again; `macos/make_app.sh` makes the Mac icon from
//! the same file.

/// Your own icon, when `assets/icon.png` was there at build time (see build.rs).
const CUSTOM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/custom_icon.png"));

/// RGBA pixels of the icon, `size` × `size`.
pub fn rgba(size: u32) -> Vec<u8> {
    if let Some(img) = custom(size) {
        return img;
    }
    drawn(size)
}

fn custom(size: u32) -> Option<Vec<u8>> {
    if CUSTOM.is_empty() {
        return None;
    }
    let img = image::load_from_memory(CUSTOM).ok()?;
    Some(img.resize_exact(size, size, image::imageops::FilterType::Lanczos3).to_rgba8().into_raw())
}

/// The window icon for eframe.
pub fn icon_data() -> egui::IconData {
    let size = 256;
    egui::IconData { rgba: rgba(size), width: size, height: size }
}

/// Writes the icon as a PNG file (used by macos/make_app.sh and linux/install.sh),
/// or as a Windows .ico when the name ends in .ico (windows/make_windows.bat).
pub fn write_png(path: &std::path::Path, size: u32) -> Result<(), String> {
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ico")) {
        return write_ico(path);
    }
    image::save_buffer(path, &rgba(size), size, size, image::ExtendedColorType::Rgba8).map_err(|e| e.to_string())
}

/// A Windows icon file holding PNG images of 16 to 256 pixels.
fn write_ico(path: &std::path::Path) -> Result<(), String> {
    let sizes = [16u32, 32, 48, 64, 128, 256];
    let mut images: Vec<Vec<u8>> = Vec::new();
    for s in sizes {
        let mut png = Vec::new();
        let enc = image::codecs::png::PngEncoder::new(&mut png);
        image::ImageEncoder::write_image(enc, &rgba(s), s, s, image::ExtendedColorType::Rgba8).map_err(|e| e.to_string())?;
        images.push(png);
    }
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (s, png) in sizes.iter().zip(&images) {
        let b = if *s >= 256 { 0 } else { *s as u8 };
        out.extend_from_slice(&[b, b, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for png in &images {
        out.extend_from_slice(png);
    }
    std::fs::write(path, out).map_err(|e| e.to_string())
}

/// The logo drawn with smooth edges (distance to each shape, one pixel of blur).
fn drawn(size: u32) -> Vec<u8> {
    let n = size as f64;
    // the 24-point logo, scaled up: centre (12, 12), ring radius 10
    let k = n / 24.0 * 0.78;
    let c = (n / 2.0, n / 2.0);
    let bg = [27.0, 31.0, 38.0];
    let accent = [79.0, 195.0, 176.0];
    let amber = [227.0, 169.0, 72.0];
    let mut out = vec![0u8; (size * size * 4) as usize];
    let radius = n * 0.22; // corner radius of the square
    let inset = n * 0.06;
    for y in 0..size {
        for x in 0..size {
            let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
            // rounded square background
            let (hx, hy) = (n / 2.0 - inset - radius, n / 2.0 - inset - radius);
            let qx = (px - c.0).abs() - hx;
            let qy = (py - c.1).abs() - hy;
            let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - radius;
            let a_bg = (0.5 - outside).clamp(0.0, 1.0);
            let mut col = bg;
            let cover = |d: f64, colour: [f64; 3], col: &mut [f64; 3]| {
                let a = (0.5 - d).clamp(0.0, 1.0);
                for i in 0..3 {
                    col[i] = col[i] * (1.0 - a) + colour[i] * a;
                }
            };
            // logo coordinates
            let (lx, ly) = ((px - c.0) / k, (py - c.1) / k);
            let dist = |ax: f64, ay: f64| ((lx - ax).powi(2) + (ly - ay).powi(2)).sqrt();
            // ring (radius 10, width 2)
            cover((dist(0.0, 0.0) - 10.0).abs() * k - 1.0 * k, accent, &mut col);
            // line from (-7, 6) to (7, -6), width 1.5
            let (ax, ay, bx, by) = (-7.0, 6.0, 7.0, -6.0);
            let t = (((lx - ax) * (bx - ax) + (ly - ay) * (by - ay)) / ((bx - ax).powi(2) + (by - ay).powi(2))).clamp(0.0, 1.0);
            let dl = dist(ax + t * (bx - ax), ay + t * (by - ay));
            cover(dl * k - 0.75 * k, accent, &mut col);
            // two dots
            cover((dist(-3.5, 2.0) - 2.6) * k, accent, &mut col);
            cover((dist(4.0, -3.0) - 2.6) * k, amber, &mut col);
            let i = ((y * size + x) * 4) as usize;
            out[i] = col[0].round() as u8;
            out[i + 1] = col[1].round() as u8;
            out[i + 2] = col[2].round() as u8;
            out[i + 3] = (a_bg * 255.0).round() as u8;
        }
    }
    out
}
