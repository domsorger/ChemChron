// SPDX-License-Identifier: GPL-3.0-or-later
//! Long-term logs of reference materials (chemical and U–Pb, side by side)
//! and the U–Pb settings: discordance and precision criteria, label
//! prefixes of reference spots, and the U–Pb uncertainty model.

use crate::app::ChemApp;
use crate::rowtable::{self, Look};
use chemchron_core::constants::*;
use chemchron_core::row::*;
use chemchron_core::settings::find_log_entries_for_row;
use chemchron_core::upbstats::{self, age_type_label, AGE_TYPES, RATIOS};
use egui::{RichText, Ui};
use chemchron_laser::theme::{form_row, section, segmented};

/// Local time as "YYYY-MM-DDTHH:MM:SS" (UTC when the zone is unknown).
pub fn timestamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    // civil from days (Howard Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}", rem / 3600, (rem / 60) % 60, rem % 60)
}

fn log_chemical(app: &mut ChemApp) {
    let tag = app.combine.lt_tag.trim().to_string();
    let ts = timestamp();
    let mut new = Vec::new();
    let mut dup = 0;
    for r in &app.data.rows {
        if !truthy(r, "Is_Reference") || truthy(r, "No_EPMA") || !chemchron_core::stats::usable_for_age_statistics(r) {
            continue;
        }
        let (code, Some(age)) = (text(r, "Reference"), num(r, "Age_Ma")) else { continue };
        if code.is_empty() {
            continue;
        }
        if !find_log_entries_for_row(&app.settings.long_term_log, &text(r, "Row_ID"), &text(r, "Label"), &code, Some(age)).is_empty() {
            dup += 1;
            continue;
        }
        let mut e = Row::new();
        set(&mut e, "Timestamp", jstr(&ts));
        set(&mut e, "Reference", jstr(&code));
        set(&mut e, "Label", jstr(text(r, "Label")));
        set(&mut e, "Age_Ma", jnum(age));
        set(&mut e, "Age_2s_Ma", jopt(num(r, "Age_2s_Ma")));
        let s = chemchron_core::sessions::epma_session(r);
        set(&mut e, "SessionTag", jstr(if s.is_empty() { tag.clone() } else { s }));
        set(&mut e, "Mineral", jstr(chemchron_core::stats::mineral_of(r)));
        set(&mut e, "Row_ID", jstr(text(r, "Row_ID")));
        new.push(e);
    }
    let n = new.len();
    app.settings.long_term_log.extend(new);
    match app.settings.save_logs() {
        Ok(()) => app.say(format!("Logged {n} reference analyses to the chemical long-term log ({dup} were already logged)."), false),
        Err(e) => app.say(e, true),
    }
    app.data.refresh(&app.settings);
}

fn log_upb(app: &mut ChemApp) {
    let tag = app.combine.lt_tag.trim().to_string();
    let crit = app.settings.upb.criteria();
    let new = upbstats::upb_log_entries_from_rows(&app.data.rows, &tag, &crit, &timestamp());
    let key = |e: &Row| (text(e, "Row_ID"), text(e, "Label"), text(e, "Reference"), text(e, "t68_Ma"));
    let existing: Vec<_> = app.settings.upb_log.iter().map(key).collect();
    let (fresh, dup): (Vec<Row>, Vec<Row>) = new.into_iter().partition(|e| !existing.contains(&key(e)));
    let n = fresh.len();
    app.settings.upb_log.extend(fresh);
    match app.settings.save_logs() {
        Ok(()) => app.say(format!("Logged {n} reference spots to the U–Pb long-term log ({} were already logged).", dup.len()), false),
        Err(e) => app.say(e, true),
    }
    app.data.refresh(&app.settings);
}

pub fn page_long_term(app: &mut ChemApp, ui: &mut Ui) {
    let t = app.theme();
    ui.horizontal(|ui| {
        ui.heading("Long-term logs");
        ui.add_space(12.0);
        let mut v = app.combine.lt_upb;
        if segmented(ui, &t, &mut v, &[(false, "Chemical (EPMA)"), (true, "Isotopic (U–Pb)")]) {
            app.combine.lt_upb = v;
        }
    });
    ui.label(RichText::new(format!("Reference-material ages logged session after session, shared by all projects ({} chemical, {} U–Pb entries). They give the long-term reproducibility and, when chosen, the systematic uncertainty term. The entries themselves are listed and edited in Settings > Uncertainty models.", app.settings.long_term_log.len(), app.settings.upb_log.len())).color(t.dim).size(12.0));
    ui.horizontal_wrapped(|ui| {
        ui.label("Session tag for analyses without a session");
        ui.add(egui::TextEdit::singleline(&mut app.combine.lt_tag).desired_width(150.0));
        if ui.button(if app.combine.lt_upb { "Log the U–Pb reference spots of this project" } else { "Log the reference analyses of this project" }).clicked() {
            if app.combine.lt_upb {
                log_upb(app);
            } else {
                log_chemical(app);
            }
        }
        ui.label("Mineral");
        egui::ComboBox::from_id_salt("lt_min").selected_text(app.combine.lt_mineral.clone()).show_ui(ui, |ui| {
            for m in MINERAL_NAMES {
                ui.selectable_value(&mut app.combine.lt_mineral, m.to_string(), m);
            }
        });
        if app.combine.lt_upb {
            ui.label("Age");
            egui::ComboBox::from_id_salt("lt_type").selected_text(age_type_label(&app.combine.lt_type)).show_ui(ui, |ui| {
                for ty in AGE_TYPES {
                    ui.selectable_value(&mut app.combine.lt_type, ty.to_string(), age_type_label(ty));
                }
            });
        }
    });
    let refs = app.settings.refs();
    let dp = app.settings.decimals;
    let mineral = app.combine.lt_mineral.clone();
    let flag_colors = app.group_colors();
    let look = Look { t: &t, dp: &dp, flag_colors: &flag_colors, show_bdl_values: true };
    if !app.combine.lt_upb {
        let log: Vec<Row> = app.settings.long_term_log.iter().filter(|e| chemchron_core::stats::mineral_of(e) == mineral).cloned().collect();
        let mut ov = chemchron_core::stats::long_term_overview(&log, &refs, &dp);
        for r in ov.iter_mut() {
            set(r, "Row_ID", jstr(text(r, "Reference")));
        }
        let (pooled, contributing) = chemchron_core::stats::pooled_long_term_rel_pct(&ov, chemchron_core::stats::MIN_N_FOR_LONG_TERM_REL_PCT);
        let s = app.settings.uncertainty.get(&mineral).cloned().unwrap_or_default();
        let mm = app.data.models.as_ref().and_then(|x| x.chem.get(&mineral));
        let ebs = mm.map(|x| x.excess_by_session.iter().filter(|(k, _)| !k.is_empty()).map(|(k, v)| (k.clone(), v.0)).collect()).unwrap_or_default();
        let (between, mats) = chemchron_core::uncertainty::between_session_systematic(&app.settings.long_term_log, &mineral, &s.excess_excluded_refs, mm.map(|x| x.excess_pct), &ebs);
        section(ui, &t, &format!("Chemical ages, {mineral}"), |ui| {
            ui.label(match pooled {
                Some(p) => format!("Pooled RMS deviation from the accepted ages: {p:.2} % ({})", contributing.iter().map(|(c, v, n)| format!("{c} {v:.2} %, n = {n}")).collect::<Vec<_>>().join("; ")),
                None => "No reference material has 3 logged analyses yet.".into(),
            });
            ui.label(match between {
                Some(p) => format!("Between-session systematic term (2σ): {p:.2} %{}", if s.systematic_source == "log" { " — in use" } else { " — not in use (Settings > Uncertainty models)" }),
                None => "Between-session systematic term: needs a material with two logged sessions of ≥ 3 analyses.".into(),
            });
            for m in mats {
                if let Some(b) = m.between_2s_pct {
                    ui.label(RichText::new(format!("{}: {} sessions usable, between-session excess {b:.2} %", m.reference, m.usable_sessions)).size(12.0).color(t.dim));
                }
            }
            let cols = chemchron_core::columns::s(&["Reference", "N_logged", "LongTerm_Mean_Age_Ma", "LongTerm_2s_Ma", "Accepted_Age_Ma", "LongTerm_Diff_Ma", "LongTerm_RelDev_pct"]);
            let idx: Vec<usize> = (0..ov.len()).collect();
            ui.allocate_ui(egui::vec2(ui.available_width(), 180.0), |ui| {
                rowtable::show(ui, "lt_chem", &ov, &idx, &cols, &mut app.combine.sum_refs, &look);
            });
        });
    } else {
        let ty = app.combine.lt_type.clone();
        let mut ov = upbstats::upb_long_term_overview(&app.settings.upb_log, &refs, &ty, Some(&mineral), dp.age);
        for r in ov.iter_mut() {
            set(r, "Row_ID", jstr(text(r, "Reference")));
        }
        let s = app.settings.upb.by_mineral().remove(&mineral).unwrap_or_default();
        let (between, _) = upbstats::between_session_upb_systematic(&app.settings.upb_log, &mineral, &ty, &s.excluded_refs);
        section(ui, &t, &format!("{} ages, {mineral}", age_type_label(&ty)), |ui| {
            ui.label(match between {
                Some(p) => format!("Between-session systematic term (2σ): {p:.2} %{}", if s.systematic_source == "log" { " — in use" } else { " — not in use (Settings > Uncertainty models)" }),
                None => "Between-session systematic term: needs a material with two logged sessions of ≥ 3 spots.".into(),
            });
            let cols = chemchron_core::columns::s(&["Reference", "N_logged", "N_sessions", "Mean_Ma", "SD2_Ma", "WM_Ma", "WM_2s_Ma", "MSWD", "Accepted_Ma", "Accepted_2s_Ma", "Diff_Ma", "Diff_pct"]);
            let idx: Vec<usize> = (0..ov.len()).collect();
            ui.allocate_ui(egui::vec2(ui.available_width(), 180.0), |ui| {
                rowtable::show(ui, "lt_upb", &ov, &idx, &cols, &mut app.combine.sum_flags, &look);
            });
        });
    }
    // the log over time
    let (age_key, s2_key) = if app.combine.lt_upb { (format!("{}_Ma", app.combine.lt_type), format!("{}_2s_Ma", app.combine.lt_type)) } else { ("Age_Ma".to_string(), "Age_2s_Ma".to_string()) };
    let log = if app.combine.lt_upb { &app.settings.upb_log } else { &app.settings.long_term_log };
    let points: Vec<crate::plots::LogPoint> = log
        .iter()
        .filter(|e| chemchron_core::stats::mineral_of(e) == mineral)
        .filter_map(|e| {
            let code = text(e, "Reference");
            let acc = chemchron_core::reference::ref_num(&refs, &code, "age")?;
            let age = num(e, &age_key)?;
            let s2 = num(e, &s2_key).unwrap_or(0.0);
            (acc > 0.0).then(|| crate::plots::LogPoint {
                reference: code,
                session: { let s = text(e, "SessionTag"); if s.is_empty() { "(no session)".into() } else { s } },
                stamp: text(e, "Timestamp"),
                label: text(e, "Label"),
                dev: 100.0 * (age - acc) / acc,
                dev2s: 100.0 * s2 / acc,
            })
        })
        .collect();
    let title = format!("{} long-term log, {mineral}", if app.combine.lt_upb { format!("{} age", age_type_label(&app.combine.lt_type)) } else { "Chemical age".to_string() });
    section(ui, &t, "Over time", |ui| {
        if points.is_empty() {
            ui.label(RichText::new("No logged analyses of reference materials with an accepted age for this mineral yet.").color(t.dim));
            return;
        }
        let fig = crate::plots::long_term_plot(&points, &title, &flag_colors);
        crate::epma_pages::offer(app, &fig, if app.combine.lt_upb { "long_term_log_upb" } else { "long_term_log_chemical" });
        if ui.button("Save figure…").clicked() {
            crate::epma_pages::save_fig(app, &fig, "long_term_log");
        }
        let h = ui.available_height().clamp(240.0, 420.0);
        crate::figure::show(ui, &t, &fig, &mut app.combine.lt_view, h);
    });
}

/// The long-term log entries (Settings > Uncertainty models, lower part).
pub fn edit_entries(app: &mut ChemApp, ui: &mut Ui, upb: bool) {
    let t = app.theme();
    let fields: Vec<String> = if upb { chemchron_core::settings::upb_log_fields() } else { chemchron_core::settings::LONG_TERM_FIELDS.iter().map(|s| s.to_string()).collect() };
    let mut delete: Option<usize> = None;
    let mut changed = false;
    let n = if upb { app.settings.upb_log.len() } else { app.settings.long_term_log.len() };
    section(ui, &t, &format!("Long-term log entries, {} ({n})", if upb { "U–Pb" } else { "chemical" }), |ui| {
        ui.label(RichText::new("Reference analyses logged session after session, shared by all projects. Edit a session tag, label or reference, or delete an entry; changes are saved straight away. New entries come from Combined results > Long-term logs (Log the reference analyses of this project).").color(t.dim).size(12.0));
        // filters: mineral, reference material, session, any text
        let log_now = if upb { &app.settings.upb_log } else { &app.settings.long_term_log };
        let distinct = |k: &str| {
            let mut v: Vec<String> = log_now.iter().map(|e| text(e, k)).filter(|x| !x.is_empty()).collect();
            v.sort();
            v.dedup();
            v
        };
        let (minerals, references, sessions) = (distinct("Mineral"), distinct("Reference"), distinct("SessionTag"));
        let f = &mut app.combine.lt_filter;
        ui.horizontal_wrapped(|ui| {
            let combo = |ui: &mut Ui, id: &str, label: &str, value: &mut String, options: &[String]| {
                ui.label(RichText::new(label).color(t.dim));
                egui::ComboBox::from_id_salt((id, upb)).selected_text(if value.is_empty() { "all".to_string() } else { value.clone() }).width(110.0).show_ui(ui, |ui| {
                    ui.selectable_value(value, String::new(), "all");
                    for o in options {
                        ui.selectable_value(value, o.clone(), o);
                    }
                });
            };
            combo(ui, "ltf_min", "Mineral", &mut f.mineral, &minerals);
            combo(ui, "ltf_ref", "Reference", &mut f.reference, &references);
            combo(ui, "ltf_ses", "Session", &mut f.session, &sessions);
            ui.label(RichText::new("Find").color(t.dim));
            ui.add(egui::TextEdit::singleline(&mut f.search).desired_width(120.0).hint_text("label, date, …"));
            if ui.small_button("Clear filters").clicked() {
                *f = Default::default();
            }
        });
        let filter = app.combine.lt_filter.clone();
        let shows = |e: &Row| {
            (filter.mineral.is_empty() || text(e, "Mineral") == filter.mineral)
                && (filter.reference.is_empty() || text(e, "Reference") == filter.reference)
                && (filter.session.is_empty() || text(e, "SessionTag") == filter.session)
                && (filter.search.trim().is_empty() || fields.iter().any(|k| text(e, k).to_lowercase().contains(&filter.search.trim().to_lowercase())))
        };
        let n_shown = log_now.iter().filter(|e| shows(e)).count();
        ui.label(RichText::new(format!("{n_shown} of {} entries shown", log_now.len())).color(t.dim).size(12.0));
        egui::ScrollArea::both().id_salt(("lt_entries_scroll", upb)).max_height((ui.available_height() - 16.0).max(120.0)).show(ui, |ui| {
            egui::Grid::new(("lt_edit", upb)).striped(true).show(ui, |ui| {
                for f in &fields {
                    ui.label(RichText::new(f).strong().size(12.0));
                }
                ui.label("");
                ui.end_row();
                let log = if upb { &mut app.settings.upb_log } else { &mut app.settings.long_term_log };
                for (i, e) in log.iter_mut().enumerate() {
                    if !shows(e) {
                        continue;
                    }
                    for f in &fields {
                        if matches!(f.as_str(), "SessionTag" | "Label" | "Reference") {
                            let mut s = text(e, f);
                            let w = match f.as_str() {
                                "Reference" => 80.0,
                                "Label" => 150.0,
                                _ => 120.0,
                            };
                            if ui.add_sized([w, 20.0], egui::TextEdit::singleline(&mut s)).changed() {
                                set(e, f, jstr(s));
                                changed = true;
                            }
                        } else {
                            ui.label(RichText::new(rowtable::cell_text(e, f, &DecimalPlaces::default(), true)).size(12.0));
                        }
                    }
                    if ui.small_button("Delete").clicked() {
                        delete = Some(i);
                    }
                    ui.end_row();
                }
            });
        });
    });
    if let Some(i) = delete {
        if upb {
            app.settings.upb_log.remove(i);
        } else {
            app.settings.long_term_log.remove(i);
        }
        changed = true;
    }
    if changed {
        if let Err(e) = app.settings.save_logs() {
            app.say(e, true);
        }
        app.data.refresh(&app.settings);
    }
}

/// `uncertainty`: only the U–Pb uncertainty model; otherwise the U–Pb criteria and REE normalisation.
pub fn page_upb_settings(app: &mut ChemApp, ui: &mut Ui, uncertainty: bool) {
    let t = app.theme();
    let before = app.settings.upb.clone();
    let before_norm = app.settings.norm.clone();
    let before_rules = app.settings.label_rules.clone();
    if app.combine.upb_mineral.is_empty() {
        // first visit: the open laser session's mineral
        app.combine.upb_mineral = app.laser_mineral();
    }
    let mineral = app.combine.upb_mineral.clone();
    let rows = app.data.rows.clone();
    let crit = app.settings.upb.criteria();
    // for the uncertainty view: sessions in use and the long-term-log term
    let upb_sessions = app.data.models.as_ref().and_then(|x| x.upb.get(&mineral)).map(|m| m.sessions.clone()).unwrap_or_default();
    let summary_type = app.settings.upb.summary_age_type.clone();
    let excluded_now = upbstats::excluded_for_age(&app.settings.upb.by_mineral().remove(&mineral).unwrap_or_default(), &summary_type);
    let (between, between_mats) = upbstats::between_session_upb_systematic(&app.settings.upb_log, &mineral, &summary_type, &excluded_now);
    let upb_log = app.settings.upb_log.clone();
    // the open laser session's excess, as the LA-ICP-MS part works it out
    let laser_excess: Option<String> = app.laser.red.upb.as_ref().map(|m| {
        use chemchron_laser_core::drs::upb::RatioKind;
        let parts: Vec<String> = RatioKind::ALL.iter().filter_map(|k| m.ratio(*k).map(|r| format!("{} {:.2} %", k.key(), 200.0 * r.excess))).collect();
        let left: Vec<String> = RatioKind::ALL.iter().filter_map(|k| m.ratio(*k).map(|r| (k, r))).flat_map(|(k, r)| r.left_out.iter().map(move |(n, _)| format!("{n} ({})", k.key()))).collect();
        let manual = RatioKind::ALL.iter().filter_map(|k| m.ratio(*k)).any(|r| r.manual);
        let n_sec = RatioKind::ALL.iter().filter_map(|k| m.ratio(*k)).map(|r| r.n_secondary).max().unwrap_or(0);
        format!(
            "Excess in use for this session (2σ): {} \u{2014} {}{}",
            parts.join(" · "),
            if manual { "the manual values".to_string() } else { format!("from {} primary and {n_sec} secondary analyses", m.primary_n) },
            if left.is_empty() { String::new() } else { format!("; left out: {}", left.join(", ")) }
        )
    });
    // the open laser session's reference materials, per ratio (as soon as data
    // are loaded), shown with the settings of the session's mineral
    let laser_mineral = app.laser_mineral();
    let laser_table: Option<(String, Vec<(&'static str, &'static str, Vec<chemchron_laser_core::drs::upb::MaterialStat>)>)> = app.laser.red.upb.as_ref().map(|m| {
        use chemchron_laser_core::drs::upb::RatioKind;
        let lab_key = |k: RatioKind| match k {
            RatioKind::R68 => "r86",
            RatioKind::R75 => "r75",
            RatioKind::R76 => "r76",
            RatioKind::R82 => "r82",
        };
        (app.laser.session.name.clone(), RatioKind::ALL.iter().filter_map(|k| m.ratio(*k).map(|r| (k.key(), lab_key(*k), r.materials.clone()))).collect())
    });
    let laser_codes: std::collections::BTreeMap<String, String> = laser_table
        .iter()
        .flat_map(|(_, v)| v.iter().flat_map(|x| x.2.iter().map(|m| m.name.clone())))
        .map(|n| (n.clone(), crate::refsync::lab_code_of_laser_rm(&app.settings.refs_nested, &n).unwrap_or(n)))
        .collect();
    egui::ScrollArea::vertical().show(ui, |ui| {
        if !uncertainty {
            ui.heading("U–Pb and Combine");
        }
        let u = &mut app.settings.upb;
        if !uncertainty {
        section(ui, &t, "Discordance and precision", |ui| {
            form_row(ui, &t, "Discordance", |ui| {
                ui.radio_value(&mut u.disc_definition, "76".to_string(), "1 − t68/t76");
                ui.radio_value(&mut u.disc_definition, "75".to_string(), "1 − t68/t75 (young ages)");
            });
            form_row(ui, &t, "Threshold (%)", |ui| {
                ui.add(egui::DragValue::new(&mut u.disc_threshold_pct).speed(0.1).range(0.0..=100.0));
            });
            ui.checkbox(&mut u.disc_ellipse_test, "Only discordant when the 95 % ellipse also misses the concordia");
            form_row(ui, &t, "Always above (%)", |ui| {
                ui.add(egui::DragValue::new(&mut u.disc_hard_limit_pct).speed(0.1).range(0.0..=100.0));
            });
            ui.checkbox(&mut u.precision_filter, "Leave out imprecise spots");
            form_row(ui, &t, "Rel. 2SE above (%)", |ui| {
                ui.add_enabled(u.precision_filter, egui::DragValue::new(&mut u.max_rel_2se_pct).speed(0.1).range(0.1..=100.0));
            });
        });
        ui.label(RichText::new("Laser spots without an EPMA analysis are recognised as reference materials by the codes and alternative codes in Settings > Reference materials.").color(t.dim).size(12.0));
        }
        if uncertainty {
            // the same layout as the chemical model: heading, what it does,
            // mineral, random excess, systematic term, what is in use
            ui.heading("Uncertainty model (U–Pb ages)");
            ui.label(RichText::new("For the whole program: the LA-ICP-MS part and Combined results use it alike.\n1. Random excess per ratio (2σ, %), per LA-ICP-MS session: from the session's reference materials (primary and secondaries; the excess that brings their pooled MSWD to 1), or manual values. It is added to each analysis's analytical uncertainty (analytical + excess) and enters weights, MSWD and outlier rejection.\n2. Systematic (2σ %): from the U–Pb long-term log (manual value until it holds 3 sessions), with the primary's accepted value and optionally the decay constants. Part of every total: single analyses, weighted means and concordia ages.").color(t.dim).size(12.0));
            ui.horizontal(|ui| {
                ui.label("Mineral");
                for m in MINERAL_NAMES {
                    ui.selectable_value(&mut app.combine.upb_mineral, m.to_string(), m);
                }
            });
            let mut s = u.by_mineral().remove(&mineral).unwrap_or_default();
            let b = s.clone();
            let table = upbstats::ratio_excess_table(&rows, &mineral, &s.excluded_refs, &s.excluded_ratios, &crit, None);
            section(ui, &t, "Random excess per ratio", |ui| {
                ui.label(RichText::new("The random excess is added to every analysis's analytical uncertainty (analytical + excess). It is one value per ratio and LA-ICP-MS session: the excess that brings the scatter of the session's reference materials to MSWD 1. A session without reference-material ratios uses the manual values.").color(t.dim).size(12.0));
                form_row(ui, &t, "Source", |ui| {
                    ui.radio_value(&mut s.excess_source, "project".to_string(), "From each session's reference materials");
                    ui.radio_value(&mut s.excess_source, "manual".to_string(), "Manual values for all sessions");
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Manual values (2σ %)").color(t.dim));
                    for r in RATIOS {
                        ui.label(upbstats::ratio_label(r));
                        let v = s.excess_manual_pct.entry(r.to_string()).or_insert(0.0);
                        ui.add(egui::DragValue::new(v).speed(0.05).range(0.0..=50.0).suffix(" %"));
                        ui.add_space(8.0);
                    }
                });
                ui.label(RichText::new("Used for all sessions when chosen above, and otherwise for sessions without reference-material ratios.").color(t.dim).size(11.5));
                ui.add_space(6.0);

                // 1. the open laser session
                t.card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    match laser_table.as_ref().filter(|_| mineral == laser_mineral) {
                        None => {
                            ui.label(RichText::new("1. LA-ICP-MS part").strong());
                            ui.label(RichText::new(if laser_table.is_some() { format!("The open laser session is {laser_mineral}; choose that mineral above to see it.") } else { "No laser session with U–Pb results is open.".to_string() }).color(t.dim).size(12.0));
                        }
                        Some((name, ratios)) => {
                            ui.label(RichText::new(format!("1. LA-ICP-MS part: open session \u{201c}{name}\u{201d}")).strong());
                            if let Some(l) = &laser_excess {
                                ui.label(RichText::new(l).strong().color(t.accent));
                            }
                            ui.label(RichText::new("Its reference materials and the scatter each shows on its own: MSWD with the analytical uncertainties only · the excess (2σ %) it alone would need. The primary is compared with its normalisation curve, the secondaries with their own mean. Untick a material or a single ratio to leave it out.").color(t.dim).size(12.0));
                            let mut names: Vec<(String, bool)> = Vec::new();
                            for (_, _, mats) in ratios {
                                for m in mats {
                                    if !names.iter().any(|x| x.0 == m.name) {
                                        names.push((m.name.clone(), m.primary));
                                    }
                                }
                            }
                            egui::Grid::new("laser_exc").striped(true).show(ui, |ui| {
                                for h in ["Use", "Reference material", "N"] {
                                    ui.label(RichText::new(h).strong());
                                }
                                for (label, _, _) in ratios {
                                    ui.label(RichText::new(format!("{label}  MSWD · excess")).strong());
                                }
                                ui.end_row();
                                for (n, primary) in &names {
                                    let code = laser_codes.get(n).cloned().unwrap_or_else(|| n.clone());
                                    let mut on = !s.excluded_refs.contains(&code);
                                    if ui.add_enabled(!*primary, egui::Checkbox::without_text(&mut on)).on_hover_text("Use this reference material for the excess").on_disabled_hover_text("The primary reference material is always used").changed() {
                                        if on {
                                            s.excluded_refs.retain(|x| *x != code);
                                        } else {
                                            s.excluded_refs.push(code.clone());
                                        }
                                    }
                                    ui.label(if *primary { format!("{n} ({code}), primary") } else if code != *n { format!("{n} ({code}), secondary") } else { format!("{n}, secondary") });
                                    let nn = ratios.iter().flat_map(|r| r.2.iter()).filter(|m| m.name == *n).map(|m| m.n).max().unwrap_or(0);
                                    ui.label(nn.to_string());
                                    for (_, key, mats) in ratios {
                                        ui.horizontal(|ui| {
                                            let Some(m) = mats.iter().find(|m| m.name == *n) else {
                                                ui.label("–");
                                                return;
                                            };
                                            let k = format!("{code}|{key}");
                                            let mut use_it = !s.excluded_ratios.contains(&k);
                                            if ui.add_enabled(on && !*primary, egui::Checkbox::without_text(&mut use_it)).on_hover_text("Use this ratio of this material").changed() {
                                                if use_it {
                                                    s.excluded_ratios.retain(|x| *x != k);
                                                } else {
                                                    s.excluded_ratios.push(k.clone());
                                                }
                                            }
                                            let text = format!("{} · {:.2} %", if m.mswd.is_finite() { format!("{:.2}", m.mswd) } else { "–".into() }, m.excess_2s_pct);
                                            match &m.left_out {
                                                Some(why) => {
                                                    ui.label(RichText::new(format!("{text} (left out)")).color(t.warn)).on_hover_text(why);
                                                }
                                                None => {
                                                    ui.label(text);
                                                }
                                            }
                                        });
                                    }
                                    ui.end_row();
                                }
                            });
                            ui.label(RichText::new("Reference materials younger than 1000 Ma are left out of ²⁰⁷Pb/²⁰⁶Pb (their Pb–Pb age is too imprecise to be trusted). Reference spots are not tested for discordance here, since their age is known.").color(t.dim).size(11.5));
                        }
                    }
                });
                ui.add_space(6.0);

                // 2. U–Pb results imported from files
                t.card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("2. U–Pb results imported from files (reduced ratio tables)").strong());
                    ui.label(RichText::new("Results imported in Combined results > Data > Import laser results from files. Their excess is worked out here the same way, per session from their reference spots (an export whose uncertainties already contain an excess gives about 0 here). Results sent from the LA-ICP-MS part are not listed: their excess is the one in part 1.").color(t.dim).size(12.0));
                    if upb_sessions.is_empty() {
                        ui.label(RichText::new("No U–Pb results from files in this project for this mineral.").color(t.dim));
                        return;
                    }
                    egui::Grid::new("upb_exc_sess").striped(true).show(ui, |ui| {
                        for h in ["Session", "Spots", "Reference spots"] {
                            ui.label(RichText::new(h).strong());
                        }
                        for r in RATIOS {
                            ui.label(RichText::new(format!("{} excess in use", upbstats::ratio_label(r))).strong());
                        }
                        ui.end_row();
                        for e in &upb_sessions {
                            ui.label(chemchron_core::sessions::display_session(&e.session));
                            ui.label(e.n_spots.to_string());
                            ui.label(e.n_ref.to_string());
                            for r in RATIOS {
                                ui.label(e.ratios.get(r).map(|(_, pct, src)| format!("{pct:.2} % ({src})")).unwrap_or("–".into()));
                            }
                            ui.end_row();
                        }
                    });
                    egui::CollapsingHeader::new("Reference materials of these files").id_salt("upb_file_refs").default_open(false).show(ui, |ui| {
                        egui::Grid::new("upb_exc").striped(true).show(ui, |ui| {
                            for h in ["Use", "Reference", "N", "Sessions"] {
                                ui.label(RichText::new(h).strong());
                            }
                            for r in RATIOS {
                                ui.label(RichText::new(format!("{}  MSWD · excess", upbstats::ratio_label(r))).strong());
                            }
                            ui.end_row();
                            for m in &table.materials {
                                let mut on = !s.excluded_refs.contains(&m.reference);
                                if ui.checkbox(&mut on, "").on_hover_text("Leave a heterogeneous material out of the estimate (all ratios)").changed() {
                                    if on {
                                        s.excluded_refs.retain(|x| *x != m.reference);
                                    } else {
                                        s.excluded_refs.push(m.reference.clone());
                                    }
                                }
                                ui.label(&m.reference);
                                ui.label(m.n.to_string());
                                ui.label(m.sessions.to_string());
                                for r in RATIOS {
                                    ui.horizontal(|ui| {
                                        let key = format!("{}|{r}", m.reference);
                                        let mut use_it = !s.excluded_ratios.contains(&key);
                                        if ui.add_enabled(on, egui::Checkbox::without_text(&mut use_it)).on_hover_text("Use this ratio of this material").changed() {
                                            if use_it {
                                                s.excluded_ratios.retain(|x| *x != key);
                                            } else {
                                                s.excluded_ratios.push(key.clone());
                                            }
                                        }
                                        let text = m.ratios.get(r).map(|(ms, ex)| format!("{ms:.2} · {ex:.2} %")).unwrap_or("–".into());
                                        match m.left_out.get(r) {
                                            Some(why) => {
                                                ui.label(RichText::new(format!("{text} (left out)")).color(t.warn)).on_hover_text(why);
                                            }
                                            None => {
                                                ui.label(text);
                                            }
                                        }
                                    });
                                }
                                ui.end_row();
                            }
                        });
                    });
                });
            });
            section(ui, &t, "Systematic (part of every total: single analyses, means, concordia ages)", |ui| {
                form_row(ui, &t, "Source", |ui| {
                    ui.radio_value(&mut s.systematic_source, "log".to_string(), "From the U–Pb long-term log (manual value until it holds 3 sessions)");
                    ui.radio_value(&mut s.systematic_source, "manual".to_string(), "Manual");
                });
                ui.checkbox(&mut u.decay_in_systematic, "Include the decay-constant uncertainties").on_hover_text("Adds the decay-constant uncertainties (Settings > Decay constants) to the systematic term of every mean, isochron and concordia age: λ²³⁸U for ²⁰⁶Pb/²³⁸U ages and chemical ages, λ²³⁵U for ²⁰⁷Pb/²³⁵U, both for ²⁰⁷Pb/²⁰⁶Pb. λ²³²Th has no stated uncertainty. The same switch for U–Pb and chemical ages.");
                ui.label(RichText::new("For laser results the accepted-value uncertainty of the session's primary reference material is added as well.").color(t.dim).size(12.0));
                form_row(ui, &t, "Manual (2σ %)", |ui| {
                    ui.add(egui::DragValue::new(&mut s.systematic_pct).speed(0.05).range(0.0..=20.0));
                });
                for mat in &between_mats {
                    ui.label(format!(
                        "{}: {} logged sessions, {} with ≥ 3 spots{}",
                        mat.reference,
                        mat.sessions,
                        mat.usable_sessions,
                        mat.between_2s_pct.map(|v| format!(", between-session excess {v:.2} %")).unwrap_or_default() + &mat.left_out.as_ref().map(|w| format!(" — left out: {w}")).unwrap_or_default()
                    ));
                }
                ui.label(match between {
                    Some(p) => format!("From the long-term log ({} ages): {p:.2} % (2σ)", age_type_label(&summary_type)),
                    None => "The U–Pb long-term log does not hold 3 sessions with ≥ 3 reference spots yet (log them in Combined results > Long-term logs); the manual value is used.".into(),
                });
            });
            // as the calculations choose it (per mineral and age type)
            let resolved = upbstats::resolve_upb_systematic(&u.by_mineral(), &upb_log);
            let (sys, sys_src) = resolved.by_mineral.get(&mineral).and_then(|m| m.get(&summary_type)).cloned().unwrap_or((s.systematic_pct, "manual".to_string()));
            let random = if s.excess_source == "project" { "from the reference spots, per session (table above)".to_string() } else {
                RATIOS.iter().map(|r| format!("{} {:.2} %", upbstats::ratio_label(r), s.excess_manual_pct.get(*r).copied().unwrap_or(0.0))).collect::<Vec<_>>().join(", ") + " (manual)"
            };
            ui.label(RichText::new(format!("In use for {mineral}: random excess {random}; systematic {sys:.2} % ({sys_src}, {} ages).", age_type_label(&summary_type))).strong());
            if s != b {
                u.uncertainty_by_mineral.insert(mineral.clone(), s);
            }
        }
        if !uncertainty {
        section(ui, &t, "Linking laser and EPMA labels", |ui| {
            ui.label(RichText::new("Laser spots are linked to EPMA analyses with the same label (case and leading zeros of the last number ignored). When the two are named differently, rules can bring them together; they also apply to the internal standard taken from EPMA.").color(t.dim).size(12.0));
            let lr = &mut app.settings.label_rules;
            ui.checkbox(&mut lr.ignore_separators, "Ignore - _ . and spaces (mnz5_01 = mnz5-1 = mnz5 1)");
            ui.label(RichText::new("Replace in laser labels (in this order):").strong());
            let mut remove: Option<usize> = None;
            egui::Grid::new("label_rules").num_columns(4).show(ui, |ui| {
                for (k, (find, by)) in lr.replace.iter_mut().enumerate() {
                    ui.add(egui::TextEdit::singleline(find).desired_width(140.0).hint_text("find"));
                    ui.label("by");
                    ui.add(egui::TextEdit::singleline(by).desired_width(140.0).hint_text("replace (empty: delete)"));
                    if ui.small_button("×").clicked() {
                        remove = Some(k);
                    }
                    ui.end_row();
                }
            });
            if let Some(k) = remove {
                lr.replace.remove(k);
            }
            if ui.button("Add a rule").clicked() {
                lr.replace.push((String::new(), String::new()));
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Try a laser label").color(t.dim));
                ui.add(egui::TextEdit::singleline(&mut app.combine.label_try).desired_width(180.0));
                if !app.combine.label_try.trim().is_empty() {
                    let key = lr.laser_key(app.combine.label_try.trim());
                    let hit = rows.iter().find(|r| lr.epma_key(&text(r, "Label")) == key).map(|r| text(r, "Label"));
                    match hit {
                        Some(l) => ui.colored_label(t.ok, format!("links to EPMA analysis {l}")),
                        None => ui.colored_label(t.warn, format!("no EPMA analysis with key \"{key}\"")),
                    };
                }
            });
        });
        section(ui, &t, "Chondrite normalisation (REE diagram)", |ui| {
            let n = &mut app.settings.norm;
            for set_ in n.sets.iter_mut() {
                ui.label(RichText::new(&set_.name).strong());
                ui.label(RichText::new(&set_.source).size(11.5).color(t.dim));
                ui.horizontal_wrapped(|ui| {
                    for el in chemchron_core::ree::NORMALIZATION_ELEMENTS {
                        ui.label(el);
                        let mut v = set_.values.get(el).copied().unwrap_or(0.0);
                        if ui.add(egui::DragValue::new(&mut v).speed(0.001).max_decimals(4).range(0.0..=100.0)).changed() {
                            if v > 0.0 {
                                set_.values.insert(el.to_string(), v);
                            } else {
                                set_.values.remove(el);
                            }
                        }
                    }
                });
            }
        });
        }
    });
    if app.settings.upb != before || app.settings.norm != before_norm || app.settings.label_rules != before_rules {
        app.save_settings();
        app.data.refresh(&app.settings);
    }
}
