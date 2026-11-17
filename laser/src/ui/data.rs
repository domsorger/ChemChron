// SPDX-License-Identifier: GPL-3.0-or-later
//! Data: open files, laser log and clocks, channels and source files.

use crate::app::TraceApp;
use crate::theme::{self, form_row, section, tag};
use egui::{RichText, Ui};
use egui_extras::{Column, TableBuilder};
use chemchron_laser_core::{autosel, io, project, stats};

impl TraceApp {
    /// Reopens a saved session after its raw files moved: asks for the
    /// session (when none is open from a file) and the folder they are in now.
    pub fn find_moved_data(&mut self) {
        let session_file = self.session_path.clone().filter(|p| p.is_file()).or_else(|| {
            rfd::FileDialog::new().set_title("The saved session").add_filter("ChemChron laser session", &["json"]).pick_file()
        });
        let Some(session_file) = session_file else { return };
        let Some(folder) = rfd::FileDialog::new().set_title("The folder the raw data is in now").pick_folder() else { return };
        match self.reopen_moved(&session_file, &folder) {
            Ok(n) => self.say(format!("Reopened with the data in {} ({n}). Save the session to keep the new place.", folder.display()), false),
            Err(e) => self.say(e, true),
        }
    }

    /// Reads `session_file` with its raw files looked for below `folder`.
    pub fn reopen_moved(&mut self, session_file: &std::path::Path, folder: &std::path::Path) -> Result<String, String> {
        let text = std::fs::read_to_string(session_file).map_err(|e| format!("{}: {e}", session_file.display()))?;
        let s: chemchron_laser_core::model::Session = serde_json::from_str(&text).map_err(|e| format!("This is not a readable session file: {e}"))?;
        let mut dirs = vec![folder.to_path_buf()];
        if let Some(p) = folder.parent() {
            dirs.push(p.to_path_buf());
        }
        let loaded = project::reopen_session(s, &dirs)?;
        let note = loaded.changed_files.first().cloned().unwrap_or_else(|| "all files found".to_string());
        self.session = loaded.session;
        self.session_path = Some(session_file.to_path_buf());
        self.recalculate();
        self.after_session_replaced();
        self.unsaved = true;
        Ok(note)
    }

    pub fn page_data(&mut self, ui: &mut Ui) {
        let t = self.theme();
        ui.horizontal(|ui| {
            ui.heading("Data");
            ui.label(RichText::new("Raw files are only read, never changed. Each one is recorded with its fingerprint.").color(t.dim));
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("Open data folder…").on_hover_text("A folder of Element FIN2 files, an Agilent batch, or text exports").clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    self.start_import(vec![path]);
                }
            }
            if ui.button("Open data files…").clicked() {
                if let Some(paths) = rfd::FileDialog::new()
                    .add_filter("Time-resolved data", &["FIN2", "csv", "txt", "asc"])
                    .pick_files()
                {
                    self.start_import(paths);
                }
            }
            if ui.button("Open saved session…").clicked() {
                if let Some(path) = rfd::FileDialog::new().add_filter("ChemChron laser session", &["json"]).pick_file() {
                    self.session_path = Some(path.clone());
                    self.start_import(vec![path]);
                }
            }
            if ui.button("Demo session").on_hover_text(if self.embedded { "Simulated laser data with a known truth, and simulated EPMA analyses of the same spots" } else { "Simulated data with a known truth" }).clicked() {
                if self.embedded {
                    self.request_demo = true;
                } else {
                    self.start_new_demo();
                }
            }
            if ui
                .button("Find moved data…")
                .on_hover_text("A saved session whose raw files were moved or copied to another computer: choose the folder they are in now")
                .clicked()
            {
                self.find_moved_data();
            }
        });
        ui.add_space(8.0);
        ui.columns(2, |cols| {
            self.data_left(&mut cols[0]);
            self.data_right(&mut cols[1]);
        });
    }

    fn data_left(&mut self, ui: &mut Ui) {
        let t = self.theme();
        let inputs = self.session.inputs.clone();
        section(ui, &t, "Laser log and clocks", |ui| {
            if let Some(log) = self.session.laser_log_file.clone() {
                ui.horizontal(|ui| {
                    tag(ui, &format!("{} firings", inputs.laser.len()), t.ok);
                    ui.label(RichText::new(&log.label).size(12.5));
                });
                ui.label(RichText::new(format!("Fingerprint {}", &log.sha256[..16])).color(t.dim).size(11.5));
            } else if !inputs.laser.is_empty() {
                ui.horizontal(|ui| tag(ui, &format!("{} simulated firings", inputs.laser.len()), t.ok));
            } else {
                ui.label(RichText::new("No laser log. Selections come from the signal, and sample names from file names.").color(t.dim));
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("Add laser log…").on_hover_text("An ESI log, or a table with name, start and end columns").clicked() {
                    if let Some(path) = rfd::FileDialog::new().add_filter("Laser log", &["csv", "txt"]).pick_file() {
                        self.add_laser_log(&path);
                    }
                }
            });
            if !inputs.laser.is_empty() {
                ui.add_space(4.0);
                form_row(ui, &t, "Clock offset (s)", |ui| {
                    let mut value = self.session.clock_offset_s;
                    let r = ui.add(egui::DragValue::new(&mut value).speed(0.05).max_decimals(2));
                    if r.changed() {
                        self.session.clock_offset_s = value;
                        self.session.clock_note = "Set by hand.".to_string();
                    }
                    // Selections follow when the value settles, not at every step of a drag.
                    if r.drag_stopped() || (r.changed() && !r.dragged()) {
                        self.relink_log();
                    }
                    if ui.button("Find again").clicked() {
                        if let Some(fit) = io::align_clock(&inputs, &inputs.laser) {
                            self.session.clock_offset_s = fit.offset_s;
                            self.session.clock_note = format!(
                                "Found automatically: laser-on signal stands {:.2} decades above the rest; {:.0} % of firings fall on recorded sweeps.",
                                fit.contrast,
                                fit.coverage * 100.0
                            );
                            self.relink_log();
                        }
                    }
                });
                ui.label(RichText::new(&self.session.clock_note).color(t.dim).size(12.0));
            }
        });
        let warnings = self.red.warnings.clone();
        if !warnings.is_empty() {
            section(ui, &t, "Notes on this session", |ui| {
                for w in warnings {
                    ui.colored_label(t.warn, w);
                }
            });
        }
        let channels: Vec<(String, f64, f64, bool)> = inputs
            .channels
            .iter()
            .map(|c| {
                let maximum = c.values.iter().copied().filter(|v| v.is_finite()).fold(0.0, f64::max);
                (c.name.clone(), stats::median(&c.values), maximum, c.is_empty_signal())
            })
            .collect();
        section(ui, &t, "Channels", |ui| {
            ui.label(RichText::new(format!(
                "{} sweeps at {:.3} s per sweep",
                inputs.time.len(),
                inputs.sweep_seconds()
            )).color(t.dim).size(12.0));
            ui.add_space(4.0);
            let height = (ui.available_height() - 8.0).max(120.0);
            TableBuilder::new(ui)
                .id_salt("channels")
                .striped(true)
                .column(Column::initial(110.0))
                .column(Column::initial(110.0))
                .column(Column::remainder())
                .max_scroll_height(height)
                .header(20.0, |mut h| {
                    h.col(|ui| { ui.label(RichText::new("Channel").strong()); });
                    h.col(|ui| { ui.label(RichText::new("Median CPS").strong()); });
                    h.col(|ui| { ui.label(RichText::new("Highest CPS").strong()); });
                })
                .body(|mut body| {
                    for (name, med, maximum, empty) in &channels {
                        body.row(20.0, |mut row| {
                            row.col(|ui| { ui.label(name); });
                            row.col(|ui| {
                                if *empty {
                                    ui.colored_label(t.warn, "no signal");
                                } else {
                                    ui.label(theme::fmt_sig(*med, 3));
                                }
                            });
                            row.col(|ui| { ui.label(theme::fmt_sig(*maximum, 3)); });
                        });
                    }
                });
        });
    }

    fn data_right(&mut self, ui: &mut Ui) {
        let t = self.theme();
        let files = self.session.files.clone();
        section(ui, &t, "Source files", |ui| {
            if files.is_empty() {
                ui.label(RichText::new("The demo session is calculated, not read from files. Open your own data with the buttons above.").color(t.dim));
                return;
            }
            let format = files.first().map_or(String::new(), |f| f.format.clone());
            ui.label(RichText::new(format!("{} files · {format}", files.len())).color(t.dim).size(12.0));
            ui.add_space(4.0);
            let height = (ui.available_height() - 8.0).max(120.0);
            TableBuilder::new(ui)
                .id_salt("files")
                .striped(true)
                .column(Column::initial(130.0))
                .column(Column::initial(170.0))
                .column(Column::initial(60.0))
                .column(Column::remainder())
                .max_scroll_height(height)
                .header(20.0, |mut h| {
                    h.col(|ui| { ui.label(RichText::new("File").strong()); });
                    h.col(|ui| { ui.label(RichText::new("Clock time in file").strong()); });
                    h.col(|ui| { ui.label(RichText::new("Sweeps").strong()); });
                    h.col(|ui| { ui.label(RichText::new("Fingerprint").strong()); });
                })
                .body(|body| {
                    body.rows(20.0, files.len(), |mut row| {
                        let f = &files[row.index()];
                        row.col(|ui| { ui.label(&f.label).on_hover_text(&f.path); });
                        row.col(|ui| {
                            let stamp = f.stamp.as_deref().unwrap_or("–");
                            let short = stamp.split(", ").last().unwrap_or(stamp);
                            ui.label(RichText::new(short).size(12.0));
                        });
                        row.col(|ui| { ui.label(f.n_sweeps.to_string()); });
                        row.col(|ui| {
                            ui.label(RichText::new(&f.sha256[..12]).monospace().color(t.dim).size(11.5)).on_hover_text(&f.sha256);
                        });
                    });
                });
        });
    }

    fn add_laser_log(&mut self, path: &std::path::Path) {
        let paths: Vec<std::path::PathBuf> = self.session.files.iter().map(|f| f.path.clone().into()).collect();
        if paths.is_empty() {
            self.say("Open your own data first; the demo session has its own simulated log.", true);
            return;
        }
        match project::session_from_paths(&paths, Some(path)) {
            Ok(session) => {
                self.session = session;
                self.recalculate();
                self.after_session_replaced();
                self.say("Laser log added. Selections were remade from it.", false);
            }
            Err(e) => self.say(e, true),
        }
    }

    /// After the clock offset changes, selections from the log move with it.
    fn relink_log(&mut self) {
        let options = self.signal.auto;
        let (upb, te) = (
            self.session.settings.upb.primary_group.and_then(|g| self.session.group(g)).map(|g| g.name.clone()),
            self.session.settings.te.primary_group.and_then(|g| self.session.group(g)).map(|g| g.name.clone()),
        );
        autosel::from_log(&mut self.session, &options);
        let find = |s: &chemchron_laser_core::model::Session, n: Option<String>| n.and_then(|n| s.groups.iter().find(|g| g.name == n).map(|g| g.id));
        self.session.settings.upb.primary_group = find(&self.session, upb);
        self.session.settings.te.primary_group = find(&self.session, te);
        self.session.apply_defaults();
        self.selected = None;
        self.signal.active_group = self.session.groups.first().map(|g| g.id);
        self.changed = true;
    }
}
