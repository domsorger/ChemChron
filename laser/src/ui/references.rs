// SPDX-License-Identifier: GPL-3.0-or-later
//! Reference materials: accepted values, editable and importable.

use crate::app::TraceApp;
use crate::theme::{fmt_sig, form_row, section, tag};
use egui::{RichText, Ui};
use chemchron_laser_core::refmat::{self, ReferenceMaterial, RmValue};

#[derive(Default)]
pub struct ReferencesState {
    pub chosen: Option<String>,
    pub new_quantity: String,
    pub new_value: f64,
    pub new_unc: f64,
    pub new_material: String,
}

impl TraceApp {
    pub fn page_references(&mut self, ui: &mut Ui) {
        let t = self.theme();
        ui.horizontal(|ui| {
            ui.heading("Reference materials");
            let note = if self.embedded {
                "Each value here decides results directly. This is the lab's one reference list, shared with EPMA and Combine, kept in the working directory and edited under Settings > Reference materials; a copy also travels inside the session file."
            } else {
                "Each value here decides results directly. The list travels inside the session file."
            };
            ui.label(RichText::new(note).color(t.dim));
        });
        ui.add_space(6.0);
        if self.references.chosen.as_ref().is_none_or(|n| refmat::find(&self.session.rms, n).is_none()) {
            self.references.chosen = self.session.rms.first().map(|m| m.name.clone());
        }
        egui::Panel::left(crate::theme::layout_id("rm_list")).resizable(true).default_size(250.0).size_range(175.0..=625.0).frame(egui::Frame::NONE).show(ui, |ui| {
            egui::ScrollArea::both().auto_shrink([false, false]).id_salt("rm_list").show(ui, |ui| {
                ui.set_width(ui.available_width() - 4.0);
                let names: Vec<(String, bool, String)> = self.session.rms.iter().map(|m| (m.name.clone(), m.confirmed, m.matrix.clone())).collect();
                section(ui, &t, "Materials", |ui| {
                    for (name, confirmed, matrix) in &names {
                        let on = self.references.chosen.as_deref() == Some(name);
                        ui.horizontal(|ui| {
                            if ui.selectable_label(on, name).clicked() {
                                self.references.chosen = Some(name.clone());
                            }
                            ui.label(RichText::new(matrix).color(t.dim).size(11.5));
                            if !confirmed {
                                ui.label(RichText::new("unchecked").color(t.warn).size(11.0));
                            }
                        });
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.references.new_material).hint_text("New material").desired_width(120.0));
                        let name = self.references.new_material.trim().to_string();
                        let fresh = !name.is_empty() && refmat::find(&self.session.rms, &name).is_none();
                        if ui.add_enabled(fresh, egui::Button::new("Add")).clicked() {
                            self.session.rms.push(ReferenceMaterial {
                                name: name.clone(),
                                aliases: Vec::new(),
                                matrix: String::new(),
                                source: "Entered by hand".to_string(),
                                confirmed: true,
                                values: Default::default(),
                            });
                            self.references.chosen = Some(name);
                            self.references.new_material.clear();
                            self.changed = true;
                        }
                    });
                });
                section(ui, &t, "Import a table", |ui| {
                    ui.label(RichText::new("A CSV file with material, quantity, value, 2s uncertainty and source columns. Ratios use names like 206Pb/238U; contents use element symbols, in ppm.").color(t.dim).size(12.0));
                    if ui.button("Import CSV…").clicked() {
                        if let Some(path) = rfd::FileDialog::new().add_filter("CSV", &["csv", "txt"]).pick_file() {
                            match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|text| refmat::merge_csv(&mut self.session.rms, &text)) {
                                Ok(count) => {
                                    self.say(format!("Imported {count} values."), false);
                                    self.changed = true;
                                }
                                Err(e) => self.say(e, true),
                            }
                        }
                    }
                });
            });
        });
        egui::CentralPanel::default().frame(egui::Frame::NONE.inner_margin(egui::Margin { left: 10, ..Default::default() })).show(ui, |ui| {
            self.reference_editor(ui);
        });
    }

    fn reference_editor(&mut self, ui: &mut Ui) {
        let t = self.theme();
        let Some(name) = self.references.chosen.clone() else { return };
        let Some(index) = self.session.rms.iter().position(|m| m.name == name) else { return };
        let used_by: Vec<String> = self
            .session
            .groups
            .iter()
            .filter(|g| g.rm.as_deref() == Some(&name))
            .map(|g| g.name.clone())
            .collect();
        let mut changed = false;
        let mut remove_material = false;
        let mut remove_value: Option<String> = None;
        egui::ScrollArea::vertical().id_salt("rm_editor").show(ui, |ui| {
            let m = &mut self.session.rms[index];
            t.card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&m.name).strong().size(15.0));
                    if m.confirmed {
                        tag(ui, "Checked", t.ok);
                    } else {
                        tag(ui, "Not checked yet", t.warn);
                    }
                    if !used_by.is_empty() {
                        ui.label(RichText::new(format!("Used by {}", used_by.join(", "))).color(t.dim));
                    }
                });
                ui.label(RichText::new(&m.source).color(t.dim).size(12.0));
                ui.add_space(4.0);
                changed |= ui.checkbox(&mut m.confirmed, "I have checked these values against a certificate or my lab's list").changed();
                form_row(ui, &t, "Matrix", |ui| {
                    changed |= ui.add(egui::TextEdit::singleline(&mut m.matrix).desired_width(140.0)).lost_focus();
                });
                form_row(ui, &t, "Source", |ui| {
                    changed |= ui.add(egui::TextEdit::singleline(&mut m.source).desired_width(420.0)).lost_focus();
                });
                let mut aliases = m.aliases.join(", ");
                form_row(ui, &t, "Other names", |ui| {
                    if ui.add(egui::TextEdit::singleline(&mut aliases).desired_width(420.0)).on_hover_text("Sample labels with these names go to this material automatically").changed() {
                        m.aliases = aliases.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                        changed = true;
                    }
                });
            });
            ui.add_space(8.0);
            t.card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("Accepted values").strong());
                ui.add_space(4.0);
                let keys: Vec<String> = m.values.keys().cloned().collect();
                egui::Grid::new("rm_values").num_columns(4).spacing([16.0, 4.0]).striped(true).show(ui, |ui| {
                    ui.label(RichText::new("Quantity").color(t.dim));
                    ui.label(RichText::new("Value").color(t.dim));
                    ui.label(RichText::new("2s").color(t.dim));
                    ui.label("");
                    ui.end_row();
                    for key in keys {
                        let v = m.values.get_mut(&key).expect("key exists");
                        let unit = if key.contains('/') { "" } else { " ppm" };
                        ui.label(format!("{key}{unit}"));
                        let speed = (v.value.abs() * 1e-3).max(1e-6);
                        changed |= ui.add(egui::DragValue::new(&mut v.value).speed(speed).custom_formatter(|x, _| fmt_sig(x, 6))).changed();
                        changed |= ui.add(egui::DragValue::new(&mut v.unc2s).speed(speed * 0.1).range(0.0..=f64::MAX).custom_formatter(|x, _| fmt_sig(x, 3))).changed();
                        if ui.small_button("Remove").clicked() {
                            remove_value = Some(key.clone());
                        }
                        ui.end_row();
                    }
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.references.new_quantity).hint_text("U or 206Pb/238U").desired_width(120.0));
                    ui.add(egui::DragValue::new(&mut self.references.new_value).speed(0.01).max_decimals(6).prefix("value "));
                    ui.add(egui::DragValue::new(&mut self.references.new_unc).speed(0.001).max_decimals(6).prefix("2s "));
                    let q = self.references.new_quantity.trim().to_string();
                    if ui.add_enabled(!q.is_empty(), egui::Button::new("Add value")).clicked() {
                        m.values.insert(q, RmValue { value: self.references.new_value, unc2s: self.references.new_unc });
                        self.references.new_quantity.clear();
                        changed = true;
                    }
                });
            });
            ui.add_space(8.0);
            if used_by.is_empty() && ui.button("Delete this material").clicked() {
                remove_material = true;
            }
        });
        if let Some(key) = remove_value {
            self.session.rms[index].values.remove(&key);
            changed = true;
        }
        if remove_material {
            self.session.rms.remove(index);
            self.references.chosen = None;
            changed = true;
        }
        self.changed |= changed;
    }
}
