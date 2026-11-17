// SPDX-License-Identifier: GPL-3.0-or-later
//! One description of a plot, drawn on screen (zoom, pan, hover, click) and
//! written as an SVG figure with the same content.

use egui::{Color32, Pos2, Rect, RichText, Shape, Stroke, Ui, Vec2};
use chemchron_laser::plot::{self, Frame2, View};
use chemchron_laser::theme::Theme;

#[derive(Clone, Debug)]
pub enum Item {
    Line { pts: Vec<(f64, f64)>, color: Color32, width: f32, dashed: bool },
    Poly { pts: Vec<(f64, f64)>, fill: Color32, stroke: Color32 },
    Marker { x: f64, y: f64, color: Color32, size: f32, hollow: bool, id: Option<usize>, hover: Vec<String> },
    /// Text at a data position; `align` places it left of, centred on or right of the point.
    Text { x: f64, y: f64, text: String, color: Color32, size: f32, bold: bool, align: egui::Align },
    /// A horizontal band across the plot (e.g. weighted mean ± 2σ).
    Band { y0: f64, y1: f64, fill: Color32 },
}

#[derive(Clone, Debug, Default)]
pub struct Figure {
    pub title: String,
    pub x_label: String,
    pub y_label: String,
    pub x: (f64, f64),
    pub y: (f64, f64),
    pub log_y: bool,
    /// No axes (ternary plots draw their own frame).
    pub bare: bool,
    pub items: Vec<Item>,
    pub legend: Vec<(String, Color32)>,
    /// Result lines shown under the title.
    pub notes: Vec<String>,
    /// Category labels on the x axis (REE diagram).
    pub x_categories: Vec<String>,
    /// How legend entries are drawn: markers, bars (columns) or lines with markers.
    pub legend_kind: LegendKind,
    /// Legend entries drawn as open symbols (e.g. laser / imported values).
    pub legend_hollow: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum LegendKind {
    #[default]
    Marker,
    Bar,
    Line,
}

/// Where the legend goes: the corner of the plot area (left, top, width,
/// height in page units) whose box covers the fewest data points; upper
/// left wins a tie, then upper right, lower left, lower right.
/// `to_page` maps data to page coordinates (y down).
pub fn legend_spot(fig: &Figure, area: (f64, f64, f64, f64), size: (f64, f64), pad: f64, to_page: &dyn Fn(f64, f64) -> (f64, f64)) -> (f64, f64) {
    let (l, t, w, h) = area;
    let (bw, bh) = size;
    let corners = [(l + pad, t + pad), (l + w - bw - pad, t + pad), (l + pad, t + h - bh - pad), (l + w - bw - pad, t + h - bh - pad)];
    let mut pts: Vec<(f64, f64)> = Vec::new();
    for it in &fig.items {
        match it {
            Item::Marker { x, y, size, .. } if *size > 0.05 => pts.push((*x, *y)),
            Item::Marker { x, y, .. } => pts.push((*x, *y)),
            Item::Line { pts: p, .. } | Item::Poly { pts: p, .. } => {
                // the points and the middle of each segment
                for w in p.windows(2) {
                    pts.push(w[0]);
                    pts.push(((w[0].0 + w[1].0) / 2.0, (w[0].1 + w[1].1) / 2.0));
                }
                if let Some(last) = p.last() {
                    pts.push(*last);
                }
            }
            _ => {}
        }
    }
    let page: Vec<(f64, f64)> = pts.iter().filter(|(x, y)| x.is_finite() && y.is_finite() && (!fig.log_y || *y > 0.0)).map(|(x, y)| to_page(*x, *y)).collect();
    let count = |c: &(f64, f64)| page.iter().filter(|(x, y)| *x >= c.0 - pad && *x <= c.0 + bw + pad && *y >= c.1 - pad && *y <= c.1 + bh + pad).count();
    let mut best = corners[0];
    let mut best_n = usize::MAX;
    for c in corners {
        let n = count(&c);
        if n < best_n {
            best = c;
            best_n = n;
        }
    }
    best
}

/// Darker edge of a marker.
pub fn edge_of(c: Color32) -> Color32 {
    let k = 0.45;
    Color32::from_rgb((c.r() as f32 * k) as u8, (c.g() as f32 * k) as u8, (c.b() as f32 * k) as u8)
}

impl Figure {
    pub fn new(title: &str, x_label: &str, y_label: &str) -> Figure {
        Figure { title: title.into(), x_label: x_label.into(), y_label: y_label.into(), x: (0.0, 1.0), y: (0.0, 1.0), ..Default::default() }
    }

    /// Ranges from the data with a margin.
    pub fn fit(&mut self, margin: f64) {
        let (mut x0, mut x1, mut y0, mut y1) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
        let mut add = |x: f64, y: f64| {
            if x.is_finite() && y.is_finite() {
                x0 = x0.min(x);
                x1 = x1.max(x);
                y0 = y0.min(y);
                y1 = y1.max(y);
            }
        };
        for it in &self.items {
            match it {
                Item::Line { pts, .. } | Item::Poly { pts, .. } => {
                    for (x, y) in pts {
                        add(*x, *y)
                    }
                }
                Item::Marker { x, y, .. } => add(*x, *y),
                _ => {}
            }
        }
        if !x0.is_finite() {
            return;
        }
        self.x = plot::pad(x0, x1, margin);
        if self.log_y {
            self.y = (y0.max(1e-30) / 2.0, y1 * 2.0);
        } else {
            self.y = plot::pad(y0, y1, margin);
        }
    }

    pub fn line(&mut self, pts: Vec<(f64, f64)>, color: Color32, width: f32) {
        self.items.push(Item::Line { pts, color, width, dashed: false });
    }
    pub fn dashed(&mut self, pts: Vec<(f64, f64)>, color: Color32, width: f32) {
        self.items.push(Item::Line { pts, color, width, dashed: true });
    }
    pub fn marker(&mut self, x: f64, y: f64, color: Color32, id: Option<usize>, hover: Vec<String>) {
        self.items.push(Item::Marker { x, y, color, size: 4.0, hollow: false, id, hover });
    }
}

/// What happened on screen this frame.
#[derive(Default)]
pub struct Interaction {
    pub clicked: Option<usize>,
}

/// Clips a polyline to the visible window (drops far-off points so huge values don't break drawing).
fn screen_pts(f: &Frame2, pts: &[(f64, f64)]) -> Vec<Pos2> {
    pts.iter().filter(|(x, y)| x.is_finite() && y.is_finite() && (!f.log_y || *y > 0.0)).map(|(x, y)| f.pos(*x, *y)).collect()
}

fn dashed_line(painter: &egui::Painter, pts: &[Pos2], stroke: Stroke) {
    for w in pts.windows(2) {
        painter.extend(Shape::dashed_line(&[w[0], w[1]], stroke, 6.0, 4.0));
    }
}

pub fn show(ui: &mut Ui, t: &Theme, fig: &Figure, view: &mut View, height: f32) -> Interaction {
    let mut out = Interaction::default();
    // notes and title lines come out of the height asked for (so the axes stay on the page)
    let top = ui.cursor().top();
    for n in &fig.notes {
        ui.label(egui::RichText::new(n).color(t.dim).size(12.0));
    }
    if !fig.title.is_empty() {
        ui.vertical_centered(|ui| {
            for line in fig.title.lines() {
                ui.label(egui::RichText::new(line).size(14.5).color(t.text));
            }
        });
    }
    let used = ui.cursor().top() - top;
    let size = Vec2::new(ui.available_width(), (height - used).min(ui.available_height() - 2.0).max(160.0));
    let (response, painter, rect) = plot::allocate(ui, size);
    let (x, y) = view.resolve((fig.x, fig.y));
    let f = Frame2 { rect, x, y, log_y: fig.log_y };
    if !fig.bare {
        if fig.x_categories.is_empty() {
            plot::axes(&painter, t, &f, &fig.x_label, &fig.y_label);
        } else {
            plot::axes_with(&painter, t, &f, "", &fig.y_label, false);
            for (i, c) in fig.x_categories.iter().enumerate() {
                let px = f.px(i as f64);
                if px >= rect.left() - 1.0 && px <= rect.right() + 1.0 {
                    painter.text(Pos2::new(px, rect.bottom() + 7.0), egui::Align2::CENTER_TOP, c, egui::FontId::proportional(12.5), t.text);
                }
            }
        }
    }
    let clip = painter.with_clip_rect(rect.expand(1.0));
    let mut best: Option<(f32, &Vec<String>, Option<usize>, Pos2)> = None;
    let hover = response.hover_pos();
    for it in &fig.items {
        match it {
            Item::Band { y0, y1, fill } => {
                let r = Rect::from_x_y_ranges(rect.x_range(), f.py(*y1).min(f.py(*y0))..=f.py(*y1).max(f.py(*y0)));
                clip.rect_filled(r.intersect(rect), 0, *fill);
            }
            Item::Poly { pts, fill, stroke } => {
                let p = screen_pts(&f, pts);
                if p.len() >= 3 {
                    clip.add(Shape::convex_polygon(p.clone(), *fill, Stroke::NONE));
                    let mut closed = p;
                    closed.push(closed[0]);
                    clip.add(Shape::line(closed, Stroke::new(1.0, *stroke)));
                }
            }
            Item::Line { pts, color, width, dashed } => {
                let p = screen_pts(&f, pts);
                if *dashed {
                    dashed_line(&clip, &p, Stroke::new(*width, *color));
                } else if p.len() >= 2 {
                    clip.add(Shape::line(p, Stroke::new(*width, *color)));
                }
            }
            Item::Marker { x, y, color, size, hollow, id, hover: lines } => {
                if !x.is_finite() || !y.is_finite() {
                    continue;
                }
                let p = f.pos(*x, *y);
                if *size > 0.05 {
                    if *hollow {
                        clip.circle_filled(p, *size, if t.dark { Color32::from_rgb(15, 17, 21) } else { Color32::WHITE });
                        clip.circle_stroke(p, *size, Stroke::new(1.4, *color));
                    } else {
                        clip.circle_filled(p, *size, *color);
                        clip.circle_stroke(p, *size, Stroke::new(0.9, edge_of(*color)));
                    }
                }
                if let Some(h) = hover {
                    let d = h.distance(p);
                    if d < 9.0 && best.as_ref().is_none_or(|b| d < b.0) {
                        best = Some((d, lines, *id, p));
                    }
                }
            }
            Item::Text { x, y, text, color, size, bold, align } => {
                let a = match align {
                    egui::Align::Min => egui::Align2::LEFT_CENTER,
                    egui::Align::Center => egui::Align2::CENTER_CENTER,
                    egui::Align::Max => egui::Align2::RIGHT_CENTER,
                };
                let c = if *bold { t.text } else { *color };
                clip.text(f.pos(*x, *y), a, text, egui::FontId::proportional(if *bold { *size + 1.0 } else { *size }), c);
            }
        }
    }
    // legend in a box, in the corner with the fewest points
    if !fig.legend.is_empty() {
        let font = egui::FontId::proportional(12.0);
        let wmax = fig.legend.iter().map(|(n, _)| painter.layout_no_wrap(n.clone(), font.clone(), t.text).size().x).fold(0.0, f32::max);
        let (row_h, sym_w) = (17.0, 22.0);
        let size = Vec2::new(wmax + sym_w + 18.0, fig.legend.len() as f32 * row_h + 10.0);
        let to_page = |x: f64, y: f64| {
            let p = f.pos(x, y);
            (p.x as f64, p.y as f64)
        };
        // the y-axis title sits inside the top-left corner: the legend stays below it
        let skip = if fig.y_label.is_empty() || fig.bare { 0.0 } else { 16.0 };
        let (lx, ly) = legend_spot(fig, (rect.left() as f64, (rect.top() + skip) as f64, rect.width() as f64, (rect.height() - skip) as f64), (size.x as f64, size.y as f64), 8.0, &to_page);
        let r = Rect::from_min_size(Pos2::new(lx as f32, ly as f32), size);
        painter.rect_filled(r, 3, if t.dark { Color32::from_rgba_unmultiplied(28, 31, 38, 235) } else { Color32::from_rgba_unmultiplied(255, 255, 255, 235) });
        painter.rect_stroke(r, 3, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
        for (k, (name, c)) in fig.legend.iter().enumerate() {
            let cy = r.top() + 5.0 + row_h * (k as f32 + 0.5);
            let sx = r.left() + 6.0 + sym_w / 2.0;
            let hollow = fig.legend_hollow.contains(name);
            match fig.legend_kind {
                LegendKind::Bar => {
                    let b = Rect::from_center_size(Pos2::new(sx, cy), Vec2::new(16.0, 9.0));
                    painter.rect_filled(b, 0, *c);
                    painter.rect_stroke(b, 0, Stroke::new(0.8, edge_of(*c)), egui::StrokeKind::Inside);
                }
                LegendKind::Line => {
                    painter.line_segment([Pos2::new(sx - 9.0, cy), Pos2::new(sx + 9.0, cy)], Stroke::new(2.0, *c));
                    painter.circle_filled(Pos2::new(sx, cy), 3.5, if hollow { if t.dark { Color32::from_rgb(28, 31, 38) } else { Color32::WHITE } } else { *c });
                    painter.circle_stroke(Pos2::new(sx, cy), 3.5, Stroke::new(1.0, *c));
                }
                LegendKind::Marker => {
                    if hollow {
                        painter.circle_stroke(Pos2::new(sx, cy), 4.5, Stroke::new(1.4, *c));
                    } else {
                        painter.circle_filled(Pos2::new(sx, cy), 4.5, *c);
                        painter.circle_stroke(Pos2::new(sx, cy), 4.5, Stroke::new(0.9, edge_of(*c)));
                    }
                }
            }
            painter.text(Pos2::new(r.left() + 10.0 + sym_w, cy), egui::Align2::LEFT_CENTER, name, font.clone(), t.text);
        }
    }
    if let Some((_, lines, id, p)) = best {
        let mut l = lines.clone();
        if id.is_some() {
            l.push("Click to include or leave out".into());
        }
        plot::hover_box(&painter, t, rect, p, &l);
        if response.clicked() {
            out.clicked = id;
        }
    } else if !fig.bare {
        view.interact(ui, &response, &f);
    }
    if response.double_clicked() {
        view.window = None;
    }
    out
}

/// The figure as an SVG document, `w_mm` × `h_mm`, light background.
pub fn to_svg(fig: &Figure, w_mm: f64, h_mm: f64, font_pt: f64, with_title: bool) -> String {
    crate::vector::svg(&crate::vector::layout(fig, w_mm, h_mm, font_pt, with_title))
}

/// Fill colour with transparency.
pub fn alpha(c: Color32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a * 255.0) as u8)
}

/// Asks for a file and writes the figure in the chosen format.
pub fn save_dialog(fig: &Figure, name: &str, pe: &chemchron_core::settings::PlotExport) -> Option<Result<String, String>> {
    let format = crate::vector::Format::from_ext(&pe.format);
    let path = rfd::FileDialog::new().set_file_name(format!("{name}.{}", format.ext())).add_filter(format.label(), &[format.ext()]).save_file()?;
    let (w, h) = export_size(pe);
    let bytes = crate::vector::write_dpi(fig, format, w, h, pe.font_pt, pe.include_title, pe.dpi);
    Some(std::fs::write(&path, bytes).map(|_| format!("Saved {} ({w:.0} × {h:.0} mm)", path.display())).map_err(|e| e.to_string()))
}

/// Aspect choices of the export window: (label, height / width; None = custom height).
pub const ASPECTS: [(&str, Option<f64>); 4] = [("4 : 3", Some(0.75)), ("3 : 2", Some(2.0 / 3.0)), ("1 : 1", Some(1.0)), ("Custom height", None)];
/// Typical journal figure widths (presets only; any width can be typed).
pub const WIDTHS: [(&str, f64); 3] = [("Single column", 85.0), ("1.5 column", 120.0), ("Double column", 175.0)];

/// Width and height in mm from the export settings.
pub fn export_size(pe: &chemchron_core::settings::PlotExport) -> (f64, f64) {
    let w = pe.width_mm.clamp(20.0, 400.0);
    match ASPECTS.iter().find(|(l, _)| *l == pe.aspect).and_then(|(_, a)| *a) {
        Some(a) => (w, w * a),
        None => (w, pe.height_mm.clamp(20.0, 400.0)),
    }
}

/// The Save figure window: width (journal presets or typed), aspect ratio or
/// height, font size, title, and format (PDF, SVG, EPS), with a preview.
/// The choices are remembered in the lab settings.
pub fn export_window(app: &mut crate::app::ChemApp, ctx: &egui::Context) {
    let Some((fig, name)) = app.fig_export.clone() else { return };
    let t = app.theme();
    let mut pe = app.settings.plot_export.clone();
    let before = pe.clone();
    let mut open = true;
    let mut save = false;
    let mut cancel = false;
    egui::Window::new("Save figure").collapsible(false).resizable(false).open(&mut open).default_width(520.0).show(ctx, |ui| {
        egui::Grid::new("fig_export_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("Width");
            ui.horizontal(|ui| {
                for (l, w) in WIDTHS {
                    if ui.selectable_label((pe.width_mm - w).abs() < 0.01, format!("{l} ({w:.0} mm)")).clicked() {
                        pe.width_mm = w;
                    }
                }
                ui.add(egui::DragValue::new(&mut pe.width_mm).range(20.0..=400.0).suffix(" mm"));
            });
            ui.end_row();
            ui.label("Aspect ratio");
            ui.horizontal(|ui| {
                if !ASPECTS.iter().any(|(l, _)| *l == pe.aspect) {
                    pe.aspect = "Custom height".into();
                }
                for (l, _) in ASPECTS {
                    ui.selectable_value(&mut pe.aspect, l.to_string(), l);
                }
                ui.add_enabled(pe.aspect == "Custom height", egui::DragValue::new(&mut pe.height_mm).range(20.0..=400.0).suffix(" mm"));
            });
            ui.end_row();
            ui.label("Font size");
            ui.add(egui::DragValue::new(&mut pe.font_pt).range(4.0..=24.0).speed(0.1).suffix(" pt"));
            ui.end_row();
            ui.label("Title");
            ui.checkbox(&mut pe.include_title, "Include the title and result lines");
            ui.end_row();
            ui.label("Format");
            ui.horizontal(|ui| {
                for f in crate::vector::Format::ALL {
                    ui.selectable_value(&mut pe.format, f.ext().to_string(), f.label());
                }
            });
            if crate::vector::Format::from_ext(&pe.format).is_raster() {
                    ui.end_row();
                    ui.label("Resolution");
                    ui.horizontal(|ui| {
                        for d in [300u32, 600] {
                            ui.selectable_value(&mut pe.dpi, d, format!("{d} dpi"));
                        }
                    });
                }
            ui.end_row();
        });
        let (w, h) = export_size(&pe);
        ui.label(RichText::new(format!("{w:.0} × {h:.0} mm, {} pt text. PDF and EPS use Helvetica; vector files scale without loss.", pe.font_pt)).color(t.dim).size(12.0));
        ui.add_space(4.0);
        // preview at the page's proportions
        let pw = ui.available_width().min(480.0);
        let ph = (pw as f64 * h / w) as f32;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(pw, ph), egui::Sense::hover());
        preview(ui.painter(), rect, &crate::vector::layout(&fig, w, h, pe.font_pt, pe.include_title));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            save = ui.button(RichText::new("Save…").strong()).clicked();
            cancel = ui.button("Cancel").clicked();
        });
    });
    if pe != before {
        app.settings.plot_export = pe.clone();
        app.save_settings();
    }
    if save {
        if let Some(r) = save_dialog(&fig, &name, &pe) {
            match r {
                Ok(m) => app.say(m, false),
                Err(e) => app.say(e, true),
            }
            app.fig_export = None;
        }
    }
    if cancel || !open {
        app.fig_export = None;
    }
}

/// Draws a laid-out page into `rect` (what the file will look like).
fn preview(p: &egui::Painter, rect: Rect, d: &crate::vector::Doc) {
    use crate::vector::{Anchor, Op};
    let k = rect.width() as f64 / d.w;
    let at = |x: f64, y: f64| Pos2::new(rect.left() + (x * k) as f32, rect.top() + (y * k) as f32);
    p.rect_filled(rect, 2, Color32::WHITE);
    let mut clip: Option<Rect> = None;
    for op in &d.ops {
        let painter = match clip {
            Some(c) => p.with_clip_rect(c),
            None => p.clone(),
        };
        match op {
            Op::Path { pts, closed, fill, stroke } => {
                let v: Vec<Pos2> = pts.iter().map(|(x, y)| at(*x, *y)).collect();
                if v.len() < 2 {
                    continue;
                }
                if let (Some((c, a)), true) = (fill, *closed) {
                    painter.add(Shape::convex_polygon(v.clone(), alpha(*c, *a as f32), Stroke::NONE));
                }
                if let Some((c, w, dashed)) = stroke {
                    let s = Stroke::new((*w * k) as f32, *c);
                    let mut v = v.clone();
                    if *closed {
                        v.push(v[0]);
                    }
                    if *dashed {
                        dashed_line(&painter, &v, s);
                    } else {
                        painter.add(Shape::line(v, s));
                    }
                }
            }
            Op::Circle { cx, cy, r, fill, stroke } => {
                let c = at(*cx, *cy);
                let r = (*r * k) as f32;
                if let Some(f) = fill {
                    painter.circle_filled(c, r, *f);
                }
                if let Some((s, w)) = stroke {
                    painter.circle_stroke(c, r, Stroke::new((*w * k) as f32, *s));
                }
            }
            Op::Text { x, y, text, size, anchor, bold: _, vertical, color } => {
                let font = egui::FontId::proportional(((*size * k) as f32).max(4.0));
                let align = match anchor {
                    Anchor::Start => egui::Align2::LEFT_BOTTOM,
                    Anchor::Middle => egui::Align2::CENTER_BOTTOM,
                    Anchor::End => egui::Align2::RIGHT_BOTTOM,
                };
                if *vertical {
                    let galley = painter.layout_no_wrap(text.clone(), font, *color);
                    let pos = at(*x, *y) + egui::vec2(-galley.size().y * 0.7, galley.size().x / 2.0);
                    painter.add(egui::epaint::TextShape::new(pos, galley, *color).with_angle(-std::f32::consts::FRAC_PI_2));
                } else {
                    painter.text(at(*x, *y) + egui::vec2(0.0, (*size * k * 0.25) as f32), align, text, font, *color);
                }
            }
            Op::ClipStart { x, y, w, h } => clip = Some(Rect::from_min_max(at(*x, *y), at(*x + *w, *y + *h))),
            Op::ClipEnd => clip = None,
        }
    }
}
