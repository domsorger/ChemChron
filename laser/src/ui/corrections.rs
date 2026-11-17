// SPDX-License-Identifier: GPL-3.0-or-later
//! Corrections: every reduction setting beside a plot of what it does.

use crate::app::TraceApp;
use crate::plot::{self, Frame2};
use crate::theme::{self, fmt_sig, form_row, section, series_color, tag, Theme};
use egui::{Pos2, RichText, Stroke, Ui};
use chemchron_laser_core::drs::upb::RatioKind;
use chemchron_laser_core::interp::{Curve, InterpKind};
use chemchron_laser_core::model::*;

#[derive(Clone, Copy, PartialEq)]
pub enum CorrectionView {
    Downhole,
    Through,
    Baseline,
    Calibration,
}

pub struct CorrectionsState {
    /// The per-spot internal-standard list is open in its own window.
    pub spots_window: bool,
    pub view: CorrectionView,
    pub ratio: RatioKind,
    pub channel: Option<usize>,
    pub te_channel: Option<usize>,
}

impl Default for CorrectionsState {
    fn default() -> Self {
        CorrectionsState {
            spots_window: false,
            view: CorrectionView::Downhole,
            ratio: RatioKind::R68,
            channel: None,
            te_channel: None,
        }
    }
}

impl TraceApp {
    /// Quality-check limits (shown in ChemChron's Settings > Laser).
    pub fn lab_settings_ui(&mut self, ui: &mut Ui) {
        let t = self.theme();
        let q = &mut self.session.settings.qa;
        let mut changed = false;
        ui.label(RichText::new("Quality checks of the open session (LA-ICP-MS > Quality); saved with the session.").strong());
        egui::Grid::new("qa_limits").show(ui, |ui| {
            for (label, v, hint) in [
                ("Warning above (%)", &mut q.warn_pct, "Difference of a reference material from its accepted value that raises a warning"),
                ("Alarm above (%)", &mut q.alarm_pct, "Difference that raises an alarm"),
                ("Flag discordance above (%)", &mut q.flag_discordance_pct, "Analyses more discordant than this are flagged"),
                ("Flag common ²⁰⁶Pb above (%)", &mut q.flag_common_pct, "Analyses with more common ²⁰⁶Pb than this are flagged"),
                ("Flag outlier sweeps above (%)", &mut q.flag_outlier_pct, "Analyses with more rejected sweeps than this are flagged"),
            ] {
                ui.label(RichText::new(label).color(t.dim)).on_hover_text(hint);
                changed |= ui.add(egui::DragValue::new(v).speed(0.1).range(0.0..=100.0)).changed();
                ui.end_row();
            }
        });
        if changed {
            self.changed = true;
        }
        ui.add_space(4.0);
        ui.checkbox(&mut self.signal.live_concordia, "Live concordia on the Signal page")
            .on_hover_text("The selected analysis on a small concordia beside the signal, following its selection as you drag it");
    }

    /// Element of the internal-standard channel (e.g. "Ca").
    pub fn is_element(&self) -> String {
        self.session
            .settings
            .te
            .is_channel
            .as_ref()
            .and_then(|n| self.session.inputs.channel_index(n))
            .and_then(|i| self.session.inputs.channels[i].isotope.as_ref().map(|x| x.element.clone()))
            .unwrap_or_default()
    }

    /// The oxide internal-standard values are typed as, when they are.
    pub fn is_oxide(&self) -> Option<String> {
        let te = &self.session.settings.te;
        if !te.is_as_oxide {
            return None;
        }
        if !te.is_oxide.is_empty() {
            return Some(te.is_oxide.clone());
        }
        chemchron_laser_core::oxides::default_oxide(&self.is_element()).map(String::from)
    }

    /// (element ppm per typed unit, unit name) of internal-standard values.
    pub fn is_unit(&self) -> (f64, String) {
        match self.is_oxide().and_then(|o| chemchron_laser_core::oxides::ppm_per_wt_pct(&o).map(|f| (f, o))) {
            Some((f, o)) => (f, format!("{o} wt%")),
            None => (1.0, format!("{} ppm", self.is_element())),
        }
    }

    /// Cut-outs (inclusion correction), for every scheme: by hand on the
    /// Signal page; the automatic spike removal needs the trace-element scheme.
    fn settings_cutouts(&mut self, ui: &mut Ui) {
        let t = self.theme();
        let channels: Vec<String> = self.session.inputs.channels.iter().map(|c| c.name.clone()).collect();
        let te_on = self.session.settings.te.enabled;
        let upb_on = self.session.settings.upb.enabled;
        let mut changed = false;
        section(ui, &t, "Cut-outs (inclusion correction)", |ui| {
            let e = &mut self.session.settings.te;
            ui.horizontal(|ui| {
                for (m, l, h) in [
                    (InclusionMode::ByHand, "By hand", "Only cut-outs you draw on the Signal page (Draw cut-out)"),
                    (InclusionMode::Automatic, "Automatic", "Spikes of an element relative to the internal standard are left out for that element (trace-element scheme)"),
                    (InclusionMode::Both, "Both", "Your cut-outs and the automatic removal"),
                ] {
                    let enabled = te_on || upb_on || m == InclusionMode::ByHand;
                    changed |= ui.add_enabled(enabled, egui::RadioButton::new(e.inclusions == m, l)).on_hover_text(h).on_disabled_hover_text("The automatic removal works on element contents: choose a scheme with trace elements").clicked() && {
                        e.inclusions = m;
                        true
                    };
                }
            });
            if te_on {
                ui.label(RichText::new("A cut-out leaves its sweeps out for the elements it names; for U–Pb, a cut-out on Pb206, Pb207, Pb208, U238 or Th232 leaves them out of the ratios using that channel.").color(t.dim).size(11.5));
            } else {
                ui.label(RichText::new("U–Pb only: draw cut-outs by hand on the raw Pb, U and Th signals (Signal page); they leave those sweeps out of the ratios using that channel.").color(t.dim).size(11.5));
            }
            if e.inclusions.automatic() {
                ui.label(RichText::new("Automatic, in unknowns only: for trace elements a run of sweeps whose element / internal-standard ratio jumps is left out for that element; for U–Pb a run whose 207Pb/206Pb or 204Pb jumps (common Pb, older inclusion) is left out of that spot's ratios.").color(t.dim).size(11.5));
                form_row(ui, &t, "Spike above", |ui| {
                    changed |= ui
                        .add(egui::DragValue::new(&mut e.inclusion_k).speed(0.1).range(2.0..=20.0).suffix(" robust SD"))
                        .on_hover_text("A run of sweeps whose element / internal-standard ratio lies this far above the spot's median is left out for that element (with its neighbours). Nothing is cut when more than 40 % of a spot would go. Only in unknowns.")
                        .changed();
                });
                if te_on {
                egui::CollapsingHeader::new("Elements checked automatically").id_salt("incl_channels").default_open(false).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for n in &channels {
                            let mut on = !e.inclusion_skip.contains(n);
                            if ui.checkbox(&mut on, n).changed() {
                                if on {
                                    e.inclusion_skip.retain(|x| x != n);
                                } else {
                                    e.inclusion_skip.push(n.clone());
                                }
                                changed = true;
                            }
                        }
                    });
                });
                }
            }
        });
        self.changed |= changed;
    }

    /// Which data reduction schemes run for this session.
    fn settings_scheme(&mut self, ui: &mut Ui) {
        let t = self.theme();
        let st = &mut self.session.settings;
        let mut v = (st.upb.enabled, st.te.enabled);
        section(ui, &t, "Data reduction scheme", |ui| {
            for (val, label, hint) in [
                ((true, true), "U–Pb geochronology and trace elements", "Ratios, ages and element contents from the same spots"),
                ((true, false), "U–Pb geochronology only", "Ratios and ages only"),
                ((false, true), "Trace elements only", "Element contents only"),
            ] {
                ui.radio_value(&mut v, val, label).on_hover_text(hint);
            }
        });
        if v != (st.upb.enabled, st.te.enabled) {
            st.upb.enabled = v.0;
            st.te.enabled = v.1;
            self.changed = true;
        }
    }

    /// Every sample spot with its own internal-standard value (or the group's).
    fn is_spot_grid(&mut self, ui: &mut Ui, max_height: f32) -> bool {
        let t = self.theme();
        let (f, unit) = self.is_unit();
        let element = self.is_element();
        let mut changed = false;
        let sample_groups: Vec<u64> = self.session.groups.iter().filter(|g| g.kind == GroupKind::Sample).map(|g| g.id).collect();
        let group_value = |groups: &[(u64, Option<f64>)], id: u64| groups.iter().find(|g| g.0 == id).and_then(|g| g.1);
        let groups: Vec<(u64, Option<f64>)> = self.session.groups.iter().map(|g| (g.id, g.is_ppm)).collect();
        ui.label(RichText::new(format!("Ticked: the spot uses its own {element} content (typed here, in {unit}). Not ticked: it uses its group's value. All element contents of a spot scale with this value.")).color(t.dim).size(11.5));
        egui::ScrollArea::vertical().id_salt(("is_spots", max_height as i32)).max_height(max_height).show(ui, |ui| {
            egui::Grid::new(("is_spot_grid", max_height as i32)).num_columns(3).striped(true).show(ui, |ui| {
                for s in self.session.selections.iter_mut().filter(|s| sample_groups.contains(&s.group)) {
                    ui.label(&s.label);
                    let mut on = s.is_ppm.is_some();
                    if ui.checkbox(&mut on, "").on_hover_text("Own value for this spot (otherwise the group value)").changed() {
                        s.is_ppm = if on { Some(group_value(&groups, s.group).unwrap_or(100000.0)) } else { None };
                        changed = true;
                    }
                    if let Some(v) = &mut s.is_ppm {
                        let mut shown = *v / f;
                        let sp = shown.abs().max(1e-3) * 1e-3;
                        if ui.add(egui::DragValue::new(&mut shown).speed(sp).max_decimals(4).range(0.0..=1e6)).changed() {
                            *v = shown * f;
                            changed = true;
                        }
                    } else {
                        let g = group_value(&groups, s.group).map(|v| fmt_sig(v / f, 5)).unwrap_or_else(|| "not set".into());
                        ui.label(RichText::new(format!("group value ({g})")).color(t.dim).size(11.5));
                    }
                    ui.end_row();
                }
            });
        });
        changed
    }

    pub fn page_corrections(&mut self, ui: &mut Ui) {
        if self.corrections.spots_window {
            let mut open = true;
            let ctx = ui.ctx().clone();
            let mut changed = false;
            egui::Window::new("Internal standard per spot").open(&mut open).resizable(true).default_size([440.0, 620.0]).show(&ctx, |ui| {
                changed = self.is_spot_grid(ui, f32::INFINITY);
            });
            self.changed |= changed;
            self.corrections.spots_window = open;
        }
        egui::Panel::left(crate::theme::layout_id("corr_settings"))
            .resizable(true).default_size(340.0).size_range(238.0..=850.0)
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                egui::ScrollArea::both().auto_shrink([false, false]).id_salt("corr_settings").show(ui, |ui| {
                    ui.set_width(ui.available_width() - 4.0);
                    self.settings_scheme(ui);
                    self.settings_baseline(ui);
                    // only the blocks of the chosen scheme
                    if self.session.settings.upb.enabled {
                        self.settings_upb(ui);
                        self.settings_common(ui);
                    }
                    if self.session.settings.te.enabled {
                        self.settings_te(ui);
                    }
                    self.settings_cutouts(ui);
                });
            });
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            let t = self.theme();
            let mut view = self.corrections.view;
            let (upb, te) = (self.session.settings.upb.enabled, self.session.settings.te.enabled);
            let mut options = Vec::new();
            if upb {
                options.push((CorrectionView::Downhole, "Down-hole fit"));
                options.push((CorrectionView::Through, "Reference through time"));
            }
            options.push((CorrectionView::Baseline, "Baseline"));
            if te {
                options.push((CorrectionView::Calibration, "Element calibration"));
            }
            if !options.iter().any(|(v, _)| *v == view) {
                view = options[0].0;
            }
            ui.horizontal_wrapped(|ui| {
                theme::segmented(ui, &t, &mut view, &options);
            });
            self.corrections.view = view;
            ui.add_space(4.0);
            match view {
                CorrectionView::Downhole => self.plot_downhole(ui),
                CorrectionView::Through => self.plot_through(ui),
                CorrectionView::Baseline => self.plot_baseline(ui),
                CorrectionView::Calibration => self.plot_calibration(ui),
            }
        });
    }

    fn interp_combo(ui: &mut Ui, id: &str, value: &mut InterpKind) -> bool {
        let mut changed = false;
        egui::ComboBox::from_id_salt(id).selected_text(value.label()).width(178.0).show_ui(ui, |ui| {
            for k in InterpKind::CHOICES {
                let same = std::mem::discriminant(value) == std::mem::discriminant(&k);
                if ui.selectable_label(same, k.label()).clicked() && !same {
                    *value = k;
                    changed = true;
                }
            }
        });
        if let InterpKind::SmoothFixed(l) = value {
            ui.horizontal(|ui| {
                ui.label("log λ");
                changed |= ui.add(egui::Slider::new(l, -8.0..=4.0).step_by(0.5)).changed();
            });
        }
        changed
    }

    fn settings_baseline(&mut self, ui: &mut Ui) {
        let t = self.theme();
        section(ui, &t, "Baseline and outliers", |ui| {
            if let Some(g) = self.session.groups.iter_mut().find(|g| g.kind == GroupKind::Baseline) {
                form_row(ui, &t, "Through time", |ui| {
                    self.changed |= Self::interp_combo(ui, "baseline_interp", &mut g.interp);
                });
            } else {
                ui.colored_label(t.warn, "No baseline group: signals are not background-corrected.");
            }
            form_row(ui, &t, "Outlier limit", |ui| {
                let r = ui.add(egui::DragValue::new(&mut self.session.settings.outlier_k).speed(0.1).range(0.0..=20.0).suffix(" robust SD"));
                self.changed |= r.changed();
            });
            ui.label(RichText::new("For every channel of every selection: sweeps further than this from the median, up or down, are left out. 0 keeps all.").color(t.dim).size(11.5));
        });
    }

    fn group_combo(ui: &mut Ui, id: &str, choice: &mut Option<u64>, groups: &[(u64, String)]) -> bool {
        let mut changed = false;
        let text = choice.and_then(|c| groups.iter().find(|g| g.0 == c)).map_or("Choose…", |g| g.1.as_str()).to_string();
        egui::ComboBox::from_id_salt(id).selected_text(text).width(178.0).show_ui(ui, |ui| {
            for (g, name) in groups {
                if ui.selectable_label(*choice == Some(*g), name).clicked() && *choice != Some(*g) {
                    *choice = Some(*g);
                    changed = true;
                }
            }
        });
        changed
    }

    fn settings_upb(&mut self, ui: &mut Ui) {
        let t = self.theme();
        let rm_groups: Vec<(u64, String)> = self
            .session
            .groups
            .iter()
            .filter(|g| g.kind == GroupKind::ReferenceMaterial)
            .map(|g| (g.id, g.name.clone()))
            .collect();
        let channels: Vec<String> = self.session.inputs.channels.iter().map(|c| c.name.clone()).collect();
        let mut changed = false;
        let has_log = !self.session.inputs.laser.is_empty();
        let primary_interp_group = self.session.settings.upb.primary_group;
        section(ui, &t, "U-Th-Pb", |ui| {
            let u = &mut self.session.settings.upb;
            form_row(ui, &t, "Primary reference", |ui| {
                changed |= Self::group_combo(ui, "upb_primary", &mut u.primary_group, &rm_groups);
            });
            form_row(ui, &t, "Beam seconds", |ui| {
                ui.add_enabled_ui(has_log, |ui| {
                    let mut laser = u.beam == BeamMethod::LaserLog;
                    if theme::segmented(ui, &t, &mut laser, &[(true, "Laser log"), (false, "Signal jump")]) {
                        u.beam = if laser { BeamMethod::LaserLog } else { BeamMethod::SignalJump };
                        changed = true;
                    }
                });
            });
            form_row(ui, &t, "Index channel", |ui| {
                let text = u.index_channel.clone().unwrap_or_default();
                egui::ComboBox::from_id_salt("index_channel").selected_text(text).width(178.0).show_ui(ui, |ui| {
                    for c in &channels {
                        if ui.selectable_label(u.index_channel.as_deref() == Some(c), c).clicked() {
                            u.index_channel = Some(c.clone());
                            changed = true;
                        }
                    }
                });
            });
            let model_row = |ui: &mut Ui, label: &str, id: &str, model: &mut DhfModel, changed: &mut bool| {
                form_row(ui, &t, label, |ui| {
                    egui::ComboBox::from_id_salt(id).selected_text(model.label()).width(178.0).show_ui(ui, |ui| {
                        for m in DhfModel::ALL {
                            if ui.selectable_label(*model == m, m.label()).clicked() && *model != m {
                                *model = m;
                                *changed = true;
                            }
                        }
                    });
                });
            };
            model_row(ui, "206Pb/238U fit", "dhf68", &mut u.dhf_r68, &mut changed);
            model_row(ui, "207Pb/235U fit", "dhf75", &mut u.dhf_r75, &mut changed);
            model_row(ui, "208Pb/232Th fit", "dhf82", &mut u.dhf_r82, &mut changed);
            form_row(ui, &t, "Leave out first", |ui| {
                changed |= ui.add(egui::DragValue::new(&mut u.dhf_trim_s).speed(0.1).range(0.0..=30.0).suffix(" s of beam")).changed();
            });
        });
        if let Some(group) = primary_interp_group {
            if let Some(g) = self.session.groups.iter_mut().find(|g| g.id == group) {
                section(ui, &t, "Reference through time", |ui| {
                    form_row(ui, &t, "Interpolation", |ui| {
                        changed |= Self::interp_combo(ui, "primary_interp", &mut g.interp);
                    });
                });
            }
        }
        self.changed |= changed;
        if let Some(model) = &self.red.upb {
            section(ui, &t, "Excess uncertainty (primary and secondary reference materials)", |ui| {
                egui::Grid::new("excess").num_columns(5).spacing([14.0, 3.0]).show(ui, |ui| {
                    for h in ["Ratio", "Excess, 1s", "Primary alone", "Primary MSWD", "Secondary analyses"] {
                        ui.label(RichText::new(h).color(t.dim).size(12.0));
                    }
                    ui.end_row();
                    for k in RatioKind::ALL {
                        if let Some(r) = model.ratio(k) {
                            ui.label(k.key());
                            ui.label(RichText::new(format!("{:.3} %", r.excess * 100.0)).strong());
                            ui.label(format!("{:.3} %", r.excess_primary * 100.0));
                            ui.label(format!("{:.2}", r.mswd_after));
                            ui.label(r.n_secondary.to_string());
                            ui.end_row();
                        }
                    }
                });
                let manual = RatioKind::ALL.iter().filter_map(|k| model.ratio(*k)).any(|r| r.manual);
                if manual {
                    ui.label(RichText::new("Manual values of the lab settings (ChemChron: Settings > Uncertainty models > U–Pb).").color(t.warn).size(12.0));
                }
                for k in RatioKind::ALL {
                    if let Some(r) = model.ratio(k) {
                        for (name, why) in &r.left_out {
                            ui.label(RichText::new(format!("{name} is left out of the {} excess: {why}.", k.key())).color(t.warn).size(12.0));
                        }
                    }
                }
                let n_sec = RatioKind::ALL.iter().filter_map(|k| model.ratio(*k)).map(|r| r.n_secondary).max().unwrap_or(0);
                ui.label(
                    RichText::new(format!(
                        "One excess per ratio for every analysis of the session: it brings the scatter of the primary ({} analyses, each against a curve fitted without it) and of the secondary reference materials ({}, each about its own mean) to MSWD 1. The 207Pb/206Pb of secondaries younger than 1000 Ma is left out (their Pb–Pb age is too imprecise); single ratios can also be left out by hand. Offsets of the secondaries from their accepted values are checked on the Quality page and enter the long-term systematic term. Set in ChemChron: Settings > Uncertainty models > U–Pb.",
                        model.primary_n,
                        if n_sec == 0 { "none in this session".to_string() } else { format!("{n_sec} analyses") }
                    ))
                    .color(t.dim)
                    .size(11.5),
                );
            });
        }
    }

    fn settings_common(&mut self, ui: &mut Ui) {
        let t = self.theme();
        let mut changed = false;
        let sample_groups: Vec<u64> = self.session.groups.iter().filter(|g| g.kind != GroupKind::Baseline).map(|g| g.id).collect();
        section(ui, &t, "Common Pb and 230Th", |ui| {
            let u = &mut self.session.settings.upb;
            form_row(ui, &t, "Common Pb", |ui| {
                egui::ComboBox::from_id_salt("common_method").selected_text(u.common_method.label()).width(178.0).show_ui(ui, |ui| {
                    for m in CommonPbMethod::ALL {
                        if ui.selectable_label(u.common_method == m, m.label()).clicked() && u.common_method != m {
                            u.common_method = m;
                            changed = true;
                        }
                    }
                });
            });
            if u.common_method != CommonPbMethod::None {
                let mut user = matches!(u.common_composition, CommonPbComposition::User(..));
                form_row(ui, &t, "Composition", |ui| {
                    if theme::segmented(ui, &t, &mut user, &[(false, "Stacey-Kramers"), (true, "My own")]) {
                        u.common_composition = if user {
                            CommonPbComposition::User(18.7, 15.628, 38.63)
                        } else {
                            CommonPbComposition::StaceyKramers
                        };
                        changed = true;
                    }
                });
                if let CommonPbComposition::User(a, b, c) = &mut u.common_composition {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("/204Pb:").color(t.dim));
                        for (name, v) in [("206", a), ("207", b), ("208", c)] {
                            ui.label(name);
                            changed |= ui.add(egui::DragValue::new(v).speed(0.01).max_decimals(3)).changed();
                        }
                    });
                }
            }
            changed |= ui.checkbox(&mut u.hg_correction, "Subtract 204Hg using the 202Hg channel").changed();
            changed |= ui.checkbox(&mut u.th230_enabled, "Correct 206Pb/238U for 230Th disequilibrium").changed();
        });
        if self.session.settings.upb.th230_enabled {
            section(ui, &t, "Th/U of the melt or fluid", |ui| {
                ui.label(RichText::new("Needed per group; groups left empty stay uncorrected.").color(t.dim).size(11.5));
                for id in sample_groups {
                    if let Some(g) = self.session.groups.iter_mut().find(|g| g.id == id) {
                        form_row(ui, &t, &g.name.clone(), |ui| {
                            let mut on = g.th_u_melt.is_some();
                            if ui.checkbox(&mut on, "").changed() {
                                g.th_u_melt = if on { Some(3.5) } else { None };
                                changed = true;
                            }
                            if let Some(v) = &mut g.th_u_melt {
                                changed |= ui.add(egui::DragValue::new(v).speed(0.05).range(0.01..=1000.0)).changed();
                            }
                        });
                    }
                }
            });
        }
        let embedded = self.embedded;
        section(ui, &t, "Decay constants", |ui| {
            let d = &mut self.session.settings.upb.decay;
            if embedded {
                ui.label(RichText::new("The lab's decay constants are used for every part (set them in Settings > Decay constants).").color(t.dim).size(11.5));
                egui::Grid::new("decay_grid").show(ui, |ui| {
                    for (label, v) in [("λ ²³⁸U (1/yr)", d.l238), ("λ ²³⁵U (1/yr)", d.l235), ("λ ²³²Th (1/yr)", d.l232)] {
                        ui.label(RichText::new(label).color(t.dim));
                        ui.label(format!("{v:.5e}"));
                        ui.end_row();
                    }
                    ui.label(RichText::new("²³⁸U/²³⁵U").color(t.dim));
                    ui.label(format!("{}", d.u85));
                    ui.end_row();
                    ui.label(RichText::new("λ ²³⁰Th (1/yr)").color(t.dim));
                    let speed = d.l230 * 1e-5;
                    changed |= ui
                        .add(egui::DragValue::new(&mut d.l230).speed(speed).custom_formatter(|x, _| format!("{x:.5e}")).custom_parser(|s| s.trim().parse::<f64>().ok()))
                        .on_hover_text("Cheng et al. (2013); only for the ²³⁰Th disequilibrium correction")
                        .changed();
                    ui.end_row();
                });
                return;
            }
            ui.label(RichText::new("Used for all ages of this session. Defaults: Steiger & Jäger (1977); ²³⁸U/²³⁵U Hiess et al. (2012); ²³⁰Th Cheng et al. (2013).").color(t.dim).size(11.5));
            egui::Grid::new("decay_grid").show(ui, |ui| {
                for (label, v, fmt) in [
                    ("λ ²³⁸U (1/yr)", &mut d.l238, 6usize),
                    ("λ ²³⁵U (1/yr)", &mut d.l235, 5),
                    ("λ ²³²Th (1/yr)", &mut d.l232, 5),
                    ("λ ²³⁰Th (1/yr)", &mut d.l230, 5),
                ] {
                    ui.label(RichText::new(label).color(t.dim));
                    let speed = *v * 1e-5;
                    changed |= ui
                        .add(egui::DragValue::new(v).speed(speed).custom_formatter(move |x, _| format!("{x:.fmt$e}")).custom_parser(|s| s.trim().parse::<f64>().ok()))
                        .changed();
                    ui.end_row();
                }
                ui.label(RichText::new("²³⁸U/²³⁵U").color(t.dim));
                changed |= ui.add(egui::DragValue::new(&mut d.u85).speed(0.001).max_decimals(4)).changed();
                ui.end_row();
            });
            if ui.button("Reset to defaults").clicked() {
                *d = chemchron_laser_core::geochron::DecayConstants::default();
                changed = true;
            }
        });
        self.changed |= changed;
    }

    fn settings_te(&mut self, ui: &mut Ui) {
        let t = self.theme();
        let rm_groups: Vec<(u64, String)> = self
            .session
            .groups
            .iter()
            .filter(|g| g.kind == GroupKind::ReferenceMaterial)
            .map(|g| (g.id, g.name.clone()))
            .collect();
        let channels: Vec<String> = self.session.inputs.channels.iter().map(|c| c.name.clone()).collect();
        let mut changed = false;
        let te_ids: Vec<u64> = self.session.groups.iter().filter(|g| g.kind != GroupKind::Baseline).map(|g| g.id).collect();
        section(ui, &t, "Trace elements", |ui| {
            let e = &mut self.session.settings.te;
            form_row(ui, &t, "Calibration ref.", |ui| {
                changed |= Self::group_combo(ui, "te_primary", &mut e.primary_group, &rm_groups);
            });
            changed |= ui.checkbox(&mut e.use_internal_standard, "Use an internal standard").on_hover_text("Off gives semi-quantitative values").changed();
            if e.use_internal_standard {
                form_row(ui, &t, "Internal standard", |ui| {
                    let text = e.is_channel.clone().unwrap_or_else(|| "Choose…".to_string());
                    egui::ComboBox::from_id_salt("is_channel").selected_text(text).width(178.0).show_ui(ui, |ui| {
                        for c in &channels {
                            if ui.selectable_label(e.is_channel.as_deref() == Some(c), c).clicked() {
                                e.is_channel = Some(c.clone());
                                changed = true;
                            }
                        }
                    });
                });
            }
            form_row(ui, &t, "Detection limit", |ui| {
                egui::ComboBox::from_id_salt("lod").selected_text(e.lod.label()).width(178.0).show_ui(ui, |ui| {
                    for m in LodMethod::ALL {
                        if ui.selectable_label(e.lod == m, m.label()).clicked() && e.lod != m {
                            e.lod = m;
                            changed = true;
                        }
                    }
                });
            });
            let mut known = e.dwell_s.is_some();
            form_row(ui, &t, "Dwell per channel", |ui| {
                if ui.checkbox(&mut known, "").on_hover_text("Off: estimated as sweep time / channels").changed() {
                    e.dwell_s = if known { Some(0.01) } else { None };
                    changed = true;
                }
                if let Some(v) = &mut e.dwell_s {
                    changed |= ui.add(egui::DragValue::new(v).speed(0.0005).range(1e-5..=1.0).suffix(" s")).changed();
                } else {
                    ui.label(RichText::new("estimated").color(t.dim));
                }
            });
        });
        if self.session.settings.te.use_internal_standard {
            let element = self.is_element();
            let (f, unit) = self.is_unit();
            section(ui, &t, &format!("{element} internal standard"), |ui| {
                let te = &mut self.session.settings.te;
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Typed as").color(t.dim));
                    changed |= ui.radio_value(&mut te.is_as_oxide, true, "oxide wt%").changed();
                    changed |= ui.radio_value(&mut te.is_as_oxide, false, "element ppm").changed();
                });
                if te.is_as_oxide {
                    let shown = if te.is_oxide.is_empty() { chemchron_laser_core::oxides::default_oxide(&element).unwrap_or("choose").to_string() } else { te.is_oxide.clone() };
                    form_row(ui, &t, "Oxide", |ui| {
                        egui::ComboBox::from_id_salt("is_oxide").selected_text(shown).width(120.0).show_ui(ui, |ui| {
                            for (o, el, _, _) in chemchron_laser_core::oxides::OXIDES.iter().filter(|x| x.1 == element || element.is_empty()) {
                                let _ = el;
                                if ui.selectable_label(te.is_oxide == *o, *o).clicked() {
                                    te.is_oxide = o.to_string();
                                    changed = true;
                                }
                            }
                        });
                    });
                }
                ui.label(RichText::new(format!("Values in {unit}. Reference materials use their accepted content unless you type one. Every spot can have its own value (list below, or on the Signal page); a group value applies to the spots without one.")).color(t.dim).size(11.5));
                ui.horizontal_wrapped(|ui| {
                    if self.embedded
                        && ui
                            .button("Take from EPMA (same label)")
                            .on_hover_text("Each spot gets the oxide content of the EPMA analysis with exactly the same label")
                            .clicked()
                    {
                        self.request_is_from_epma = true;
                    }
                    if ui.button("Import a table…").on_hover_text(format!("A CSV file: spot label, value in {unit}; labels must match exactly")).clicked() {
                        if let Some(path) = rfd::FileDialog::new().add_filter("CSV", &["csv", "txt"]).pick_file() {
                            let result = std::fs::read_to_string(&path)
                                .map_err(|e| e.to_string())
                                .and_then(|text| chemchron_laser_core::project::apply_is_table_in(&mut self.session, &text, f));
                            match result {
                                Ok((count, unmatched)) => {
                                    self.changed = true;
                                    if unmatched.is_empty() {
                                        self.say(format!("Set the internal standard on {count} spots."), false);
                                    } else {
                                        self.say(format!("Set {count} spots. No spot is called exactly: {}", unmatched.join(", ")), true);
                                    }
                                }
                                Err(e) => self.say(e, true),
                            }
                        }
                    }
                });
                ui.add_space(4.0);
                ui.label(RichText::new("Per group").strong());
                for id in te_ids.iter().copied() {
                    let rm_value = self.session.rm_of(id).and_then(|m| m.get(&element)).map(|v| v.value);
                    if let Some(g) = self.session.groups.iter_mut().find(|g| g.id == id) {
                        form_row(ui, &t, &g.name.clone(), |ui| {
                            let mut on = g.is_ppm.is_some();
                            if ui.checkbox(&mut on, "").changed() {
                                g.is_ppm = if on { Some(rm_value.unwrap_or(100000.0)) } else { None };
                                changed = true;
                            }
                            if let Some(v) = &mut g.is_ppm {
                                let mut shown = *v / f;
                                let sp = shown.abs().max(1e-3) * 1e-3;
                                if ui.add(egui::DragValue::new(&mut shown).speed(sp).max_decimals(4).range(0.0..=1e6)).changed() {
                                    *v = shown * f;
                                    changed = true;
                                }
                            } else if let Some(v) = rm_value {
                                ui.label(RichText::new(format!("{} accepted", fmt_sig(v / f, 5))).color(t.dim));
                            } else {
                                ui.label(RichText::new("not set").color(t.warn));
                            }
                        });
                    }
                }
                ui.add_space(4.0);
                let sample_groups: Vec<u64> = self.session.groups.iter().filter(|g| g.kind == GroupKind::Sample).map(|g| g.id).collect();
                let n_spots = self.session.selections.iter().filter(|s| sample_groups.contains(&s.group)).count();
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Per spot ({n_spots} sample spots)")).strong());
                    if ui.button("Open the list in a window").on_hover_text("A large window you can move and resize").clicked() {
                        self.corrections.spots_window = true;
                    }
                });
                egui::CollapsingHeader::new("Show the list here").id_salt("is_per_spot").default_open(false).show(ui, |ui| {
                    changed |= self.is_spot_grid(ui, 320.0);
                });
            });
        }
        self.changed |= changed;
    }

    fn ratio_picker(&mut self, ui: &mut Ui, t: &Theme, include_76: bool) {
        let mut ratio = self.corrections.ratio;
        let mut options = vec![(RatioKind::R68, "206Pb/238U"), (RatioKind::R75, "207Pb/235U")];
        if include_76 {
            options.push((RatioKind::R76, "207Pb/206Pb"));
        }
        options.push((RatioKind::R82, "208Pb/232Th"));
        if !include_76 && ratio == RatioKind::R76 {
            ratio = RatioKind::R68;
        }
        theme::segmented(ui, t, &mut ratio, &options);
        self.corrections.ratio = ratio;
    }

    fn plot_downhole(&mut self, ui: &mut Ui) {
        let t = self.theme();
        self.ratio_picker(ui, &t, false);
        let Some(model) = &self.red.upb else {
            ui.label(RichText::new("Set a primary reference material to see the down-hole fit.").color(t.dim));
            return;
        };
        let Some(r) = model.ratio(self.corrections.ratio) else {
            ui.label(RichText::new("This ratio has no accepted value on the primary reference, so it isn't corrected.").color(t.dim));
            return;
        };
        let Some(fit) = r.dhf.clone() else {
            ui.label(RichText::new("Too few sweeps to fit a down-hole curve.").color(t.dim));
            return;
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Model: {}", fit.model.label())).strong());
            if fit.suggested != fit.model {
                tag(ui, &format!("This data prefers: {}", fit.suggested.label()), t.warn);
                if ui.small_button("Use it").clicked() {
                    let u = &mut self.session.settings.upb;
                    match self.corrections.ratio {
                        RatioKind::R68 => u.dhf_r68 = fit.suggested,
                        RatioKind::R75 => u.dhf_r75 = fit.suggested,
                        RatioKind::R82 => u.dhf_r82 = fit.suggested,
                        RatioKind::R76 => {}
                    }
                    self.changed = true;
                }
            }
        });
        if fit.bins.is_empty() {
            return;
        }
        let total = ui.available_height();
        let width = ui.available_width();
        let x = plot::pad(0.0, fit.bins.last().map_or(1.0, |b| b.0), 0.04);
        // the curve is drawn over the fitted beam seconds; the range holds it and the points
        let t0 = fit.bins.first().map_or(0.0, |b| b.0);
        let t1 = fit.bins.last().map_or(1.0, |b| b.0);
        let curve: Vec<(f64, f64)> = (0..=200).map(|i| t0 + (t1 - t0) * i as f64 / 200.0).map(|tau| (tau, fit.fitted(tau))).filter(|p| p.1.is_finite()).collect();
        let lo = fit.bins.iter().map(|b| b.1).chain(curve.iter().map(|p| p.1)).fold(f64::INFINITY, f64::min);
        let hi = fit.bins.iter().map(|b| b.1).chain(curve.iter().map(|p| p.1)).fold(f64::NEG_INFINITY, f64::max);
        let (response, painter, rect) = plot::allocate(ui, egui::vec2(width, total * 0.66));
        let f = Frame2 { rect, x, y: plot::pad(lo, hi, 0.12), log_y: false };
        plot::axes(&painter, &t, &f, "Beam seconds", "Ratio relative to its analysis mean");
        let clip = painter.with_clip_rect(rect);
        for (tau, v, _) in &fit.bins {
            clip.circle_filled(f.pos(*tau, *v), 3.0, series_color(0));
        }
        let points: Vec<Pos2> = curve.iter().map(|(tau, v)| f.pos(*tau, *v)).collect();
        clip.line(points, Stroke::new(2.0, t.curve));
        if let Some(p) = response.hover_pos() {
            if rect.contains(p) {
                let tau = f.x_at(p.x);
                plot::hover_box(&painter, &t, rect, p, &[
                    format!("Beam time {tau:.1} s"),
                    format!("Fit {}", fmt_sig(fit.fitted(tau), 4)),
                    format!("Divides the raw ratio by {}", fmt_sig(fit.factor(tau), 4)),
                ]);
            }
        }
        // Residuals.
        let residuals: Vec<(f64, f64)> = fit.bins.iter().map(|b| (b.0, 100.0 * (b.1 / fit.fitted(b.0) - 1.0))).collect();
        let reach = residuals.iter().map(|r| r.1.abs()).fold(0.1, f64::max);
        let (_, painter2, rect2) = plot::allocate(ui, egui::vec2(width, (total * 0.34 - 8.0).max(90.0)));
        let g = Frame2 { rect: rect2, x, y: (-reach * 1.2, reach * 1.2), log_y: false };
        plot::axes(&painter2, &t, &g, "Beam seconds", "Residual (%)");
        let clip2 = painter2.with_clip_rect(rect2);
        clip2.line_segment([g.pos(g.x.0, 0.0), g.pos(g.x.1, 0.0)], Stroke::new(1.0, t.curve));
        for (tau, v) in &residuals {
            clip2.circle_filled(g.pos(*tau, *v), 2.5, series_color(0));
        }
    }

    /// Plot a curve through time, with the analyses that made it. Clicking an analysis leaves it out or in.
    fn plot_curve(&mut self, ui: &mut Ui, curve: &Curve, y_label: &str, ids: &[(f64, u64)], reference: Option<f64>) {
        let t = self.theme();
        let height = ui.available_height();
        let width = ui.available_width();
        let (response, painter, rect) = plot::allocate(ui, egui::vec2(width, height - 22.0));
        let times = &self.session.inputs.time;
        let (x0, x1) = (times.first().copied().unwrap_or(0.0), times.last().copied().unwrap_or(1.0));
        let n = curve.xs.len();
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for i in 0..n {
            lo = lo.min(curve.ys[i] - 2.0 * curve.sigmas[i]);
            hi = hi.max(curve.ys[i] + 2.0 * curve.sigmas[i]);
        }
        let f = Frame2 { rect, x: plot::pad(x0, x1, 0.02), y: plot::pad(lo, hi, 0.1), log_y: false };
        plot::axes(&painter, &t, &f, "Time (s)", y_label);
        let clip = painter.with_clip_rect(rect);
        if let Some(v) = reference {
            clip.line_segment([f.pos(f.x.0, v), f.pos(f.x.1, v)], Stroke::new(1.0, t.dim));
        }
        for i in 0..n {
            let (x, y, s) = (curve.xs[i], curve.ys[i], 2.0 * curve.sigmas[i]);
            clip.line_segment([f.pos(x, y - s), f.pos(x, y + s)], Stroke::new(1.0, series_color(0).linear_multiply(0.7)));
            clip.circle_filled(f.pos(x, y), 3.5, series_color(0));
        }
        let segments = (rect.width() / 2.0).max(10.0) as usize;
        let points: Vec<Pos2> = (0..=segments)
            .map(|i| {
                let x = f.x.0 + (f.x.1 - f.x.0) * i as f64 / segments as f64;
                f.pos(x, curve.eval(x))
            })
            .collect();
        clip.line(points, Stroke::new(2.0, t.curve));
        let nearest = response.hover_pos().and_then(|p| {
            (0..n)
                .map(|i| (i, f.pos(curve.xs[i], curve.ys[i]).distance(p)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .filter(|(_, d)| *d < 10.0)
                .map(|(i, _)| i)
        });
        if let Some(i) = nearest {
            clip.circle_stroke(f.pos(curve.xs[i], curve.ys[i]), 6.0, Stroke::new(1.5, t.accent));
            let label = ids
                .iter()
                .find(|(x, _)| (x - curve.xs[i]).abs() < 1e-6)
                .and_then(|(_, id)| self.session.selections.iter().find(|s| s.id == *id))
                .map_or(String::new(), |s| s.label.clone());
            plot::hover_box(&painter, &t, rect, f.pos(curve.xs[i], curve.ys[i]), &[
                label,
                format!("{} ± {}", fmt_sig(curve.ys[i], 5), fmt_sig(2.0 * curve.sigmas[i], 2)),
                "Click to leave this analysis out".to_string(),
            ]);
            if response.clicked() {
                if let Some((_, id)) = ids.iter().find(|(x, _)| (x - curve.xs[i]).abs() < 1e-6) {
                    if let Some(s) = self.session.selections.iter_mut().find(|s| s.id == *id) {
                        s.excluded = true;
                        let label = s.label.clone();
                        self.changed = true;
                        self.say(format!("Left out {label}. Undo brings it back."), false);
                    }
                }
            }
        }
        ui.label(RichText::new(format!(
            "{}  ·  {} points  ·  points show 2 standard errors{}",
            curve.kind.label(),
            n,
            curve.log_lambda.map_or(String::new(), |l| format!("  ·  chosen log λ = {l:.1}"))
        )).color(t.dim).size(11.5));
    }

    fn selection_ids_at_mid(&self, group: u64) -> Vec<(f64, u64)> {
        self.session
            .selections
            .iter()
            .filter(|s| s.group == group && !s.excluded)
            .map(|s| (0.5 * (s.start + s.end), s.id))
            .collect()
    }

    fn plot_through(&mut self, ui: &mut Ui) {
        let t = self.theme();
        self.ratio_picker(ui, &t, true);
        let Some(model) = self.red.upb.clone() else {
            ui.label(RichText::new("Set a primary reference material to see it through time.").color(t.dim));
            return;
        };
        let Some(r) = model.ratio(self.corrections.ratio) else {
            ui.label(RichText::new("This ratio has no accepted value on the primary reference.").color(t.dim));
            return;
        };
        let ids = self.selection_ids_at_mid(model.primary_group);
        let label = format!("{} after down-hole correction", r.kind.key());
        self.plot_curve(ui, &r.curve, &label, &ids, None);
    }

    fn plot_baseline(&mut self, ui: &mut Ui) {
        let t = self.theme();
        let channels: Vec<String> = self.session.inputs.channels.iter().map(|c| c.name.clone()).collect();
        if channels.is_empty() {
            return;
        }
        let mut c = self.corrections.channel.unwrap_or(0).min(channels.len() - 1);
        egui::ComboBox::from_id_salt("baseline_channel").selected_text(&channels[c]).show_ui(ui, |ui| {
            for (i, n) in channels.iter().enumerate() {
                ui.selectable_value(&mut c, i, n);
            }
        });
        self.corrections.channel = Some(c);
        let Some(curve) = self.red.baseline.get(c).and_then(|b| b.clone()) else {
            ui.label(RichText::new("No baseline selections to fit.").color(t.dim));
            return;
        };
        let group = self.session.groups.iter().find(|g| g.kind == GroupKind::Baseline).map(|g| g.id);
        let ids = group.map_or_else(Vec::new, |g| self.selection_ids_at_mid(g));
        let label = format!("{} baseline (CPS)", channels[c]);
        self.plot_curve(ui, &curve, &label, &ids, None);
    }

    fn plot_calibration(&mut self, ui: &mut Ui) {
        let t = self.theme();
        let Some(te) = self.red.te.clone() else {
            ui.label(RichText::new("Set a calibration reference material to see it here.").color(t.dim));
            return;
        };
        let channels: Vec<(usize, String)> = te
            .calibrations
            .iter()
            .enumerate()
            .filter(|(_, c)| c.is_some())
            .map(|(i, _)| (i, self.session.inputs.channels[i].name.clone()))
            .collect();
        if channels.is_empty() {
            ui.label(RichText::new("No channel has an accepted value on the calibration reference.").color(t.dim));
            return;
        }
        let mut c = self.corrections.te_channel.filter(|v| channels.iter().any(|x| x.0 == *v)).unwrap_or(channels[0].0);
        let name = channels.iter().find(|x| x.0 == c).map_or("", |x| x.1.as_str()).to_string();
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("te_channel").selected_text(name.clone()).show_ui(ui, |ui| {
                for (i, n) in &channels {
                    ui.selectable_value(&mut c, *i, n);
                }
            });
            if let Some(cal) = &te.calibrations[c] {
                ui.label(RichText::new(format!("Excess {:.2} %", cal.excess * 100.0)).color(t.dim));
            }
            ui.separator();
            ui.label(RichText::new("Calibrate against").color(t.dim));
            let refs: Vec<(u64, String)> = self
                .session
                .groups
                .iter()
                .filter(|g| g.kind == GroupKind::ReferenceMaterial)
                .map(|g| (g.id, g.name.clone()))
                .collect();
            let current = self.session.settings.te.channel_group.get(&name).copied().or(self.session.settings.te.primary_group);
            let text = current.and_then(|g| refs.iter().find(|r| r.0 == g)).map_or("–", |r| r.1.as_str()).to_string();
            egui::ComboBox::from_id_salt("te_channel_group").selected_text(text).show_ui(ui, |ui| {
                for (id, group) in &refs {
                    if ui.selectable_label(current == Some(*id), group).clicked() && current != Some(*id) {
                        self.session.settings.te.channel_group.insert(name.clone(), *id);
                        self.changed = true;
                    }
                }
            });
        });
        self.corrections.te_channel = Some(c);
        if let Some(cal) = te.calibrations[c].clone() {
            let label = if te.is_channel.is_some() {
                format!("{name} sensitivity relative to the internal standard")
            } else {
                format!("{name} sensitivity (CPS per ppm)")
            };
            let ids = self.selection_ids_at_mid(cal.group);
            self.plot_curve(ui, &cal.curve, &label, &ids, None);
        }
    }
}
