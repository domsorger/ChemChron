// SPDX-License-Identifier: GPL-3.0-or-later
//! EPMA uncertainty model, reference materials and lab settings pages.

use crate::app::ChemApp;
use crate::plots;
use chemchron_core::constants::*;
use chemchron_core::row::*;
use egui::{RichText, Ui};
use chemchron_laser::theme::{form_row, section};

pub fn page_uncertainty(app: &mut ChemApp, ui: &mut Ui) {
    let t = app.theme();
    if app.epma.unc_mineral.is_empty() {
        app.epma.unc_mineral = DEFAULT_MINERAL.into();
    }
    let m = app.epma.unc_mineral.clone();
    let mut s = app.settings.uncertainty.get(&m).cloned().unwrap_or_default();
    let before = s.clone();
    let mut decay = app.settings.upb.decay_in_systematic;
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.heading("Uncertainty model (chemical ages)");
        ui.label(RichText::new("1. Random excess per analysis (2σ, % of age), from each EPMA session's reference materials: the excess that brings their pooled MSWD to 1; sessions with too few use the project value, then the manual value. It enters weights, MSWD and outlier rejection.\n2. Systematic per session (2σ %): manual, or from the long-term log (between-session scatter). Added once to means and isochrons, and to each analysis's total uncertainty.").color(t.dim).size(12.0));
        ui.horizontal(|ui| {
            ui.label("Mineral");
            for mm in MINERAL_NAMES {
                ui.selectable_value(&mut app.epma.unc_mineral, mm.to_string(), mm);
            }
        });
        let table = chemchron_core::uncertainty::reference_excess_table(&app.data.rows, &m, &s.excess_excluded_refs, None);
        section(ui, &t, "Random excess per analysis", |ui| {
            form_row(ui, &t, "Source", |ui| {
                ui.radio_value(&mut s.excess_source, "project".to_string(), "From the reference materials (per session)");
                ui.radio_value(&mut s.excess_source, "manual".to_string(), "Manual value for all");
            });
            form_row(ui, &t, "Manual (2σ %)", |ui| {
                ui.add(egui::DragValue::new(&mut s.excess_manual_pct).speed(0.05).range(0.0..=50.0));
            });
            ui.add_space(4.0);
            egui::Grid::new("exc_tab").striped(true).show(ui, |ui| {
                for h in ["Use", "Reference", "N", "Sessions", "MSWD (analytical)", "Excess 2σ %"] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for mat in &table.materials {
                    let mut on = !s.excess_excluded_refs.contains(&mat.reference);
                    if ui.checkbox(&mut on, "").on_hover_text("Leave a heterogeneous material out of the estimate").changed() {
                        if on {
                            s.excess_excluded_refs.retain(|x| *x != mat.reference);
                        } else {
                            s.excess_excluded_refs.push(mat.reference.clone());
                        }
                    }
                    ui.label(&mat.reference);
                    ui.label(mat.n.to_string());
                    ui.label(mat.sessions.to_string());
                    ui.label(mat.mswd.map(|v| format!("{v:.2}")).unwrap_or("–".into()));
                    ui.label(mat.excess_2s_pct.map(|v| format!("{v:.2}")).unwrap_or("–".into()));
                    ui.end_row();
                }
            });
            ui.label(match table.pooled_pct {
                Some(p) => format!("Pooled over the project: {p:.2} % ({} analyses, {} materials, {} degrees of freedom)", table.n, table.n_materials, table.dof),
                None => format!("Too few reference analyses to estimate the excess ({} degrees of freedom, need {}).", table.dof, chemchron_core::uncertainty::MIN_DOF_FOR_POOLED_EXCESS),
            });
            ui.add_space(4.0);
            let st = chemchron_core::uncertainty::session_excess_table(&app.data.rows, &m, &s.excess_excluded_refs, s.excess_manual_pct, s.excess_source == "project");
            egui::Grid::new("exc_sess").striped(true).show(ui, |ui| {
                for h in ["EPMA session", "Analyses", "Reference analyses", "Own estimate", "In use", "Source"] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for e in st {
                    ui.label(chemchron_core::sessions::display_session(&e.session));
                    ui.label(e.n_analyses.to_string());
                    ui.label(e.n_ref.to_string());
                    ui.label(e.session_pct.map(|v| format!("{v:.2} %")).unwrap_or("–".into()));
                    ui.label(format!("{:.2} %", e.pct));
                    ui.label(e.source);
                    ui.end_row();
                }
            });
        });
        section(ui, &t, "Systematic (part of every total: single analyses, means, isochrons)", |ui| {
            form_row(ui, &t, "Source", |ui| {
                ui.radio_value(&mut s.systematic_source, "log".to_string(), "From the long-term log (manual value until it holds 3 sessions)");
                ui.radio_value(&mut s.systematic_source, "manual".to_string(), "Manual");
            });
            ui.checkbox(&mut decay, "Include the decay-constant uncertainties").on_hover_text("Adds the decay-constant uncertainties (Settings > Decay constants) to the systematic term of every mean, isochron and concordia age: λ²³⁸U for ²⁰⁶Pb/²³⁸U ages and chemical ages, λ²³⁵U for ²⁰⁷Pb/²³⁵U, both for ²⁰⁷Pb/²⁰⁶Pb. λ²³²Th has no stated uncertainty. The same switch for U–Pb and chemical ages.");
            form_row(ui, &t, "Manual (2σ %)", |ui| {
                ui.add(egui::DragValue::new(&mut s.manual_override_pct).speed(0.05).range(0.0..=50.0));
            });
            let mm = app.data.models.as_ref().and_then(|x| x.chem.get(&m));
            let ebs = mm.map(|x| x.excess_by_session.iter().filter(|(k, _)| !k.is_empty()).map(|(k, v)| (k.clone(), v.0)).collect()).unwrap_or_default();
            let ex = mm.map(|x| x.excess_pct);
            let (pct, mats) = chemchron_core::uncertainty::between_session_systematic(&app.settings.long_term_log, &m, &s.excess_excluded_refs, ex, &ebs);
            for mat in mats {
                ui.label(format!(
                    "{}: {} logged sessions, {} with ≥ 3 analyses{}",
                    mat.reference,
                    mat.sessions,
                    mat.usable_sessions,
                    mat.between_2s_pct.map(|v| format!(", between-session excess {v:.2} %")).unwrap_or_default() + &mat.left_out.as_ref().map(|w| format!(" — left out: {w}")).unwrap_or_default()
                ));
            }
            ui.label(match pct {
                Some(p) => format!("From the long-term log: {p:.2} % (2σ)"),
                None => "The long-term log does not hold 3 sessions with ≥ 3 reference analyses yet (log reference materials in Combined results > Long-term logs); the manual value is used.".into(),
            });
        });
        if let Some(mm) = app.data.models.as_ref().and_then(|x| x.chem.get(&m)) {
            ui.label(RichText::new(format!("In use for {m}: random excess {:.2} % ({}), systematic {:.2} % ({}).", mm.excess_pct, mm.excess_source, mm.systematic_pct, mm.systematic_source)).strong());
        }
    });
    if s != before || decay != app.settings.upb.decay_in_systematic {
        app.settings.uncertainty.insert(m, s);
        app.settings.upb.decay_in_systematic = decay;
        app.save_settings();
        app.data.unsaved = true;
        app.data.refresh(&app.settings);
    }
}

/// Which group of lab settings a Settings page shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Which {
    General,
    Decay,
    Epma,
}

/// Lab settings (Settings in the top bar): `which` picks the page.
pub fn page_settings(app: &mut ChemApp, ui: &mut Ui, which: Which) {
    let general = which == Which::General;
    let epma = which == Which::Epma;
    let t = app.theme();
    let before_decay = app.settings.decay.clone();
    let before_dp = app.settings.decimals;
    let before_th = app.settings.thresholds.clone();
    let mut other = false;
    // groups of the project and groups made here (they have a colour but no analyses yet)
    let mut flags: Vec<String> = app.data.rows.iter().map(|r| text(r, "Flag")).filter(|f| !f.is_empty()).chain(app.settings.flag_colors.keys().cloned()).collect();
    flags.sort();
    flags.dedup();
    let mut new_group = std::mem::take(&mut app.epma.new_group_name);
    let mut folder_action: Option<bool> = None;
    let s = &mut app.settings;
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.heading(match which {
            Which::General => "General",
            Which::Decay => "Decay constants",
            Which::Epma => "EPMA settings",
        });
        if general {
            section(ui, &t, "Where settings are stored", |ui| {
                ui.label(RichText::new("Lab settings are shared by all projects and kept as readable JSON files in one settings folder. A project file (*.chemchron.json) holds only that project's analyses, links and laser session.").color(t.dim).size(12.0));
                ui.label(RichText::new(format!("Settings folder: {}", s.dir.display())).strong());
                egui::Grid::new("settings_files").striped(true).show(ui, |ui| {
                    for (f, what) in chemchron_core::workdir::SETTINGS_FILES {
                        let p = s.dir.join(f);
                        ui.label(f);
                        ui.label(RichText::new(what).color(t.dim).size(12.0));
                        ui.label(RichText::new(if p.exists() { "saved" } else { "default, not saved yet" }).size(12.0).color(if p.exists() { t.ok } else { t.dim }));
                        ui.end_row();
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Set working directory…").on_hover_text("Keep the settings in <folder>/option_data, e.g. on a shared drive. The current settings are copied there if it has none.").clicked() {
                        folder_action = Some(true);
                    }
                });
            });
        }
        if which == Which::Decay {
            section(ui, &t, "One set for the Laser, EPMA and Combine parts", |ui| {
                let d = &mut s.decay;
                egui::Grid::new("decay").show(ui, |ui| {
                    let row = |ui: &mut Ui, label: &str, v: &mut f64, speed: f64, digits: usize, source: &str| {
                        ui.label(label);
                        ui.add(egui::DragValue::new(v).speed(speed).custom_formatter(move |x, _| format!("{x:.digits$e}")).custom_parser(|s| s.trim().parse::<f64>().ok()));
                        ui.label(RichText::new(source).color(t.dim).size(12.0));
                        ui.end_row();
                    };
                    row(ui, "λ ²³⁸U (1/yr)", &mut d.lambda_238, 1e-15, 6, "Jaffey et al. (1971)");
                    row(ui, "λ ²³⁵U (1/yr)", &mut d.lambda_235, 1e-14, 5, "Jaffey et al. (1971)");
                    row(ui, "λ ²³²Th (1/yr)", &mut d.lambda_232, 1e-15, 5, "Le Roux & Glendenin (1963)");
                    ui.label("²³⁸U/²³⁵U");
                    ui.add(egui::DragValue::new(&mut d.u238_u235).speed(0.001).max_decimals(4).range(100.0..=200.0));
                    ui.label(RichText::new("Hiess et al. (2012); 137.88 in Steiger & Jäger (1977)").color(t.dim).size(12.0));
                    ui.end_row();
                    ui.label("²³⁸U atom fraction");
                    ui.add(egui::DragValue::new(&mut d.u238_frac).speed(1e-6).max_decimals(6));
                    ui.label(RichText::new(format!("²³⁵U fraction follows: {:.7}", d.u238_frac / d.u238_u235)).color(t.dim).size(12.0));
                    ui.end_row();
                    ui.label("λ ²³⁸U 1σ (%)");
                    ui.add(egui::DragValue::new(&mut d.lambda_238_rel_1s_pct).speed(1e-4).max_decimals(6));
                    ui.end_row();
                    ui.label("λ ²³⁵U 1σ (%)");
                    ui.add(egui::DragValue::new(&mut d.lambda_235_rel_1s_pct).speed(1e-4).max_decimals(6));
                    ui.end_row();
                });
                d.sync_fractions();
                if ui.button("Reset to the recommended values").clicked() {
                    s.decay = DecayConstants::default();
                }
                ui.label(RichText::new("The decay constants recommended by Steiger & Jäger (1977), with the present-day ²³⁸U/²³⁵U of Hiess et al. (2012), which moves ²⁰⁷Pb/²⁰⁶Pb ages by under 1 Ma and chemical ages by far less than the older value of 137.88. Every part uses this one set: chemical ages, isochrons, U–Pb ages and concordia, and the open laser session.").color(t.dim).size(12.0));
            });
        }
        if epma {
            section(ui, &t, "Displayed decimal places", |ui| {
                let dp = &mut s.decimals;
                egui::Grid::new("dp").show(ui, |ui| {
                    for (label, v) in [("Ages (Ma, %)", &mut dp.age), ("Oxides (wt%)", &mut dp.wt_pct), ("apfu", &mut dp.apfu), ("CSUTPS, end-members", &mut dp.csutps), ("Site sums", &mut dp.site_sum), ("Calculated PbO", &mut dp.pb_calc)] {
                        ui.label(label);
                        ui.add(egui::DragValue::new(v).range(0..=8));
                        ui.end_row();
                    }
                });
            });
        }
        if epma {
            section(ui, &t, "Outlier rejection", |ui| {
                other |= ui.checkbox(&mut s.reject_outliers, "Reject outliers in weighted means and isochrons").changed();
                form_row(ui, &t, "Threshold (σ)", |ui| {
                    other |= ui.add(egui::DragValue::new(&mut s.rejection_sigma).speed(0.05).range(1.0..=5.0)).changed();
                });
            });
        }
        if epma {
            section(ui, &t, "Table display", |ui| {
                other |= ui.checkbox(&mut s.show_actual_below_dl, "Show measured values below detection limit (otherwise 'b.d.l.')").changed();
                other |= ui.checkbox(&mut s.show_uncertainty_columns, "Show oxide 2σ and detection-limit columns").changed();
            });
        }
        if general {
            section(ui, &t, "Sample groups and colours", |ui| {
                if flags.is_empty() {
                    ui.label(RichText::new("No sample groups yet.").color(t.dim));
                }
                egui::Grid::new("group_colours").num_columns(4).spacing([10.0, 4.0]).show(ui, |ui| {
                    for (i, f) in flags.iter().enumerate() {
                        let c0 = plots::group_color(f, &s.flag_colors, i);
                        let mut rgb = [c0.r(), c0.g(), c0.b()];
                        ui.label(f);
                        if ui.color_edit_button_srgb(&mut rgb).changed() {
                            s.flag_colors.insert(f.clone(), plots::to_hex(egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2])));
                            other = true;
                        }
                        // the colour as hex code (#RRGGBB), typed or pasted; applied with Enter or when leaving the field
                        let id = ui.make_persistent_id(("group_hex", f));
                        let current = plots::to_hex(c0);
                        let mut buf: String = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| current.clone());
                        let r = ui.add_sized([82.0, 22.0], egui::TextEdit::singleline(&mut buf).font(egui::TextStyle::Monospace).hint_text("#RRGGBB"));
                        let valid = plots::parse_hex(&buf);
                        if r.lost_focus() {
                            match valid {
                                Some(c) => {
                                    if plots::to_hex(c) != current || !s.flag_colors.contains_key(f) {
                                        s.flag_colors.insert(f.clone(), plots::to_hex(c));
                                        other = true;
                                    }
                                }
                                None => buf = current.clone(),
                            }
                            ui.data_mut(|d| d.remove::<String>(id));
                        } else if r.has_focus() {
                            ui.data_mut(|d| d.insert_temp(id, buf.clone()));
                        } else {
                            ui.data_mut(|d| d.remove::<String>(id));
                        }
                        if r.has_focus() && valid.is_none() && !buf.trim().is_empty() {
                            r.on_hover_text("Six hex digits, e.g. #1F77B4");
                        }
                        if s.flag_colors.contains_key(f) {
                            if ui.small_button("×").on_hover_text("Back to the automatic colour (a group without analyses disappears)").clicked() {
                                s.flag_colors.remove(f);
                                other = true;
                            }
                        } else {
                            ui.label("");
                        }
                        ui.end_row();
                    }
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut new_group).hint_text("new sample group").desired_width(160.0));
                    if ui.button("Add sample group").on_hover_text("It appears in the Sample group menus of the tables; give it a colour here").clicked() && !new_group.trim().is_empty() {
                        let name = new_group.trim().to_string();
                        if !s.flag_colors.contains_key(&name) {
                            let c = plots::group_color(&name, &s.flag_colors, flags.len());
                            s.flag_colors.insert(name, plots::to_hex(c));
                            other = true;
                        }
                        new_group.clear();
                    }
                });
            });
        }
        if epma {
            section(ui, &t, "Automatic mineral identification (wt%)", |ui| {
                let th = &mut s.thresholds;
                egui::Grid::new("thr").show(ui, |ui| {
                    for (label, v) in [
                        ("Phosphate: P₂O₅ at least", &mut th.phosphate_p2o5_min),
                        ("Phosphate: SiO₂ below", &mut th.phosphate_sio2_max),
                        ("Thorite: SiO₂ at least", &mut th.thorite_sio2_min),
                        ("Thorite: ThO₂ at least", &mut th.thorite_tho2_min),
                        ("Thorite: P₂O₅ below", &mut th.thorite_p2o5_max),
                        ("Oxide: ThO₂ + UO₂ at least", &mut th.oxide_thpu_min),
                        ("Oxide: P₂O₅ below", &mut th.oxide_p2o5_max),
                        ("Oxide: SiO₂ below", &mut th.oxide_sio2_max),
                    ] {
                        ui.label(label);
                        ui.add(egui::DragValue::new(v).speed(0.1).range(0.0..=100.0));
                        ui.end_row();
                    }
                });
                ui.label(RichText::new("Applies to new imports and to Mineral > Identify again. Zircon is never assigned automatically.").color(t.dim).size(12.0));
            });
        }
    });
    app.epma.new_group_name = new_group;
    match folder_action {
        Some(true) => {
            if let Some(dir) = rfd::FileDialog::new().set_title("Working directory for the ChemChron settings").pick_folder() {
                match chemchron_core::workdir::set_working_directory(&app.settings, &dir) {
                    Ok(target) => {
                        app.settings = chemchron_core::settings::Settings::load_from(&target);
                        app.data.refresh(&app.settings);
                        app.say(format!("Settings are now kept in {}.", target.display()), false);
                    }
                    Err(e) => app.say(e, true),
                }
            }
            return;
        }
        Some(false) => {
            if let Some(dir) = rfd::FileDialog::new().set_title("An option_data folder to take the settings from").pick_folder() {
                let dir = if dir.join("option_data").is_dir() { dir.join("option_data") } else { dir };
                match chemchron_core::workdir::import_from(&mut app.settings, &dir) {
                    Ok(m) => {
                        app.reprocess("settings taken over");
                        app.say(m, false);
                    }
                    Err(e) => app.say(e, true),
                }
            }
            return;
        }
        None => {}
    }
    let numbers = app.settings.decay != before_decay || app.settings.decimals != before_dp;
    if numbers || app.settings.thresholds != before_th || other {
        app.save_settings();
    }
    if numbers {
        app.reprocess("decay constants or decimals changed");
    }
}
