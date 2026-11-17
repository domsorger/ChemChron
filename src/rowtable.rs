// SPDX-License-Identifier: GPL-3.0-or-later
//! The analysis table: chosen columns, click a header to sort, filter by
//! text or by a column condition, select with click / shift / ctrl.

use chemchron_core::columns::{decimals_for, display_name};
use chemchron_core::constants::{mineral_abbreviation, DecimalPlaces};
use chemchron_core::reference::{OUTSIDE_2S, WITHIN_2S};
use chemchron_core::row::*;
use egui::{Color32, RichText, Ui};
use egui_extras::{Column, TableBuilder};
use std::collections::{BTreeMap, BTreeSet};
use chemchron_laser::theme::Theme;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Op {
    #[default]
    Contains,
    Equals,
    Greater,
    Less,
    Empty,
    NotEmpty,
}

impl Op {
    pub const ALL: [Op; 6] = [Op::Contains, Op::Equals, Op::Greater, Op::Less, Op::Empty, Op::NotEmpty];
    pub fn label(&self) -> &'static str {
        match self {
            Op::Contains => "contains",
            Op::Equals => "is",
            Op::Greater => ">",
            Op::Less => "<",
            Op::Empty => "is empty",
            Op::NotEmpty => "is not empty",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Condition {
    pub column: String,
    pub op: Op,
    pub value: String,
}

#[derive(Default)]
pub struct TableState {
    pub sort: Option<(String, bool)>,
    /// Second sort key (rows equal in the first are ordered by this).
    pub sort2: Option<(String, bool)>,
    /// Sample groups / reference materials hidden from the table.
    pub hidden_groups: BTreeSet<String>,
    /// Hide analyses left out of averages.
    pub hide_left_out: bool,
    /// Hide discordant or imprecise U–Pb analyses (needs `criteria`).
    pub hide_discordant: bool,
    /// Hide analyses with PbO below the detection limit.
    pub hide_bdl: bool,
    /// Hide analyses outside 2σ of their sample group's mean (outliers).
    pub hide_group_outliers: bool,
    /// U–Pb criteria of the lab (set by the page each frame).
    pub criteria: Option<chemchron_core::upb::Criteria>,
    pub search: String,
    pub conditions: Vec<Condition>,
    /// Selected rows by Row_ID.
    pub selected: BTreeSet<String>,
    pub anchor: Option<String>,
    pub hidden: BTreeSet<String>,
    pub show_columns: bool,
    /// Column groups (blocks) shown as a band above the headers; a collapsed
    /// block keeps only its first column.
    pub block_of: Option<fn(&str) -> &'static str>,
    pub collapsed: BTreeSet<String>,
    /// Label, comment and session cells can be edited (double-click a cell).
    pub edit_labels: bool,
    /// (Row_ID, column, text being typed) of the cell being edited.
    pub editing: Option<(String, String, String)>,
    /// Row_IDs in the order last shown (filter and sort applied).
    pub order: Vec<String>,
}

pub fn cell_text(row: &Row, col: &str, dp: &DecimalPlaces, show_bdl_values: bool) -> String {
    let v = row.get(col);
    match col {
        "Is_Reference" => return if truthy(row, "Is_Reference") { "Ref".into() } else { "Sample".into() },
        "Include_In_Average" => return if flag(row, col, true) { "yes".into() } else { "–".into() },
        "UPb_Include" => {
            if !chemchron_core::upb::has_upb(row) {
                return String::new();
            }
            return if chemchron_core::upb::included_in_upb(row) { "yes".into() } else { "–".into() };
        }
        "Mineral" => return mineral_abbreviation(&text(row, "Mineral")).to_string(),
        _ => {}
    }
    if let Some(ox) = col.strip_suffix("_wt%") {
        if !show_bdl_values && !col.contains("_2s_") && !col.contains("_DL_") && chemchron_core::sessions::below_dl_set(row).iter().any(|o| o == ox) {
            return "b.d.l.".into();
        }
    }
    match v {
        Some(serde_json::Value::Number(n)) => {
            let x = n.as_f64().unwrap_or(f64::NAN);
            if chemchron_core::upb::ratio_fields().iter().any(|f| f == col) {
                return chemchron_laser::theme::fmt_sig(x, 5);
            }
            if let Some(d) = decimals_for(col, dp) {
                return format!("{x:.d$}");
            }
            if col.contains("MSWD") || col == "P" || col.ends_with("_P") {
                return format!("{x:.2}");
            }
            if x == x.trunc() && x.abs() < 1e9 {
                return format!("{x:.0}");
            }
            chemchron_laser::theme::fmt_sig(x, 5)
        }
        Some(serde_json::Value::Bool(b)) => if *b { "True".into() } else { "False".into() },
        other => value_text(other),
    }
}

fn sort_key(row: &Row, col: &str) -> (u8, f64, String) {
    match row.get(col) {
        Some(serde_json::Value::Number(n)) => (0, n.as_f64().unwrap_or(f64::NAN), String::new()),
        Some(serde_json::Value::Bool(b)) => (0, if *b { 1.0 } else { 0.0 }, String::new()),
        Some(serde_json::Value::String(s)) if !s.is_empty() => (1, 0.0, s.to_lowercase()),
        _ => (2, 0.0, String::new()),
    }
}

/// Sample group or reference material of a row.
pub fn group_of(row: &Row) -> String {
    let g = if truthy(row, "Is_Reference") { text(row, "Reference") } else { text(row, "Flag") };
    if g.is_empty() { "(no group)".into() } else { g }
}

fn passes(row: &Row, st: &TableState, cols: &[String], dp: &DecimalPlaces) -> bool {
    if !st.hidden_groups.is_empty() && st.hidden_groups.contains(&group_of(row)) {
        return false;
    }
    if st.hide_left_out && !flag(row, "Include_In_Average", true) {
        return false;
    }
    if st.hide_bdl && chemchron_core::stats::pbo_below_detection_limit(row) {
        return false;
    }
    if st.hide_group_outliers && text(row, "Group_Within_2s") == OUTSIDE_2S {
        return false;
    }
    if st.hide_discordant && chemchron_core::upb::has_upb(row) {
        if let Some(c) = &st.criteria {
            if chemchron_core::upb::upb_rejection(row, c).is_some() {
                return false;
            }
        }
    }
    if !st.search.trim().is_empty() {
        let q = st.search.to_lowercase();
        let hit = cols.iter().any(|c| cell_text(row, c, dp, true).to_lowercase().contains(&q));
        if !hit {
            return false;
        }
    }
    for c in &st.conditions {
        if c.column.is_empty() {
            continue;
        }
        let t = cell_text(row, &c.column, dp, true);
        let ok = match c.op {
            Op::Contains => t.to_lowercase().contains(&c.value.to_lowercase()),
            Op::Equals => t.eq_ignore_ascii_case(c.value.trim()),
            Op::Greater | Op::Less => {
                let (Some(a), Ok(b)) = (num_loose(row, &c.column), c.value.trim().replace(',', ".").parse::<f64>()) else { return false };
                if c.op == Op::Greater { a > b } else { a < b }
            }
            Op::Empty => t.is_empty(),
            Op::NotEmpty => !t.is_empty(),
        };
        if !ok {
            return false;
        }
    }
    true
}

/// Indices of `rows` (from `subset`) to show, filtered and sorted.
pub fn visible(rows: &[Row], subset: &[usize], st: &TableState, cols: &[String], dp: &DecimalPlaces) -> Vec<usize> {
    let mut v: Vec<usize> = subset.iter().copied().filter(|i| passes(&rows[*i], st, cols, dp)).collect();
    let cmp = |a: usize, b: usize, c: &str, asc: bool| {
        let (ka, kb) = (sort_key(&rows[a], c), sort_key(&rows[b], c));
        let o = ka.0.cmp(&kb.0).then(ka.1.partial_cmp(&kb.1).unwrap_or(std::cmp::Ordering::Equal)).then(ka.2.cmp(&kb.2));
        if asc { o } else { o.reverse() }
    };
    if st.sort.is_some() || st.sort2.is_some() {
        v.sort_by(|a, b| {
            let first = st.sort.as_ref().map_or(std::cmp::Ordering::Equal, |(c, asc)| cmp(*a, *b, c, *asc));
            first.then_with(|| st.sort2.as_ref().map_or(std::cmp::Ordering::Equal, |(c, asc)| cmp(*a, *b, c, *asc)))
        });
    }
    v
}

/// Columns whose cells can be edited by double-clicking.
pub const EDITABLE: [&str; 4] = ["Label", "Comment", "EPMA_Session", "UPb_Session"];

pub struct Look<'a> {
    pub t: &'a Theme,
    pub dp: &'a DecimalPlaces,
    pub flag_colors: &'a BTreeMap<String, String>,
    pub show_bdl_values: bool,
}

/// What the user did in the table this frame.
#[derive(Default)]
pub struct TableEvents {
    pub toggled_include: Option<usize>,
    pub toggled_upb_include: Option<usize>,
    pub double_clicked: Option<usize>,
    /// A cell was edited: (row, column, new text).
    pub edited: Option<(usize, String, String)>,
}

/// Colour of a row: green / red for reference analyses within / outside 2σ
/// of the accepted age, the sample group's colour for samples.
pub fn row_tint(row: &Row, look: &Look) -> Option<Color32> {
    if truthy(row, "Is_Reference") {
        return match text(row, "Ref_Within_2s").as_str() {
            WITHIN_2S => Some(look.t.ok),
            OUTSIDE_2S => Some(look.t.alarm),
            _ => None,
        };
    }
    let f = text(row, "Flag");
    look.flag_colors.get(&f).and_then(|c| crate::plots::parse_hex(c))
}

/// Filter bar above a table.
pub fn filter_bar(ui: &mut Ui, st: &mut TableState, cols: &[String], t: &Theme, id: &str) {
    filter_bar_with(ui, st, cols, t, id, &[]);
}

/// The filter bar with a Show menu for these groups (sample groups or reference materials).
pub fn filter_bar_with(ui: &mut Ui, st: &mut TableState, cols: &[String], t: &Theme, id: &str, groups: &[String]) {
    ui.horizontal_wrapped(|ui| {
        let hidden = st.hidden_groups.len() + st.hide_left_out as usize + st.hide_discordant as usize + st.hide_bdl as usize + st.hide_group_outliers as usize;
        let label = if hidden > 0 { format!("Show ({hidden} hidden)") } else { "Show".to_string() };
        egui::containers::menu::MenuButton::new(label)
            .config(egui::containers::menu::MenuConfig::new().close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside))
            .ui(ui, |ui| {
                if !groups.is_empty() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Groups").strong());
                        if ui.small_button("all").clicked() {
                            st.hidden_groups.clear();
                        }
                        if ui.small_button("none").clicked() {
                            st.hidden_groups = groups.iter().cloned().collect();
                        }
                    });
                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        for g in groups {
                            let mut on = !st.hidden_groups.contains(g);
                            if ui.checkbox(&mut on, g).changed() {
                                if on {
                                    st.hidden_groups.remove(g);
                                } else {
                                    st.hidden_groups.insert(g.clone());
                                }
                            }
                        }
                    });
                    ui.separator();
                }
                ui.checkbox(&mut st.hide_left_out, "Hide analyses left out of averages");
                ui.checkbox(&mut st.hide_bdl, "Hide analyses with PbO below d.l.");
                ui.checkbox(&mut st.hide_group_outliers, "Hide outliers (outside 2σ of their group's mean age)");
                ui.checkbox(&mut st.hide_discordant, "Hide discordant / imprecise U–Pb (Settings > U–Pb)");
            });
        ui.separator();
        ui.label(RichText::new("Sort").color(t.dim));
        sort_combo(ui, (id, "sort1"), &mut st.sort, cols);
        ui.label(RichText::new("then").color(t.dim));
        sort_combo(ui, (id, "sort2"), &mut st.sort2, cols);
    });
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Find").color(t.dim));
        ui.add(egui::TextEdit::singleline(&mut st.search).desired_width(150.0).hint_text("label, group, …"));
        if ui.button("+ Condition").on_hover_text("Show only rows where a column meets a condition").clicked() {
            st.conditions.push(Condition { column: "Age_Ma".into(), op: Op::Greater, value: String::new() });
        }
        let mut remove = None;
        for (k, c) in st.conditions.iter_mut().enumerate() {
            ui.separator();
            egui::ComboBox::from_id_salt((id, "cond_col", k)).selected_text(display_name(&c.column)).width(150.0).show_ui(ui, |ui| {
                for col in cols {
                    ui.selectable_value(&mut c.column, col.clone(), display_name(col));
                }
            });
            egui::ComboBox::from_id_salt((id, "cond_op", k)).selected_text(c.op.label()).width(80.0).show_ui(ui, |ui| {
                for op in Op::ALL {
                    ui.selectable_value(&mut c.op, op, op.label());
                }
            });
            if !matches!(c.op, Op::Empty | Op::NotEmpty) {
                ui.add(egui::TextEdit::singleline(&mut c.value).desired_width(70.0));
            }
            if ui.small_button("x").clicked() {
                remove = Some(k);
            }
        }
        if let Some(k) = remove {
            st.conditions.remove(k);
        }
        ui.separator();
        if ui.button(if st.show_columns { "Hide columns" } else { "Columns" }).on_hover_text("Show or hide columns").clicked() {
            st.show_columns = !st.show_columns;
        }
    });
    if st.show_columns {
        ui.horizontal_wrapped(|ui| {
            for c in cols {
                let mut on = !st.hidden.contains(c);
                if ui.checkbox(&mut on, display_name(c)).changed() {
                    if on {
                        st.hidden.remove(c);
                    } else {
                        st.hidden.insert(c.clone());
                    }
                }
            }
        });
    }
}

pub fn show(ui: &mut Ui, id: &str, rows: &[Row], vis: &[usize], cols: &[String], st: &mut TableState, look: &Look) -> TableEvents {
    show_menu(ui, id, rows, vis, cols, st, look, None)
}

/// The table with a right-click menu: `menu` draws it for the rows it acts on
/// (the selection, or the row clicked when it isn't selected).
#[allow(clippy::too_many_arguments)]
pub fn show_menu(
    ui: &mut Ui,
    id: &str,
    rows: &[Row],
    vis: &[usize],
    cols: &[String],
    st: &mut TableState,
    look: &Look,
    mut menu: Option<&mut dyn FnMut(&mut Ui, &[usize])>,
) -> TableEvents {
    let t = look.t;
    let selected_now: Vec<usize> = selected_indices(rows, st);
    let mut select_only: Option<usize> = None;
    let tint_alpha = if t.dark { 0.30 } else { 0.20 };
    let mut ev = TableEvents::default();
    let visible_cols: Vec<&String> = cols.iter().filter(|c| !st.hidden.contains(*c)).collect();
    // (column, block name when it is the first column of its block, columns hidden by collapsing)
    let mut shown: Vec<&String> = Vec::new();
    let mut block_head: Vec<Option<(&'static str, usize)>> = Vec::new();
    if let Some(bf) = st.block_of {
        let mut k = 0;
        while k < visible_cols.len() {
            let b = bf(visible_cols[k]);
            let mut j = k;
            while j < visible_cols.len() && bf(visible_cols[j]) == b {
                j += 1;
            }
            let collapsed = st.collapsed.contains(b);
            for (n, c) in visible_cols[k..j].iter().enumerate() {
                if n == 0 || !collapsed {
                    shown.push(c);
                    block_head.push(if n == 0 { Some((b, if collapsed { j - k - 1 } else { 0 })) } else { None });
                }
            }
            k = j;
        }
    } else {
        shown = visible_cols.clone();
        block_head = vec![None; shown.len()];
    }
    let blocks = st.block_of.is_some();
    let mut toggle_block: Option<&'static str> = None;
    let mut start_edit: Option<(String, String, String)> = None;
    let mut end_edit = false;
    let mut clicked: Option<(usize, egui::Modifiers)> = None;
    let mut sort_click: Option<String> = None;
    let mods = ui.input(|i| i.modifiers);
    st.order = vis.iter().map(|i| text(&rows[*i], "Row_ID")).collect();
    let editing = st.editing.clone();
    let mut edit_buf = editing.as_ref().map(|e| e.2.clone()).unwrap_or_default();
    egui::ScrollArea::horizontal().id_salt((id, "hscroll")).auto_shrink(false).show(ui, |ui| {
    let mut builder = TableBuilder::new(ui)
        .id_salt(id)
        .striped(true)
        .sense(egui::Sense::click())
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .auto_shrink(false)
        .min_scrolled_height(120.0);
    for c in &shown {
        let w = match c.as_str() {
            "Label" => 170.0,
            "Mineral_Formula" => 260.0,
            "Comment" => 160.0,
            "Mineral" | "Is_Reference" | "Include_In_Average" | "UPb_Include" => 66.0,
            _ => 96.0,
        };
        builder = builder.column(Column::initial(w).clip(true).resizable(true));
    }
    builder
        .header(if blocks { 50.0 } else { 34.0 }, |mut h| {
            for (ci, c) in shown.iter().enumerate() {
                let head = block_head[ci];
                h.col(|ui| {
                    if blocks {
                        ui.vertical(|ui| {
                            match head {
                                Some((b, hidden)) => {
                                    let label = if hidden > 0 { format!("+ {b} ({hidden} hidden)") } else { format!("- {b}") };
                                    let r = ui.add(egui::Label::new(RichText::new(label).size(10.5).color(t.accent).strong()).sense(egui::Sense::click()).truncate());
                                    if r.clicked() {
                                        toggle_block = Some(b);
                                    }
                                    r.on_hover_text(if hidden > 0 { "Click to show the whole group" } else { "Click to collapse this group to its first column" });
                                }
                                None => {
                                    ui.add_space(14.0);
                                }
                            }
                            header_cell(ui, c, st_sort_arrow(&st.sort, c), &mut sort_click);
                        });
                        return;
                    }
                    header_cell(ui, c, st_sort_arrow(&st.sort, c), &mut sort_click);
                });
            }
        })
        .body(|body| {
            body.rows(21.0, vis.len(), |mut tr| {
                let i = vis[tr.index()];
                let row = &rows[i];
                let rid = text(row, "Row_ID");
                tr.set_selected(st.selected.contains(&rid));
                let tint = row_tint(row, look);
                let dim = !flag(row, "Include_In_Average", true);
                for c in &shown {
                    let (_, cell) = tr.col(|ui| {
                        if let Some(col) = tint {
                            let r = ui.max_rect().expand2(egui::vec2(4.0, 1.0));
                            ui.painter().rect_filled(r, 0, crate::figure::alpha(col, tint_alpha));
                        }
                        if c.as_str() == "Label" {
                            if let Some(col) = tint {
                                let (r, _) = ui.allocate_exact_size(egui::vec2(8.0, 14.0), egui::Sense::hover());
                                ui.painter().rect_filled(r, 2, col);
                            }
                        }
                        if c.as_str() == "Include_In_Average" {
                            let mut on = flag(row, "Include_In_Average", true);
                            if ui.checkbox(&mut on, "").on_hover_text("Include in averages").changed() {
                                ev.toggled_include = Some(i);
                            }
                            return;
                        }
                        if c.as_str() == "UPb_Include" && chemchron_core::upb::has_upb(row) {
                            let mut on = chemchron_core::upb::included_in_upb(row);
                            if ui.checkbox(&mut on, "").on_hover_text("Include in U–Pb averages").changed() {
                                ev.toggled_upb_include = Some(i);
                            }
                            return;
                        }
                        if editing.as_ref().is_some_and(|e| e.0 == rid && e.1 == **c) {
                            let r = ui.add(egui::TextEdit::singleline(&mut edit_buf).desired_width(f32::INFINITY));
                            if !r.has_focus() && !r.lost_focus() {
                                r.request_focus();
                            }
                            if r.lost_focus() {
                                if !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                    ev.edited = Some((i, c.to_string(), edit_buf.trim().to_string()));
                                }
                                end_edit = true;
                            }
                            return;
                        }
                        let s = cell_text(row, c, look.dp, look.show_bdl_values);
                        let mut color = if dim { t.dim } else { t.text };
                        if (c.ends_with("Within_2s")) && !s.is_empty() {
                            color = if s == WITHIN_2S { t.ok } else { t.alarm };
                        }
                        if s == "b.d.l." {
                            color = t.warn;
                        }
                        // measured, but within twice the detection limit: amber, italic
                        let near_dl = near_detection_limit(row, c);
                        if near_dl {
                            color = t.warn;
                        }
                        let mut rt = RichText::new(s).size(12.5).color(color);
                        if near_dl {
                            rt = rt.italics();
                        }
                        let l = ui.add(egui::Label::new(rt).selectable(false).truncate());
                        if near_dl {
                            l.on_hover_text("Less than twice the detection limit");
                        }
                    });
                    if st.edit_labels && EDITABLE.contains(&c.as_str()) {
                        if cell.double_clicked() {
                            start_edit = Some((rid.clone(), c.to_string(), text(row, c)));
                        }
                        cell.on_hover_text("Double-click to edit");
                    }
                }
                let resp = tr.response();
                if resp.clicked() {
                    clicked = Some((i, mods));
                }
                if let Some(m) = menu.as_mut() {
                    let targets: Vec<usize> = if selected_now.contains(&i) { selected_now.clone() } else { vec![i] };
                    if resp.secondary_clicked() && !selected_now.contains(&i) {
                        select_only = Some(i);
                    }
                    resp.context_menu(|ui| {
                        ui.label(RichText::new(if targets.len() == 1 { text(&rows[targets[0]], "Label") } else { format!("{} rows", targets.len()) }).strong());
                        ui.separator();
                        m(ui, &targets);
                    });
                }
                if resp.double_clicked() {
                    ev.double_clicked = Some(i);
                }
            });
        });
    });
    if let Some(e) = &st.editing {
        if !end_edit {
            st.editing = Some((e.0.clone(), e.1.clone(), edit_buf));
        }
    }
    if end_edit {
        st.editing = None;
    }
    if let Some(e) = start_edit {
        st.editing = Some(e);
    }
    if let Some(b) = toggle_block {
        if !st.collapsed.remove(b) {
            st.collapsed.insert(b.to_string());
        }
    }
    if let Some(i) = select_only {
        let rid = text(&rows[i], "Row_ID");
        st.selected.clear();
        st.selected.insert(rid.clone());
        st.anchor = Some(rid);
    }
    if let Some(c) = sort_click {
        st.sort = match &st.sort {
            Some((s, true)) if *s == c => Some((c, false)),
            Some((s, false)) if *s == c => None,
            _ => Some((c, true)),
        };
    }
    if let Some((i, m)) = clicked {
        let rid = text(&rows[i], "Row_ID");
        if m.shift {
            if let Some(a) = &st.anchor {
                let pa = vis.iter().position(|j| text(&rows[*j], "Row_ID") == *a);
                let pb = vis.iter().position(|j| *j == i);
                if let (Some(pa), Some(pb)) = (pa, pb) {
                    let (lo, hi) = (pa.min(pb), pa.max(pb));
                    for j in &vis[lo..=hi] {
                        st.selected.insert(text(&rows[*j], "Row_ID"));
                    }
                }
            }
        } else if m.command {
            if !st.selected.remove(&rid) {
                st.selected.insert(rid.clone());
            }
            st.anchor = Some(rid);
        } else {
            st.selected.clear();
            st.selected.insert(rid.clone());
            st.anchor = Some(rid);
        }
    }
    ev
}

fn st_sort_arrow(sort: &Option<(String, bool)>, c: &str) -> &'static str {
    match sort {
        Some((s, true)) if s == c => " ^",
        Some((s, false)) if s == c => " v",
        _ => "",
    }
}

fn header_cell(ui: &mut Ui, c: &str, arrow: &str, sort_click: &mut Option<String>) {
    let r = ui.add(egui::Label::new(RichText::new(format!("{}{arrow}", display_name(c))).strong().size(11.5)).sense(egui::Sense::click()).wrap());
    if r.clicked() {
        *sort_click = Some(c.to_string());
    }
    r.on_hover_text(format!("{c} — click to sort"));
}

/// A sort key: column and up / down (None: unsorted).
fn sort_combo(ui: &mut Ui, id: (&str, &str), key: &mut Option<(String, bool)>, cols: &[String]) {
    let shown = key.as_ref().map_or("–".to_string(), |(c, _)| display_name(c));
    egui::ComboBox::from_id_salt(id).selected_text(shown).width(130.0).show_ui(ui, |ui| {
        if ui.selectable_label(key.is_none(), "–").clicked() {
            *key = None;
        }
        for c in cols {
            if ui.selectable_label(key.as_ref().is_some_and(|k| k.0 == *c), display_name(c)).clicked() {
                *key = Some((c.clone(), key.as_ref().map_or(true, |k| k.1)));
            }
        }
    });
    if let Some((_, asc)) = key {
        if ui.small_button(if *asc { "up" } else { "down" }).on_hover_text("Ascending / descending").clicked() {
            *asc = !*asc;
        }
    }
}

/// An oxide value at or above the detection limit but below twice it.
pub fn near_detection_limit(row: &Row, col: &str) -> bool {
    let Some(ox) = col.strip_suffix("_wt%") else { return false };
    if ox.ends_with("_2s") || ox.ends_with("_DL") {
        return false;
    }
    match (num(row, col), num(row, &format!("{ox}_DL_wt%"))) {
        (Some(v), Some(dl)) if dl > 0.0 => v >= dl && v < 2.0 * dl,
        _ => false,
    }
}

/// Rows (indices) currently selected.
pub fn selected_indices(rows: &[Row], st: &TableState) -> Vec<usize> {
    rows.iter().enumerate().filter(|(_, r)| st.selected.contains(&text(r, "Row_ID"))).map(|(i, _)| i).collect()
}

/// Tab-separated text of rows for the clipboard.
pub fn copy_text(rows: &[Row], idx: &[usize], cols: &[String], dp: &DecimalPlaces) -> String {
    let mut s = cols.iter().map(|c| display_name(c)).collect::<Vec<_>>().join("\t");
    s.push('\n');
    for i in idx {
        s.push_str(&cols.iter().map(|c| cell_text(&rows[*i], c, dp, true)).collect::<Vec<_>>().join("\t"));
        s.push('\n');
    }
    s
}
