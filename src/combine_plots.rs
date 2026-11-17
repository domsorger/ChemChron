// SPDX-License-Identifier: GPL-3.0-or-later
//! Combine plots: concordia, isotopic weighted mean, REE diagram,
//! composition plots and chemical against isotopic ages.

use crate::app::ChemApp;
use crate::epma::{group_choices, group_picker, group_rows, GroupPick};
use crate::epma_pages::{offer, save_fig};
use crate::figure::{self, Figure};
use crate::plots::{self, Bar, Colors};
use chemchron_core::columns::{display_name, plottable_fields};
use chemchron_core::concordia::{concordia_age, discordia, reject_concordia_outliers, Pt};
use chemchron_core::row::*;
use chemchron_core::upb::{has_upb, upb_rejection};
use chemchron_core::upbstats::{age_type_label, concordia_data, isotopic_weighted_mean, preferred_age_type, systematic_for, AGE_TYPES};
use egui::{Color32, RichText, Ui};
use chemchron_laser::theme::{fmt_pm, segmented, stat};

fn sel_state(app: &ChemApp) -> &crate::rowtable::TableState {
    crate::epma_pages::selection_state(app)
}

fn age_type_combo(ui: &mut Ui, id: &str, v: &mut String) {
    egui::ComboBox::from_id_salt(id).selected_text(age_type_label(v)).show_ui(ui, |ui| {
        for t in AGE_TYPES {
            ui.selectable_value(v, t.to_string(), age_type_label(t));
        }
    });
}

pub fn page_concordia(app: &mut ChemApp, ui: &mut Ui) {
    let t = app.theme();
    ui.heading("Concordia");
    let choices = group_choices(&app.data.rows, true);
    let reject = app.settings.reject_outliers;
    {
        let c = &mut app.combine;
        let before = (c.conc_group.clone(), c.conc_wetherill);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Analyses").color(t.dim));
            group_picker(ui, "conc_group", &mut c.conc_group, &choices);
            segmented(ui, &t, &mut c.conc_wetherill, &[(false, "Tera–Wasserburg"), (true, "Wetherill")]);
            ui.checkbox(&mut c.conc_show_disc, "Show discordant / imprecise / excluded");
        });
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut c.conc_age, "Concordia age");
            ui.add_enabled(c.conc_age, egui::Checkbox::new(&mut c.conc_decay, "incl. decay-constant errors"));
            ui.separator();
            ui.checkbox(&mut c.conc_discordia, "Discordia");
            ui.add_enabled(c.conc_discordia, egui::Checkbox::new(&mut c.conc_anchor, "anchor lower intercept at"));
            ui.add_enabled(c.conc_discordia && c.conc_anchor, egui::DragValue::new(&mut c.conc_anchor_ma).suffix(" Ma").range(0.0..=4500.0));
        });
        // another group or diagram starts from the whole view again (as in the laser part)
        if before != (c.conc_group.clone(), c.conc_wetherill) {
            c.conc_view = chemchron_laser::plot::View::default();
        }
    }
    let idx = group_rows(&app.data.rows, &app.combine.conc_group.clone(), sel_state(app));
    let rows: Vec<&Row> = idx.iter().map(|i| &app.data.rows[*i]).collect();
    let crit = app.settings.upb.criteria();
    let group_colors = app.group_colors();
    let colors = Colors::for_rows(&group_colors, &rows);
    let wetherill = app.combine.conc_wetherill;
    // all spots (once each), with whether they are used
    let all_rows: Vec<&Row> = rows.iter().copied().filter(|r| has_upb(r)).collect();
    let mut shown = Vec::new();
    for (i, x, sx, y, sy, rho) in concordia_data(&all_rows, wetherill) {
        let r = all_rows[i];
        let used = upb_rejection(r, &crit).is_none();
        if !used && !app.combine.conc_show_disc {
            continue;
        }
        let why = upb_rejection(r, &crit).map(|w| format!(" ({w})")).unwrap_or_default();
        shown.push((i, x, sx, y, sy, rho, colors.of(r), used, format!("{}{why}", text(r, "Label"))));
    }
    let incl: Vec<&Row> = all_rows.iter().copied().filter(|r| upb_rejection(r, &crit).is_none()).collect();
    let tw: Vec<Pt> = concordia_data(&incl, false).into_iter().map(|(_, x, sx, y, sy, r)| (x, sx, y, sy, r)).collect();
    let mut kept = tw.clone();
    let mut n_rej = 0;
    if reject && tw.len() > 2 {
        let (k, r) = reject_concordia_outliers(&tw, app.settings.rejection_sigma, 2);
        n_rej = r.len();
        kept = k.into_iter().map(|i| tw[i]).collect();
    }
    let decay = app.settings.decay.clone();
    let rel = (decay.lambda_238_rel_1s_pct / 100.0, decay.lambda_235_rel_1s_pct / 100.0);
    let ca = if app.combine.conc_age && !kept.is_empty() { concordia_age(&kept, &decay, if app.combine.conc_decay { Some(rel) } else { None }) } else { None };
    let dc = if app.combine.conc_discordia {
        let anchor = if app.combine.conc_anchor { Some(app.combine.conc_anchor_ma) } else { None };
        discordia(&tw, &decay, anchor)
    } else {
        None
    };
    let mut fig = plots::concordia(&shown, wetherill, &decay, ca.as_ref(), dc.as_ref(), &app.combine.conc_group.label(), if t.dark { Color32::from_rgb(200, 200, 200) } else { Color32::from_rgb(60, 60, 60) }, t.curve);
    if let Some(c) = &ca {
        let (sys, src) = systematic_for(&chemchron_core::pipeline::upb_options(&app.settings, false).systematic, &incl, "concordia", app.settings.upb.systematic_pct);
        let s2 = sys / 100.0 * c.age_ma;
        let total = (c.age_2s_ma.powi(2) + s2 * s2).sqrt();
        fig.notes.push(format!("With the systematic term ({sys:.2} %, {src}): {:.2} ± {:.2} Ma{}", c.age_ma, total, if n_rej > 0 { format!(" · {n_rej} spots rejected as outliers") } else { String::new() }));
    }
    fig.legend = colors.legend();
    let row_ids: Vec<String> = all_rows.iter().map(|r| text(r, "Row_ID")).collect();
    let n_shown = shown.len();
    let mut save = false;
    ui.horizontal(|ui| {
        if let Some(c) = &ca {
            stat(ui, &t, "Concordia age (Ma)", &fmt_pm(c.age_ma, c.age_2s_ma), t.accent, 170.0);
            stat(ui, &t, "MSWD", &format!("{:.2}", c.mswd), t.text, 70.0);
            stat(ui, &t, "n", &c.n.to_string(), t.text, 50.0);
        }
        if let Some(d) = &dc {
            for i in &d.intercepts {
                stat(ui, &t, if i.anchored { "Anchored intercept" } else { "Intercept (Ma)" }, &fmt_pm(i.age_ma, i.age_2s_ma), t.text, 150.0);
            }
        }
        save = ui.button("Save figure…").clicked();
        if ui
            .add_enabled(app.combine.conc_view.window.is_some(), egui::Button::new("Reset view"))
            .on_hover_text("Show all analyses again (double-clicking the plot does the same)")
            .clicked()
        {
            app.combine.conc_view = chemchron_laser::plot::View::default();
        }
    });
    offer(app, &fig, if wetherill { "concordia_wetherill" } else { "concordia_tw" });
    if save {
        save_fig(app, &fig, if wetherill { "concordia_wetherill" } else { "concordia_tw" });
    }
    if n_shown == 0 {
        ui.label(RichText::new("No U–Pb spots in this choice. Link laser data first (Link laser data).").color(t.warn));
        return;
    }
    let h = ui.available_height() - 10.0;
    let ev = figure::show(ui, &t, &fig, &mut app.combine.conc_view, h);
    if let Some(local) = ev.clicked {
        if let Some(i) = row_ids.get(local).and_then(|l| app.data.index_of(l)) {
            toggle_upb(app, i);
        }
    }
}

fn toggle_upb(app: &mut ChemApp, i: usize) {
    app.data.checkpoint("Include / leave out of U–Pb averages");
    if chemchron_core::upb::included_in_upb(&app.data.rows[i]) {
        set(&mut app.data.rows[i], "UPb_Include", serde_json::Value::Bool(false));
    } else {
        app.data.rows[i].remove("UPb_Include");
    }
    app.data.refresh(&app.settings);
}

pub fn page_upb_mean(app: &mut ChemApp, ui: &mut Ui) {
    let t = app.theme();
    ui.heading("U–Pb weighted mean");
    let choices = group_choices(&app.data.rows, true);
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Analyses").color(t.dim));
        group_picker(ui, "iwm_group", &mut app.combine.iwm_group, &choices);
        ui.label("Age");
        age_type_combo(ui, "iwm_type", &mut app.combine.iwm_type);
        ui.checkbox(&mut app.combine.iwm_show_disc, "Show discordant / imprecise");
        let mut rank = app.settings.rank_by_age;
        if ui.checkbox(&mut rank, "Rank by age").changed() {
            app.settings.rank_by_age = rank;
            app.save_settings();
        }
    });
    let idx = group_rows(&app.data.rows, &app.combine.iwm_group.clone(), sel_state(app));
    let rows: Vec<&Row> = idx.iter().map(|i| &app.data.rows[*i]).collect();
    let ty = app.combine.iwm_type.clone();
    let crit = app.settings.upb.criteria();
    let opts = chemchron_core::pipeline::upb_options(&app.settings, false);
    let (sys, src) = systematic_for(&opts.systematic, &rows, &ty, app.settings.upb.systematic_pct);
    let reject = app.settings.reject_outliers;
    let w = isotopic_weighted_mean(&rows, &ty, sys, reject, app.settings.rejection_sigma, &crit);
    let group_colors = app.group_colors();
    let colors = Colors::for_rows(&group_colors, &rows);
    let split = chemchron_core::upbstats::split_spots(&rows, &ty, &crit);
    let mut bars: Vec<Bar> = Vec::new();
    let rejected: Vec<usize> = w.as_ref().map(|w| w.wm.rejected_idx.clone()).unwrap_or_default();
    for (i, a, s) in &split.used {
        bars.push(Bar { id: idx[*i], group: Colors::group_of(rows[*i]), label: format!("{}{}", text(rows[*i], "Label"), if rejected.contains(i) { " (rejected)" } else { "" }), age: *a, s2: 2.0 * s, used: !rejected.contains(i), color: colors.of(rows[*i]) });
    }
    if app.combine.iwm_show_disc {
        for (i, a, s) in &split.rejected {
            let why = upb_rejection(rows[*i], &crit).unwrap_or("");
            bars.push(Bar { id: idx[*i], group: Colors::group_of(rows[*i]), label: format!("{} ({why})", text(rows[*i], "Label")), age: *a, s2: 2.0 * s, used: false, color: colors.of(rows[*i]) });
        }
    }
    let mean = w.as_ref().map(|w| (w.wm.mean, w.wm.total_2s, w.wm.mswd, w.wm.p_value));
    if app.combine.iwm_group == GroupPick::Selection && !app.settings.rank_by_age {
        crate::epma_pages::order_like_table(app, &mut bars);
    }
    let mut fig = plots::weighted_mean(&mut bars, mean, &app.combine.iwm_group.label(), &format!("{} age (Ma)", age_type_label(&ty)), app.settings.rank_by_age, t.accent);
    if let Some(w) = &w {
        fig.notes.push(format!(
            "Weighted mean {:.2} ± {:.2} Ma (2σ incl. systematic {sys:.2} % [{src}]; analytical {:.2}{}) · MSWD {:.2}, P = {:.3}, n = {} · {} discordant/imprecise not used{}",
            w.wm.mean,
            w.wm.total_2s,
            w.wm.expanded_2s,
            if w.wm.inflated { " incl. √MSWD" } else { "" },
            w.wm.mswd,
            w.wm.p_value,
            w.wm.n,
            w.discordant.len(),
            if w.n_duplicates > 0 { format!(" · {} repeated spots counted once", w.n_duplicates) } else { String::new() }
        ));
    }
    if ty == "t68" && preferred_age_type(&rows) == "t76" {
        fig.notes.push("Median ²⁰⁶Pb/²³⁸U age above 1000 Ma: ²⁰⁷Pb/²⁰⁶Pb ages are usually preferred.".into());
    }
    let mut save = false;
    ui.horizontal(|ui| {
        match &w {
            Some(w) => {
                stat(ui, &t, "Weighted mean (Ma)", &fmt_pm(w.wm.mean, w.wm.total_2s), t.accent, 170.0);
                stat(ui, &t, "MSWD", &format!("{:.2}", w.wm.mswd), if w.wm.inflated { t.warn } else { t.text }, 70.0);
                stat(ui, &t, "n", &w.wm.n.to_string(), t.text, 50.0);
            }
            None => {
                ui.label(RichText::new("A weighted mean needs at least 2 usable spots.").color(t.warn));
            }
        }
        save = ui.button("Save figure…").clicked();
    });
    offer(app, &fig, "weighted_mean_upb");
    if save {
        save_fig(app, &fig, "weighted_mean_upb");
    }
    let h = ui.available_height() - 10.0;
    let ev = figure::show(ui, &t, &fig, &mut app.combine.iwm_view, h);
    if let Some(i) = ev.clicked {
        toggle_upb(app, i);
    }
}

fn multi_groups(ui: &mut Ui, id: &str, sel: &mut Vec<GroupPick>, choices: &[GroupPick]) {
    if sel.is_empty() {
        sel.extend(choices.iter().filter(|c| matches!(c, GroupPick::Flag(..))).cloned());
    }
    ui.menu_button(format!("Groups ({})", sel.len()), |ui| {
        ui.push_id(id, |ui| {
            for c in choices {
                let mut on = sel.contains(c);
                if ui.checkbox(&mut on, c.label()).changed() {
                    if on {
                        sel.push(c.clone());
                    } else {
                        sel.retain(|x| x != c);
                    }
                }
            }
            if ui.button("All sample groups").clicked() {
                sel.clear();
                sel.extend(choices.iter().filter(|c| matches!(c, GroupPick::Flag(..))).cloned());
            }
        });
    });
}

fn rows_of_groups(app: &ChemApp, sel: &[GroupPick]) -> Vec<usize> {
    let mut v: Vec<usize> = Vec::new();
    for g in sel {
        for i in group_rows(&app.data.rows, g, sel_state(app)) {
            if !v.contains(&i) {
                v.push(i);
            }
        }
    }
    v
}

pub fn page_ree(app: &mut ChemApp, ui: &mut Ui) {
    let t = app.theme();
    ui.heading("REE diagram (chondrite-normalised)");
    let choices = group_choices(&app.data.rows, true);
    let mut norm_name = app.settings.norm.active.clone();
    {
        let c = &mut app.combine;
        ui.horizontal_wrapped(|ui| {
            multi_groups(ui, "ree_groups", &mut c.ree_sel, &choices);
            let mut mode = (c.ree_groups, c.ree_range);
            if segmented(ui, &t, &mut mode, &[((false, false), "Each analysis"), ((true, false), "Median + IQR per group"), ((true, true), "Median + min–max per group")]) {
                c.ree_groups = mode.0;
                c.ree_range = mode.1;
            }
            ui.checkbox(&mut c.ree_y, "Include Y (between Dy and Ho)");
            ui.label("Values");
            egui::ComboBox::from_id_salt("ree_src").selected_text(c.ree_src.label()).show_ui(ui, |ui| {
                for s in chemchron_core::ree::Source::ALL {
                    ui.selectable_value(&mut c.ree_src, s, s.label());
                }
            });
            ui.label("Normalised to");
            egui::ComboBox::from_id_salt("ree_norm").selected_text(norm_name.clone()).show_ui(ui, |ui| {
                for s in &app.settings.norm.sets {
                    ui.selectable_value(&mut norm_name, s.name.clone(), &s.name);
                }
            });
        });
    }
    if norm_name != app.settings.norm.active {
        app.settings.norm.active = norm_name;
        app.save_settings();
    }
    let idx = rows_of_groups(app, &app.combine.ree_sel.clone());
    let rows: Vec<&Row> = idx.iter().map(|i| &app.data.rows[*i]).filter(|r| flag(r, "Include_In_Average", true)).collect();
    let group_colors = app.group_colors();
    let colors = Colors::for_rows(&group_colors, &rows);
    let norm = app.settings.norm.by_name(&app.settings.norm.active).clone();
    let group_of = |r: &Row| if truthy(r, "Is_Reference") { text(r, "Reference") } else { text(r, "Flag") };
    let fig = plots::ree_diagram(&rows, &norm, app.combine.ree_y, app.combine.ree_src, app.combine.ree_groups, app.combine.ree_range, "REE pattern", &colors, &group_of);
    let mut save = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{} analyses. Undetermined or below-d.l. elements are bridged with a dashed line. LA-ICP-MS values repeated within a grain count once in group medians.", rows.len())).color(t.dim).size(12.0));
        save = ui.button("Save figure…").clicked();
    });
    offer(app, &fig, "ree_diagram");
    if save {
        save_fig(app, &fig, "ree_diagram");
    }
    let h = ui.available_height() - 10.0;
    figure::show(ui, &t, &fig, &mut app.combine.ree_view, h);
}

const COMP_KINDS: [&str; 4] = ["Ternary Mnz–Cher–Ht", "Exchange vector", "X–Y plot", "Ternary (choose fields)"];

pub fn page_composition(app: &mut ChemApp, ui: &mut Ui) {
    let t = app.theme();
    ui.heading("Composition plots");
    let choices = group_choices(&app.data.rows, true);
    let mut fields = plottable_fields(&app.data.active);
    for el in &app.data.trace {
        fields.push(("Trace elements (ppm)".into(), format!("{el}_ppm"), format!("{el} (ppm)")));
    }
    for f in ["UPb_t68_Ma", "UPb_t76_Ma", "UPb_t82_Ma", "UPb_Disc76_pct"] {
        fields.push(("U–Pb".into(), f.into(), display_name(f)));
    }
    {
        let c = &mut app.combine;
        ui.horizontal_wrapped(|ui| {
            multi_groups(ui, "comp_groups", &mut c.comp_sel, &choices);
            for (i, k) in COMP_KINDS.iter().enumerate() {
                ui.selectable_value(&mut c.comp_kind, i, *k);
            }
        });
        if c.comp_kind >= 2 {
            ui.horizontal_wrapped(|ui| {
                let pick = |ui: &mut Ui, id: &str, v: &mut String| {
                    egui::ComboBox::from_id_salt(id).selected_text(display_name(v)).width(170.0).show_ui(ui, |ui| {
                        let mut last = String::new();
                        for (g, f, l) in &fields {
                            if *g != last {
                                ui.label(RichText::new(g).strong());
                                last = g.clone();
                            }
                            ui.selectable_value(v, f.clone(), l);
                        }
                    });
                };
                ui.label(if c.comp_kind == 2 { "x" } else { "A" });
                pick(ui, "comp_x", &mut c.comp_x);
                ui.label(if c.comp_kind == 2 { "y" } else { "B" });
                pick(ui, "comp_y", &mut c.comp_y);
                if c.comp_kind == 3 {
                    ui.label("C");
                    pick(ui, "comp_z", &mut c.comp_z);
                } else {
                    ui.checkbox(&mut c.comp_log, "log y");
                }
            });
        }
    }
    let idx = rows_of_groups(app, &app.combine.comp_sel.clone());
    let rows: Vec<&Row> = idx.iter().map(|i| &app.data.rows[*i]).filter(|r| !truthy(r, "No_EPMA") || app.combine.comp_kind == 2).collect();
    let group_colors = app.group_colors();
    let colors = Colors::for_rows(&group_colors, &rows);
    let frame = if t.dark { Color32::from_rgb(200, 200, 200) } else { Color32::from_rgb(68, 68, 68) };
    let c = &app.combine;
    let (fig, n): (Figure, usize) = match c.comp_kind {
        0 => plots::ternary(&rows, ["X_monazite", "X_cheralite", "X_huttonite"], ["Mnz", "Cher", "Ht"], "Mnz–Cher–Ht ternary", &colors, frame),
        1 => plots::exchange_vector(&rows, "Exchange vector diagram", &colors),
        2 => plots::binary(&rows, &c.comp_x, &c.comp_y, &display_name(&c.comp_x), &display_name(&c.comp_y), "X–Y plot", &colors, c.comp_log),
        _ => {
            let (a, b, z) = (display_name(&c.comp_x), display_name(&c.comp_y), display_name(&c.comp_z));
            plots::ternary(&rows, [&c.comp_x, &c.comp_y, &c.comp_z], [&a, &b, &z], "Ternary", &colors, frame)
        }
    };
    let mut save = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{n} of {} analyses plotted (others lack the values; ternary and exchange vector need monazite or xenotime)", rows.len())).color(t.dim).size(12.0));
        save = ui.button("Save figure…").clicked();
    });
    offer(app, &fig, "composition");
    if save {
        save_fig(app, &fig, "composition");
    }
    let h = ui.available_height() - 10.0;
    figure::show(ui, &t, &fig, &mut app.combine.comp_view, h);
}

pub fn page_compare(app: &mut ChemApp, ui: &mut Ui) {
    let t = app.theme();
    ui.heading("Chemical against U–Pb ages");
    ui.label(RichText::new("Analyses with both an EPMA chemical age and a linked laser age. The 1:1 line marks agreement; the lower plot shows the age distributions.").color(t.dim).size(12.0));
    ui.horizontal(|ui| {
        ui.label("U–Pb age");
        age_type_combo(ui, "cmp_type", &mut app.combine.cmp_type);
        ui.label("Bandwidth (Ma, 0 = automatic)");
        ui.add(egui::DragValue::new(&mut app.combine.kde_bw).range(0.0..=1000.0));
    });
    let ty = app.combine.cmp_type.clone();
    let crit = app.settings.upb.criteria();
    let group_colors = app.group_colors();
    let colors = Colors::for_rows(&group_colors, &app.data.rows.iter().collect::<Vec<_>>());
    let mut fig = Figure::new("", "Chemical age (Ma)", &format!("{} age (Ma)", age_type_label(&ty)));
    let mut n = 0;
    let mut chem = Vec::new();
    let mut iso = Vec::new();
    for (i, r) in app.data.rows.iter().enumerate() {
        if !flag(r, "Include_In_Average", true) {
            continue;
        }
        if let Some(a) = num(r, "Age_Ma") {
            if chemchron_core::stats::usable_for_age_statistics(r) && !truthy(r, "Is_Reference") {
                chem.push(a);
            }
        }
        if has_upb(r) && chemchron_core::upb::included_in_upb(r) && upb_rejection(r, &crit).is_none() && !truthy(r, "Is_Reference") {
            if let Some((a, _)) = chemchron_core::upbstats::age_and_sigma(r, &ty) {
                iso.push(a);
            }
        }
        let (Some((ca, cs)), Some((ia, is))) = (chemchron_core::stats::row_age_sigma(r), chemchron_core::upbstats::age_and_sigma(r, &ty)) else { continue };
        let c = colors.of(r);
        fig.items.push(crate::figure::Item::Line { pts: vec![(ca - 2.0 * cs, ia), (ca + 2.0 * cs, ia)], color: c, width: 1.0, dashed: false });
        fig.items.push(crate::figure::Item::Line { pts: vec![(ca, ia - 2.0 * is), (ca, ia + 2.0 * is)], color: c, width: 1.0, dashed: false });
        fig.marker(ca, ia, c, Some(i), vec![text(r, "Label"), format!("chemical {ca:.1} ± {:.1}", 2.0 * cs), format!("{} {ia:.1} ± {:.1}", age_type_label(&ty), 2.0 * is)]);
        n += 1;
    }
    fig.fit(0.08);
    let lo = fig.x.0.min(fig.y.0);
    let hi = fig.x.1.max(fig.y.1);
    fig.dashed(vec![(lo, lo), (hi, hi)], t.dim, 1.0);
    fig.legend = colors.legend();
    fig.notes.push(format!("{n} analyses with both ages (2σ analytical + excess)"));
    let kde = plots::age_distribution(&[("Chemical (EPMA)".into(), chem, t.accent), (format!("{} (laser)", age_type_label(&ty)), iso, Color32::from_rgb(227, 169, 72))], app.combine.kde_bw, "");
    let mut save = (false, false);
    ui.horizontal(|ui| {
        save.0 = ui.button("Save comparison figure…").clicked();
        save.1 = ui.button("Save distribution figure…").clicked();
    });
    offer(app, &fig, "chemical_vs_upb");
    if save.0 {
        save_fig(app, &fig, "chemical_vs_upb");
    }
    if save.1 {
        save_fig(app, &kde, "age_distribution");
    }
    let h = (ui.available_height() - 20.0) * 0.6;
    figure::show(ui, &t, &fig, &mut app.combine.cmp_view, h);
    let h2 = ui.available_height() - 10.0;
    figure::show(ui, &t, &kde, &mut app.combine.kde_view, h2);
}

/// Age distributions (kernel density) of chosen groups: chemical ages, U–Pb
/// ages, or both side by side.
pub fn page_age_distribution(app: &mut ChemApp, ui: &mut Ui) {
    let t = app.theme();
    ui.heading("Age distribution");
    ui.label(RichText::new("Kernel density of the ages of the chosen groups (analyses included in averages; U–Pb ages that pass the acceptance criteria). Each age is marked on the axis.").color(t.dim).size(12.0));
    let choices = group_choices(&app.data.rows, true);
    {
        let c = &mut app.combine;
        ui.horizontal_wrapped(|ui| {
            multi_groups(ui, "dist_groups", &mut c.dist_sel, &choices);
            segmented(ui, &t, &mut c.dist_kind, &[(0, "Chemical ages"), (1, "U–Pb ages"), (2, "Both")]);
            if c.dist_kind > 0 {
                ui.label("U–Pb age");
                age_type_combo(ui, "dist_type", &mut c.dist_type);
            }
            ui.checkbox(&mut c.dist_pool, "All chosen groups as one curve");
            ui.label("Bandwidth (Ma, 0 = automatic)");
            ui.add(egui::DragValue::new(&mut c.dist_bw).range(0.0..=1000.0));
        });
    }
    let ty = app.combine.dist_type.clone();
    let crit = app.settings.upb.criteria();
    let group_colors = app.group_colors();
    let sel = app.combine.dist_sel.clone();
    let kind = app.combine.dist_kind;
    let mut series: Vec<(String, Vec<f64>, Color32)> = Vec::new();
    let groups: Vec<(String, Vec<usize>)> = if app.combine.dist_pool {
        vec![("All chosen".to_string(), rows_of_groups(app, &sel))]
    } else {
        sel.iter().map(|g| (g.label(), group_rows(&app.data.rows, g, sel_state(app)))).collect()
    };
    for (gi, (name, idx)) in groups.iter().enumerate() {
        let mut chem = Vec::new();
        let mut iso = Vec::new();
        for &i in idx {
            let r = &app.data.rows[i];
            if !flag(r, "Include_In_Average", true) {
                continue;
            }
            if let Some(a) = num(r, "Age_Ma") {
                if chemchron_core::stats::usable_for_age_statistics(r) && !truthy(r, "No_EPMA") {
                    chem.push(a);
                }
            }
            if has_upb(r) && chemchron_core::upb::included_in_upb(r) && upb_rejection(r, &crit).is_none() {
                if let Some((a, _)) = chemchron_core::upbstats::age_and_sigma(r, &ty) {
                    iso.push(a);
                }
            }
        }
        let gname = match &sel.get(gi) {
            Some(GroupPick::Flag(f, _)) if !app.combine.dist_pool => f.clone(),
            Some(GroupPick::Reference(c)) if !app.combine.dist_pool => c.clone(),
            _ => name.clone(),
        };
        let col = crate::plots::group_color(&gname, &group_colors, gi);
        let alt = Color32::from_rgb(227, 169, 72);
        if kind != 1 && !chem.is_empty() {
            series.push((if kind == 2 { format!("{name}, chemical") } else { name.clone() }, chem, col));
        }
        if kind != 0 && !iso.is_empty() {
            let c2 = if kind == 2 { if groups.len() == 1 { alt } else { col.gamma_multiply(0.55) } } else { col };
            series.push((if kind == 2 { format!("{name}, {}", age_type_label(&ty)) } else { name.clone() }, iso, c2));
        }
    }
    let what = match kind {
        0 => "Chemical age".to_string(),
        1 => format!("{} age", age_type_label(&ty)),
        _ => format!("Chemical and {} ages", age_type_label(&ty)),
    };
    let fig = plots::age_distribution(&series, app.combine.dist_bw, &format!("{what} distribution"));
    offer(app, &fig, "age_distribution");
    let mut save = false;
    ui.horizontal(|ui| {
        let n: usize = series.iter().map(|s| s.1.len()).sum();
        ui.label(RichText::new(format!("{n} ages in {} curves.", series.len())).color(t.dim).size(12.0));
        save = ui.button("Save figure…").clicked();
    });
    if save {
        save_fig(app, &fig, "age_distribution");
    }
    if series.is_empty() {
        ui.label(RichText::new("No ages in the chosen groups.").color(t.dim));
        return;
    }
    let h = ui.available_height() - 10.0;
    figure::show(ui, &t, &fig, &mut app.combine.dist_view, h);
}
