// SPDX-License-Identifier: GPL-3.0-or-later
//! Plot choices kept in the project file (groups, axes, options per plot),
//! and "Export all figures": every plot page drawn off-screen for each
//! group, written into one folder.

use crate::app::ChemApp;
use crate::epma::GroupPick;
use crate::figure::Figure;
use serde_json::{json, Map, Value};

fn pick_to(v: &GroupPick) -> Value {
    match v {
        GroupPick::None => Value::Null,
        GroupPick::Flag(f, m) => json!({"group": f, "mineral": m}),
        GroupPick::Reference(r) => json!({"reference": r}),
        GroupPick::Selection => json!("selection"),
    }
}

fn pick_from(v: &Value) -> Option<GroupPick> {
    if v.as_str() == Some("selection") {
        return Some(GroupPick::Selection);
    }
    if let Some(r) = v.get("reference").and_then(|r| r.as_str()) {
        return Some(GroupPick::Reference(r.to_string()));
    }
    let f = v.get("group")?.as_str()?;
    Some(GroupPick::Flag(f.to_string(), v.get("mineral").and_then(|m| m.as_str()).unwrap_or("").to_string()))
}

fn picks_to(v: &[GroupPick]) -> Value {
    Value::Array(v.iter().map(pick_to).collect())
}

fn picks_from(v: &Value) -> Option<Vec<GroupPick>> {
    Some(v.as_array()?.iter().filter_map(pick_from).collect())
}

/// The plot choices of the open project.
pub fn to_value(app: &ChemApp) -> Value {
    let c = &app.combine;
    let e = &app.epma;
    json!({
        "isochron": {"group": pick_to(&e.iso_group), "uranium": e.iso_u},
        "weighted_mean": {"group": pick_to(&e.wm_group), "hide_excluded": e.wm_hide_excluded, "upb": c.wm_upb,
                           "upb_group": pick_to(&c.iwm_group), "upb_type": c.iwm_type, "upb_show_discordant": c.iwm_show_disc},
        "concordia": {"group": pick_to(&c.conc_group), "wetherill": c.conc_wetherill, "age": c.conc_age, "decay_uncertainty": c.conc_decay,
                      "discordia": c.conc_discordia, "anchor": c.conc_anchor, "anchor_ma": c.conc_anchor_ma, "show_discordant": c.conc_show_disc},
        "ree": {"groups": picks_to(&c.ree_sel), "per_group": c.ree_groups, "range": c.ree_range, "with_y": c.ree_y, "values": c.ree_src},
        "composition": {"groups": picks_to(&c.comp_sel), "kind": c.comp_kind, "x": c.comp_x, "y": c.comp_y, "z": c.comp_z, "log": c.comp_log},
        "compare": {"type": c.cmp_type, "bandwidth": c.kde_bw},
        "age_distribution": {"groups": picks_to(&c.dist_sel), "kind": c.dist_kind, "type": c.dist_type, "pool": c.dist_pool, "bandwidth": c.dist_bw},
        "long_term": {"upb": c.lt_upb, "type": c.lt_type, "mineral": c.lt_mineral},
    })
}

/// Takes the plot choices saved in a project (missing ones keep their defaults).
pub fn apply(app: &mut ChemApp, v: &Value) {
    let Some(v) = v.as_object() else { return };
    let o = |k: &str| v.get(k).and_then(|x| x.as_object()).cloned().unwrap_or_default();
    fn b(m: &Map<String, Value>, k: &str, t: &mut bool) {
        if let Some(x) = m.get(k).and_then(|x| x.as_bool()) {
            *t = x;
        }
    }
    fn s(m: &Map<String, Value>, k: &str, t: &mut String) {
        if let Some(x) = m.get(k).and_then(|x| x.as_str()) {
            *t = x.to_string();
        }
    }
    fn f(m: &Map<String, Value>, k: &str, t: &mut f64) {
        if let Some(x) = m.get(k).and_then(|x| x.as_f64()) {
            *t = x;
        }
    }
    fn g(m: &Map<String, Value>, k: &str, t: &mut GroupPick) {
        if let Some(x) = m.get(k).and_then(pick_from) {
            *t = x;
        }
    }
    fn gs(m: &Map<String, Value>, k: &str, t: &mut Vec<GroupPick>) {
        if let Some(x) = m.get(k).and_then(picks_from) {
            *t = x;
        }
    }
    crate::combine::init_defaults(&mut app.combine);
    let c = &mut app.combine;
    let e = &mut app.epma;
    let m = o("isochron");
    g(&m, "group", &mut e.iso_group);
    b(&m, "uranium", &mut e.iso_u);
    let m = o("weighted_mean");
    g(&m, "group", &mut e.wm_group);
    b(&m, "hide_excluded", &mut e.wm_hide_excluded);
    b(&m, "upb", &mut c.wm_upb);
    g(&m, "upb_group", &mut c.iwm_group);
    s(&m, "upb_type", &mut c.iwm_type);
    b(&m, "upb_show_discordant", &mut c.iwm_show_disc);
    let m = o("concordia");
    g(&m, "group", &mut c.conc_group);
    b(&m, "wetherill", &mut c.conc_wetherill);
    b(&m, "age", &mut c.conc_age);
    b(&m, "decay_uncertainty", &mut c.conc_decay);
    b(&m, "discordia", &mut c.conc_discordia);
    b(&m, "anchor", &mut c.conc_anchor);
    f(&m, "anchor_ma", &mut c.conc_anchor_ma);
    b(&m, "show_discordant", &mut c.conc_show_disc);
    let m = o("ree");
    gs(&m, "groups", &mut c.ree_sel);
    b(&m, "per_group", &mut c.ree_groups);
    b(&m, "range", &mut c.ree_range);
    b(&m, "with_y", &mut c.ree_y);
    if let Some(x) = m.get("values").and_then(|x| serde_json::from_value(x.clone()).ok()) {
        c.ree_src = x;
    }
    let m = o("composition");
    gs(&m, "groups", &mut c.comp_sel);
    if let Some(k) = m.get("kind").and_then(|x| x.as_u64()) {
        c.comp_kind = (k as usize).min(3);
    }
    s(&m, "x", &mut c.comp_x);
    s(&m, "y", &mut c.comp_y);
    s(&m, "z", &mut c.comp_z);
    b(&m, "log", &mut c.comp_log);
    let m = o("compare");
    s(&m, "type", &mut c.cmp_type);
    f(&m, "bandwidth", &mut c.kde_bw);
    let m = o("age_distribution");
    gs(&m, "groups", &mut c.dist_sel);
    if let Some(k) = m.get("kind").and_then(|x| x.as_u64()) {
        c.dist_kind = (k as u8).min(2);
    }
    s(&m, "type", &mut c.dist_type);
    b(&m, "pool", &mut c.dist_pool);
    f(&m, "bandwidth", &mut c.dist_bw);
    let m = o("long_term");
    b(&m, "upb", &mut c.lt_upb);
    s(&m, "type", &mut c.lt_type);
    s(&m, "mineral", &mut c.lt_mineral);
}

// ---- export all figures --------------------------------------------------------------------

type PageFn = fn(&mut ChemApp, &mut egui::Ui);

/// Draws a page off-screen and returns the figures it offered.
fn gather(ctx: &egui::Context, app: &mut ChemApp, page: PageFn) -> Vec<(String, Figure)> {
    app.collect = Some(Vec::new());
    let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
    let mut out = ctx.run_ui(input, |ui| {
        egui::CentralPanel::default().show(ui, |ui| page(app, ui));
    });
    out.textures_delta.clear();
    app.collect.take().unwrap_or_default()
}

fn file_part(s: &str) -> String {
    let v: String = s.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '.' { c } else { '_' }).collect();
    v.trim_matches('_').to_string()
}

/// Every figure of the project: each plot page, the per-group ones once for
/// every sample group and reference material. The page choices are restored.
pub fn all_figures(app: &mut ChemApp) -> Vec<(String, Figure)> {
    let keep_page = (app.combine.page, app.combine.wm_upb);
    let keep = (app.epma.iso_group.clone(), app.epma.wm_group.clone(), app.combine.conc_group.clone(), app.combine.iwm_group.clone(), app.combine.lt_upb);
    let fig_export = app.fig_export.take();
    if !app.combine.init {
        crate::combine::init_defaults(&mut app.combine);
    }
    let ctx = egui::Context::default();
    let groups: Vec<GroupPick> = crate::epma::group_choices(&app.data.rows, true).into_iter().filter(|g| !matches!(g, GroupPick::Selection)).collect();
    let mut out: Vec<(String, Figure)> = Vec::new();
    let add = |out: &mut Vec<(String, Figure)>, figs: Vec<(String, Figure)>, suffix: &str| {
        for (n, f) in figs {
            if f.items.is_empty() {
                continue;
            }
            let base = if suffix.is_empty() { n } else { format!("{n}_{}", file_part(suffix)) };
            let mut name = base.clone();
            let mut k = 2;
            while out.iter().any(|(x, _)| *x == name) {
                name = format!("{base}_{k}");
                k += 1;
            }
            out.push((name, f));
        }
    };
    let chem = app.data.rows.iter().any(|r| !chemchron_core::row::truthy(r, "No_EPMA"));
    let upb = app.data.rows.iter().any(chemchron_core::upb::has_upb);
    for g in &groups {
        let suffix = match g {
            GroupPick::Flag(f, _) => f.clone(),
            GroupPick::Reference(r) => r.clone(),
            _ => String::new(),
        };
        if chem {
            app.epma.iso_group = g.clone();
            app.epma.wm_group = g.clone();
            let f = gather(&ctx, app, crate::epma_pages::page_isochron);
            add(&mut out, f, &suffix);
            let f = gather(&ctx, app, crate::epma_pages::page_weighted_mean);
            add(&mut out, f, &suffix);
        }
        if upb {
            app.combine.conc_group = g.clone();
            app.combine.iwm_group = g.clone();
            let f = gather(&ctx, app, crate::combine_plots::page_concordia);
            add(&mut out, f, &suffix);
            let f = gather(&ctx, app, crate::combine_plots::page_upb_mean);
            add(&mut out, f, &suffix);
        }
    }
    if !app.data.rows.is_empty() {
        for page in [crate::combine_plots::page_ree as PageFn, crate::combine_plots::page_composition, crate::combine_plots::page_age_distribution] {
            let f = gather(&ctx, app, page);
            add(&mut out, f, "");
        }
        if upb {
            let f = gather(&ctx, app, crate::combine_plots::page_compare);
            add(&mut out, f, "");
        }
    }
    for lt in [false, true] {
        app.combine.lt_upb = lt;
        let f = gather(&ctx, app, crate::longterm::page_long_term);
        add(&mut out, f, "");
    }
    (app.combine.page, app.combine.wm_upb) = keep_page;
    (app.epma.iso_group, app.epma.wm_group, app.combine.conc_group, app.combine.iwm_group, app.combine.lt_upb) = keep;
    app.fig_export = fig_export;
    out
}

/// Writes every figure into `dir` with the lab's figure settings (size, font, format).
pub fn export_all(app: &mut ChemApp, dir: &std::path::Path) -> Result<String, String> {
    let figs = all_figures(app);
    if figs.is_empty() {
        return Err("There are no figures to export yet.".into());
    }
    let pe = app.settings.plot_export.clone();
    let format = crate::vector::Format::from_ext(&pe.format);
    let (w, h) = crate::figure::export_size(&pe);
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for (name, fig) in &figs {
        let bytes = crate::vector::write_dpi(fig, format, w, h, pe.font_pt, pe.include_title, pe.dpi);
        let p = dir.join(format!("{name}.{}", format.ext()));
        std::fs::write(&p, bytes).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    Ok(format!("Wrote {} figures ({}, {w:.0} × {h:.0} mm) to {}", figs.len(), format.label(), dir.display()))
}
