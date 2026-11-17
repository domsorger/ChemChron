// SPDX-License-Identifier: GPL-3.0-or-later
//! Ages and ratios: weighted-mean plot of one group's ages or ratios (with
//! the accepted value of a reference material), and the age distribution.

use crate::app::TraceApp;
use crate::plot::{self, Frame2};
use crate::theme::{self, fmt_pm, group_color, stat};
use egui::{Pos2, Rect, RichText, Shape, Stroke, Ui};
use chemchron_laser_core::model::*;
use chemchron_laser_core::stats::{self, weighted_mean};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum AgeKind {
    A68,
    A75,
    A76,
    A82,
    Corrected,
    Th230,
    R68,
    R75,
    R76,
    R82,
}

impl AgeKind {
    pub const ALL: [AgeKind; 10] = [AgeKind::A68, AgeKind::A75, AgeKind::A76, AgeKind::A82, AgeKind::Corrected, AgeKind::Th230, AgeKind::R68, AgeKind::R75, AgeKind::R76, AgeKind::R82];
    pub fn is_ratio(&self) -> bool {
        matches!(self, AgeKind::R68 | AgeKind::R75 | AgeKind::R76 | AgeKind::R82)
    }
    /// Key of the ratio in the reference-material values.
    fn ratio_key(&self) -> Option<&'static str> {
        Some(match self {
            AgeKind::R68 => chemchron_laser_core::refmat::K_R68,
            AgeKind::R75 => chemchron_laser_core::refmat::K_R75,
            AgeKind::R76 => chemchron_laser_core::refmat::K_R76,
            AgeKind::R82 => chemchron_laser_core::refmat::K_R82,
            _ => return None,
        })
    }
    fn label(&self) -> &'static str {
        match self {
            AgeKind::A68 => "206Pb/238U",
            AgeKind::A75 => "207Pb/235U",
            AgeKind::A76 => "207Pb/206Pb",
            AgeKind::A82 => "208Pb/232Th",
            AgeKind::Corrected => "206/238 common-Pb corrected",
            AgeKind::Th230 => "206/238 with 230Th",
            AgeKind::R68 => "206Pb/238U ratio",
            AgeKind::R75 => "207Pb/235U ratio",
            AgeKind::R76 => "207Pb/206Pb ratio",
            AgeKind::R82 => "208Pb/232Th ratio",
        }
    }
}

pub struct AgesState {
    pub kind: AgeKind,
    pub group: Option<u64>,
    pub hide_flagged: bool,
    pub bandwidth: f64,
}

impl Default for AgesState {
    fn default() -> Self {
        AgesState { kind: AgeKind::A68, group: None, hide_flagged: true, bandwidth: 0.0 }
    }
}

struct Bar {
    id: u64,
    label: String,
    age: f64,
    unc: f64,
    flagged: bool,
}

impl TraceApp {
    /// Systematic term of a mean of this kind (2σ, relative): the primary's
    /// accepted value (from the analyses of the group) and the lab's term in quadrature.
    fn systematic_rel(&self, group: u64) -> (f64, f64) {
        let (k, of): (usize, fn(&chemchron_laser_core::drs::upb::UpbValue) -> Option<(f64, f64, f64)>) = match self.ages.kind {
            AgeKind::A68 | AgeKind::Corrected => (0, |u| Some((u.a68.age, u.a68.prop2s, u.a68.rminc2s))),
            AgeKind::Th230 => (0, |u| u.a68_th230.map(|a| (a.age, a.prop2s, a.rminc2s))),
            AgeKind::A75 => (1, |u| Some((u.a75.age, u.a75.prop2s, u.a75.rminc2s))),
            AgeKind::A76 => (2, |u| Some((u.a76.age, u.a76.prop2s, u.a76.rminc2s))),
            AgeKind::A82 => (3, |u| u.a82.map(|a| (a.age, a.prop2s, a.rminc2s))),
            AgeKind::R68 => (0, |u| Some((u.r68.mean, u.r68.prop2s, u.r68.rminc2s))),
            AgeKind::R75 => (1, |u| Some((u.r75.mean, u.r75.prop2s, u.r75.rminc2s))),
            AgeKind::R76 => (2, |u| Some((u.r76.mean, u.r76.prop2s, u.r76.rminc2s))),
            AgeKind::R82 => (3, |u| u.r82.as_ref().map(|r| (r.mean, r.prop2s, r.rminc2s))),
        };
        let mut rm: Vec<f64> = self
            .session
            .selections
            .iter()
            .enumerate()
            .filter(|(_, s)| s.group == group && !s.excluded)
            .filter_map(|(i, _)| self.red.selections[i].upb.as_ref().and_then(of))
            .filter(|(v, p, r)| v.is_finite() && *v != 0.0 && p.is_finite() && r.is_finite())
            .map(|(v, p, r)| (r * r - p * p).max(0.0).sqrt() / v.abs())
            .collect();
        rm.sort_by(|a, b| a.total_cmp(b));
        let rm = rm.get(rm.len() / 2).copied().unwrap_or(0.0);
        (rm, self.sys_pct(k) / 100.0)
    }

    fn age_of(&self, index: usize) -> Option<(f64, f64)> {
        let u = self.red.selections[index].upb.as_ref()?;
        let pair = match self.ages.kind {
            AgeKind::A68 => (u.a68.age, u.a68.prop2s),
            AgeKind::A75 => (u.a75.age, u.a75.prop2s),
            AgeKind::A76 => (u.a76.age, u.a76.prop2s),
            AgeKind::A82 => u.a82.map(|a| (a.age, a.prop2s))?,
            AgeKind::Corrected => u.common.as_ref().map(|c| (c.age68, c.age68_2s))?,
            AgeKind::Th230 => u.a68_th230.map(|a| (a.age, a.prop2s))?,
            AgeKind::R68 => (u.r68.mean, u.r68.prop2s),
            AgeKind::R75 => (u.r75.mean, u.r75.prop2s),
            AgeKind::R76 => (u.r76.mean, u.r76.prop2s),
            AgeKind::R82 => u.r82.as_ref().map(|r| (r.mean, r.prop2s))?,
        };
        (pair.0.is_finite() && pair.1.is_finite() && pair.1 > 0.0).then_some(pair)
    }

    fn bars_for(&self, group: u64) -> Vec<Bar> {
        let mut bars = Vec::new();
        for (i, s) in self.session.selections.iter().enumerate() {
            if s.group != group || s.excluded {
                continue;
            }
            let flagged = !self.red.selections[i].flags.is_empty();
            if self.ages.hide_flagged && flagged {
                continue;
            }
            if let Some((age, unc)) = self.age_of(i) {
                bars.push(Bar { id: s.id, label: s.label.clone(), age, unc, flagged });
            }
        }
        bars
    }

    pub fn page_ages(&mut self, ui: &mut Ui) {
        let t = self.theme();
        if self.red.upb.is_none() {
            ui.label(RichText::new("No ages yet. Check the Corrections page.").color(t.dim));
            return;
        }
        let groups: Vec<(u64, String, [u8; 3])> = self
            .session
            .groups
            .iter()
            .filter(|g| g.kind != GroupKind::Baseline && !self.session.rm_of(g.id).is_some_and(|m| m.matrix == "Glass"))
            .map(|g| (g.id, g.name.clone(), g.color))
            .collect();
        if self.ages.group.is_none_or(|g| !groups.iter().any(|x| x.0 == g)) {
            self.ages.group = groups
                .iter()
                .find(|g| self.session.group(g.0).is_some_and(|x| x.kind == GroupKind::Sample))
                .or(groups.first())
                .map(|g| g.0);
        }
        ui.horizontal_wrapped(|ui| {
            let text = self.ages.group.and_then(|c| groups.iter().find(|g| g.0 == c)).map_or("–", |g| g.1.as_str()).to_string();
            ui.label(RichText::new("Group").color(t.dim));
            egui::ComboBox::from_id_salt("ages_group").selected_text(text).width(120.0).show_ui(ui, |ui| {
                for (id, name, _) in &groups {
                    if ui.selectable_label(self.ages.group == Some(*id), name).clicked() {
                        self.ages.group = Some(*id);
                    }
                }
            });
            ui.label(RichText::new("Show").color(t.dim));
            egui::ComboBox::from_id_salt("ages_kind").selected_text(self.ages.kind.label()).width(190.0).show_ui(ui, |ui| {
                ui.label(RichText::new("Ages").strong());
                for k in AgeKind::ALL.iter().filter(|k| !k.is_ratio()) {
                    ui.selectable_value(&mut self.ages.kind, *k, k.label());
                }
                ui.separator();
                ui.label(RichText::new("Ratios").strong());
                for k in AgeKind::ALL.iter().filter(|k| k.is_ratio()) {
                    ui.selectable_value(&mut self.ages.kind, *k, k.label());
                }
            });
            ui.checkbox(&mut self.ages.hide_flagged, "Leave flagged out");
            if let Some(group) = self.ages.group {
                if !self.embedded && ui.button("Save figure…").on_hover_text("206/238 ages as an SVG, ready for a paper").clicked() {
                    if let Some(path) = rfd::FileDialog::new().set_file_name("weighted_mean.svg").add_filter("SVG figure", &["svg"]).save_file() {
                        let text = chemchron_laser_core::svg::weighted_mean_svg(&self.session, &self.red, group);
                        match std::fs::write(&path, text) {
                            Ok(()) => self.say(format!("Saved {}", path.display()), false),
                            Err(e) => self.say(e.to_string(), true),
                        }
                    }
                }
            }
        });
        ui.add_space(6.0);
        let Some(group) = self.ages.group else { return };
        let color = groups.iter().find(|g| g.0 == group).map_or(t.accent, |g| group_color(g.2));
        let bars = self.bars_for(group);
        let values: Vec<(f64, f64)> = bars.iter().map(|b| (b.age, b.unc)).collect();
        let w = weighted_mean(&values);
        let ratio = self.ages.kind.is_ratio();
        let unit = if ratio { "" } else { " Ma" };
        // accepted value of a reference material (ratios)
        let accepted = self.ages.kind.ratio_key().and_then(|k| self.session.rm_of(group).and_then(|m| m.get(k)).map(|v| (v.value, v.unc2s)));
        ui.horizontal(|ui| {
            let (value, mswd, p, n) = w.as_ref().map_or(("–".to_string(), "–".to_string(), "–".to_string(), "0".to_string()), |w| {
                (
                    format!("{}{unit}", fmt_pm(w.mean, w.unc2s_mswd)),
                    format!("{:.2}", w.mswd),
                    format!("{:.2}", w.p),
                    w.n.to_string(),
                )
            });
            stat(ui, &t, "Weighted mean, 2s", &value, color, 220.0);
            if let Some(m) = &w {
                let (rm, lab) = self.systematic_rel(group);
                let sys = rm.hypot(lab) * m.mean.abs();
                let total = m.unc2s_mswd.hypot(sys);
                let src = self.lab_systematic.as_ref().map_or("primary RM value only".to_string(), |l| l.source.clone());
                crate::theme::stat_r(ui, &t, "Total, 2s", &format!("{}{unit}", fmt_pm(m.mean, total)), t.text, 200.0).on_hover_text(format!(
                    "Weighted mean 2s and the systematic term {:.2} % in quadrature; the systematic term is the primary's accepted value {:.2} % and the lab systematic {:.2} % ({src}), also in quadrature. The same as the totals in Combined results.",
                    100.0 * rm.hypot(lab),
                    100.0 * rm,
                    100.0 * lab
                ));
            }
            stat(ui, &t, "MSWD", &mswd, if w.as_ref().is_some_and(|w| w.mswd > 2.5) { t.warn } else { t.text }, 70.0);
            stat(ui, &t, "p", &p, t.text, 70.0);
            stat(ui, &t, "Analyses", &n, t.text, 70.0);
            if let Some((a, a2)) = accepted {
                stat(ui, &t, "Accepted, 2s", &fmt_pm(a, a2), t.ok, 180.0);
                if let Some(m) = &w {
                    stat(ui, &t, "Difference", &format!("{:+.2} %", 100.0 * (m.mean - a) / a), t.text, 90.0);
                }
            }
            if let Some(m) = &w {
                if m.unc2s_mswd > m.unc2s {
                    ui.label(RichText::new("The 2s is enlarged by √MSWD: the scatter is more than the uncertainties explain (P < 0.05).").color(t.dim).size(12.0));
                }
            }
        });
        ui.add_space(6.0);
        let height = ui.available_height();
        let width = ui.available_width();
        if ratio {
            self.weighted_plot(ui, egui::vec2(width, height - 30.0), &bars, w.as_ref(), color, accepted, self.ages.kind.label());
        } else {
            self.weighted_plot(ui, egui::vec2(width, height * 0.55), &bars, w.as_ref(), color, None, "Age (Ma)");
            ui.add_space(4.0);
            self.density_plot(ui, egui::vec2(width, ui.available_height()), &groups);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn weighted_plot(&mut self, ui: &mut Ui, size: egui::Vec2, bars: &[Bar], w: Option<&stats::WeightedMean>, color: egui::Color32, accepted: Option<(f64, f64)>, y_label: &str) {
        let t = self.theme();
        let (response, painter, rect) = plot::allocate(ui, size);
        if bars.is_empty() {
            plot::axes(&painter, &t, &Frame2 { rect, x: (0.0, 1.0), y: (0.0, 1.0), log_y: false }, "Analysis", y_label);
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, "No analyses with this value", egui::FontId::proportional(13.0), t.dim);
            return;
        }
        let mut lo = bars.iter().map(|b| b.age - b.unc).fold(f64::INFINITY, f64::min);
        let mut hi = bars.iter().map(|b| b.age + b.unc).fold(f64::NEG_INFINITY, f64::max);
        if let Some((a, _)) = accepted {
            lo = lo.min(a);
            hi = hi.max(a);
        }
        let f = Frame2 { rect, x: (0.0, bars.len() as f64 + 1.0), y: plot::pad(lo, hi, 0.08), log_y: false };
        plot::axes(&painter, &t, &f, "Analysis", y_label);
        let clip = painter.with_clip_rect(rect);
        if let Some(m) = w {
            let band = Rect::from_min_max(
                Pos2::new(rect.left(), f.py(m.mean + m.unc2s_mswd)),
                Pos2::new(rect.right(), f.py(m.mean - m.unc2s_mswd)),
            );
            clip.rect_filled(band, 0, color.linear_multiply(0.18));
            clip.line_segment([f.pos(f.x.0, m.mean), f.pos(f.x.1, m.mean)], Stroke::new(1.5, t.text));
        }
        if let Some((a, _)) = accepted {
            let y = f.py(a);
            clip.extend(Shape::dashed_line(&[Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(1.5, t.ok), 7.0, 4.0));
            clip.text(Pos2::new(rect.right() - 6.0, y - 3.0), egui::Align2::RIGHT_BOTTOM, "accepted", egui::FontId::proportional(11.5), t.ok);
        }
        let mut hover: Option<usize> = None;
        for (i, b) in bars.iter().enumerate() {
            let x = f.px(i as f64 + 1.0);
            let width = (rect.width() / (bars.len() as f32 + 1.0) * 0.6).clamp(2.0, 16.0);
            let r = Rect::from_min_max(Pos2::new(x - width / 2.0, f.py(b.age + b.unc)), Pos2::new(x + width / 2.0, f.py(b.age - b.unc)));
            let selected = self.selected == Some(b.id);
            let fill = if b.flagged { color.linear_multiply(0.25) } else { color };
            clip.rect_filled(r, 1, fill);
            if selected {
                clip.rect_stroke(r, 1, Stroke::new(2.0, t.text), egui::StrokeKind::Outside);
            }
            if let Some(p) = response.hover_pos() {
                if (p.x - x).abs() <= width / 2.0 + 2.0 {
                    hover = Some(i);
                }
            }
        }
        if let Some(i) = hover {
            let b = &bars[i];
            let p = response.hover_pos().unwrap_or(rect.center());
            plot::hover_box(&painter, &t, rect, p, &[b.label.clone(), fmt_pm(b.age, b.unc)]);
            if response.clicked() {
                self.selected = Some(b.id);
            }
        }
    }

    fn density_plot(&mut self, ui: &mut Ui, size: egui::Vec2, groups: &[(u64, String, [u8; 3])]) {
        let t = self.theme();
        let (response, painter, rect) = plot::allocate(ui, size);
        let mut sets: Vec<(u64, Vec<f64>, egui::Color32)> = Vec::new();
        for (id, _, color) in groups {
            if self.session.group(*id).is_some_and(|g| g.kind == GroupKind::ReferenceMaterial) {
                continue;
            }
            let ages: Vec<f64> = self.bars_for(*id).iter().map(|b| b.age).filter(|a| *a > 0.0 && *a < 4600.0).collect();
            if !ages.is_empty() {
                sets.push((*id, ages, group_color(*color)));
            }
        }
        if sets.is_empty() {
            plot::axes(&painter, &t, &Frame2 { rect, x: (0.0, 1.0), y: (0.0, 1.0), log_y: false }, "Age (Ma)", "");
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, "No sample ages to show", egui::FontId::proportional(13.0), t.dim);
            return;
        }
        let all: Vec<f64> = sets.iter().flat_map(|s| s.1.iter().copied()).collect();
        let lo = all.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = all.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let x = plot::pad(lo, hi, 0.1);
        let columns = 480usize;
        // Silverman's rule of thumb per group, floored at 0.5 % of the range shown.
        let floor = (x.1 - x.0) * 0.005;
        let bandwidth = |ages: &[f64]| -> f64 {
            let m = ages.iter().sum::<f64>() / ages.len() as f64;
            let sd = (ages.iter().map(|a| (a - m).powi(2)).sum::<f64>() / ages.len().max(2) as f64).sqrt();
            (0.9 * sd * (ages.len() as f64).powf(-0.2)).max(floor)
        };
        let density = |ages: &[f64], at: f64| -> f64 {
            let bw = bandwidth(ages);
            ages.iter().map(|a| (-0.5 * ((at - a) / bw).powi(2)).exp()).sum::<f64>() / bw
        };
        let mut peak: f64 = 0.0;
        let curves: Vec<(Vec<(f64, f64)>, egui::Color32, &[f64])> = sets
            .iter()
            .map(|(_, ages, color)| {
                let pts: Vec<(f64, f64)> = (0..=columns)
                    .map(|i| {
                        let at = x.0 + (x.1 - x.0) * i as f64 / columns as f64;
                        let v = density(ages, at);
                        peak = peak.max(v);
                        (at, v)
                    })
                    .collect();
                (pts, *color, ages.as_slice())
            })
            .collect();
        let f = Frame2 { rect, x, y: (0.0, peak * 1.12), log_y: false };
        plot::axes(&painter, &t, &f, "Age (Ma)", "Age distribution of samples");
        let clip = painter.with_clip_rect(rect);
        for (pts, color, ages) in &curves {
            let line: Vec<Pos2> = pts.iter().map(|(a, b)| f.pos(*a, *b)).collect();
            clip.add(Shape::line(line, Stroke::new(1.8, *color)));
            for a in ages.iter() {
                let px = f.px(*a);
                clip.line_segment([Pos2::new(px, rect.bottom()), Pos2::new(px, rect.bottom() - 7.0)], Stroke::new(1.0, *color));
            }
        }
        let _ = response;
        let _ = theme::fmt_sig;
    }
}
