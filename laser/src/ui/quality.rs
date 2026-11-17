// SPDX-License-Identifier: GPL-3.0-or-later
//! Quality: reference materials against their accepted values, and every flagged analysis.

use crate::app::TraceApp;
use crate::theme::{self, fmt_pm, fmt_sig, form_row, section, stat, tag};
use egui::{RichText, Ui};
use egui_extras::{Column, TableBuilder};
use chemchron_laser_core::drs::qa::QaStatus;
use chemchron_laser_core::model::*;

impl TraceApp {
    pub fn page_quality(&mut self, ui: &mut Ui) {
        let t = self.theme();
        let alarms = self.red.qa.alarms();
        let warnings = self.red.qa.warnings();
        let passes = self.red.qa.rows.iter().filter(|r| r.status == QaStatus::Pass).count();
        let flagged: Vec<(u64, String, String)> = self
            .session
            .selections
            .iter()
            .zip(&self.red.selections)
            .filter(|(_, r)| !r.flags.is_empty())
            .map(|(s, r)| (s.id, s.label.clone(), r.flags.join(", ")))
            .collect();
        ui.horizontal(|ui| {
            stat(ui, &t, "Checks passed", &passes.to_string(), t.ok, 100.0);
            stat(ui, &t, "Warnings", &warnings.to_string(), if warnings > 0 { t.warn } else { t.text }, 100.0);
            stat(ui, &t, "Alarms", &alarms.to_string(), if alarms > 0 { t.alarm } else { t.text }, 100.0);
            stat(ui, &t, "Flagged analyses", &flagged.len().to_string(), if flagged.is_empty() { t.text } else { t.warn }, 110.0);
        });
        let unconfirmed: Vec<String> = self
            .session
            .groups
            .iter()
            .filter_map(|g| self.session.rm_of(g.id))
            .filter(|m| !m.confirmed)
            .map(|m| m.name.clone())
            .collect();
        if !unconfirmed.is_empty() {
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(t.warn, format!("Accepted values not checked yet: {}.", unconfirmed.join(", ")));
                ui.label(RichText::new(if self.embedded { "Check them against the certificates in Settings > Reference materials (tick \"Checked against the certificate\") before trusting a pass or an alarm." } else { "Check them against the certificates on the Reference materials page before trusting a pass or an alarm." }).color(t.dim));
            });
        }
        ui.add_space(8.0);
        egui::Panel::right(crate::theme::layout_id("qa_side")).resizable(true).default_size(320.0).size_range(224.0..=800.0).frame(egui::Frame::NONE).show(ui, |ui| {
            egui::ScrollArea::both().auto_shrink([false, false]).id_salt("qa_side").show(ui, |ui| {
                egui::Frame::NONE.outer_margin(egui::Margin { left: 10, ..Default::default() }).show(ui, |ui| {
                    self.qa_limits(ui);
                    let notes = self.red.warnings.clone();
                    if !notes.is_empty() {
                        section(ui, &t, "Notes", |ui| {
                            for w in notes {
                                ui.colored_label(t.warn, w);
                            }
                        });
                    }
                    section(ui, &t, "Flagged analyses", |ui| {
                        if flagged.is_empty() {
                            ui.label(RichText::new("None. Every analysis passed the automatic checks.").color(t.dim));
                        }
                        for (id, label, flags) in &flagged {
                            ui.horizontal_wrapped(|ui| {
                                if ui.link(label).clicked() {
                                    self.results.jump_to = Some(*id);
                                    self.page = crate::app::Page::Results;
                                }
                                ui.label(RichText::new(flags).color(t.warn).size(12.0));
                            });
                        }
                    });
                });
            });
        });
        egui::CentralPanel::default().frame(t.card().inner_margin(egui::Margin::same(6))).show(ui, |ui| {
            let rows = self.red.qa.rows.clone();
            if rows.is_empty() {
                ui.label(RichText::new("No reference material has accepted values to compare with. Assign one to a group on the Signal page.").color(t.dim));
                return;
            }
            TableBuilder::new(ui)
                .id_salt("qa_table")
                .striped(true)
                .column(Column::initial(92.0).clip(true).resizable(true))
                .column(Column::initial(132.0).clip(true).resizable(true))
                .column(Column::initial(136.0).clip(true).resizable(true))
                .column(Column::initial(120.0).clip(true).resizable(true))
                .column(Column::initial(64.0))
                .column(Column::initial(54.0))
                .column(Column::initial(30.0))
                .column(Column::remainder().at_least(70.0))
                .header(22.0, |mut h| {
                    for title in ["Group", "Quantity", "Measured, 2s", "Accepted, 2s", "Diff. %", "MSWD", "n", "Result"] {
                        h.col(|ui| { ui.label(RichText::new(title).strong().size(12.5)); });
                    }
                })
                .body(|body| {
                    body.rows(22.0, rows.len(), |mut row| {
                        let r = &rows[row.index()];
                        let cell = |ui: &mut Ui, text: String| {
                            ui.add(egui::Label::new(RichText::new(text).size(12.5)).selectable(false).truncate());
                        };
                        row.col(|ui| cell(ui, r.group.clone()));
                        row.col(|ui| cell(ui, r.quantity.clone()));
                        row.col(|ui| cell(ui, fmt_pm(r.measured, r.measured_2s)));
                        row.col(|ui| {
                            let text = if r.accepted_2s > 0.0 { fmt_pm(r.accepted, r.accepted_2s) } else { fmt_sig(r.accepted, 5) };
                            cell(ui, text);
                        });
                        row.col(|ui| cell(ui, format!("{:+.2}", r.diff_pct)));
                        row.col(|ui| cell(ui, format!("{:.2}", r.mswd)));
                        row.col(|ui| cell(ui, r.n.to_string()));
                        row.col(|ui| match r.status {
                            QaStatus::Pass => tag(ui, "Pass", t.ok),
                            QaStatus::Warn => tag(ui, "Warning", t.warn),
                            QaStatus::Alarm => tag(ui, "Alarm", t.alarm),
                            QaStatus::Primary => {
                                ui.label(RichText::new("Primary").color(t.dim).size(12.0)).on_hover_text("Used for normalising, so it agrees by construction");
                            }
                        });
                    });
                });
        });
        let _ = theme::fmt_sig;
    }

    fn qa_limits(&mut self, ui: &mut Ui) {
        let t = self.theme();
        let mut changed = false;
        section(ui, &t, "Limits", |ui| {
            let q = &mut self.session.settings.qa;
            form_row(ui, &t, "Warn at", |ui| {
                changed |= ui.add(egui::DragValue::new(&mut q.warn_pct).speed(0.1).range(0.0..=100.0).suffix(" %")).changed();
            });
            form_row(ui, &t, "Alarm at", |ui| {
                changed |= ui.add(egui::DragValue::new(&mut q.alarm_pct).speed(0.1).range(0.0..=100.0).suffix(" %")).changed();
            });
            form_row(ui, &t, "Discordance flag", |ui| {
                changed |= ui.add(egui::DragValue::new(&mut q.flag_discordance_pct).speed(0.5).range(0.0..=100.0).suffix(" %")).changed();
            });
            form_row(ui, &t, "Common Pb flag", |ui| {
                changed |= ui.add(egui::DragValue::new(&mut q.flag_common_pct).speed(0.1).range(0.0..=100.0).suffix(" %")).changed();
            });
            form_row(ui, &t, "Outlier flag", |ui| {
                changed |= ui.add(egui::DragValue::new(&mut q.flag_outlier_pct).speed(0.5).range(0.0..=100.0).suffix(" %")).changed();
            });
            ui.label(RichText::new("Trace elements use at least 5 % and 10 %: reference values for elements are less exact than isotope ratios.").color(t.dim).size(11.5));
        });
        self.changed |= changed;
        let _ = GroupKind::Sample;
    }
}
