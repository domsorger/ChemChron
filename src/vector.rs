// SPDX-License-Identifier: GPL-3.0-or-later
//! Figures as vector files: one page description (`Doc`, in CSS pixels at
//! 96 per inch, origin top left) written as SVG, PDF or EPS. PDF and EPS use
//! the standard Helvetica and Symbol fonts, so nothing is embedded; raised
//! and lowered digits (²⁰⁶Pb, H₂O) are drawn as smaller raised / lowered text
//! and Greek letters come from Symbol. EPS has no transparency, so
//! see-through fills are mixed with white.

use crate::figure::{Figure, Item};
use egui::Color32;
use chemchron_laser::plot;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Anchor {
    Start,
    Middle,
    End,
}

#[derive(Clone, Debug)]
pub enum Op {
    /// Polyline or polygon; `fill` with opacity, `stroke` with width and dash.
    Path { pts: Vec<(f64, f64)>, closed: bool, fill: Option<(Color32, f64)>, stroke: Option<(Color32, f64, bool)> },
    Circle { cx: f64, cy: f64, r: f64, fill: Option<Color32>, stroke: Option<(Color32, f64)> },
    Text { x: f64, y: f64, text: String, size: f64, anchor: Anchor, bold: bool, vertical: bool, color: Color32 },
    ClipStart { x: f64, y: f64, w: f64, h: f64 },
    ClipEnd,
}

#[derive(Clone, Debug)]
pub struct Doc {
    pub w_mm: f64,
    pub h_mm: f64,
    pub w: f64,
    pub h: f64,
    pub font: f64,
    pub ops: Vec<Op>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Pdf,
    Svg,
    Eps,
    Png,
    Tiff,
}

impl Format {
    pub const ALL: [Format; 5] = [Format::Pdf, Format::Svg, Format::Eps, Format::Png, Format::Tiff];
    pub fn is_raster(&self) -> bool {
        matches!(self, Format::Png | Format::Tiff)
    }
    pub fn ext(&self) -> &'static str {
        match self {
            Format::Pdf => "pdf",
            Format::Svg => "svg",
            Format::Eps => "eps",
            Format::Png => "png",
            Format::Tiff => "tiff",
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            Format::Pdf => "PDF (vector)",
            Format::Svg => "SVG (vector)",
            Format::Eps => "EPS (vector)",
            Format::Png => "PNG",
            Format::Tiff => "TIFF",
        }
    }
    pub fn from_ext(s: &str) -> Format {
        match s.to_lowercase().as_str() {
            "svg" => Format::Svg,
            "eps" => Format::Eps,
            "png" => Format::Png,
            "tif" | "tiff" => Format::Tiff,
            _ => Format::Pdf,
        }
    }
}

const DARK: Color32 = Color32::from_rgb(0x33, 0x33, 0x33);

fn rect(x: f64, y: f64, w: f64, h: f64) -> Vec<(f64, f64)> {
    vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
}

/// Lays the figure out on a `w_mm` × `h_mm` page.
pub fn layout(fig: &Figure, w_mm: f64, h_mm: f64, font_pt: f64, with_title: bool) -> Doc {
    let (w, h) = (w_mm * 96.0 / 25.4, h_mm * 96.0 / 25.4);
    let fs = font_pt * 96.0 / 72.0;
    let n_notes = if with_title { fig.notes.len() } else { 0 };
    let title_lines: Vec<&str> = if with_title { fig.title.lines().filter(|l| !l.trim().is_empty()).collect() } else { Vec::new() };
    let ts = fs * 1.15;
    let top = if with_title { fs * 0.6 + ts * 1.3 * title_lines.len() as f64 + fs * 1.15 * n_notes as f64 + fs * 0.6 } else { fs * 0.8 };
    // room below the axis: tick, gap, tick labels, gap, axis title
    let (l, r, b) = if fig.bare { (fs, fs, fs) } else { (fs * 5.4, fs * 1.2, fs * 4.0) };
    let pw = (w - l - r).max(10.0);
    let ph = (h - top - b).max(10.0);
    let (x0, x1) = fig.x;
    let (ya, yb) = if fig.log_y { (fig.y.0.max(1e-30).log10(), fig.y.1.max(1e-30).log10()) } else { fig.y };
    let sx = |x: f64| l + (x - x0) / (x1 - x0) * pw;
    let sy = |y: f64| {
        let v = if fig.log_y { y.max(1e-30).log10() } else { y };
        top + ph - (v - ya) / (yb - ya) * ph
    };
    let mut ops = Vec::new();
    let text = |x: f64, y: f64, s: &str, anchor: Anchor| Op::Text { x, y, text: s.to_string(), size: fs, anchor, bold: false, vertical: false, color: DARK };
    let tick = fs * 0.45;
    if with_title {
        // centred title lines above the plot
        let mut y = fs * 0.6;
        for line in &title_lines {
            y += ts * 1.3;
            ops.push(Op::Text { x: l + pw / 2.0, y: y - ts * 0.3, text: line.to_string(), size: ts, anchor: Anchor::Middle, bold: false, vertical: false, color: Color32::BLACK });
        }
        for n in fig.notes.iter() {
            y += fs * 1.15;
            ops.push(Op::Text { x: l + pw / 2.0, y: y - fs * 0.25, text: n.clone(), size: fs * 0.85, anchor: Anchor::Middle, bold: false, vertical: false, color: DARK });
        }
    }
    if !fig.bare {
        ops.push(Op::Path { pts: rect(l, top, pw, ph), closed: true, fill: None, stroke: Some((DARK, 0.8, false)) });
        // tick labels sit a clear gap below the tick marks
        let label_y = top + ph + tick + fs * 1.15;
        if fig.x_categories.is_empty() {
            for v in chemchron_laser_core::svg::nice_ticks(x0, x1, 6) {
                let px = sx(v);
                ops.push(Op::Path { pts: vec![(px, top + ph), (px, top + ph + tick)], closed: false, fill: None, stroke: Some((DARK, 0.8, false)) });
                ops.push(text(px, label_y, &plot::fmt_tick(v, x1 - x0), Anchor::Middle));
            }
        } else {
            for (i, c) in fig.x_categories.iter().enumerate() {
                let px = sx(i as f64);
                ops.push(Op::Path { pts: vec![(px, top + ph), (px, top + ph + tick)], closed: false, fill: None, stroke: Some((DARK, 0.8, false)) });
                ops.push(text(px, label_y, c, Anchor::Middle));
            }
        }
        let ticks: Vec<(f64, String)> = if fig.log_y {
            let (a, bb) = (ya.floor() as i32, yb.ceil() as i32);
            (a..=bb).map(|e| 10f64.powi(e)).map(|v| (v, plot::decade_label(v))).collect()
        } else {
            chemchron_laser_core::svg::nice_ticks(fig.y.0, fig.y.1, 6).into_iter().map(|v| (v, plot::fmt_tick(v, fig.y.1 - fig.y.0))).collect()
        };
        for (v, s) in ticks {
            let py = sy(v);
            if py < top - 0.5 || py > top + ph + 0.5 {
                continue;
            }
            ops.push(Op::Path { pts: vec![(l - tick, py), (l, py)], closed: false, fill: None, stroke: Some((DARK, 0.8, false)) });
            ops.push(text(l - tick - fs * 0.4, py + fs * 0.35, &s, Anchor::End));
        }
        ops.push(text(l + pw / 2.0, h - fs * 0.55, &fig.x_label, Anchor::Middle));
        ops.push(Op::Text { x: fs * 1.1, y: top + ph / 2.0, text: fig.y_label.clone(), size: fs, anchor: Anchor::Middle, bold: false, vertical: true, color: DARK });
    }
    ops.push(Op::ClipStart { x: l, y: top, w: pw, h: ph });
    let pts = |p: &[(f64, f64)]| -> Vec<(f64, f64)> { p.iter().filter(|(x, y)| x.is_finite() && y.is_finite() && (!fig.log_y || *y > 0.0)).map(|(x, y)| (sx(*x), sy(*y))).collect() };
    let marker_r = |size: f32| fs * 0.36 * (size as f64 / 4.0);
    for it in &fig.items {
        match it {
            Item::Band { y0, y1, fill } => {
                let (a, bb) = (sy(*y1).min(sy(*y0)), sy(*y1).max(sy(*y0)));
                ops.push(Op::Path { pts: rect(l, a, pw, bb - a), closed: true, fill: Some((opaque(*fill), (fill.a() as f64 / 255.0).clamp(0.1, 1.0))), stroke: None });
            }
            Item::Poly { pts: p, fill, stroke } => {
                let alpha = (fill.a() as f64 / 255.0).max(0.15);
                ops.push(Op::Path { pts: pts(p), closed: true, fill: Some((opaque(*fill), alpha)), stroke: if stroke.a() == 0 { None } else { Some((opaque(*stroke), 0.6, false)) } });
            }
            Item::Line { pts: p, color, width, dashed } => {
                ops.push(Op::Path { pts: pts(p), closed: false, fill: None, stroke: Some((opaque(*color), *width as f64 * 0.75, *dashed)) });
            }
            Item::Marker { x, y, color, size, hollow, .. } => {
                if x.is_finite() && y.is_finite() && *size > 0.05 && (!fig.log_y || *y > 0.0) {
                    let c = opaque(*color);
                    if *hollow {
                        ops.push(Op::Circle { cx: sx(*x), cy: sy(*y), r: marker_r(*size), fill: Some(Color32::WHITE), stroke: Some((c, 0.9)) });
                    } else {
                        ops.push(Op::Circle { cx: sx(*x), cy: sy(*y), r: marker_r(*size), fill: Some(c), stroke: Some((crate::figure::edge_of(c), 0.5)) });
                    }
                }
            }
            Item::Text { x, y, text: s, color, bold, align, .. } => {
                let anchor = match align {
                    egui::Align::Min => Anchor::Start,
                    egui::Align::Center => Anchor::Middle,
                    egui::Align::Max => Anchor::End,
                };
                ops.push(Op::Text { x: sx(*x), y: sy(*y) + fs * 0.35, text: s.clone(), size: if *bold { fs * 1.1 } else { fs }, anchor, bold: *bold, vertical: false, color: if *bold { Color32::BLACK } else { opaque(*color) } });
            }
        }
    }
    ops.push(Op::ClipEnd);
    // legend in a box, in the corner with the most room
    if !fig.legend.is_empty() {
        let row = fs * 1.35;
        let sym = fs * 1.6;
        let wmax = fig.legend.iter().map(|(n, _)| width(&runs(n, false, false), fs)).fold(0.0, f64::max);
        let (bw, bh) = (wmax + sym + fs * 1.4, row * fig.legend.len() as f64 + fs * 0.6);
        let (bx, by) = crate::figure::legend_spot(fig, (l, top, pw, ph), (bw, bh), fs * 0.5, &|x, y| (sx(x), sy(y)));
        ops.push(Op::Path { pts: rect(bx, by, bw, bh), closed: true, fill: Some((Color32::WHITE, 0.9)), stroke: Some((Color32::from_rgb(0xbb, 0xbb, 0xbb), 0.6, false)) });
        for (k, (name, c)) in fig.legend.iter().enumerate() {
            let c = opaque(*c);
            let cy = by + fs * 0.3 + row * (k as f64 + 0.5);
            let cx = bx + fs * 0.5 + sym / 2.0;
            let hollow = fig.legend_hollow.contains(name);
            match fig.legend_kind {
                crate::figure::LegendKind::Bar => {
                    ops.push(Op::Path { pts: rect(cx - sym * 0.4, cy - fs * 0.3, sym * 0.8, fs * 0.6), closed: true, fill: Some((c, 1.0)), stroke: Some((crate::figure::edge_of(c), 0.5, false)) });
                }
                crate::figure::LegendKind::Line => {
                    ops.push(Op::Path { pts: vec![(cx - sym * 0.45, cy), (cx + sym * 0.45, cy)], closed: false, fill: None, stroke: Some((c, 1.2, false)) });
                    ops.push(Op::Circle { cx, cy, r: fs * 0.28, fill: Some(if hollow { Color32::WHITE } else { c }), stroke: Some((c, 0.7)) });
                }
                crate::figure::LegendKind::Marker => {
                    if hollow {
                        ops.push(Op::Circle { cx, cy, r: fs * 0.32, fill: Some(Color32::WHITE), stroke: Some((c, 0.9)) });
                    } else {
                        ops.push(Op::Circle { cx, cy, r: fs * 0.32, fill: Some(c), stroke: Some((crate::figure::edge_of(c), 0.5)) });
                    }
                }
            }
            ops.push(Op::Text { x: bx + fs * 0.9 + sym, y: cy + fs * 0.35, text: name.clone(), size: fs, anchor: Anchor::Start, bold: false, vertical: false, color: DARK });
        }
    }
    Doc { w_mm, h_mm, w, h, font: fs, ops }
}

fn opaque(c: Color32) -> Color32 {
    let [r, g, b, _] = c.to_srgba_unmultiplied();
    Color32::from_rgb(r, g, b)
}

pub fn write(fig: &Figure, format: Format, w_mm: f64, h_mm: f64, font_pt: f64, with_title: bool) -> Vec<u8> {
    write_dpi(fig, format, w_mm, h_mm, font_pt, with_title, 300)
}

/// The figure in any format; PNG and TIFF at `dpi` pixels per inch.
pub fn write_dpi(fig: &Figure, format: Format, w_mm: f64, h_mm: f64, font_pt: f64, with_title: bool, dpi: u32) -> Vec<u8> {
    let d = layout(fig, w_mm, h_mm, font_pt, with_title);
    match format {
        Format::Svg => svg(&d).into_bytes(),
        Format::Pdf => pdf(&d),
        Format::Eps => eps(&d).into_bytes(),
        Format::Png | Format::Tiff => raster(&d, format, dpi).unwrap_or_default(),
    }
}

/// The page as a picture (the SVG drawn by resvg): system fonts, with the
/// program's own font as fallback; white background.
pub fn raster(d: &Doc, format: Format, dpi: u32) -> Result<Vec<u8>, String> {
    let mut opt = resvg::usvg::Options::default();
    {
        let db = opt.fontdb_mut();
        db.load_system_fonts();
        db.load_font_data(epaint_default_fonts::UBUNTU_LIGHT.to_vec());
        if db.faces().all(|f| !f.families.iter().any(|(n, _)| n == "Arial" || n == "Helvetica")) {
            db.set_sans_serif_family("Ubuntu");
        }
    }
    let tree = resvg::usvg::Tree::from_str(&svg(d), &opt).map_err(|e| e.to_string())?;
    let scale = dpi.max(72) as f32 / 96.0;
    let (w, h) = ((tree.size().width() * scale).ceil() as u32, (tree.size().height() * scale).ceil() as u32);
    let mut pix = resvg::tiny_skia::Pixmap::new(w.max(1), h.max(1)).ok_or("The picture would be too large")?;
    pix.fill(resvg::tiny_skia::Color::WHITE);
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pix.as_mut());
    match format {
        Format::Tiff => {
            let img = image::RgbaImage::from_raw(w.max(1), h.max(1), pix.data().to_vec()).ok_or("picture")?;
            let mut out = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(img).to_rgb8().write_to(&mut out, image::ImageFormat::Tiff).map_err(|e| e.to_string())?;
            Ok(out.into_inner())
        }
        _ => pix.encode_png().map_err(|e| e.to_string()),
    }
}

// ---- SVG ---------------------------------------------------------------------------------------

fn hex(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub fn svg(d: &Doc) -> String {
    let mut s = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}mm\" height=\"{}mm\" viewBox=\"0 0 {:.2} {:.2}\" font-family=\"Helvetica, Arial, sans-serif\" font-size=\"{:.2}\">\n<rect width=\"100%\" height=\"100%\" fill=\"white\"/>\n",
        d.w_mm, d.h_mm, d.w, d.h, d.font
    );
    let mut clip = 0;
    for op in &d.ops {
        match op {
            Op::Path { pts, closed, fill, stroke } => {
                if pts.is_empty() {
                    continue;
                }
                let path: String = pts.iter().enumerate().map(|(i, (x, y))| format!("{}{x:.2},{y:.2}", if i == 0 { "M" } else { " L" })).collect();
                let f = match fill {
                    Some((c, a)) => format!(" fill=\"{}\" fill-opacity=\"{a:.2}\"", hex(*c)),
                    None => " fill=\"none\"".into(),
                };
                let st = match stroke {
                    Some((c, w, dash)) => format!(" stroke=\"{}\" stroke-width=\"{w:.2}\"{}", hex(*c), if *dash { " stroke-dasharray=\"4,3\"" } else { "" }),
                    None => String::new(),
                };
                s += &format!("<path d=\"{path}{}\"{f}{st}/>\n", if *closed { " Z" } else { "" });
            }
            Op::Circle { cx, cy, r, fill, stroke } => {
                let f = fill.map(hex).unwrap_or_else(|| "none".into());
                let st = stroke.map(|(c, w)| format!(" stroke=\"{}\" stroke-width=\"{w:.2}\"", hex(c))).unwrap_or_default();
                s += &format!("<circle cx=\"{cx:.2}\" cy=\"{cy:.2}\" r=\"{r:.2}\" fill=\"{f}\"{st}/>\n");
            }
            Op::Text { x, y, text, size, anchor, bold, vertical, color } => {
                let a = match anchor {
                    Anchor::Start => "start",
                    Anchor::Middle => "middle",
                    Anchor::End => "end",
                };
                let tr = if *vertical { format!(" transform=\"translate({x:.2},{y:.2}) rotate(-90)\" x=\"0\" y=\"0\"") } else { format!(" x=\"{x:.2}\" y=\"{y:.2}\"") };
                s += &format!(
                    "<text{tr} text-anchor=\"{a}\" font-size=\"{size:.2}\" fill=\"{}\"{}>{}</text>\n",
                    hex(*color),
                    if *bold { " font-weight=\"bold\"" } else { "" },
                    esc(text)
                );
            }
            Op::ClipStart { x, y, w, h } => {
                clip += 1;
                s += &format!("<clipPath id=\"c{clip}\"><rect x=\"{x:.2}\" y=\"{y:.2}\" width=\"{w:.2}\" height=\"{h:.2}\"/></clipPath>\n<g clip-path=\"url(#c{clip})\">\n");
            }
            Op::ClipEnd => s += "</g>\n",
        }
    }
    s += "</svg>\n";
    s
}

// ---- text for PDF / EPS ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Font {
    Regular,
    Bold,
    Symbol,
}

/// A piece of text in one font, size factor and baseline shift (both relative to the font size).
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub font: Font,
    pub scale: f64,
    pub rise: f64,
    pub bytes: Vec<u8>,
}

fn sup(c: char) -> Option<char> {
    Some(match c {
        '⁰' => '0',
        '¹' => '1',
        '²' => '2',
        '³' => '3',
        '⁴' => '4',
        '⁵' => '5',
        '⁶' => '6',
        '⁷' => '7',
        '⁸' => '8',
        '⁹' => '9',
        '\u{207a}' => '+',
        '\u{207b}' => '-',
        _ => return None,
    })
}

fn sub(c: char) -> Option<char> {
    Some(match c {
        '₀'..='₉' => char::from(b'0' + (c as u32 - '₀' as u32) as u8),
        _ => return None,
    })
}

fn symbol(c: char) -> Option<u8> {
    Some(match c {
        'α' => b'a',
        'β' => b'b',
        'χ' => b'c',
        'δ' => b'd',
        'ε' => b'e',
        'γ' => b'g',
        'λ' => b'l',
        'μ' => b'm',
        'π' => b'p',
        'θ' => b'q',
        'ρ' => b'r',
        'σ' => b's',
        'τ' => b't',
        'Δ' => b'D',
        'Σ' => b'S',
        '√' => 0xD6,
        '≈' => 0xBB,
        '≤' => 0xA3,
        '≥' => 0xB3,
        '\u{2192}' => 0xAE,
        '∑' => b'S',
        _ => return None,
    })
}

/// Latin text byte for Helvetica (WinAnsi for PDF; `latin1_only` for EPS).
fn latin(c: char, latin1_only: bool) -> u8 {
    let u = c as u32;
    if (0x20..0x7f).contains(&u) || (0xa0..=0xff).contains(&u) {
        return u as u8;
    }
    match c {
        '–' => {
            if latin1_only {
                b'-'
            } else {
                0x96
            }
        }
        '—' => {
            if latin1_only {
                b'-'
            } else {
                0x97
            }
        }
        '−' => b'-',
        '’' => b'\'',
        '‘' => b'\'',
        '“' | '”' => b'"',
        '…' => b'.',
        _ => b'?',
    }
}

pub fn runs(s: &str, bold: bool, latin1_only: bool) -> Vec<Run> {
    let base = if bold { Font::Bold } else { Font::Regular };
    let mut out: Vec<Run> = Vec::new();
    for c in s.chars() {
        let (font, scale, rise, byte) = if let Some(d) = sup(c) {
            (base, 0.68, 0.36, d as u8)
        } else if let Some(d) = sub(c) {
            (base, 0.68, -0.16, d as u8)
        } else if let Some(b) = symbol(c) {
            (Font::Symbol, 1.0, 0.0, b)
        } else {
            (base, 1.0, 0.0, latin(c, latin1_only))
        };
        match out.last_mut() {
            Some(r) if r.font == font && r.scale == scale && r.rise == rise => r.bytes.push(byte),
            _ => out.push(Run { font, scale, rise, bytes: vec![byte] }),
        }
    }
    out
}

/// Helvetica advance widths (1/1000 em) for ASCII 32..126.
const HELV: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667, 611,
    778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833,
    556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];

fn char_w(font: Font, b: u8) -> f64 {
    match font {
        Font::Symbol => 0.6,
        _ => {
            let w = if (32..127).contains(&b) { HELV[(b - 32) as usize] as f64 } else { 556.0 };
            let bold = if font == Font::Bold { 1.06 } else { 1.0 };
            w / 1000.0 * bold
        }
    }
}

/// Width of the runs at font size `size`.
pub fn width(rs: &[Run], size: f64) -> f64 {
    rs.iter().map(|r| r.bytes.iter().map(|b| char_w(r.font, *b)).sum::<f64>() * r.scale * size).sum()
}

fn ps_string(bytes: &[u8]) -> String {
    let mut s = String::from("(");
    for b in bytes {
        match b {
            b'(' | b')' | b'\\' => {
                s.push('\\');
                s.push(*b as char);
            }
            0x20..=0x7e => s.push(*b as char),
            _ => s += &format!("\\{:03o}", b),
        }
    }
    s.push(')');
    s
}

fn rgb(c: Color32) -> (f64, f64, f64) {
    (c.r() as f64 / 255.0, c.g() as f64 / 255.0, c.b() as f64 / 255.0)
}

fn blend_white(c: Color32, a: f64) -> Color32 {
    let m = |v: u8| (v as f64 * a + 255.0 * (1.0 - a)).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(m(c.r()), m(c.g()), m(c.b()))
}

/// Four Bézier arcs for a circle, in page coordinates.
fn circle_path(cx: f64, cy: f64, r: f64) -> Vec<[f64; 6]> {
    let k = 0.552_284_75 * r;
    vec![
        [cx + r, cy + k, cx + k, cy + r, cx, cy + r],
        [cx - k, cy + r, cx - r, cy + k, cx - r, cy],
        [cx - r, cy - k, cx - k, cy - r, cx, cy - r],
        [cx + k, cy - r, cx + r, cy - k, cx + r, cy],
    ]
}

// ---- PDF -------------------------------------------------------------------------------------------

pub fn pdf(d: &Doc) -> Vec<u8> {
    let k = 0.75; // px to pt
    let ph = d.h * k;
    let p = |x: f64, y: f64| (x * k, ph - y * k);
    let mut alphas: Vec<u32> = Vec::new();
    let gs = |a: f64, alphas: &mut Vec<u32>| -> String {
        let q = (a.clamp(0.0, 1.0) * 100.0).round() as u32;
        if !alphas.contains(&q) {
            alphas.push(q);
        }
        format!("/GS{q} gs\n")
    };
    let mut c = String::new();
    c += &format!("1 1 1 rg 0 0 {:.2} {:.2} re f\n", d.w * k, ph);
    for op in &d.ops {
        match op {
            Op::Path { pts, closed, fill, stroke } => {
                if pts.len() < 2 {
                    continue;
                }
                c += "q\n";
                let mut path = String::new();
                for (i, (x, y)) in pts.iter().enumerate() {
                    let (a, b) = p(*x, *y);
                    path += &format!("{a:.2} {b:.2} {}\n", if i == 0 { "m" } else { "l" });
                }
                if *closed {
                    path += "h\n";
                }
                if let Some((col, a)) = fill {
                    let (r, g, b) = rgb(*col);
                    c += &gs(*a, &mut alphas);
                    c += &format!("{r:.3} {g:.3} {b:.3} rg\n{path}f\n");
                    c += &gs(1.0, &mut alphas);
                }
                if let Some((col, w, dash)) = stroke {
                    let (r, g, b) = rgb(*col);
                    c += &format!("{r:.3} {g:.3} {b:.3} RG {:.2} w {} 1 j 1 J\n{path}S\n", w * k, if *dash { "[3 2.2] 0 d" } else { "[] 0 d" });
                }
                c += "Q\n";
            }
            Op::Circle { cx, cy, r, fill, stroke } => {
                let (x, y) = p(*cx, *cy);
                let rr = r * k;
                let mut path = format!("{:.2} {:.2} m\n", x + rr, y);
                for s in circle_path(x, y, rr) {
                    path += &format!("{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c\n", s[0], s[1], s[2], s[3], s[4], s[5]);
                }
                path += "h\n";
                if let Some(col) = fill {
                    let (r, g, b) = rgb(*col);
                    c += &format!("{r:.3} {g:.3} {b:.3} rg\n{path}f\n");
                }
                if let Some((col, w)) = stroke {
                    let (r, g, b) = rgb(*col);
                    c += &format!("{r:.3} {g:.3} {b:.3} RG {:.2} w [] 0 d\n{path}S\n", w * k);
                }
            }
            Op::Text { x, y, text, size, anchor, bold, vertical, color } => {
                let rs = runs(text, *bold, false);
                let size = size * k;
                let wdt = width(&rs, size);
                let shift = match anchor {
                    Anchor::Start => 0.0,
                    Anchor::Middle => -wdt / 2.0,
                    Anchor::End => -wdt,
                };
                let (px, py) = p(*x, *y);
                let (r, g, b) = rgb(*color);
                c += &format!("BT {r:.3} {g:.3} {b:.3} rg\n");
                if *vertical {
                    c += &format!("0 1 -1 0 {px:.2} {:.2} Tm\n", py + shift);
                } else {
                    c += &format!("1 0 0 1 {:.2} {py:.2} Tm\n", px + shift);
                }
                for run in &rs {
                    let f = match run.font {
                        Font::Regular => "F1",
                        Font::Bold => "F2",
                        Font::Symbol => "F3",
                    };
                    c += &format!("/{f} {:.2} Tf {:.2} Ts {} Tj\n", size * run.scale, size * run.rise, ps_string(&run.bytes));
                }
                c += "ET\n";
            }
            Op::ClipStart { x, y, w, h } => {
                let (a, b) = p(*x, *y + *h);
                c += &format!("q {a:.2} {b:.2} {:.2} {:.2} re W n\n", w * k, h * k);
            }
            Op::ClipEnd => c += "Q\n",
        }
    }
    let gstates: String = alphas.iter().map(|q| format!("/GS{q} << /Type /ExtGState /ca {:.2} /CA {:.2} >> ", *q as f64 / 100.0, *q as f64 / 100.0)).collect();
    let objs: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {:.2} {:.2}] /Resources << /Font << /F1 5 0 R /F2 6 0 R /F3 7 0 R >> /ExtGState << {gstates}>> >> /Contents 4 0 R >>",
            d.w * k,
            ph
        ),
        format!("<< /Length {} >>\nstream\n{c}endstream", c.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>".into(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Symbol >>".into(),
    ];
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}

// ---- EPS -------------------------------------------------------------------------------------------

pub fn eps(d: &Doc) -> String {
    let k = 0.75;
    let ph = d.h * k;
    let p = |x: f64, y: f64| (x * k, ph - y * k);
    let mut s = format!(
        "%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 {} {}\n%%HiResBoundingBox: 0 0 {:.2} {:.2}\n%%Creator: ChemChron\n%%EndComments\n",
        (d.w * k).ceil(),
        ph.ceil(),
        d.w * k,
        ph
    );
    s += "/L1 { dup length dict begin {1 index /FID ne {def} {pop pop} ifelse} forall /Encoding ISOLatin1Encoding def currentdict end } def\n";
    s += "/Helvetica-L1 /Helvetica findfont L1 definefont pop\n/Helvetica-Bold-L1 /Helvetica-Bold findfont L1 definefont pop\n";
    s += &format!("1 1 1 setrgbcolor 0 0 {:.2} {:.2} rectfill\n", d.w * k, ph);
    for op in &d.ops {
        match op {
            Op::Path { pts, closed, fill, stroke } => {
                if pts.len() < 2 {
                    continue;
                }
                let mut path = String::from("newpath\n");
                for (i, (x, y)) in pts.iter().enumerate() {
                    let (a, b) = p(*x, *y);
                    path += &format!("{a:.2} {b:.2} {}\n", if i == 0 { "moveto" } else { "lineto" });
                }
                if *closed {
                    path += "closepath\n";
                }
                if let Some((col, a)) = fill {
                    let (r, g, b) = rgb(blend_white(*col, *a));
                    s += &format!("gsave {r:.3} {g:.3} {b:.3} setrgbcolor\n{path}fill grestore\n");
                }
                if let Some((col, w, dash)) = stroke {
                    let (r, g, b) = rgb(*col);
                    s += &format!("gsave {r:.3} {g:.3} {b:.3} setrgbcolor {:.2} setlinewidth 1 setlinejoin 1 setlinecap {} setdash\n{path}stroke grestore\n", w * k, if *dash { "[3 2.2] 0" } else { "[] 0" });
                }
            }
            Op::Circle { cx, cy, r, fill, stroke } => {
                let (x, y) = p(*cx, *cy);
                let path = format!("newpath {x:.2} {y:.2} {:.2} 0 360 arc closepath\n", r * k);
                if let Some(col) = fill {
                    let (r, g, b) = rgb(*col);
                    s += &format!("gsave {r:.3} {g:.3} {b:.3} setrgbcolor {path}fill grestore\n");
                }
                if let Some((col, w)) = stroke {
                    let (r, g, b) = rgb(*col);
                    s += &format!("gsave {r:.3} {g:.3} {b:.3} setrgbcolor {:.2} setlinewidth [] 0 setdash {path}stroke grestore\n", w * k);
                }
            }
            Op::Text { x, y, text, size, anchor, bold, vertical, color } => {
                let rs = runs(text, *bold, true);
                let size = size * k;
                let wdt = width(&rs, size);
                let shift = match anchor {
                    Anchor::Start => 0.0,
                    Anchor::Middle => -wdt / 2.0,
                    Anchor::End => -wdt,
                };
                let (px, py) = p(*x, *y);
                let (r, g, b) = rgb(*color);
                s += &format!("gsave {r:.3} {g:.3} {b:.3} setrgbcolor {px:.2} {py:.2} translate {} 0 0 moveto\n", if *vertical { "90 rotate" } else { "" });
                s += &format!("{shift:.2} 0 rmoveto\n");
                for run in &rs {
                    let f = match run.font {
                        Font::Regular => "/Helvetica-L1",
                        Font::Bold => "/Helvetica-Bold-L1",
                        Font::Symbol => "/Symbol",
                    };
                    let rise = size * run.rise;
                    s += &format!("{f} findfont {:.2} scalefont setfont 0 {rise:.2} rmoveto {} show 0 {:.2} rmoveto\n", size * run.scale, ps_string(&run.bytes), -rise);
                }
                s += "grestore\n";
            }
            Op::ClipStart { x, y, w, h } => {
                let (a, b) = p(*x, *y + *h);
                s += &format!("gsave newpath {a:.2} {b:.2} {:.2} {:.2} rectclip\n", w * k, h * k);
            }
            Op::ClipEnd => s += "grestore\n",
        }
    }
    s += "showpage\n%%EOF\n";
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fig() -> Figure {
        let mut f = Figure::new("²⁰⁶Pb/²³⁸U mean = 425.1 ± 2.3 Ma (2σ)", "Analysis", "Age (Ma)");
        f.items.push(Item::Band { y0: 420.0, y1: 430.0, fill: Color32::from_rgba_unmultiplied(170, 51, 51, 40) });
        f.items.push(Item::Poly { pts: vec![(1.0, 410.0), (1.6, 410.0), (1.6, 440.0), (1.0, 440.0)], fill: Color32::from_rgb(58, 95, 138), stroke: Color32::from_rgb(31, 58, 87) });
        f.marker(2.0, 425.0, Color32::RED, Some(1), vec![]);
        f.legend.push(("DEMO-A".into(), Color32::BLUE));
        f.x = (0.0, 3.0);
        f.y = (400.0, 450.0);
        f
    }

    #[test]
    fn superscripts_and_greek_become_runs() {
        let r = runs("²⁰⁶Pb ± 2σ", false, false);
        assert_eq!(r[0], Run { font: Font::Regular, scale: 0.68, rise: 0.36, bytes: b"206".to_vec() });
        assert_eq!(r[1].bytes, b"Pb \xb1 2".to_vec());
        assert_eq!(r[2].font, Font::Symbol);
        assert_eq!(r[2].bytes, b"s".to_vec());
    }

    #[test]
    fn every_format_is_written() {
        let f = fig();
        let pdf = write(&f, Format::Pdf, 85.0, 70.0, 7.0, true);
        let s = String::from_utf8_lossy(&pdf);
        assert!(s.starts_with("%PDF-1.4") && s.contains("/MediaBox [0 0 240.94 198.43]") && s.trim_end().ends_with("%%EOF"));
        assert!(s.contains("/GS16 gs"), "transparent band");
        // xref offsets point at the objects
        let xref_at: usize = s.rsplit("startxref\n").next().unwrap().lines().next().unwrap().parse().unwrap();
        assert!(pdf[xref_at..].starts_with(b"xref"));
        let eps = String::from_utf8(write(&f, Format::Eps, 85.0, 70.0, 7.0, true)).unwrap();
        assert!(eps.starts_with("%!PS-Adobe-3.0 EPSF-3.0") && eps.contains("%%BoundingBox: 0 0 241 199"));
        let svg = String::from_utf8(write(&f, Format::Svg, 85.0, 70.0, 7.0, false)).unwrap();
        assert!(svg.contains("width=\"85mm\"") && !svg.contains("mean = 425.1"));
    }
}
