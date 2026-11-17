// SPDX-License-Identifier: GPL-3.0-or-later
//! Settings > Reference materials: the lab's one list, the same for the
//! Laser, EPMA and Combine parts. Each material has a code (e.g. WC425),
//! alternative codes that also identify it in labels (M44, 44069), one
//! reference age with its 2σ, and the values the laser part calibrates and
//! checks with (isotope ratios, element contents).

use crate::app::ChemApp;
use crate::refsync::{sections, GLASS};
use chemchron_core::row::{jnum, jstr, parse_locale, value_text};
use egui::{RichText, Ui};
use serde_json::{json, Map, Value};
use chemchron_laser::theme::{section, tag};

#[derive(Default)]
pub struct RefsUi {
    /// (section, code) shown on the right.
    pub chosen: Option<(String, String)>,
    pub search: String,
    pub new_code: String,
    pub new_section: String,
    pub new_alias: String,
    pub new_key: String,
}

fn section_label(sec: &str) -> String {
    if sec == GLASS {
        "Glass (laser only)".into()
    } else {
        sec.to_string()
    }
}

fn num(v: Option<&Value>) -> Option<f64> {
    v.and_then(|x| x.as_f64().or_else(|| x.as_str().and_then(|s| s.trim().replace(',', ".").parse().ok())))
}

/// What the list shows beside a code: the reference age and the laser values.
fn badges(ui: &mut Ui, t: &chemchron_laser::theme::Theme, e: &Value) {
    if let Some(a) = num(e.get("age")) {
        ui.label(RichText::new(format!("{a} Ma")).size(11.5).color(t.dim));
    }
    if let Some(l) = e.get("laser") {
        let n = l.get("values").and_then(|x| x.as_object()).map(|x| x.len()).unwrap_or(0);
        let ok = l.get("confirmed").and_then(|x| x.as_bool()).unwrap_or(false);
        ui.label(RichText::new(format!("{n} laser")).size(11.5).color(if ok { t.ok } else { t.warn }))
            .on_hover_text(if ok { "Laser values checked" } else { "Laser values not checked against the certificate yet" });
    }
}

pub fn page_references(app: &mut ChemApp, ui: &mut Ui) {
    let t = app.theme();
    let mut st = std::mem::take(&mut app.refs_ui);
    let mut nested = app.settings.refs_nested.clone();
    let before = nested.clone();
    let secs = sections(&nested);
    if st.new_section.is_empty() {
        st.new_section = "Monazite".into();
    }
    if st.chosen.as_ref().is_none_or(|(s, c)| nested.get(s).and_then(|m| m.get(c)).is_none()) {
        st.chosen = secs.iter().find_map(|s| nested.get(s).and_then(|m| m.as_object()).and_then(|m| m.keys().next().map(|c| (s.clone(), c.clone()))));
    }
    let mut again = false;
    let mut import = false;
    let mut shipped = false;
    ui.horizontal(|ui| {
        ui.heading("Reference materials");
        ui.label(
            RichText::new(format!("One list for the lab: Laser, EPMA and Combine all use it. Saved in {}.", app.settings.dir.join("reference_materials.json").display()))
                .color(t.dim)
                .size(12.0),
        );
    });
    ui.add_space(4.0);
    egui::Panel::left(chemchron_laser::theme::layout_id("refs_list")).resizable(true).default_size(270.0).size_range(189.0..=675.0).frame(egui::Frame::NONE).show(ui, |ui| {
        ui.add(egui::TextEdit::singleline(&mut st.search).hint_text("Find code, name or alternative code").desired_width(255.0));
        let q = st.search.trim().to_lowercase();
        egui::ScrollArea::vertical().id_salt("refs_list_scroll").max_height((ui.available_height() - 165.0).max(120.0)).show(ui, |ui| {
            ui.set_width(ui.available_width() - 4.0);
            for sec in &secs {
                let Some(entries) = nested.get(sec).and_then(|m| m.as_object()) else { continue };
                let shown: Vec<(&String, &Value)> = entries
                    .iter()
                    .filter(|(code, e)| {
                        q.is_empty()
                            || chemchron_core::reference::codes_of(code, e).iter().any(|c| c.to_lowercase().contains(&q))
                            || e.get("name").and_then(|n| n.as_str()).is_some_and(|n| n.to_lowercase().contains(&q))
                    })
                    .collect();
                if shown.is_empty() {
                    continue;
                }
                ui.add_space(4.0);
                ui.label(RichText::new(section_label(sec)).strong().color(t.dim).size(12.0));
                for (code, e) in shown {
                    let on = st.chosen.as_ref() == Some(&(sec.clone(), code.clone()));
                    ui.horizontal(|ui| {
                        if ui.selectable_label(on, RichText::new(code.as_str()).strong()).clicked() {
                            st.chosen = Some((sec.clone(), code.clone()));
                        }
                        badges(ui, &t, e);
                    });
                }
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut st.new_code).hint_text("new code").desired_width(80.0));
            egui::ComboBox::from_id_salt("new_ref_section").selected_text(section_label(&st.new_section)).width(100.0).show_ui(ui, |ui| {
                for s in &secs {
                    ui.selectable_value(&mut st.new_section, s.clone(), section_label(s));
                }
            });
            if ui.button("Add").clicked() {
                let c = st.new_code.trim().to_string();
                if !c.is_empty() {
                    let m = nested.entry(st.new_section.clone()).or_insert_with(|| Value::Object(Map::new()));
                    if let Value::Object(m) = m {
                        m.entry(c.clone()).or_insert_with(|| json!({"name": "", "age": "", "age_2s": "", "source": "", "aliases": []}));
                    }
                    st.chosen = Some((st.new_section.clone(), c));
                    st.new_code.clear();
                }
            }
        });
        import = ui.button("Import reference-material files (JSON)…").on_hover_text("Reference-material files in JSON (Properties: name, matrix, data source; Results: name, value, uncertainty, units). Values the list already has are kept; new ones are added, marked unchecked.").clicked();
        shipped = ui
            .button("Load the program's list")
            .on_hover_text("Replace the values with the reference list that comes with this version of ChemChron (codes, ages, sources, laser values). Materials only your lab has are kept; the old file is saved next to it as a backup.")
            .clicked();
        again = ui.button("Recognise references in labels again").on_hover_text("Run the identification over every EPMA analysis again").clicked();
    });
    egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
        let Some((sec, code)) = st.chosen.clone() else {
            ui.label(RichText::new("No reference material yet. Add one on the left or import reference-material files.").color(t.dim));
            return;
        };
        let mut delete = false;
        let mut move_to: Option<String> = None;
        egui::ScrollArea::vertical().id_salt("ref_detail").show(ui, |ui| {
            let Some(Value::Object(e)) = nested.get_mut(&sec).and_then(|m| m.as_object_mut()).and_then(|m| m.get_mut(&code)) else { return };
            ui.horizontal(|ui| {
                ui.heading(code.as_str());
                egui::ComboBox::from_id_salt("ref_section").selected_text(section_label(&sec)).width(130.0).show_ui(ui, |ui| {
                    for s in &secs {
                        if ui.selectable_label(*s == sec, section_label(s)).clicked() && *s != sec {
                            move_to = Some(s.clone());
                        }
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Delete material").clicked() {
                        delete = true;
                    }
                });
            });
            section(ui, &t, "Material and reference age", |ui| {
                egui::Grid::new("ref_main").num_columns(2).show(ui, |ui| {
                    ui.label(RichText::new("Name").color(t.dim));
                    let mut s = value_text(e.get("name"));
                    if ui.add(egui::TextEdit::singleline(&mut s).desired_width(420.0)).changed() {
                        e.insert("name".into(), jstr(&s));
                    }
                    ui.end_row();
                    ui.label(RichText::new("Reference age (Ma)").color(t.dim));
                    ui.horizontal(|ui| {
                        for (k, w) in [("age", 80.0), ("age_2s", 60.0)] {
                            if k == "age_2s" {
                                ui.label("± 2σ");
                            }
                            let mut s = value_text(e.get(k));
                            if ui.add(egui::TextEdit::singleline(&mut s).desired_width(w)).changed() {
                                let v = match parse_locale(&s) {
                                    Ok(Some(x)) => jnum(x),
                                    _ => jstr(s.trim()),
                                };
                                e.insert(k.into(), v);
                            }
                        }
                    });
                    ui.end_row();
                    ui.label(RichText::new("Source").color(t.dim));
                    let mut s = value_text(e.get("source"));
                    if ui.add(egui::TextEdit::singleline(&mut s).desired_width(420.0)).changed() {
                        e.insert("source".into(), jstr(&s));
                    }
                    ui.end_row();
                });
                ui.label(RichText::new("The one accepted age of this material: EPMA compares chemical ages with it, Combine compares U–Pb ages with it.").color(t.dim).size(11.5));
            });
            section(ui, &t, "Identified in labels by", |ui| {
                ui.label(
                    RichText::new("An analysis whose label contains the code or one of these alternative codes as a word (e.g. WC425_03, M44-12) is taken as this reference material — in EPMA imports, laser sessions and linked laser data alike.")
                        .color(t.dim)
                        .size(11.5),
                );
                let laser_name = e.get("laser").and_then(|l| l.get("name")).and_then(|n| n.as_str()).map(String::from);
                let codes = chemchron_core::reference::codes_of(&code, &Value::Object(e.clone()));
                let mut remove: Option<String> = None;
                ui.horizontal_wrapped(|ui| {
                    tag(ui, &code, t.accent);
                    for c in codes.iter().skip(1) {
                        if laser_name.as_deref() == Some(c.as_str()) {
                            tag(ui, c, t.ok);
                            ui.label(RichText::new("(laser name)").size(11.0).color(t.dim)).on_hover_text("The name the laser sessions use for this material");
                        } else {
                            tag(ui, c, t.dim);
                            if ui.small_button("x").on_hover_text("Remove this alternative code").clicked() {
                                remove = Some(c.clone());
                            }
                        }
                        ui.add_space(4.0);
                    }
                    ui.add(egui::TextEdit::singleline(&mut st.new_alias).hint_text("alternative code").desired_width(110.0));
                    if ui.button("Add").clicked() && !st.new_alias.trim().is_empty() {
                        let a = st.new_alias.trim().to_string();
                        let list = e.entry("aliases").or_insert_with(|| json!([]));
                        if !list.is_array() {
                            *list = json!([]);
                        }
                        let arr = list.as_array_mut().unwrap();
                        if !codes.iter().any(|c| c.eq_ignore_ascii_case(&a)) {
                            arr.push(json!(a));
                        }
                        st.new_alias.clear();
                    }
                });
                if let Some(r) = remove {
                    if let Some(Value::Array(a)) = e.get_mut("aliases") {
                        a.retain(|x| x.as_str() != Some(r.as_str()));
                    }
                    if let Some(Value::Array(a)) = e.get_mut("laser").and_then(|l| l.get_mut("aliases")) {
                        a.retain(|x| x.as_str() != Some(r.as_str()));
                    }
                }
            });
            section(ui, &t, "Laser values (ratios and element contents)", |ui| {
                if !e.contains_key("laser") {
                    ui.label(RichText::new("No laser values for this material.").color(t.dim));
                    if ui.button("Add laser values").clicked() {
                        e.insert("laser".into(), json!({"name": code, "aliases": [], "matrix": sec, "source": "", "confirmed": false, "values": {}}));
                    }
                    return;
                }
                let Some(Value::Object(l)) = e.get_mut("laser") else { return };
                ui.horizontal(|ui| {
                    let mut ok = l.get("confirmed").and_then(|x| x.as_bool()).unwrap_or(false);
                    if ui.checkbox(&mut ok, "Checked against the certificate").changed() {
                        l.insert("confirmed".into(), json!(ok));
                    }
                    ui.label(RichText::new("Source").color(t.dim));
                    let mut s = value_text(l.get("source"));
                    if ui.add(egui::TextEdit::singleline(&mut s).desired_width(320.0)).changed() {
                        l.insert("source".into(), jstr(&s));
                    }
                });
                let vals = l.entry("values").or_insert_with(|| json!({}));
                if !vals.is_object() {
                    *vals = json!({});
                }
                let vals = vals.as_object_mut().unwrap();
                let mut remove: Option<String> = None;
                // ratios first, then elements
                let mut keys: Vec<String> = vals.keys().cloned().collect();
                keys.sort_by_key(|k| (!k.contains('/'), k.clone()));
                egui::Grid::new("ref_values").striped(true).num_columns(5).show(ui, |ui| {
                    for h in ["Value of", "Accepted", "2σ", "Unit", ""] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for k in keys {
                        let v = vals.get_mut(&k).unwrap();
                        ui.label(k.as_str());
                        for f in ["value", "unc2s"] {
                            let mut x = num(v.get(f)).unwrap_or(0.0);
                            let speed = x.abs().max(1e-6) * 1e-3;
                            if ui.add(egui::DragValue::new(&mut x).speed(speed).max_decimals(8)).changed() {
                                if let Value::Object(o) = v {
                                    o.insert(f.into(), json!(x));
                                }
                            }
                        }
                        ui.label(RichText::new(if k.contains('/') { "ratio" } else { "ppm" }).color(t.dim).size(11.5));
                        if ui.small_button("x").clicked() {
                            remove = Some(k.clone());
                        }
                        ui.end_row();
                    }
                });
                if let Some(r) = remove {
                    vals.remove(&r);
                }
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut st.new_key).hint_text("206Pb/238U or element, e.g. Ce").desired_width(200.0));
                    if ui.button("Add value").clicked() && !st.new_key.trim().is_empty() {
                        vals.entry(st.new_key.trim().to_string()).or_insert_with(|| json!({"value": 0.0, "unc2s": 0.0}));
                        st.new_key.clear();
                    }
                });
            });
        });
        if delete {
            if let Some(Value::Object(m)) = nested.get_mut(&sec) {
                m.remove(&code);
            }
            st.chosen = None;
        } else if let Some(to) = move_to {
            let e = nested.get_mut(&sec).and_then(|m| m.as_object_mut()).and_then(|m| m.remove(&code));
            if let Some(e) = e {
                if let Value::Object(m) = nested.entry(to.clone()).or_insert_with(|| Value::Object(Map::new())) {
                    m.insert(code.clone(), e);
                }
                st.chosen = Some((to, code));
            }
        }
    });
    if import {
        if let Some(files) = rfd::FileDialog::new().set_title("Reference-material files (JSON)").add_filter("Reference material (JSON)", &["json"]).pick_files() {
            let (added, merged, bad) = crate::refsync::import_reference_files(&mut nested, &files);
            app.say(import_message(&added, &merged, &bad), !bad.is_empty());
        }
    }
    if shipped {
        let backup = app.settings.dir.join(format!("reference_materials.before_{}.json", crate::longterm::timestamp().get(..10).unwrap_or("old").to_string()));
        let _ = std::fs::write(&backup, serde_json::to_string_pretty(&Value::Object(nested.clone())).unwrap_or_default());
        let (n, kept) = crate::refsync::with_shipped(&nested);
        nested = n;
        st.chosen = None;
        app.say(
            if kept.is_empty() {
                format!("Loaded the program's reference list. The old list is in {}.", backup.display())
            } else {
                format!("Loaded the program's reference list; kept your own {}. The old list is in {}.", kept.join(", "), backup.display())
            },
            false,
        );
    }
    app.refs_ui = st;
    if nested != before {
        app.settings.refs_nested = nested;
        app.save_settings();
        let refs = app.settings.refs();
        let dp = app.settings.decimals;
        for r in app.data.rows.iter_mut() {
            chemchron_core::reference::recompute_ref_fields_for_row(r, &refs, &dp);
        }
        app.data.refresh(&app.settings);
    }
    if again {
        app.reprocess("reference materials changed");
    }
}

pub fn import_message(added: &[String], merged: &[String], bad: &[String]) -> String {
    let mut parts = vec![format!("{} new reference materials", added.len()), format!("values added to {}", merged.len())];
    if !bad.is_empty() {
        parts.push(format!("could not read {}", bad.join(", ")));
    }
    format!("Reference-material files: {}.", parts.join("; "))
}
