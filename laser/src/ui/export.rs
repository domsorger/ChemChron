// SPDX-License-Identifier: GPL-3.0-or-later
//! Export: publication tables with full metadata, IsoplotR input, figures, session.

use crate::app::{sanitize, TraceApp};
use crate::theme::{form_row, group_color, section};
use egui::{RichText, Ui};
use egui_extras::{Column, TableBuilder};
use std::collections::BTreeSet;
use chemchron_laser_core::export;
use chemchron_laser_core::model::*;

#[derive(Default)]
pub struct ExportState {
    pub skip: BTreeSet<u64>,
}

impl TraceApp {
    pub fn page_export(&mut self, ui: &mut Ui) {
        let t = self.theme();
        ui.horizontal(|ui| {
            ui.heading("Export");
            ui.label(RichText::new("Every file states the method, settings and reference values with the numbers.").color(t.dim));
        });
        ui.add_space(6.0);
        egui::Panel::left(crate::theme::layout_id("export_left")).resizable(true).default_size(420.0).size_range(294.0..=1050.0).frame(egui::Frame::NONE).show(ui, |ui| {
            egui::ScrollArea::both().auto_shrink([false, false]).id_salt("export_left").show(ui, |ui| {
                ui.set_width(ui.available_width() - 4.0);
                self.export_files(ui);
                self.export_metadata_form(ui);
            });
        });
        egui::CentralPanel::default().frame(egui::Frame::NONE.inner_margin(egui::Margin { left: 10, ..Default::default() })).show(ui, |ui| {
            ui.label(RichText::new("What goes in the metadata").strong());
            ui.add_space(4.0);
            let rows = export::metadata_rows(&self.session, &self.red);
            t.card().inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                ui.set_height(ui.available_height());
                TableBuilder::new(ui)
                    .id_salt("meta_table")
                    .striped(true)
                    .column(Column::initial(230.0).clip(true).resizable(true))
                    .column(Column::remainder().clip(true))
                    .header(20.0, |mut h| {
                        h.col(|ui| { ui.label(RichText::new("Item").strong()); });
                        h.col(|ui| { ui.label(RichText::new("Value").strong()); });
                    })
                    .body(|body| {
                        body.rows(20.0, rows.len(), |mut row| {
                            let (k, v) = &rows[row.index()];
                            row.col(|ui| { ui.add(egui::Label::new(RichText::new(k).size(12.5)).selectable(false).truncate()); });
                            row.col(|ui| {
                                ui.add(egui::Label::new(RichText::new(v).size(12.5).color(t.dim)).selectable(true).truncate()).on_hover_text(v);
                            });
                        });
                    });
            });
        });
    }

    fn write_text(&mut self, name: &str, filter: (&str, &[&str]), text: String) {
        if let Some(path) = rfd::FileDialog::new().set_file_name(name).add_filter(filter.0, filter.1).save_file() {
            match std::fs::write(&path, text) {
                Ok(()) => self.say(format!("Saved {}", path.display()), false),
                Err(e) => self.say(e.to_string(), true),
            }
        }
    }

    fn export_files(&mut self, ui: &mut Ui) {
        let t = self.theme();
        let stem = sanitize(&self.session.name);
        section(ui, &t, "Results", |ui| {
            ui.horizontal(|ui| {
                if ui.button("Excel workbook…").on_hover_text("Metadata, results, reference-material checks and group ages on separate sheets").clicked() {
                    if let Some(path) = rfd::FileDialog::new().set_file_name(format!("{stem}.xlsx")).add_filter("Excel", &["xlsx"]).save_file() {
                        match export::write_xlsx(&self.session, &self.red, &path) {
                            Ok(()) => self.say(format!("Saved {}", path.display()), false),
                            Err(e) => self.say(e, true),
                        }
                    }
                }
                if ui.button("CSV with metadata…").clicked() {
                    if let Some(path) = rfd::FileDialog::new().set_file_name(format!("{stem}.csv")).add_filter("CSV", &["csv"]).save_file() {
                        match export::write_results_csv(&self.session, &self.red, &path) {
                            Ok(()) => self.say(format!("Saved {}", path.display()), false),
                            Err(e) => self.say(e, true),
                        }
                    }
                }
            });
        });
        let groups: Vec<(u64, String, [u8; 3])> = self
            .session
            .groups
            .iter()
            .filter(|g| g.kind != GroupKind::Baseline && !self.session.rm_of(g.id).is_some_and(|m| m.matrix == "Glass"))
            .map(|g| (g.id, g.name.clone(), g.color))
            .collect();
        section(ui, &t, "IsoplotR input", |ui| {
            ui.label(RichText::new("U-Pb formats 1 and 2, with propagated uncertainties at 1 sigma, IsoplotR's default.").color(t.dim).size(12.0));
            ui.horizontal_wrapped(|ui| {
                for (id, name, color) in &groups {
                    let mut on = !self.export.skip.contains(id);
                    // colour and tick box stay together when the row wraps
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                        ui.painter().rect_filled(r, 2, group_color(*color));
                        if ui.checkbox(&mut on, name).changed() {
                            if on {
                                self.export.skip.remove(id);
                            } else {
                                self.export.skip.insert(*id);
                            }
                        }
                    });
                }
            });
            let chosen: Vec<u64> = groups.iter().map(|g| g.0).filter(|g| !self.export.skip.contains(g)).collect();
            ui.horizontal(|ui| {
                if ui.button("Wetherill (format 1)…").clicked() {
                    let text = export::isoplotr_text(&self.session, &self.red, export::IsoplotFormat::Wetherill, &chosen);
                    self.write_text(&format!("{stem}_isoplotr_wetherill.csv"), ("CSV", &["csv"]), text);
                }
                if ui.button("Tera-Wasserburg (format 2)…").clicked() {
                    let text = export::isoplotr_text(&self.session, &self.red, export::IsoplotFormat::TeraWasserburg, &chosen);
                    self.write_text(&format!("{stem}_isoplotr_tw.csv"), ("CSV", &["csv"]), text);
                }
            });
        });
        if self.embedded {
            // inside ChemChron the session is saved from the header ("Save laser session")
            section(ui, &t, "Combined results", |ui| {
                ui.label(RichText::new("Sends the ratios, ages and element contents of every analysis to Combined results, linked to EPMA analyses with the same label (spots without one become rows of their own). Send again after changing the reduction.").color(t.dim).size(12.0));
                match self.sent_revision {
                    Some(r) if r != self.revision => {
                        ui.colored_label(t.warn, "The results changed since they were last sent: send them again to update Combined results.");
                    }
                    Some(_) => {
                        ui.colored_label(t.ok, "Combined results has the current results.");
                    }
                    None => {}
                }
                let has_common = self.session.settings.upb.common_method != CommonPbMethod::None;
                let has_th = self.session.settings.upb.th230_enabled;
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("U–Pb ratios").color(t.dim));
                    ui.radio_value(&mut self.send_ratios, crate::app::SendRatios::Measured, "as measured");
                    ui.add_enabled(has_common, egui::RadioButton::new(self.send_ratios == crate::app::SendRatios::CommonPb, "common-Pb corrected"))
                        .on_disabled_hover_text("Choose a common-Pb method on Corrections")
                        .clicked()
                        .then(|| self.send_ratios = crate::app::SendRatios::CommonPb);
                    ui.add_enabled(has_common && has_th, egui::RadioButton::new(self.send_ratios == crate::app::SendRatios::CommonPbTh230, "common Pb + ²³⁰Th corrected"))
                        .on_disabled_hover_text("Needs a common-Pb method and the 230Th correction on Corrections")
                        .clicked()
                        .then(|| self.send_ratios = crate::app::SendRatios::CommonPbTh230);
                });
                if ui.button(RichText::new("Send to Combined results").strong()).clicked() {
                    self.request_push = true;
                }
            });
            return;
        }
        section(ui, &t, "Session", |ui| {
            ui.label(RichText::new("Selections and settings as readable text. Raw files are referenced by path and fingerprint, not copied.").color(t.dim).size(12.0));
            if let Some(p) = &self.session_path {
                ui.label(RichText::new(p.display().to_string()).color(t.dim).size(11.5));
            }
            ui.horizontal(|ui| {
                if ui.button("Save session").clicked() {
                    self.save(None);
                }
                if ui.button("Save a copy…").clicked() {
                    if let Some(path) = rfd::FileDialog::new().set_file_name(format!("{stem}.laser.json")).add_filter("ChemChron laser session", &["json"]).save_file() {
                        self.save(Some(path));
                    }
                }
            });
        });
    }

    fn export_metadata_form(&mut self, ui: &mut Ui) {
        let t = self.theme();
        let mut changed = false;
        section(ui, &t, "About this analysis", |ui| {
            ui.label(RichText::new("For the methods table in a paper. Spot size and repetition rate come from the laser log.").color(t.dim).size(12.0));
            let m = &mut self.session.metadata;
            for (label, value) in [
                ("Laboratory", &mut m.laboratory),
                ("Operator", &mut m.operator),
                ("Mass spectrometer", &mut m.instrument),
                ("Laser system", &mut m.laser),
                ("Carrier gas", &mut m.carrier_gas),
                ("Notes", &mut m.notes),
            ] {
                form_row(ui, &t, label, |ui| {
                    changed |= ui.add(egui::TextEdit::singleline(value).desired_width(260.0)).lost_focus();
                });
            }
        });
        self.changed |= changed;
    }
}
