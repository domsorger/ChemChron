// SPDX-License-Identifier: GPL-3.0-or-later
//! EPMA plots: chemical isochron and weighted mean. The uncertainty model,
//! reference materials and settings pages are in `settings_pages`.

use crate::app::ChemApp;
use crate::epma::{group_choices, group_picker, group_rows, toggle_include, GroupPick};
use crate::figure::{self, Figure};
use crate::plots::{self, Bar, Colors};
use crate::rowtable::TableState;
use chemchron_core::row::*;
use chemchron_core::stats::usable_for_age_statistics;
use egui::{RichText, Ui};
use chemchron_laser::theme::{fmt_pm, segmented, stat};

pub use crate::settings_pages::{page_settings, page_uncertainty};

/// The table whose marked rows "Rows selected in the table" means: the one of
/// Combined results > Data (the tab and samples / references shown last), or
/// of the EPMA part when the plot is opened from there.
pub fn selection_state(app: &ChemApp) -> &TableState {
    if app.part == crate::app::Part::Combine {
        let c = &app.combine;
        let st = match (c.data_tab, c.show_refs) {
            (crate::combine::DataTab::Composition, false) => &c.comp_table,
            (crate::combine::DataTab::Composition, true) => &c.comp_ref_table,
            (_, true) => &c.ref_table,
            _ => &c.table,
        };
        // nothing marked there: the rows marked in the EPMA part, if any
        if !st.selected.is_empty() {
            return st;
        }
        let e = &app.epma;
        return [&e.geo_table, &e.ref_table, &e.comp_table, &e.comp_ref_table].into_iter().find(|t| !t.selected.is_empty()).unwrap_or(st);
    }
    let e = &app.epma;
    match (e.page == crate::epma::Page::Composition, e.show_refs) {
        (true, false) => &e.comp_table,
        (true, true) => &e.comp_ref_table,
        (false, true) => &e.ref_table,
        (false, false) => &e.geo_table,
    }
}

/// Opens the "Save figure" window for this figure (size, format, title).
pub fn save_fig(app: &mut ChemApp, fig: &Figure, name: &str) {
    app.fig_export = Some((fig.clone(), name.to_string()));
}

/// Hands a page's figure to Export all figures (only while it gathers them).
pub fn offer(app: &mut ChemApp, fig: &Figure, name: &str) {
    if let Some(c) = &mut app.collect {
        c.push((name.to_string(), fig.clone()));
    }
}

/// Puts the columns of a weighted-mean plot in the order the rows have in
/// the table last shown (its sorting and filter), as they are shown.
pub fn order_like_table(app: &ChemApp, bars: &mut [plots::Bar]) {
    let pos: std::collections::HashMap<&str, usize> = app.table_order.iter().enumerate().map(|(k, r)| (r.as_str(), k)).collect();
    let key = |b: &plots::Bar| app.data.rows.get(b.id).and_then(|r| r.get("Row_ID")).and_then(|v| v.as_str()).and_then(|r| pos.get(r).copied()).unwrap_or(usize::MAX);
    bars.sort_by_key(|b| key(b));
}

/// Rows usable for chemical statistics among `idx`.
pub fn usable(rows: &[Row], idx: &[usize]) -> Vec<usize> {
    idx.iter().copied().filter(|i| usable_for_age_statistics(&rows[*i]) && !truthy(&rows[*i], "No_EPMA")).collect()
}

pub fn page_isochron(app: &mut ChemApp, ui: &mut Ui) {
    let t = app.theme();
    ui.heading("Chemical isochron");
    ui.label(RichText::new("PbO against ThO₂* or UO₂* (Suzuki & Dunkley 2014), York regression with each analysis's own errors plus the random excess; full-model age uncertainty; systematic term added once. Click a point to leave it out.").color(t.dim).size(12.0));
    let choices = group_choices(&app.data.rows, true);
    let mut reject = app.settings.reject_outliers;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Analyses").color(t.dim));
        group_picker(ui, "iso_group", &mut app.epma.iso_group, &choices);
        let mut u = app.epma.iso_u;
        segmented(ui, &t, &mut u, &[(false, "Th (PbO vs ThO₂*)"), (true, "U (PbO vs UO₂*)")]);
        app.epma.iso_u = u;
        if ui.checkbox(&mut reject, format!("Reject outliers ({:.1}σ)", app.settings.rejection_sigma)).changed() {
            app.settings.reject_outliers = reject;
            app.save_settings();
        }
    });
    let idx = group_rows(&app.data.rows, &app.epma.iso_group.clone(), selection_state(app));
    let ids = usable(&app.data.rows, &idx);
    let rows: Vec<&Row> = ids.iter().map(|i| &app.data.rows[*i]).collect();
    let iso = chemchron_core::isochron::compute_isochron(&rows, &app.settings.decay, !app.epma.iso_u, reject, app.settings.rejection_sigma);
    let Some(iso) = iso else {
        ui.add_space(20.0);
        ui.label(RichText::new(format!("An isochron needs at least 3 usable analyses ({} here). Left-out analyses and PbO below detection limit are not used.", rows.len())).color(t.warn));
        return;
    };
    let group_colors = app.group_colors();
    let colors = Colors::for_rows(&group_colors, &rows);
    let fig = plots::isochron(&rows, &iso, &app.epma.iso_group.label(), &colors, t.curve);
    let mut save = false;
    ui.horizontal(|ui| {
        stat(ui, &t, "Isochron age (Ma)", &fmt_pm(iso.age_ma.unwrap_or(f64::NAN), iso.age_2s_ma.unwrap_or(f64::NAN)), t.accent, 170.0);
        stat(ui, &t, "MSWD", &iso.mswd.map(|m| format!("{m:.2}")).unwrap_or("–".into()), t.text, 80.0);
        stat(ui, &t, "n", &iso.n.to_string(), t.text, 50.0);
        stat(ui, &t, "PbO intercept (wt%)", &fmt_pm(iso.intercept, iso.intercept_2s), t.text, 170.0);
        save = ui.button("Save figure…").clicked();
    });
    offer(app, &fig, "isochron");
    if save {
        save_fig(app, &fig, "isochron");
    }
    let h = ui.available_height() - 10.0;
    let ev = figure::show(ui, &t, &fig, &mut app.epma.iso_view, h);
    if let Some(local) = ev.clicked {
        if let Some(i) = ids.get(local) {
            toggle_include(app, *i);
        }
    }
}

pub fn wm_bars(rows: &[Row], idx: &[usize], show_excluded: bool, colors: &Colors) -> Vec<Bar> {
    let mut out = Vec::new();
    for &i in idx {
        let r = &rows[i];
        if truthy(r, "No_EPMA") || chemchron_core::stats::pbo_below_detection_limit(r) {
            continue;
        }
        let Some((age, s1)) = chemchron_core::stats::row_age_sigma(r) else { continue };
        let used = usable_for_age_statistics(r);
        if !used && !show_excluded {
            continue;
        }
        out.push(Bar { id: i, group: Colors::group_of(r), label: text(r, "Label"), age, s2: 2.0 * s1, used, color: colors.of(r) });
    }
    out
}

pub fn page_weighted_mean(app: &mut ChemApp, ui: &mut Ui) {
    let t = app.theme();
    ui.heading("Weighted mean (chemical ages)");
    ui.label(RichText::new("Inverse-variance weighted on analytical + excess (the random excess of the EPMA session); √MSWD expansion when P < 0.05; systematic term added once. Click a bar to leave it out or include it.").color(t.dim).size(12.0));
    let choices = group_choices(&app.data.rows, true);
    let mut reject = app.settings.reject_outliers;
    let mut rank = app.settings.rank_by_age;
    let mut show_ex = !app.epma.wm_hide_excluded;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Analyses").color(t.dim));
        group_picker(ui, "wm_group", &mut app.epma.wm_group, &choices);
        if ui.checkbox(&mut reject, format!("Reject outliers ({:.1}σ)", app.settings.rejection_sigma)).changed() {
            app.settings.reject_outliers = reject;
            app.save_settings();
        }
        if ui.checkbox(&mut rank, "Rank by age").changed() {
            app.settings.rank_by_age = rank;
            app.save_settings();
        }
        ui.checkbox(&mut show_ex, "Show excluded analyses");
    });
    app.epma.wm_hide_excluded = !show_ex;
    let idx = group_rows(&app.data.rows, &app.epma.wm_group.clone(), selection_state(app));
    let used_idx = usable(&app.data.rows, &idx);
    let rows: Vec<&Row> = used_idx.iter().map(|i| &app.data.rows[*i]).collect();
    let wm = chemchron_core::stats::weighted_mean_for_rows(&rows, reject, app.settings.rejection_sigma);
    let all: Vec<&Row> = idx.iter().map(|i| &app.data.rows[*i]).collect();
    let group_colors = app.group_colors();
    let colors = Colors::for_rows(&group_colors, &all);
    let mut bars = wm_bars(&app.data.rows, &idx, show_ex, &colors);
    if let Some(w) = &wm {
        let rejected: Vec<usize> = w.rejected_idx.iter().map(|k| used_idx[*k]).collect();
        for b in bars.iter_mut() {
            if rejected.contains(&b.id) {
                b.used = false;
                b.label.push_str(" (rejected)");
            }
        }
    }
    let mean = wm.as_ref().map(|w| (w.mean, w.total_2s, w.mswd, w.p_value));
    if app.epma.wm_group == GroupPick::Selection && !rank {
        order_like_table(app, &mut bars);
    }
    let mut fig = plots::weighted_mean(&mut bars, mean, &app.epma.wm_group.label(), "Chemical age (Ma)", rank, t.accent);
    if let Some(w) = &wm {
        fig.notes.push(format!(
            "Weighted mean {:.2} ± {:.2} Ma (2σ total; analytical {:.2}{}, systematic {:.2}) · MSWD {:.2} (acceptable {:.2}–{:.2}), P = {:.3}, n = {}",
            w.mean,
            w.total_2s,
            w.expanded_2s,
            if w.inflated { " incl. √MSWD" } else { "" },
            w.long_term_2s,
            w.mswd,
            w.mswd_lower,
            w.mswd_upper,
            w.p_value,
            w.n
        ));
        let warn = chemchron_core::stats::rejection_warning(w.rejected.len(), w.n + w.rejected.len());
        if !warn.is_empty() {
            fig.notes.push(warn);
        }
    }
    let mut save = false;
    ui.horizontal(|ui| {
        if let Some(w) = &wm {
            stat(ui, &t, "Weighted mean (Ma)", &fmt_pm(w.mean, w.total_2s), t.accent, 170.0);
            stat(ui, &t, "MSWD", &format!("{:.2}", w.mswd), if w.inflated { t.warn } else { t.text }, 80.0);
            stat(ui, &t, "n", &w.n.to_string(), t.text, 50.0);
        } else {
            ui.label(RichText::new("A weighted mean needs at least 2 usable analyses.").color(t.warn));
        }
        save = ui.button("Save figure…").clicked();
    });
    offer(app, &fig, "weighted_mean_chemical");
    if save {
        save_fig(app, &fig, "weighted_mean_chemical");
    }
    let h = ui.available_height() - 10.0;
    let ev = figure::show(ui, &t, &fig, &mut app.epma.wm_view, h);
    if let Some(i) = ev.clicked {
        toggle_include(app, i);
    }
}
