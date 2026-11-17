// SPDX-License-Identifier: GPL-3.0-or-later
//! Elements: chondrite-normalised REE patterns, and any quantity against any
//! other from the same spot, such as Y content against age.

use crate::app::TraceApp;
use crate::plot::{self, Frame2};
use crate::theme::{self, fmt_sig, group_color};
use egui::{FontId, Pos2, RichText, Shape, Stroke, Ui};
use chemchron_laser_core::model::*;

/// CI chondrite in ppm, McDonough & Sun (1995).
const CHONDRITE: [(&str, f64); 14] = [
    ("La", 0.237), ("Ce", 0.613), ("Pr", 0.0928), ("Nd", 0.457), ("Sm", 0.148), ("Eu", 0.0563),
    ("Gd", 0.199), ("Tb", 0.0361), ("Dy", 0.246), ("Ho", 0.0546), ("Er", 0.160), ("Tm", 0.0247),
    ("Yb", 0.161), ("Lu", 0.0246),
];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Quantity {
    Ppm(usize),
    Age68,
    Age76,
    Age82,
    ThU,
    Discordance,
}

impl Quantity {
    fn value(&self, r: &chemchron_laser_core::drs::SelResult) -> Option<f64> {
        let v = match self {
            Quantity::Ppm(c) => r.te.get(*c)?.as_ref()?.ppm,
            Quantity::Age68 => r.upb.as_ref()?.a68.age,
            Quantity::Age76 => r.upb.as_ref()?.a76.age,
            Quantity::Age82 => r.upb.as_ref()?.a82?.age,
            Quantity::ThU => r.upb.as_ref()?.th_u,
            Quantity::Discordance => r.upb.as_ref()?.discordance_pct,
        };
        v.is_finite().then_some(v)
    }
}

pub struct ElementsState {
    pub x: Quantity,
    pub y: Option<Quantity>,
    pub log_x: bool,
    pub log_y: bool,
    pub hide_flagged: bool,
}

impl Default for ElementsState {
    fn default() -> Self {
        ElementsState { x: Quantity::Age68, y: None, log_x: false, log_y: true, hide_flagged: true }
    }
}

impl TraceApp {
    fn quantity_name(&self, q: Quantity) -> String {
        match q {
            Quantity::Ppm(c) => format!("{} (ppm)", self.session.inputs.channels[c].name),
            Quantity::Age68 => "206/238 age (Ma)".to_string(),
            Quantity::Age76 => "207/206 age (Ma)".to_string(),
            Quantity::Age82 => "208/232 age (Ma)".to_string(),
            Quantity::ThU => "Th/U".to_string(),
            Quantity::Discordance => "Discordance (%)".to_string(),
        }
    }

    fn quantities(&self) -> Vec<Quantity> {
        let mut out = Vec::new();
        if self.red.upb.is_some() {
            out.extend([Quantity::Age68, Quantity::Age76, Quantity::Age82, Quantity::ThU, Quantity::Discordance]);
        }
        if let Some(te) = &self.red.te {
            for (i, c) in te.calibrations.iter().enumerate() {
                if c.is_some() {
                    out.push(Quantity::Ppm(i));
                }
            }
        }
        out
    }

    pub fn page_elements(&mut self, ui: &mut Ui) {
        let t = self.theme();
        if self.red.te.is_none() {
            ui.label(RichText::new("No concentrations yet. Set a calibration reference on the Corrections page.").color(t.dim));
            return;
        }
        ui.horizontal(|ui| {
            let groups: Vec<(u64, String, [u8; 3])> = self
                .session
                .groups
                .iter()
                .filter(|g| g.kind != GroupKind::Baseline)
                .map(|g| (g.id, g.name.clone(), g.color))
                .collect();
            for (id, name, color) in groups {
                let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().rect_filled(r, 2, group_color(color));
                let mut shown = !self.hidden_groups.contains(&id);
                if ui.checkbox(&mut shown, name).changed() {
                    if shown {
                        self.hidden_groups.remove(&id);
                    } else {
                        self.hidden_groups.insert(id);
                    }
                }
            }
            ui.separator();
            ui.checkbox(&mut self.elements.hide_flagged, "Leave flagged out");
        });
        ui.add_space(4.0);
        let height = ui.available_height();
        ui.columns(2, |cols| {
            self.ree_plot(&mut cols[0], height);
            self.cross_plot(&mut cols[1], height);
        });
    }

    fn ree_plot(&mut self, ui: &mut Ui, height: f32) {
        let t = self.theme();
        let inputs = self.session.inputs.clone();
        let channels: Vec<(usize, f64)> = CHONDRITE
            .iter()
            .filter_map(|(e, c)| {
                let i = inputs.channels.iter().position(|x| x.isotope.as_ref().is_some_and(|y| y.element == *e))?;
                self.red.te.as_ref()?.calibrations[i].as_ref()?;
                Some((i, *c))
            })
            .collect();
        ui.label(RichText::new("REE pattern, relative to CI chondrite").strong());
        let (response, painter, rect) = plot::allocate(ui, egui::vec2(ui.available_width(), height - 28.0));
        if channels.len() < 2 {
            plot::axes(&painter, &t, &Frame2 { rect, x: (0.0, 1.0), y: (1.0, 10.0), log_y: true }, "", "");
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, "This session has too few rare-earth channels", FontId::proportional(13.0), t.dim);
            return;
        }
        let mut lines: Vec<(u64, egui::Color32, Vec<(f64, f64)>, Vec<bool>)> = Vec::new();
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for (s, r) in self.session.selections.iter().zip(&self.red.selections) {
            if s.excluded || (self.elements.hide_flagged && !r.flags.is_empty()) {
                continue;
            }
            let Some(g) = self.session.group_of(s) else { continue };
            if g.kind == GroupKind::Baseline || self.hidden_groups.contains(&g.id) {
                continue;
            }
            let mut pts = Vec::new();
            let mut below = Vec::new();
            for (k, (c, ch)) in channels.iter().enumerate() {
                if let Some(v) = r.te.get(*c).and_then(|v| v.as_ref()) {
                    let n = v.ppm / ch;
                    if n > 0.0 {
                        pts.push((k as f64, n));
                        below.push(v.below_lod);
                        lo = lo.min(n);
                        hi = hi.max(n);
                    }
                }
            }
            if pts.len() >= 2 {
                lines.push((s.id, group_color(g.color), pts, below));
            }
        }
        if !lo.is_finite() {
            (lo, hi) = (1.0, 10.0);
        }
        let f = Frame2 {
            rect,
            x: (-0.5, channels.len() as f64 - 0.5),
            y: (10f64.powf((lo.log10() - 0.03).floor()), 10f64.powf((hi.log10() + 0.03).ceil())),
            log_y: true,
        };
        plot::axes(&painter, &t, &f, "", "Sample / chondrite");
        let clip = painter.with_clip_rect(rect);
        // Replace the numeric axis with element names.
        painter.rect_filled(
            egui::Rect::from_min_max(Pos2::new(rect.left() - 6.0, rect.bottom() + 1.0), Pos2::new(rect.right() + 6.0, rect.bottom() + 19.0)),
            0,
            t.bg,
        );
        for (k, (c, _)) in channels.iter().enumerate() {
            let name = inputs.channels[*c].isotope.as_ref().map_or("", |i| i.element.as_str());
            painter.text(Pos2::new(f.px(k as f64), rect.bottom() + 5.0), egui::Align2::CENTER_TOP, name, FontId::proportional(11.5), t.dim);
        }
        let mut nearest: Option<(u64, f32, f64, f64)> = None;
        for (id, color, pts, below) in &lines {
            let selected = self.selected == Some(*id);
            let screen: Vec<Pos2> = pts.iter().map(|(x, y)| f.pos(*x, *y)).collect();
            clip.add(Shape::line(screen.clone(), Stroke::new(if selected { 2.5 } else { 1.0 }, if selected { t.text } else { color.linear_multiply(0.75) })));
            for (i, p) in screen.iter().enumerate() {
                if below[i] {
                    clip.circle_stroke(*p, 3.0, Stroke::new(1.0, *color));
                } else {
                    clip.circle_filled(*p, if selected { 3.0 } else { 1.8 }, *color);
                }
                if let Some(h) = response.hover_pos() {
                    let d = p.distance(h);
                    if d < 8.0 && nearest.is_none_or(|n| d < n.1) {
                        nearest = Some((*id, d, pts[i].0, pts[i].1));
                    }
                }
            }
        }
        if let Some((id, _, x, y)) = nearest {
            let label = self.session.selections.iter().find(|s| s.id == id).map_or(String::new(), |s| s.label.clone());
            let name = channels.get(x as usize).and_then(|c| inputs.channels[c.0].isotope.as_ref()).map_or("", |i| i.element.as_str());
            plot::hover_box(&painter, &t, rect, f.pos(x, y), &[label, format!("{name}: {} × chondrite", fmt_sig(y, 3))]);
            if response.clicked() {
                self.selected = Some(id);
            }
        }
    }

    fn cross_plot(&mut self, ui: &mut Ui, height: f32) {
        let t = self.theme();
        let list = self.quantities();
        if self.elements.y.is_none() {
            self.elements.y = list.iter().copied().find(|q| {
                matches!(q, Quantity::Ppm(c) if self.session.inputs.channels[*c].isotope.as_ref().is_some_and(|i| i.element == "Y"))
            }).or_else(|| list.iter().copied().find(|q| matches!(q, Quantity::Ppm(_))));
        }
        ui.horizontal(|ui| {
            for (id, is_y) in [("cross_x", false), ("cross_y", true)] {
                ui.label(RichText::new(if is_y { "Y" } else { "X" }).color(t.dim));
                let current = if is_y { self.elements.y } else { Some(self.elements.x) };
                let text = current.map_or("–".to_string(), |q| self.quantity_name(q));
                egui::ComboBox::from_id_salt(id).selected_text(text).width(130.0).show_ui(ui, |ui| {
                    for q in &list {
                        let name = self.quantity_name(*q);
                        if ui.selectable_label(current == Some(*q), name).clicked() {
                            if is_y {
                                self.elements.y = Some(*q);
                            } else {
                                self.elements.x = *q;
                            }
                        }
                    }
                });
            }
            theme::segmented(ui, &t, &mut self.elements.log_y, &[(true, "Log Y"), (false, "Linear Y")]);
        });
        let Some(yq) = self.elements.y else { return };
        let xq = self.elements.x;
        let mut pts: Vec<(u64, String, f64, f64, egui::Color32)> = Vec::new();
        for (s, r) in self.session.selections.iter().zip(&self.red.selections) {
            if s.excluded || (self.elements.hide_flagged && !r.flags.is_empty()) {
                continue;
            }
            let Some(g) = self.session.group_of(s) else { continue };
            if g.kind == GroupKind::Baseline || self.hidden_groups.contains(&g.id) {
                continue;
            }
            if let (Some(x), Some(y)) = (xq.value(r), yq.value(r)) {
                if !self.elements.log_y || y > 0.0 {
                    pts.push((s.id, s.label.clone(), x, y, group_color(g.color)));
                }
            }
        }
        let (response, painter, rect) = plot::allocate(ui, egui::vec2(ui.available_width(), height - 30.0));
        let x_name = self.quantity_name(xq);
        let y_name = self.quantity_name(yq);
        if pts.is_empty() {
            plot::axes(&painter, &t, &Frame2 { rect, x: (0.0, 1.0), y: (0.0, 1.0), log_y: false }, &x_name, &y_name);
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, "No spots have both values", FontId::proportional(13.0), t.dim);
            return;
        }
        let lo_x = pts.iter().map(|p| p.2).fold(f64::INFINITY, f64::min);
        let hi_x = pts.iter().map(|p| p.2).fold(f64::NEG_INFINITY, f64::max);
        let lo_y = pts.iter().map(|p| p.3).fold(f64::INFINITY, f64::min);
        let hi_y = pts.iter().map(|p| p.3).fold(f64::NEG_INFINITY, f64::max);
        let y = if self.elements.log_y {
            {
                // a little room around the points; whole decades only when the data spans several
                let (a, b) = (lo_y.log10(), hi_y.log10());
                let pad = ((b - a) * 0.08).max(0.08);
                if b - a > 2.0 { (10f64.powf((a - 0.03).floor()), 10f64.powf((b + 0.03).ceil())) } else { (10f64.powf(a - pad), 10f64.powf(b + pad)) }
            }
        } else {
            plot::pad(lo_y, hi_y, 0.08)
        };
        let f = Frame2 { rect, x: plot::pad(lo_x, hi_x, 0.06), y, log_y: self.elements.log_y };
        plot::axes(&painter, &t, &f, &x_name, &y_name);
        let clip = painter.with_clip_rect(rect);
        let mut nearest: Option<(usize, f32)> = None;
        for (i, p) in pts.iter().enumerate() {
            let s = f.pos(p.2, p.3);
            let selected = self.selected == Some(p.0);
            clip.circle_filled(s, if selected { 5.5 } else { 3.5 }, p.4);
            if selected {
                clip.circle_stroke(s, 6.5, Stroke::new(1.5, t.text));
            }
            if let Some(h) = response.hover_pos() {
                let d = s.distance(h);
                if d < 10.0 && nearest.is_none_or(|n| d < n.1) {
                    nearest = Some((i, d));
                }
            }
        }
        if let Some((i, _)) = nearest {
            let p = &pts[i];
            plot::hover_box(&painter, &t, rect, f.pos(p.2, p.3), &[p.1.clone(), format!("{x_name}: {}", fmt_sig(p.2, 4)), format!("{y_name}: {}", fmt_sig(p.3, 4))]);
            if response.clicked() {
                self.selected = Some(p.0);
            }
        }
    }
}
