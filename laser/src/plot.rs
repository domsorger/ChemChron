// SPDX-License-Identifier: GPL-3.0-or-later
//! A small plotting frame: data-to-screen mapping, axes with round ticks,
//! zoom and pan. Plots paint their own data inside it.

use crate::theme::Theme;
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};
use chemchron_laser_core::svg::nice_ticks;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame2 {
    pub rect: Rect,
    pub x: (f64, f64),
    pub y: (f64, f64),
    pub log_y: bool,
}

impl Frame2 {
    pub fn px(&self, x: f64) -> f32 {
        self.rect.left() + ((x - self.x.0) / (self.x.1 - self.x.0)) as f32 * self.rect.width()
    }
    pub fn py(&self, y: f64) -> f32 {
        let v = if self.log_y { y.max(1e-30).log10() } else { y };
        let (a, b) = if self.log_y {
            (self.y.0.max(1e-30).log10(), self.y.1.max(1e-30).log10())
        } else {
            self.y
        };
        self.rect.bottom() - ((v - a) / (b - a)) as f32 * self.rect.height()
    }
    pub fn pos(&self, x: f64, y: f64) -> Pos2 {
        Pos2::new(self.px(x), self.py(y))
    }
    pub fn x_at(&self, px: f32) -> f64 {
        self.x.0 + ((px - self.rect.left()) / self.rect.width()) as f64 * (self.x.1 - self.x.0)
    }
    pub fn y_at(&self, py: f32) -> f64 {
        let f = ((self.rect.bottom() - py) / self.rect.height()) as f64;
        if self.log_y {
            let (a, b) = (self.y.0.max(1e-30).log10(), self.y.1.max(1e-30).log10());
            10f64.powf(a + f * (b - a))
        } else {
            self.y.0 + f * (self.y.1 - self.y.0)
        }
    }
}

pub const MARGIN_LEFT: f32 = 66.0;
pub const MARGIN_BOTTOM: f32 = 44.0;

/// Reserve room for a plot with axis margins; returns the response over the data area.
pub fn allocate(ui: &mut Ui, size: Vec2) -> (Response, Painter, Rect) {
    allocate_with(ui, size, 8.0, MARGIN_BOTTOM)
}

/// The same with chosen margins above and below, for plots stacked on one time axis.
pub fn allocate_with(ui: &mut Ui, size: Vec2, top: f32, bottom: f32) -> (Response, Painter, Rect) {
    let (response, painter) = ui.allocate_painter(size, Sense::click_and_drag());
    let outer = response.rect;
    let rect = Rect::from_min_max(
        Pos2::new(outer.left() + MARGIN_LEFT, outer.top() + top),
        Pos2::new(outer.right() - 12.0, outer.bottom() - bottom),
    );
    (response, painter, rect)
}

pub fn fmt_tick(v: f64, span: f64) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    let a = v.abs();
    if a >= 1e5 || a < 1e-4 {
        return format!("{v:.1e}");
    }
    let decimals = (-(span / 8.0).log10().floor()).clamp(0.0, 6.0) as usize;
    format!("{v:.decimals$}")
}

/// "10ⁿ" for a decade on a log axis (as matplotlib); small ones as decimals.
pub fn decade_label(v: f64) -> String {
    let e = v.log10().round() as i32;
    if e < 0 {
        return format!("{v}");
    }
    const SUP: [char; 10] = ['\u{2070}', '\u{b9}', '\u{b2}', '\u{b3}', '\u{2074}', '\u{2075}', '\u{2076}', '\u{2077}', '\u{2078}', '\u{2079}'];
    let digits: String = e.to_string().chars().map(|c| SUP[c.to_digit(10).unwrap_or(0) as usize]).collect();
    format!("10{digits}")
}

/// Paint background, grid, ticks and axis labels.
pub fn axes(painter: &Painter, t: &Theme, f: &Frame2, x_label: &str, y_label: &str) {
    axes_with(painter, t, f, x_label, y_label, true);
}

/// The same, optionally without the time labels when another plot below shows them.
pub fn axes_with(painter: &Painter, t: &Theme, f: &Frame2, x_label: &str, y_label: &str, show_x: bool) {
    painter.rect_filled(f.rect, 2, if t.dark { Color32::from_rgb(15, 17, 21) } else { Color32::WHITE });
    let font = FontId::proportional(11.5);
    let text = t.dim;
    for v in nice_ticks(f.x.0, f.x.1, 8) {
        let x = f.px(v);
        painter.line_segment(
            [Pos2::new(x, f.rect.top()), Pos2::new(x, f.rect.bottom())],
            Stroke::new(1.0, t.grid),
        );
        if show_x {
            painter.text(
                Pos2::new(x, f.rect.bottom() + 7.0),
                Align2::CENTER_TOP,
                fmt_tick(v, f.x.1 - f.x.0),
                font.clone(),
                text,
            );
        }
    }
    if f.log_y {
        // a range shorter than about two decades also gets 2 and 5 lines (labelled when short)
        let (l0, l1) = (f.y.0.max(1e-30).log10(), f.y.1.max(1e-30).log10());
        if l1 - l0 < 2.0 {
            for e in (l0.floor() as i32)..=(l1.ceil() as i32) {
                for m in [2.0, 5.0] {
                    let v = m * 10f64.powi(e);
                    if v < f.y.0 || v > f.y.1 {
                        continue;
                    }
                    let y = f.py(v);
                    painter.line_segment([Pos2::new(f.rect.left(), y), Pos2::new(f.rect.right(), y)], Stroke::new(1.0, t.grid));
                    {
                        painter.text(Pos2::new(f.rect.left() - 6.0, y), Align2::RIGHT_CENTER, crate::theme::fmt_sig(v, 2), font.clone(), text);
                    }
                }
            }
        }
        let mut e = f.y.0.max(1e-30).log10().ceil() as i32;
        let top = f.y.1.max(1e-30).log10().floor() as i32;
        while e <= top {
            let v = 10f64.powi(e);
            let y = f.py(v);
            painter.line_segment(
                [Pos2::new(f.rect.left(), y), Pos2::new(f.rect.right(), y)],
                Stroke::new(1.0, t.grid),
            );
            painter.text(Pos2::new(f.rect.left() - 6.0, y), Align2::RIGHT_CENTER, decade_label(v), font.clone(), text);
            e += 1;
        }
    } else {
        // Fewer labels on a short strip, so they don't run together.
        let count = ((f.rect.height() / 26.0) as usize).clamp(2, 6);
        for v in nice_ticks(f.y.0, f.y.1, count) {
            let y = f.py(v);
            painter.line_segment(
                [Pos2::new(f.rect.left(), y), Pos2::new(f.rect.right(), y)],
                Stroke::new(1.0, t.grid),
            );
            painter.text(
                Pos2::new(f.rect.left() - 6.0, y),
                Align2::RIGHT_CENTER,
                fmt_tick(v, f.y.1 - f.y.0),
                font.clone(),
                text,
            );
        }
    }
    if show_x {
        painter.text(
            Pos2::new(f.rect.center().x, f.rect.bottom() + 26.0),
            Align2::CENTER_TOP,
            x_label,
            FontId::proportional(12.0),
            text,
        );
    }
    if !y_label.is_empty() {
        painter.text(
            Pos2::new(f.rect.left() + 6.0, f.rect.top() + 4.0),
            Align2::LEFT_TOP,
            y_label,
            FontId::proportional(12.0),
            text,
        );
    }
    painter.rect_stroke(f.rect, 2, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
}

/// View window of a pan-and-zoom plot. `None` means "fit to data".
#[derive(Clone, Copy, Debug, Default)]
pub struct View {
    pub window: Option<((f64, f64), (f64, f64))>,
}

impl View {
    pub fn resolve(&self, fit: ((f64, f64), (f64, f64))) -> ((f64, f64), (f64, f64)) {
        self.window.unwrap_or(fit)
    }

    /// Mouse wheel zooms around the pointer, dragging pans. Linear axes only.
    pub fn interact(&mut self, ui: &Ui, response: &Response, f: &Frame2) -> bool {
        let mut changed = false;
        let (mut x, mut y) = (f.x, f.y);
        if response.dragged_by(egui::PointerButton::Primary) {
            let d = response.drag_delta();
            if d != Vec2::ZERO {
                let dx = d.x as f64 / f.rect.width() as f64 * (x.1 - x.0);
                let dy = d.y as f64 / f.rect.height() as f64 * (y.1 - y.0);
                x = (x.0 - dx, x.1 - dx);
                y = (y.0 + dy, y.1 + dy);
                changed = true;
            }
        }
        if response.hovered() {
            let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = (zoom as f64) * (scroll as f64 / 300.0).exp();
            if (factor - 1.0).abs() > 1e-4 {
                if let Some(p) = response.hover_pos() {
                    let (cx, cy) = (f.x_at(p.x), f.y_at(p.y));
                    x = (cx + (x.0 - cx) / factor, cx + (x.1 - cx) / factor);
                    y = (cy + (y.0 - cy) / factor, cy + (y.1 - cy) / factor);
                    changed = true;
                }
            }
        }
        if changed {
            self.window = Some((x, y));
        }
        changed
    }
}

/// Add a margin around a range; a flat range gets a small width.
pub fn pad(lo: f64, hi: f64, fraction: f64) -> (f64, f64) {
    if !lo.is_finite() || !hi.is_finite() {
        return (0.0, 1.0);
    }
    let span = hi - lo;
    if span <= 0.0 {
        let w = if lo != 0.0 { lo.abs() * 0.05 } else { 1.0 };
        return (lo - w, hi + w);
    }
    (lo - span * fraction, hi + span * fraction)
}

/// Draw a text box next to the pointer inside a plot.
pub fn hover_box(painter: &Painter, t: &Theme, rect: Rect, at: Pos2, lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    let font = FontId::proportional(12.0);
    let galleys: Vec<_> = lines
        .iter()
        .map(|l| painter.layout_no_wrap(l.clone(), font.clone(), t.text))
        .collect();
    let width = galleys.iter().map(|g| g.size().x).fold(0.0, f32::max) + 16.0;
    let height: f32 = galleys.iter().map(|g| g.size().y).sum::<f32>() + 12.0;
    let mut x = at.x + 14.0;
    if x + width > rect.right() {
        x = at.x - width - 10.0;
    }
    let y = (at.y - height / 2.0).clamp(rect.top() + 2.0, (rect.bottom() - height - 2.0).max(rect.top() + 2.0));
    let bg = Rect::from_min_size(Pos2::new(x, y), Vec2::new(width, height));
    painter.rect_filled(bg, 4, t.panel);
    painter.rect_stroke(bg, 4, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    let mut cy = y + 6.0;
    for g in galleys {
        let h = g.size().y;
        painter.galley(Pos2::new(x + 8.0, cy), g, t.text);
        cy += h;
    }
}
