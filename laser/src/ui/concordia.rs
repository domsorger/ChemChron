// SPDX-License-Identifier: GPL-3.0-or-later
//! Concordia diagrams with error ellipses.

use crate::app::TraceApp;
use crate::plot::{self, Frame2, View};
use crate::theme::{self, fmt_pm, group_color, section};
use egui::{Align2, FontId, Pos2, RichText, Shape, Stroke, Ui};
use chemchron_laser_core::geochron::{concordia_age, ellipse, ConcordiaPoint};
use chemchron_laser_core::model::*;
use chemchron_laser_core::svg::{age_ticks, curve_point, point_for, visible_ages, window, ConcordiaKind};

#[derive(Clone, Copy, PartialEq)]
pub enum PlotData {
    Measured,
    Corrected,
}

pub struct ConcordiaState {
    pub kind: ConcordiaKind,
    pub data: PlotData,
    pub hide_flagged: bool,
    pub view: View,
    pub last_key: u64,
}

impl Default for ConcordiaState {
    fn default() -> Self {
        ConcordiaState {
            kind: ConcordiaKind::Wetherill,
            data: PlotData::Measured,
            hide_flagged: true,
            view: View::default(),
            last_key: 0,
        }
    }
}

struct Plotted {
    id: u64,
    label: String,
    group: u64,
    point: ConcordiaPoint,
    age: f64,
    age_2s: f64,
    disc: f64,
    color: [u8; 3],
    flagged: bool,
}

impl TraceApp {
    fn concordia_points(&self) -> Vec<Plotted> {
        let d = &self.session.settings.upb.decay;
        let corrected = self.concordia.data == PlotData::Corrected;
        let mut out = Vec::new();
        for (s, r) in self.session.selections.iter().zip(&self.red.selections) {
            if s.excluded || self.hidden_groups.contains(&s.group) {
                continue;
            }
            let Some(g) = self.session.group_of(s) else { continue };
            if g.kind == GroupKind::Baseline {
                continue;
            }
            let Some(u) = &r.upb else { continue };
            let flagged = !r.flags.is_empty();
            if self.concordia.hide_flagged && flagged {
                continue;
            }
            let (point, age, age_2s) = if corrected {
                let Some(c) = &u.common else { continue };
                let p = match self.concordia.kind {
                    ConcordiaKind::Wetherill => ConcordiaPoint { x: c.r75, y: c.r68, sx: c.r75_2s, sy: c.r68_2s, rho: u.rho_68_75 },
                    ConcordiaKind::TeraWasserburg => {
                        let r38 = 1.0 / c.r68;
                        let r76 = c.r75 / c.r68 / d.u85;
                        ConcordiaPoint {
                            x: r38,
                            y: r76,
                            sx: c.r68_2s / (c.r68 * c.r68),
                            sy: r76 * ((c.r75_2s / c.r75).powi(2) + (c.r68_2s / c.r68).powi(2)).sqrt(),
                            rho: u.rho_76_38,
                        }
                    }
                };
                (p, c.age68, c.age68_2s)
            } else {
                let Some(p) = point_for(self.concordia.kind, u) else { continue };
                (p, u.a68.age, u.a68.prop2s)
            };
            if !(point.x.is_finite() && point.y.is_finite() && point.sx > 0.0 && point.sy > 0.0) {
                continue;
            }
            out.push(Plotted {
                id: s.id,
                label: s.label.clone(),
                group: s.group,
                point,
                age,
                age_2s,
                disc: u.discordance_pct,
                color: g.color,
                flagged,
            });
        }
        out
    }

    pub fn page_concordia(&mut self, ui: &mut Ui) {
        let t = self.theme();
        if self.red.upb.is_none() {
            ui.label(RichText::new("No ages yet. Check the Corrections page.").color(t.dim));
            return;
        }
        let points = self.concordia_points();
        egui::Panel::right(crate::theme::layout_id("conc_side")).resizable(true).default_size(280.0).size_range(196.0..=700.0).frame(egui::Frame::NONE).show(ui, |ui| {
            egui::ScrollArea::both().auto_shrink([false, false]).id_salt("conc_side").show(ui, |ui| {
                egui::Frame::NONE.outer_margin(egui::Margin { left: 10, ..Default::default() }).show(ui, |ui| {
                    self.concordia_side(ui, &points);
                });
            });
        });
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let mut kind = self.concordia.kind;
                if theme::segmented(ui, &t, &mut kind, &[(ConcordiaKind::Wetherill, "Wetherill"), (ConcordiaKind::TeraWasserburg, "Tera-Wasserburg")]) {
                    self.concordia.kind = kind;
                    self.concordia.view = View::default();
                }
                ui.separator();
                let has_common = self.session.settings.upb.common_method != CommonPbMethod::None;
                ui.add_enabled_ui(has_common, |ui| {
                    let mut data = self.concordia.data;
                    if theme::segmented(ui, &t, &mut data, &[(PlotData::Measured, "As measured"), (PlotData::Corrected, "Common-Pb corrected")]) {
                        self.concordia.data = data;
                        self.concordia.view = View::default();
                    }
                }).response.on_disabled_hover_text("Choose a common-Pb method on the Corrections page");
                if !has_common {
                    self.concordia.data = PlotData::Measured;
                }
                ui.separator();
                if ui.checkbox(&mut self.concordia.hide_flagged, "Leave flagged out").changed() {
                    self.concordia.view = View::default();
                }
                if ui.button("Reset view").on_hover_text("Show all analyses again (double-click the plot does the same)").clicked() {
                    self.concordia.view = View::default();
                }
                if !self.embedded && ui.button("Save figure…").on_hover_text("As an SVG, ready for a paper").clicked() {
                    self.save_concordia_svg();
                }
            });
            ui.add_space(4.0);
            self.concordia_plot(ui, &points);
            ui.label(RichText::new("Ellipses show 2s with propagated uncertainties. Scroll to zoom, drag to pan, click an ellipse to select the analysis.").color(t.dim).size(11.5));
        });
    }

    fn save_concordia_svg(&mut self) {
        let groups: Vec<u64> = self
            .session
            .groups
            .iter()
            .filter(|g| g.kind != GroupKind::Baseline && !self.hidden_groups.contains(&g.id))
            .map(|g| g.id)
            .collect();
        if let Some(path) = rfd::FileDialog::new().set_file_name("concordia.svg").add_filter("SVG figure", &["svg"]).save_file() {
            let text = chemchron_laser_core::svg::concordia_svg(&self.session, &self.red, self.concordia.kind, &groups);
            match std::fs::write(&path, text) {
                Ok(()) => self.say(format!("Saved {}", path.display()), false),
                Err(e) => self.say(e.to_string(), true),
            }
        }
    }

    fn concordia_plot(&mut self, ui: &mut Ui, points: &[Plotted]) {
        let t = self.theme();
        let d = self.session.settings.upb.decay;
        let kind = self.concordia.kind;
        let size = egui::vec2(ui.available_width(), ui.available_height() - 22.0);
        let (response, painter, rect) = plot::allocate(ui, size);
        let only: Vec<ConcordiaPoint> = points.iter().map(|p| p.point).collect();
        let fit = window(&only).unwrap_or(((0.0, 1.0), (0.0, 0.2)));
        let (x, y) = self.concordia.view.resolve(fit);
        let f = Frame2 { rect, x, y, log_y: false };
        let (x_label, y_label) = match kind {
            ConcordiaKind::Wetherill => ("207Pb/235U", "206Pb/238U"),
            ConcordiaKind::TeraWasserburg => ("238U/206Pb", "207Pb/206Pb"),
        };
        plot::axes(&painter, &t, &f, x_label, y_label);
        let clip = painter.with_clip_rect(rect);
        // Concordia curve.
        let (lo, hi) = visible_ages(kind, &d, x, y);
        let span = (hi - lo).max(1.0);
        let start = (lo - span * 0.15).max(0.5);
        let end = hi + span * 0.15;
        let curve: Vec<Pos2> = (0..=300)
            .map(|i| {
                let age = start + (end - start) * i as f64 / 300.0;
                let (px, py) = curve_point(kind, &d, age);
                f.pos(px, py)
            })
            .collect();
        clip.add(Shape::line(curve, Stroke::new(1.8, t.curve)));
        for age in age_ticks(lo, hi) {
            let (px, py) = curve_point(kind, &d, age);
            let p = f.pos(px, py);
            clip.circle_filled(p, 3.5, t.curve);
            clip.text(p + egui::vec2(6.0, -7.0), Align2::LEFT_BOTTOM, format!("{age:.0} Ma"), FontId::proportional(11.5), t.curve);
        }
        // Ellipses.
        let hover = response.hover_pos();
        let mut nearest: Option<(usize, f32)> = None;
        for (i, p) in points.iter().enumerate() {
            let shape: Vec<Pos2> = ellipse(&p.point, 40).iter().map(|(a, b)| f.pos(*a, *b)).collect();
            let color = group_color(p.color);
            let selected = self.selected == Some(p.id);
            if let Some(h) = hover {
                let centre = f.pos(p.point.x, p.point.y);
                let reach = shape.iter().map(|s| s.distance(centre)).fold(0.0, f32::max).max(6.0);
                let dist = centre.distance(h);
                if dist < reach && nearest.is_none_or(|n| dist < n.1) {
                    nearest = Some((i, dist));
                }
            }
            let (fill, stroke) = if selected {
                (color.linear_multiply(0.5), Stroke::new(2.0, t.text))
            } else if p.flagged {
                (color.linear_multiply(0.05), Stroke::new(1.0, color.linear_multiply(0.55)))
            } else {
                (color.linear_multiply(0.22), Stroke::new(1.0, color))
            };
            clip.add(Shape::convex_polygon(shape, fill, stroke));
        }
        let mut consumed = false;
        if let Some((i, _)) = nearest {
            let p = &points[i];
            if response.clicked() {
                self.selected = Some(p.id);
                consumed = true;
            }
            plot::hover_box(&painter, &t, rect, f.pos(p.point.x, p.point.y), &[
                p.label.clone(),
                format!("206/238 age {} Ma", fmt_pm(p.age, p.age_2s)),
                format!("Discordance {:.1} %", p.disc),
            ]);
        }
        if !consumed {
            self.concordia.view.interact(ui, &response, &f);
        }
        if response.double_clicked() {
            self.concordia.view = View::default();
        }
    }

    /// The selected analysis on a small concordia that follows its selection
    /// live (as in VizualAge): its group's analyses faded, the analysis bold.
    pub fn live_concordia(&mut self, ui: &mut Ui) {
        let t = self.theme();
        let Some(sel) = self.selected else { return };
        let kind = self.concordia.kind;
        let d = self.session.settings.upb.decay;
        let hide = self.concordia.hide_flagged;
        self.concordia.hide_flagged = false;
        let all = self.concordia_points();
        self.concordia.hide_flagged = hide;
        let Some(me) = all.iter().find(|p| p.id == sel) else {
            section(ui, &t, "Live concordia", |ui| {
                ui.label(RichText::new("No U–Pb result for this selection.").color(t.dim));
            });
            return;
        };
        let group: Vec<&Plotted> = all.iter().filter(|p| p.group == me.group).collect();
        section(ui, &t, "Live concordia", |ui| {
            ui.horizontal(|ui| {
                let mut k = kind;
                if theme::segmented(ui, &t, &mut k, &[(ConcordiaKind::Wetherill, "Wetherill"), (ConcordiaKind::TeraWasserburg, "T-W")]) {
                    self.concordia.kind = k;
                    self.concordia.view = View::default();
                }
                ui.separator();
                let mut g = self.signal.live_group_view;
                if theme::segmented(ui, &t, &mut g, &[(false, "Analysis"), (true, "Group")]) {
                    self.signal.live_group_view = g;
                }
            });
            ui.label(RichText::new(format!("{}: {} Ma, disc. {:.1} %", me.label, fmt_pm(me.age, me.age_2s), me.disc)).strong().size(12.0));
            // window: around the analysis (2.5× its ellipse), or its whole group
            let p = me.point;
            let ((mut x0, mut x1), (mut y0, mut y1)) = ((p.x - 2.5 * p.sx, p.x + 2.5 * p.sx), (p.y - 2.5 * p.sy, p.y + 2.5 * p.sy));
            if self.signal.live_group_view {
                let pts: Vec<ConcordiaPoint> = group.iter().map(|p| p.point).collect();
                if let Some(((a, b), (c, d))) = window(&pts) {
                    x0 = x0.min(a);
                    x1 = x1.max(b);
                    y0 = y0.min(c);
                    y1 = y1.max(d);
                }
            }
            let size = egui::vec2(ui.available_width(), 220.0);
            let (_response, painter, rect) = plot::allocate(ui, size);
            let f = Frame2 { rect, x: (x0, x1), y: (y0, y1), log_y: false };
            plot::axes(&painter, &t, &f, "", "");
            let clip = painter.with_clip_rect(rect);
            let (lo, hi) = visible_ages(kind, &d, f.x, f.y);
            let span = (hi - lo).max(1.0);
            let (start, end) = ((lo - span * 0.2).max(0.5), hi + span * 0.2);
            let curve: Vec<Pos2> = (0..=200)
                .map(|i| {
                    let age = start + (end - start) * i as f64 / 200.0;
                    let (px, py) = curve_point(kind, &d, age);
                    f.pos(px, py)
                })
                .collect();
            clip.add(Shape::line(curve, Stroke::new(1.5, t.curve)));
            for age in age_ticks(lo, hi) {
                let (px, py) = curve_point(kind, &d, age);
                let q = f.pos(px, py);
                clip.circle_filled(q, 3.0, t.curve);
                clip.text(q + egui::vec2(5.0, -6.0), Align2::LEFT_BOTTOM, format!("{age:.0}"), FontId::proportional(10.5), t.curve);
            }
            for g in &group {
                if g.id == sel {
                    continue;
                }
                let c = group_color(g.color);
                let shape: Vec<Pos2> = ellipse(&g.point, 32).iter().map(|(a, b)| f.pos(*a, *b)).collect();
                clip.add(Shape::convex_polygon(shape, c.linear_multiply(0.06), Stroke::new(0.8, c.linear_multiply(0.45))));
            }
            let c = group_color(me.color);
            let shape: Vec<Pos2> = ellipse(&me.point, 48).iter().map(|(a, b)| f.pos(*a, *b)).collect();
            clip.add(Shape::convex_polygon(shape, c.linear_multiply(0.45), Stroke::new(2.0, t.text)));
            ui.label(RichText::new("Drag the selection's edges on the signal: the ellipse follows. Faded: other analyses of its group.").color(t.dim).size(11.0));
        });
    }

    fn concordia_side(&mut self, ui: &mut Ui, points: &[Plotted]) {
        let t = self.theme();
        let groups: Vec<(u64, String, [u8; 3], bool)> = self
            .session
            .groups
            .iter()
            .filter(|g| g.kind != GroupKind::Baseline)
            .map(|g| (g.id, g.name.clone(), g.color, self.hidden_groups.contains(&g.id)))
            .collect();
        section(ui, &t, "Groups", |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for (id, name, color, hidden) in &groups {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().rect_filled(r, 2, group_color(*color));
                    let mut shown = !*hidden;
                    if ui.checkbox(&mut shown, name).changed() {
                        if shown {
                            self.hidden_groups.remove(id);
                        } else {
                            self.hidden_groups.insert(*id);
                        }
                        self.concordia.view = View::default();
                    }
                });
            }
        });
        let d = self.session.settings.upb.decay;
        let corrected = self.concordia.data == PlotData::Corrected;
        for (id, name, color, hidden) in &groups {
            if *hidden {
                continue;
            }
            let chosen: Vec<ConcordiaPoint> = if corrected || self.concordia.hide_flagged || self.concordia.kind == ConcordiaKind::TeraWasserburg {
                // Concordia ages are always calculated on Wetherill ratios.
                self.session
                    .selections
                    .iter()
                    .zip(&self.red.selections)
                    .filter(|(s, r)| s.group == *id && !s.excluded && (!self.concordia.hide_flagged || r.flags.is_empty()))
                    .filter_map(|(_, r)| {
                        let u = r.upb.as_ref()?;
                        if corrected {
                            let c = u.common.as_ref()?;
                            Some(ConcordiaPoint { x: c.r75, y: c.r68, sx: c.r75_2s, sy: c.r68_2s, rho: u.rho_68_75 })
                        } else {
                            point_for(ConcordiaKind::Wetherill, u)
                        }
                    })
                    .collect()
            } else {
                points.iter().filter(|p| p.group == *id).map(|p| p.point).collect()
            };
            if chosen.is_empty() {
                continue;
            }
            let age = concordia_age(&chosen, &d);
            let stats = self.red.qa.groups.iter().find(|g| g.group == *id);
            t.card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().rect_filled(r, 2, group_color(*color));
                    ui.label(RichText::new(name).strong());
                    ui.label(RichText::new(format!("n = {}", chosen.len())).color(t.dim).size(12.0));
                    let left_out = self
                        .session
                        .selections
                        .iter()
                        .zip(&self.red.selections)
                        .filter(|(s, r)| s.group == *id && !s.excluded && !r.flags.is_empty())
                        .count();
                    if self.concordia.hide_flagged && left_out > 0 {
                        ui.label(RichText::new(format!("{left_out} flagged left out")).color(t.warn).size(12.0));
                    }
                });
                if let Some(c) = age {
                    ui.label(RichText::new(format!("Concordia age {} Ma", fmt_pm(c.age, c.unc2s))).size(13.0));
                    let mswd_color = if c.mswd > 2.5 { t.warn } else { t.dim };
                    ui.label(RichText::new(format!("MSWD {:.2}, p = {:.2}", c.mswd, c.p)).color(mswd_color).size(12.0));
                }
                if let Some(w) = stats.and_then(|s| s.wm_a82.as_ref()) {
                    ui.label(RichText::new(format!("208/232 mean {} Ma", fmt_pm(w.mean, w.unc2s_mswd))).color(t.dim).size(12.0));
                }
            });
            ui.add_space(6.0);
        }
    }
}
