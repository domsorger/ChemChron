// SPDX-License-Identifier: GPL-3.0-or-later
//! Results: one row per analysis, and a step-by-step view of how any row was calculated.

use crate::app::TraceApp;
use crate::theme::{self, fmt_pm, fmt_sig, group_color, tag};
use egui::{RichText, Ui};
use egui_extras::{Column, TableBuilder};
use chemchron_laser_core::drs::te::TeValue;
use chemchron_laser_core::drs::upb::{AgeValue, RatioValue};
use chemchron_laser_core::export;
use chemchron_laser_core::model::*;

#[derive(Clone, Copy, PartialEq)]
pub enum ResultsView {
    Ages,
    Ratios,
    Elements,
}

/// Uncertainty levels: analytical (the sweeps of the analysis), analytical +
/// excess (the session's excess from its reference materials), total (also
/// the primary's accepted value and the lab's systematic term).
#[derive(Clone, Copy, PartialEq)]
pub enum Uncertainty {
    Analytical,
    WithExcess,
    Total,
}

impl Uncertainty {
    pub const CHOICES: [(Uncertainty, &'static str); 3] = [(Uncertainty::Analytical, "Analytical"), (Uncertainty::WithExcess, "Analytical + excess"), (Uncertainty::Total, "Total")];
    /// `sys_pct`: the lab's systematic term (2σ %) for this age type.
    fn of_age(&self, a: &AgeValue, sys_pct: f64) -> f64 {
        match self {
            Uncertainty::Analytical => a.int2s,
            Uncertainty::WithExcess => a.prop2s,
            Uncertainty::Total => a.rminc2s.hypot(sys_pct / 100.0 * a.age),
        }
    }
    fn of_ratio(&self, r: &RatioValue, sys_pct: f64) -> f64 {
        match self {
            Uncertainty::Analytical => r.int2se,
            Uncertainty::WithExcess => r.prop2s,
            Uncertainty::Total => r.rminc2s.hypot(sys_pct / 100.0 * r.mean),
        }
    }
    fn of_te(&self, v: &TeValue) -> f64 {
        match self {
            Uncertainty::Analytical => v.int2se,
            Uncertainty::WithExcess => v.prop2s,
            Uncertainty::Total => v.rminc2s,
        }
    }
}

impl TraceApp {
    /// The lab's systematic term (2σ %) for age type 0..4 (206/238, 207/235, 207/206, 208/232).
    pub fn sys_pct(&self, i: usize) -> f64 {
        self.lab_systematic.as_ref().map_or(0.0, |l| l.pct[i])
    }
}

/// A number to fixed decimals, or a dash when there is none.
fn fixed(v: f64, decimals: usize) -> String {
    if v.is_finite() {
        format!("{v:.decimals$}")
    } else {
        "–".to_string()
    }
}

pub struct ResultsState {
    pub view: ResultsView,
    pub uncertainty: Uncertainty,
    pub search: String,
    pub group: Option<u64>,
    pub flagged_only: bool,
    pub sort: Option<(usize, bool)>,
    pub jump_to: Option<u64>,
}

impl Default for ResultsState {
    fn default() -> Self {
        ResultsState {
            view: ResultsView::Ages,
            uncertainty: Uncertainty::WithExcess,
            search: String::new(),
            group: None,
            flagged_only: false,
            sort: None,
            jump_to: None,
        }
    }
}

impl TraceApp {
    pub fn page_results(&mut self, ui: &mut Ui) {
        let t = self.theme();
        if let Some(id) = self.results.jump_to.take() {
            self.selected = Some(id);
            self.results.group = None;
            self.results.flagged_only = false;
            self.results.search.clear();
        }
        ui.horizontal_wrapped(|ui| {
            let mut view = self.results.view;
            let mut options = Vec::new();
            if self.red.upb.is_some() {
                options.push((ResultsView::Ages, "Ages"));
                options.push((ResultsView::Ratios, "Ratios"));
            }
            if self.red.te.is_some() {
                options.push((ResultsView::Elements, "Elements"));
            }
            theme::segmented(ui, &t, &mut view, &options);
            if view != self.results.view {
                self.results.view = view;
                self.results.sort = None;
            }
            ui.separator();
            ui.label(RichText::new("Uncertainty").color(t.dim));
            let mut u = self.results.uncertainty;
            theme::segmented(ui, &t, &mut u, &Uncertainty::CHOICES);
            ui.label(RichText::new("(?)").color(t.dim)).on_hover_text("Analytical: the sweeps of the analysis. Analytical + excess: with the session's excess from the primary and secondary reference materials. Total: also the primary's accepted-value uncertainty and the lab's systematic term (long-term log), as used for means in Combined results.");
            self.results.uncertainty = u;
            ui.separator();
            let groups: Vec<(u64, String)> = self.session.groups.iter().filter(|g| g.kind != GroupKind::Baseline).map(|g| (g.id, g.name.clone())).collect();
            let text = self.results.group.and_then(|c| groups.iter().find(|g| g.0 == c)).map_or("All groups", |g| g.1.as_str()).to_string();
            egui::ComboBox::from_id_salt("results_group").selected_text(text).width(110.0).show_ui(ui, |ui| {
                if ui.selectable_label(self.results.group.is_none(), "All groups").clicked() {
                    self.results.group = None;
                }
                for (id, name) in &groups {
                    if ui.selectable_label(self.results.group == Some(*id), name).clicked() {
                        self.results.group = Some(*id);
                    }
                }
            });
            ui.add(egui::TextEdit::singleline(&mut self.results.search).hint_text("Search labels").desired_width(110.0));
            ui.checkbox(&mut self.results.flagged_only, "Flagged only");
            if ui.button("Copy table").on_hover_text("Copy what is shown, ready to paste into a spreadsheet").clicked() {
                let text = self.table_as_text();
                ui.ctx().copy_text(text);
                self.say("Copied the table.", false);
            }
        });
        ui.add_space(4.0);
        // The table takes what the step-by-step panel leaves, and scrolls inside that.
        let width = ui.available_width();
        let trace_height = 244.0;
        let table_height = (ui.available_height() - trace_height - 8.0).max(120.0);
        ui.allocate_ui_with_layout(egui::vec2(width, table_height), egui::Layout::top_down(egui::Align::Min), |ui| {
            t.card().inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                ui.set_width(width - 14.0);
                ui.set_height(table_height - 14.0);
                match self.results.view {
                    ResultsView::Elements => self.elements_table(ui),
                    _ => self.upb_table(ui),
                }
            });
        });
        ui.add_space(8.0);
        ui.allocate_ui_with_layout(egui::vec2(width, trace_height), egui::Layout::top_down(egui::Align::Min), |ui| {
            self.trace_panel(ui);
        });
    }

    /// Selections shown with the current filters, in the current order.
    fn visible_rows(&self, sort_value: impl Fn(usize) -> f64) -> Vec<usize> {
        let search = self.results.search.to_lowercase();
        let mut rows: Vec<usize> = self
            .session
            .selections
            .iter()
            .enumerate()
            .filter(|(i, s)| {
                let Some(g) = self.session.group_of(s) else { return false };
                if g.kind == GroupKind::Baseline {
                    return false;
                }
                if self.results.group.is_some_and(|f| f != s.group) {
                    return false;
                }
                if !search.is_empty() && !s.label.to_lowercase().contains(&search) {
                    return false;
                }
                !self.results.flagged_only || !self.red.selections[*i].flags.is_empty()
            })
            .map(|(i, _)| i)
            .collect();
        if let Some((_, ascending)) = self.results.sort {
            rows.sort_by(|a, b| {
                let (x, y) = (sort_value(*a), sort_value(*b));
                // Rows without a value stay at the end either way.
                match (x.is_nan(), y.is_nan()) {
                    (true, true) => std::cmp::Ordering::Equal,
                    (true, false) => std::cmp::Ordering::Greater,
                    (false, true) => std::cmp::Ordering::Less,
                    _ => if ascending { x.total_cmp(&y) } else { y.total_cmp(&x) },
                }
            });
        }
        rows
    }

    fn upb_columns(&self) -> Vec<&'static str> {
        match self.results.view {
            ResultsView::Ratios => vec!["206Pb/238U", "207Pb/235U", "207Pb/206Pb", "208Pb/232Th", "rho 6/8 v 7/5", "rho 7/6 v 8/6", "238U/206Pb"],
            _ => vec!["206/238 age (Ma)", "207/235 age (Ma)", "207/206 age (Ma)", "208/232 age (Ma)", "Disc. %", "Th/U", "f206 %"],
        }
    }

    fn upb_cell_number(&self, index: usize, column: usize) -> f64 {
        let Some(u) = self.red.selections[index].upb.as_ref() else { return f64::NAN };
        match self.results.view {
            ResultsView::Ratios => match column {
                0 => u.r68.mean,
                1 => u.r75.mean,
                2 => u.r76.mean,
                3 => u.r82.as_ref().map_or(f64::NAN, |r| r.mean),
                4 => u.rho_68_75,
                5 => u.rho_76_38,
                _ => u.r38,
            },
            _ => match column {
                0 => u.a68.age,
                1 => u.a75.age,
                2 => u.a76.age,
                3 => u.a82.map_or(f64::NAN, |a| a.age),
                4 => u.discordance_pct,
                5 => u.th_u,
                _ => u.f206_pct_204,
            },
        }
    }

    fn upb_cell_text(&self, index: usize, column: usize) -> String {
        let Some(u) = self.red.selections[index].upb.as_ref() else { return "–".to_string() };
        let m = self.results.uncertainty;
        match self.results.view {
            ResultsView::Ratios => match column {
                0 => fmt_pm(u.r68.mean, m.of_ratio(&u.r68, self.sys_pct(0))),
                1 => fmt_pm(u.r75.mean, m.of_ratio(&u.r75, self.sys_pct(1))),
                2 => fmt_pm(u.r76.mean, m.of_ratio(&u.r76, self.sys_pct(2))),
                3 => u.r82.as_ref().map_or("–".to_string(), |r| fmt_pm(r.mean, m.of_ratio(r, self.sys_pct(3)))),
                4 => fixed(u.rho_68_75, 2),
                5 => fixed(u.rho_76_38, 2),
                _ => fmt_pm(u.r38, u.r38_prop2s),
            },
            _ => match column {
                0 => fmt_pm(u.a68.age, m.of_age(&u.a68, self.sys_pct(0))),
                1 => fmt_pm(u.a75.age, m.of_age(&u.a75, self.sys_pct(1))),
                2 => fmt_pm(u.a76.age, m.of_age(&u.a76, self.sys_pct(2))),
                3 => u.a82.as_ref().map_or("–".to_string(), |a| fmt_pm(a.age, m.of_age(a, self.sys_pct(3)))),
                4 => fixed(u.discordance_pct, 1),
                5 => fmt_sig(u.th_u, 3),
                _ => fixed(u.f206_pct_204, 2),
            },
        }
    }

    fn header_cell(&mut self, ui: &mut Ui, title: &str, column: usize) {
        let arrow = match self.results.sort {
            Some((c, true)) if c == column => " ↑",
            Some((c, false)) if c == column => " ↓",
            _ => "",
        };
        let label = egui::Label::new(RichText::new(format!("{title}{arrow}")).strong().size(12.5))
            .sense(egui::Sense::click())
            .truncate();
        if ui.add(label).on_hover_text("Click to sort").clicked() {
            self.results.sort = match self.results.sort {
                Some((c, true)) if c == column => Some((c, false)),
                Some((c, false)) if c == column => None,
                _ => Some((column, true)),
            };
        }
    }

    fn upb_table(&mut self, ui: &mut Ui) {
        let t = self.theme();
        if self.red.upb.is_none() {
            ui.label(RichText::new("No ages yet. Check the Corrections page.").color(t.dim));
            return;
        }
        let columns = self.upb_columns();
        let sort_column = self.results.sort.map(|s| s.0);
        let rows = self.visible_rows(|i| sort_column.map_or(f64::NAN, |c| self.upb_cell_number(i, c)));
        let info: Vec<(u64, String, String, [u8; 3], bool)> = rows
            .iter()
            .map(|i| {
                let s = &self.session.selections[*i];
                let g = self.session.group_of(s);
                (s.id, s.label.clone(), g.map_or(String::new(), |g| g.name.clone()), g.map_or([120, 120, 120], |g| g.color), s.excluded)
            })
            .collect();
        let cells: Vec<Vec<String>> = rows.iter().map(|i| (0..columns.len()).map(|c| self.upb_cell_text(*i, c)).collect()).collect();
        let flags: Vec<Vec<String>> = rows.iter().map(|i| self.red.selections[*i].flags.clone()).collect();
        let mut clicked = None;
        egui::ScrollArea::horizontal().id_salt("upb_h").show(ui, |ui| {
        let mut builder = TableBuilder::new(ui)
            .id_salt(("upb_table", self.results.view == ResultsView::Ratios))
            .striped(true)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .auto_shrink(false)
            .column(Column::initial(118.0).clip(true).resizable(true))
            .column(Column::initial(92.0).clip(true).resizable(true));
        for _ in &columns {
            builder = builder.column(Column::initial(122.0).clip(true).resizable(true));
        }
        builder = builder.column(Column::remainder().at_least(120.0).clip(true));
        let header_titles = columns.clone();
        builder
            .header(22.0, |mut h| {
                h.col(|ui| { ui.label(RichText::new("Analysis").strong().size(12.5)); });
                h.col(|ui| { ui.label(RichText::new("Group").strong().size(12.5)); });
                for (c, title) in header_titles.iter().enumerate() {
                    h.col(|ui| self.header_cell(ui, title, c));
                }
                h.col(|ui| { ui.label(RichText::new("Checks").strong().size(12.5)); });
            })
            .body(|body| {
                body.rows(22.0, rows.len(), |mut row| {
                    let i = row.index();
                    let (id, label, group, color, excluded) = &info[i];
                    row.set_selected(self.selected == Some(*id));
                    let dim = *excluded;
                    let cell = |ui: &mut Ui, text: &str| {
                        ui.add(egui::Label::new(RichText::new(text).size(12.5).color(if dim { t.dim } else { t.text })).selectable(false).truncate());
                    };
                    row.col(|ui| cell(ui, label));
                    row.col(|ui| {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(9.0, 9.0), egui::Sense::hover());
                        ui.painter().rect_filled(r, 2, group_color(*color));
                        cell(ui, group);
                    });
                    for text in &cells[i] {
                        row.col(|ui| cell(ui, text));
                    }
                    row.col(|ui| {
                        if *excluded {
                            ui.label(RichText::new("Left out").color(t.dim).size(12.0));
                        } else if flags[i].is_empty() {
                            ui.label(RichText::new("OK").color(t.ok).size(12.0));
                        } else {
                            let text = flags[i].join(", ");
                            ui.add(egui::Label::new(RichText::new(&text).color(t.warn).size(12.0)).selectable(false).truncate())
                                .on_hover_text(&text);
                        }
                    });
                    if row.response().clicked() {
                        clicked = Some(*id);
                    }
                });
            });
        });
        if let Some(id) = clicked {
            self.selected = Some(id);
        }
    }

    fn elements_table(&mut self, ui: &mut Ui) {
        let t = self.theme();
        let Some(te) = self.red.te.clone() else {
            ui.label(RichText::new("No concentrations yet. Check the Corrections page.").color(t.dim));
            return;
        };
        let channels: Vec<usize> = te.calibrations.iter().enumerate().filter(|(_, c)| c.is_some()).map(|(i, _)| i).collect();
        let sort_column = self.results.sort.map(|s| s.0);
        let number = |this: &TraceApp, i: usize, c: usize| -> f64 {
            this.red.selections[i].te.get(channels[c]).and_then(|v| v.as_ref()).map_or(f64::NAN, |v| v.ppm)
        };
        let rows = self.visible_rows(|i| sort_column.map_or(f64::NAN, |c| number(self, i, c)));
        let names: Vec<String> = channels.iter().map(|c| self.session.inputs.channels[*c].name.clone()).collect();
        let mut clicked = None;
        let lod = self.session.settings.te.lod;
        let m = self.results.uncertainty;
        let semi = te.is_channel.is_none();
        let (dim, warn) = (t.dim, t.warn);
        if semi {
            tag(ui, "Semi-quantitative: no internal standard", t.warn);
        }
        let selections = self.session.selections.clone();
        let results = self.red.selections.clone();
        let selected = self.selected;
        egui::ScrollArea::horizontal().id_salt("te_h").show(ui, |ui| {
            let mut builder = TableBuilder::new(ui)
                .id_salt("te_table")
                .striped(true)
                .sense(egui::Sense::click())
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .auto_shrink(false)
                .column(Column::exact(118.0).clip(true));
            for _ in &names {
                builder = builder.column(Column::exact(118.0).clip(true));
            }
            builder
                .header(22.0, |mut h| {
                    h.col(|ui| { ui.label(RichText::new("Analysis").strong().size(12.5)); });
                    for (c, n) in names.iter().enumerate() {
                        h.col(|ui| self.header_cell(ui, &format!("{n} (ppm)"), c));
                    }
                })
                .body(|body| {
                    body.rows(22.0, rows.len(), |mut row| {
                        let index = rows[row.index()];
                        let s = &selections[index];
                        row.set_selected(selected == Some(s.id));
                        row.col(|ui| {
                            ui.add(egui::Label::new(RichText::new(&s.label).size(12.5)).selectable(false).truncate());
                        });
                        for c in &channels {
                            row.col(|ui| match results[index].te.get(*c).and_then(|v| v.as_ref()) {
                                Some(v) if v.below_lod => {
                                    ui.add(egui::Label::new(RichText::new(format!("< LOD ({})", fmt_sig(v.lod(lod), 2))).size(12.5).color(warn)).selectable(false).truncate());
                                }
                                Some(v) => {
                                    ui.add(egui::Label::new(RichText::new(fmt_pm(v.ppm, m.of_te(v))).size(12.5)).selectable(false).truncate());
                                }
                                None => {
                                    ui.add(egui::Label::new(RichText::new("–").color(dim)).selectable(false));
                                }
                            });
                        }
                        if row.response().clicked() {
                            clicked = Some(s.id);
                        }
                    });
                });
        });
        if let Some(id) = clicked {
            self.selected = Some(id);
        }
    }

    /// The step-by-step route from raw sweeps to the final value of one analysis.
    fn trace_panel(&mut self, ui: &mut Ui) {
        let t = self.theme();
        t.card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(ui.available_height());
            let Some(id) = self.selected else {
                ui.label(RichText::new("How was this calculated?").strong());
                ui.label(RichText::new("Click a row to follow its result from raw sweeps to the final value, step by step.").color(t.dim));
                return;
            };
            let Some(index) = self.session.selections.iter().position(|s| s.id == id) else { return };
            let s = self.session.selections[index].clone();
            let r = self.red.selections[index].clone();
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("How {} was calculated", s.label)).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Export its sweeps…").on_hover_text("Every sweep at every step, for checking by hand").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_file_name(format!("{}_sweeps.csv", crate::app::sanitize(&s.label)))
                            .add_filter("CSV", &["csv"])
                            .save_file()
                        {
                            match std::fs::write(&path, export::sweeps_csv(&self.session, &self.red, &s)) {
                                Ok(()) => self.say(format!("Saved {}", path.display()), false),
                                Err(e) => self.say(e.to_string(), true),
                            }
                        }
                    }
                    let label = if s.excluded { "Use in calculations" } else { "Leave out" };
                    if ui.button(label).clicked() {
                        if let Some(x) = self.session.selections.iter_mut().find(|x| x.id == id) {
                            x.excluded = !x.excluded;
                        }
                        self.changed = true;
                    }
                    if ui.button("Show on signal").clicked() {
                        self.page = crate::app::Page::Signal;
                        let pad = ((s.end - s.start) * 1.2).max(10.0);
                        self.signal.x = (s.start - pad, s.end + pad);
                    }
                    // Flags fill what the buttons leave, and are cut off with a hover if too long.
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.set_clip_rect(ui.max_rect().intersect(ui.clip_rect()));
                        for f in &r.flags {
                            tag(ui, f, t.warn);
                        }
                    });
                });
            });
            ui.add_space(2.0);
            let Some(u) = &r.upb else {
                let shown = r.te.iter().flatten().count();
                if shown > 0 {
                    ui.label(RichText::new(format!("{shown} element contents calculated. Export the sweeps to follow them one by one.")).color(t.dim));
                } else {
                    ui.label(RichText::new("No result for this selection.").color(t.dim));
                }
                return;
            };
            ui.label(RichText::new(format!(
                "{} sweeps used, {} rejected as outliers. Beam seconds count from {:.1} s.",
                u.r68.n, u.r68.n_outliers, u.beam_start
            )).color(t.dim).size(12.0));
            ui.add_space(4.0);
            egui::Grid::new("trace_grid").num_columns(8).spacing([14.0, 3.0]).striped(false).show(ui, |ui| {
                for head in ["Ratio", "Raw mean", "Down-hole corrected", "× Normalised by", "= Final", "Analytical 2SE", "+ excess 2s", "+ primary RM value 2s"] {
                    ui.label(RichText::new(head).color(t.dim).size(12.0));
                }
                ui.end_row();
                let rows: Vec<(&str, Option<&RatioValue>)> = vec![
                    ("206Pb/238U", Some(&u.r68)),
                    ("207Pb/235U", Some(&u.r75)),
                    ("207Pb/206Pb", Some(&u.r76)),
                    ("208Pb/232Th", u.r82.as_ref()),
                ];
                for (name, value) in rows {
                    let Some(v) = value else { continue };
                    ui.label(RichText::new(name).size(12.5));
                    for x in [v.raw, v.downhole, v.factor, v.mean] {
                        ui.label(RichText::new(fmt_sig(x, 5)).size(12.5));
                    }
                    for x in [v.int2se, v.prop2s, v.rminc2s] {
                        ui.label(RichText::new(fmt_sig(x, 2)).size(12.5));
                    }
                    ui.end_row();
                }
            });
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                let mut line = format!(
                    "Ages: {} Ma from 206/238, {} Ma from 207/206.",
                    fmt_pm(u.a68.age, u.a68.prop2s),
                    fmt_pm(u.a76.age, u.a76.prop2s)
                );
                if let Some(c) = &u.common {
                    line.push_str(&format!(
                        "  {} correction: {:.2} % common 206Pb, 206/238 age {} Ma.",
                        c.method,
                        c.f206_pct,
                        fmt_pm(c.age68, c.age68_2s)
                    ));
                }
                if let Some(a) = u.a68_th230 {
                    line.push_str(&format!("  With 230Th correction: {} Ma.", fmt_pm(a.age, a.prop2s)));
                }
                ui.label(RichText::new(line).size(12.5));
            });
        });
    }

    fn table_as_text(&self) -> String {
        let mut out = String::new();
        match self.results.view {
            ResultsView::Elements => {
                let table = export::results_table(&self.session, &self.red);
                out.push_str(&table.headers.join("\t"));
                out.push('\n');
                for row in &table.rows {
                    let cells: Vec<String> = row
                        .iter()
                        .map(|c| match c {
                            export::Cell::Text(s) => s.clone(),
                            export::Cell::Num(v) => v.to_string(),
                            export::Cell::Empty => String::new(),
                        })
                        .collect();
                    out.push_str(&cells.join("\t"));
                    out.push('\n');
                }
            }
            _ => {
                let columns = self.upb_columns();
                out.push_str("Analysis\tGroup\t");
                out.push_str(&columns.join("\t"));
                out.push_str("\tChecks\n");
                let sort_column = self.results.sort.map(|s| s.0);
                for i in self.visible_rows(|i| sort_column.map_or(f64::NAN, |c| self.upb_cell_number(i, c))) {
                    let s = &self.session.selections[i];
                    let g = self.session.group_of(s).map_or("", |g| g.name.as_str());
                    out.push_str(&format!("{}\t{}", s.label, g));
                    for c in 0..columns.len() {
                        out.push('\t');
                        out.push_str(&self.upb_cell_text(i, c));
                    }
                    out.push('\t');
                    out.push_str(&self.red.selections[i].flags.join("; "));
                    out.push('\n');
                }
            }
        }
        out
    }
}
