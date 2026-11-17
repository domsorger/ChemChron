// SPDX-License-Identifier: GPL-3.0-or-later
//! Spot map: every analysis at its stage position from the laser log,
//! coloured by group, age, discordance or an element content, to see zoning
//! or mixed spots. Click a spot to select it.

use crate::app::TraceApp;
use crate::plot::{self, Frame2};
use crate::theme::{fmt_sig, group_color};
use egui::{Align2, Color32, FontId, Pos2, RichText, Stroke, Ui};
use chemchron_laser_core::model::*;

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum MapColour {
    #[default]
    Group,
    Age68,
    Age76,
    Discordance,
    /// Content of this channel (ppm).
    Element(usize),
}

#[derive(Default)]
pub struct MapState {
    pub colour: MapColour,
    pub show_refs: bool,
}

/// Blue – teal – yellow – red, for 0..1.
pub fn ramp(v: f64) -> Color32 {
    let stops = [(0.0, [49, 54, 149]), (0.33, [79, 195, 176]), (0.66, [253, 216, 53]), (1.0, [215, 48, 39])];
    let v = v.clamp(0.0, 1.0);
    for w in stops.windows(2) {
        let ((a, ca), (b, cb)) = (w[0], w[1]);
        if v <= b {
            let f = (v - a) / (b - a);
            let mix = |i: usize| (ca[i] as f64 + f * (cb[i] as f64 - ca[i] as f64)).round() as u8;
            return Color32::from_rgb(mix(0), mix(1), mix(2));
        }
    }
    Color32::from_rgb(215, 48, 39)
}

impl TraceApp {
    /// Whether the laser log gave positions.
    pub fn has_positions(&self) -> bool {
        self.session.selections.iter().any(|s| s.x.is_some() && s.y.is_some())
    }

    fn map_value(&self, index: usize) -> Option<f64> {
        let r = self.red.selections.get(index)?;
        match self.map.colour {
            MapColour::Group => None,
            MapColour::Age68 => r.upb.as_ref().map(|u| u.a68.age),
            MapColour::Age76 => r.upb.as_ref().map(|u| u.a76.age),
            MapColour::Discordance => r.upb.as_ref().map(|u| u.discordance_pct),
            MapColour::Element(c) => r.te.get(c).and_then(|v| v.as_ref()).map(|v| v.ppm),
        }
        .filter(|v| v.is_finite())
    }

    pub fn page_map(&mut self, ui: &mut Ui) {
        let t = self.theme();
        let te_channels: Vec<(usize, String)> = self
            .red
            .te
            .as_ref()
            .map(|te| te.calibrations.iter().enumerate().filter(|(_, c)| c.is_some()).map(|(i, _)| (i, self.session.inputs.channels[i].name.clone())).collect())
            .unwrap_or_default();
        ui.horizontal_wrapped(|ui| {
            ui.heading("Spot map");
            ui.add_space(10.0);
            ui.label(RichText::new("Colour").color(t.dim));
            let label = |c: MapColour| -> String {
                match c {
                    MapColour::Group => "Group".into(),
                    MapColour::Age68 => "206Pb/238U age".into(),
                    MapColour::Age76 => "207Pb/206Pb age".into(),
                    MapColour::Discordance => "discordance".into(),
                    MapColour::Element(i) => format!("{} (ppm)", te_channels.iter().find(|x| x.0 == i).map(|x| x.1.as_str()).unwrap_or("?")),
                }
            };
            egui::ComboBox::from_id_salt("map_colour").selected_text(label(self.map.colour)).width(160.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.map.colour, MapColour::Group, "Group");
                if self.red.upb.is_some() {
                    for c in [MapColour::Age68, MapColour::Age76, MapColour::Discordance] {
                        ui.selectable_value(&mut self.map.colour, c, label(c));
                    }
                }
                for (i, _) in &te_channels {
                    ui.selectable_value(&mut self.map.colour, MapColour::Element(*i), label(MapColour::Element(*i)));
                }
            });
            ui.checkbox(&mut self.map.show_refs, "Show reference materials");
        });
        if !self.has_positions() {
            ui.label(RichText::new("No stage positions: they come from the laser log (Data > Add laser log).").color(t.dim));
            return;
        }
        // spots to show
        let mut spots: Vec<(usize, f64, f64, f64, Color32, String)> = Vec::new();
        for (i, s) in self.session.selections.iter().enumerate() {
            let (Some(x), Some(y)) = (s.x, s.y) else { continue };
            let Some(g) = self.session.group_of(s) else { continue };
            if g.kind == GroupKind::Baseline || (g.kind == GroupKind::ReferenceMaterial && !self.map.show_refs) {
                continue;
            }
            spots.push((i, x, y, s.spot_size.unwrap_or(0.0), group_color(g.color), s.label.clone()));
        }
        let values: Vec<f64> = spots.iter().filter_map(|sp| self.map_value(sp.0)).collect();
        let (lo, hi) = if values.is_empty() {
            (0.0, 1.0)
        } else {
            let mut v = values.clone();
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let q = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
            let (a, b) = (q(0.02), q(0.98));
            if b > a { (a, b) } else { (a - 1.0, a + 1.0) }
        };
        if self.map.colour == MapColour::Group {
            // which colour is which group
            let mut shown: Vec<(String, Color32)> = Vec::new();
            for s in &self.session.selections {
                if s.x.is_none() || s.y.is_none() {
                    continue;
                }
                let Some(g) = self.session.group_of(s) else { continue };
                if g.kind == GroupKind::Baseline || (g.kind == GroupKind::ReferenceMaterial && !self.map.show_refs) {
                    continue;
                }
                if !shown.iter().any(|(n, _)| *n == g.name) {
                    shown.push((g.name.clone(), group_color(g.color)));
                }
            }
            ui.horizontal_wrapped(|ui| {
                for (n, c) in &shown {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                        ui.painter().circle_filled(r.center(), 4.5, *c);
                        ui.label(RichText::new(n).size(12.0));
                    });
                    ui.add_space(8.0);
                }
            });
        } else {
            ui.horizontal(|ui| {
                ui.label(RichText::new(fmt_sig(lo, 4)).size(11.5));
                let (r, _) = ui.allocate_exact_size(egui::vec2(220.0, 12.0), egui::Sense::hover());
                for k in 0..44 {
                    let x0 = r.left() + r.width() * k as f32 / 44.0;
                    ui.painter().rect_filled(egui::Rect::from_min_max(Pos2::new(x0, r.top()), Pos2::new(x0 + r.width() / 44.0 + 0.5, r.bottom())), 0, ramp(k as f64 / 43.0));
                }
                ui.label(RichText::new(fmt_sig(hi, 4)).size(11.5));
                ui.label(RichText::new("(2nd to 98th percentile; grey: no value)").color(t.dim).size(11.0));
            });
        }
        if spots.is_empty() {
            ui.label(RichText::new("No analyses to show.").color(t.dim));
            return;
        }
        // equal scale on both axes (µm)
        let (mut x0, mut x1, mut y0, mut y1) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
        for sp in &spots {
            x0 = x0.min(sp.1);
            x1 = x1.max(sp.1);
            y0 = y0.min(sp.2);
            y1 = y1.max(sp.2);
        }
        let pad = ((x1 - x0).max(y1 - y0) * 0.06).max(50.0);
        let size = egui::vec2(ui.available_width(), ui.available_height() - 24.0);
        let (response, painter, rect) = plot::allocate(ui, size);
        let (w, h) = ((x1 - x0) + 2.0 * pad, (y1 - y0) + 2.0 * pad);
        let scale = (rect.width() as f64 / w).min(rect.height() as f64 / h);
        let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        let (hw, hh) = (rect.width() as f64 / scale / 2.0, rect.height() as f64 / scale / 2.0);
        let f = Frame2 { rect, x: (cx - hw, cx + hw), y: (cy - hh, cy + hh), log_y: false };
        plot::axes(&painter, &t, &f, "x (µm)", "y (µm)");
        let clip = painter.with_clip_rect(rect);
        let hover = response.hover_pos();
        let mut nearest: Option<(usize, f32)> = None;
        for (k, sp) in spots.iter().enumerate() {
            let p = f.pos(sp.1, sp.2);
            let r = ((sp.3 / 2.0) * scale).max(4.0) as f32;
            let colour = match self.map.colour {
                MapColour::Group => sp.4,
                _ => self.map_value(sp.0).map(|v| ramp((v - lo) / (hi - lo))).unwrap_or(Color32::GRAY),
            };
            clip.circle_filled(p, r, colour);
            let selected = self.selected == Some(self.session.selections[sp.0].id);
            clip.circle_stroke(p, r, Stroke::new(if selected { 2.5 } else { 0.8 }, if selected { t.text } else { Color32::from_black_alpha(140) }));
            if let Some(hp) = hover {
                let d = hp.distance(p);
                if d <= r + 3.0 && nearest.is_none_or(|n| d < n.1) {
                    nearest = Some((k, d));
                }
            }
        }
        if let Some((k, _)) = nearest {
            let sp = &spots[k];
            let mut lines = vec![sp.5.clone(), format!("x {:.0} µm, y {:.0} µm", sp.1, sp.2)];
            if let Some(v) = self.map_value(sp.0) {
                lines.push(fmt_sig(v, 5));
            }
            plot::hover_box(&painter, &t, rect, f.pos(sp.1, sp.2), &lines);
            if response.clicked() {
                self.selected = Some(self.session.selections[sp.0].id);
            }
        }
        let _ = (Align2::LEFT_TOP, FontId::default());
        ui.label(RichText::new("Positions from the laser log; circles at spot size. Click a spot to select it (the Signal page then shows it).").color(t.dim).size(11.5));
    }
}

/// CI chondrite (McDonough & Sun 1995), ppm, for the live REE pattern.
const CHONDRITE: [(&str, f64); 14] = [
    ("La", 0.237), ("Ce", 0.613), ("Pr", 0.0928), ("Nd", 0.457), ("Sm", 0.148), ("Eu", 0.0563), ("Gd", 0.199),
    ("Tb", 0.0361), ("Dy", 0.246), ("Ho", 0.0546), ("Er", 0.160), ("Tm", 0.0247), ("Yb", 0.161), ("Lu", 0.0246),
];

impl TraceApp {
    /// Chondrite-normalised REE of one analysis (None where not determined).
    fn ree_pattern(&self, index: usize) -> Vec<Option<f64>> {
        let ch = &self.session.inputs.channels;
        CHONDRITE
            .iter()
            .map(|(el, c1)| {
                let i = ch.iter().position(|c| c.isotope.as_ref().is_some_and(|x| x.element == *el))?;
                let v = self.red.selections.get(index)?.te.get(i)?.as_ref()?.ppm;
                (v.is_finite() && v > 0.0).then_some(v / c1)
            })
            .collect()
    }

    /// The selected analysis's REE pattern, following its selection live, with
    /// its group's median faded behind it.
    pub fn live_ree(&mut self, ui: &mut Ui) {
        let t = self.theme();
        let Some(sel) = self.selected else { return };
        let Some(index) = self.session.selections.iter().position(|s| s.id == sel) else { return };
        let me = self.ree_pattern(index);
        if me.iter().all(|v| v.is_none()) {
            return;
        }
        let group = self.session.selections[index].group;
        let colour = self.session.group(group).map(|g| group_color(g.color)).unwrap_or(t.accent);
        let others: Vec<Vec<Option<f64>>> = self.session.selections.iter().enumerate().filter(|(i, s)| *i != index && s.group == group && !s.excluded).map(|(i, _)| self.ree_pattern(i)).collect();
        let median: Vec<Option<f64>> = (0..CHONDRITE.len())
            .map(|k| {
                let mut v: Vec<f64> = others.iter().filter_map(|p| p[k]).collect();
                if v.is_empty() {
                    return None;
                }
                v.sort_by(|a, b| a.partial_cmp(b).unwrap());
                Some(v[v.len() / 2])
            })
            .collect();
        crate::theme::section(ui, &t, "Live REE pattern", |ui| {
            let all: Vec<f64> = me.iter().chain(median.iter()).flatten().copied().collect();
            let lo = all.iter().copied().fold(f64::INFINITY, f64::min).max(1e-3);
            let hi = all.iter().copied().fold(f64::NEG_INFINITY, f64::max).max(lo * 10.0);
            let (response, painter, rect) = plot::allocate(ui, egui::vec2(ui.available_width(), 190.0));
            let f = Frame2 { rect, x: (-0.5, CHONDRITE.len() as f64 - 0.5), y: (lo / 2.0, hi * 2.0), log_y: true };
            plot::axes_with(&painter, &t, &f, "", "/ chondrite", false);
            for (k, (el, _)) in CHONDRITE.iter().enumerate() {
                painter.text(Pos2::new(f.px(k as f64), rect.bottom() + 4.0), Align2::CENTER_TOP, *el, FontId::proportional(10.5), t.dim);
            }
            let clip = painter.with_clip_rect(rect);
            let line = |pat: &[Option<f64>], c: Color32, w: f32| {
                let pts: Vec<Pos2> = pat.iter().enumerate().filter_map(|(k, v)| v.map(|v| f.pos(k as f64, v))).collect();
                if pts.len() > 1 {
                    clip.add(egui::Shape::line(pts.clone(), Stroke::new(w, c)));
                }
                for p in pts {
                    clip.circle_filled(p, w + 1.5, c);
                }
            };
            line(&median, colour.linear_multiply(0.35), 1.2);
            line(&me, colour, 2.0);
            let _ = response;
            ui.label(RichText::new("Drag the selection or its cut-outs: the pattern follows. Faded: the median of its group.").color(t.dim).size(11.0));
        });
    }
}
