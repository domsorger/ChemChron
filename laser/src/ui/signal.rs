// SPDX-License-Identifier: GPL-3.0-or-later
//! Signal & selections: the time series with strips for ratios and ages, a list
//! for going through analyses one by one, and drawing or editing selections.

use crate::app::TraceApp;
use crate::plot::{self, Frame2};
use crate::theme::{self, fmt_pm, form_row, group_color, section, series_color, tag};
use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Shape, Stroke, Ui};
use std::collections::{BTreeSet, HashMap};
use chemchron_laser_core::autosel::AutoOptions;
use chemchron_laser_core::drs::upb::RatioKind;
use chemchron_laser_core::drs::SeriesKey;
use chemchron_laser_core::model::*;
use chemchron_laser_core::project;

/// Minerals a laser group can be (sent with the results to ChemChron).
pub const MINERALS: [&str; 10] = ["Monazite", "Xenotime", "Zircon", "Apatite", "Titanite", "Rutile", "Allanite", "Thorite", "Uraninite", "Thorianite"];

#[derive(Clone, Copy, PartialEq, Debug)]
enum DragKind {
    New { anchor: f64 },
    Start(u64),
    End(u64),
    Move { id: u64, grab: f64 },
    Pan { last: f32 },
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ChannelTab {
    Input,
    Intermediate,
    Output,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SignalTab {
    Selections,
    Automatic,
}

pub struct SignalState {
    pub x: (f64, f64),
    pub log_y: bool,
    /// Ticked channels. Count rates share the main plot; the rest get a strip each.
    pub picks: Vec<SeriesKey>,
    pub channel_tab: ChannelTab,
    pub show_baseline: bool,
    pub active_group: Option<u64>,
    /// The group the selections list shows; None means all analyses.
    pub list_group: Option<u64>,
    /// Zoom to each analysis when stepping through the list.
    pub follow: bool,
    pub tab: SignalTab,
    /// Selections marked in the list (ctrl / shift click) for moving to another group.
    pub marked: BTreeSet<u64>,
    /// Show the live concordia of the selected analysis beside the signal.
    pub live_concordia: bool,
    /// Dragging on the plot draws a cut-out (inclusion) in the selected analysis.
    pub cut_mode: bool,
    /// Channels a new cut-out applies to.
    pub cut_channels: BTreeSet<String>,
    /// Live concordia window: false = around the analysis, true = its whole group.
    pub live_group_view: bool,
    mark_anchor: Option<u64>,
    pub move_target: Option<u64>,
    scroll_to: Option<usize>,
    hover_x: Option<f64>,
    pub auto: AutoOptions,
    drag: Option<DragKind>,
    caches: HashMap<usize, PlotCache>,
    needs_fit: bool,
}

impl Default for SignalState {
    fn default() -> Self {
        SignalState {
            x: (0.0, 1.0),
            log_y: true,
            picks: Vec::new(),
            channel_tab: ChannelTab::Input,
            show_baseline: false,
            active_group: None,
            list_group: None,
            follow: true,
            tab: SignalTab::Selections,
            marked: BTreeSet::new(),
            live_concordia: true,
            cut_mode: false,
            cut_channels: BTreeSet::new(),
            live_group_view: false,
            mark_anchor: None,
            move_target: None,
            scroll_to: None,
            hover_x: None,
            auto: AutoOptions::default(),
            drag: None,
            caches: HashMap::new(),
            needs_fit: true,
        }
    }
}

impl SignalState {
    pub fn reset_for(&mut self, session: &Session) {
        let inputs = &session.inputs;
        let mut pick = Vec::new();
        for (e, m) in [("U", 238), ("Pb", 206), ("Pb", 207), ("Hg", 202)] {
            if let Some(i) = inputs.find_isotope(e, m) {
                pick.push(SeriesKey::Raw(i));
            }
        }
        if pick.is_empty() {
            pick = (0..inputs.channels.len().min(6)).map(SeriesKey::Raw).collect();
        }
        if inputs.find_isotope("U", 238).is_some() && inputs.find_isotope("Pb", 206).is_some() {
            pick.push(SeriesKey::RatioFinal(RatioKind::R68));
        }
        self.picks = pick;
        // Start close enough to see single spots; "Fit all" shows the whole session.
        let spots: Vec<&Selection> = session
            .selections
            .iter()
            .filter(|s| session.group_of(s).is_some_and(|g| g.kind != GroupKind::Baseline))
            .collect();
        if spots.len() > 8 {
            let a = spots[0].start - 25.0;
            self.x = (a, spots[7].end + 25.0);
            self.needs_fit = false;
        } else {
            self.needs_fit = true;
        }
        self.caches.clear();
        self.drag = None;
        self.hover_x = None;
        self.scroll_to = None;
        self.log_y = true;
        self.list_group = None;
        self.active_group = session
            .groups
            .iter()
            .find(|g| g.kind == GroupKind::Sample)
            .or(session.groups.first())
            .map(|g| g.id);
    }
}

#[derive(Clone)]
struct PlotCache {
    key: u64,
    y: (f64, f64),
    lines: Vec<(Color32, Vec<Vec<(f64, f64)>>)>,
    width: f32,
}

fn hash_key(parts: &[u64]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for p in parts {
        h ^= *p;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Reduce a series to at most a few points per pixel column, splitting where
/// values are missing or acquisition paused.
fn decimate(time: &[f64], values: &[f64], x: (f64, f64), width: f32, gap: f64) -> Vec<Vec<(f64, f64)>> {
    let a = time.partition_point(|t| *t < x.0).saturating_sub(1);
    let b = (time.partition_point(|t| *t <= x.1) + 1).min(time.len());
    let mut out: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut current: Vec<(f64, f64)> = Vec::new();
    let span = (x.1 - x.0).max(1e-12);
    let columns = width.max(1.0) as f64;
    let mut bucket = i64::MIN;
    let (mut lo, mut hi, mut first, mut last, mut bx, mut count) = (0.0, 0.0, 0.0, 0.0, 0.0, 0usize);
    let flush = |line: &mut Vec<(f64, f64)>, lo: f64, hi: f64, first: f64, last: f64, bx: f64, count: usize| {
        if count == 0 {
            return;
        }
        if count == 1 {
            line.push((bx, first));
        } else {
            line.push((bx, first));
            line.push((bx, lo));
            line.push((bx, hi));
            line.push((bx, last));
        }
    };
    let mut previous_t = f64::NEG_INFINITY;
    for i in a..b {
        let (t, v) = (time[i], values[i]);
        if !v.is_finite() || t - previous_t > gap {
            flush(&mut current, lo, hi, first, last, bx, count);
            count = 0;
            bucket = i64::MIN;
            if current.len() > 1 {
                out.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
        }
        previous_t = t;
        if !v.is_finite() {
            continue;
        }
        let k = (((t - x.0) / span) * columns).floor() as i64;
        if k != bucket {
            flush(&mut current, lo, hi, first, last, bx, count);
            bucket = k;
            count = 0;
        }
        if count == 0 {
            lo = v;
            hi = v;
            first = v;
            bx = t;
        }
        lo = f64::min(lo, v);
        hi = f64::max(hi, v);
        last = v;
        count += 1;
    }
    flush(&mut current, lo, hi, first, last, bx, count);
    if current.len() > 1 {
        out.push(current);
    }
    out
}

fn key_color(key: SeriesKey) -> Color32 {
    match key {
        SeriesKey::Raw(c) | SeriesKey::Subtracted(c) | SeriesKey::Ppm(c) | SeriesKey::BaselineCurve(c) => series_color(c),
        SeriesKey::TotalBeam => Color32::from_rgb(150, 150, 150),
        SeriesKey::BeamSeconds => series_color(13),
        SeriesKey::RatioRaw(k) | SeriesKey::RatioDownhole(k) | SeriesKey::RatioFinal(k) | SeriesKey::Age(k) => {
            series_color(k as usize + 1)
        }
    }
}

impl TraceApp {
    fn picked(&self, counts: bool) -> Vec<(SeriesKey, Color32, String)> {
        self.signal
            .picks
            .iter()
            .filter(|k| k.is_counts() == counts)
            .map(|k| (*k, key_color(*k), k.label(&self.session)))
            .collect()
    }

    pub fn page_signal(&mut self, ui: &mut Ui) {
        let t = self.theme();
        let typing = ui.ctx().memory(|m| m.focused().is_some());
        if !typing {
            let (up, down) = ui.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                )
            });
            if up {
                self.step_selection(-1);
            }
            if down {
                self.step_selection(1);
            }
        }
        egui::Panel::left(crate::theme::layout_id("signal_left"))
            .resizable(true).default_size(246.0).size_range(172.2..=615.0)
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                egui::ScrollArea::both().auto_shrink([false, false]).id_salt("signal_left").show(ui, |ui| {
                    ui.set_width(ui.available_width() - 4.0);
                    self.signal_channels(ui);
                    self.signal_groups(ui);
                });
            });
        egui::Panel::right(crate::theme::layout_id("signal_right"))
            .resizable(true).default_size(296.0).size_range(207.2..=740.0)
            .frame(egui::Frame::NONE.inner_margin(egui::Margin { left: 10, ..Default::default() }))
            .show(ui, |ui| {
                egui::ScrollArea::horizontal().auto_shrink([false, false]).id_salt("signal_right_h").show(ui, |ui| {
                let mut tab = self.signal.tab;
                theme::segmented(ui, &t, &mut tab, &[(SignalTab::Selections, "Selections"), (SignalTab::Automatic, "Automatic")]);
                self.signal.tab = tab;
                ui.add_space(6.0);
                match tab {
                    SignalTab::Selections => {
                        self.selection_list(ui);
                        ui.add_space(8.0);
                        egui::ScrollArea::vertical().id_salt("signal_inspector").show(ui, |ui| {
                            if self.signal.live_concordia && self.selected.is_some() && self.red.upb.is_some() {
                                self.live_concordia(ui);
                                ui.add_space(6.0);
                            }
                            if self.signal.live_concordia && self.selected.is_some() && self.red.te.is_some() {
                                self.live_ree(ui);
                                ui.add_space(6.0);
                            }
                            self.signal_inspector(ui);
                        });
                    }
                    SignalTab::Automatic => {
                        egui::ScrollArea::vertical().id_salt("signal_auto").show(ui, |ui| {
                            self.signal_auto(ui);
                        });
                    }
                }
                });
            });
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            self.signal_toolbar(ui);
            self.signal_plots(ui);
            ui.label(
                RichText::new("Drag to draw or resize. Up and Down go through the list. Scroll pans, pinch zooms.")
                    .color(t.dim)
                    .size(11.5),
            );
        });
    }

    fn pick_row(&mut self, ui: &mut Ui, key: SeriesKey, label: &str, note: Option<&str>) {
        let t = self.theme();
        let mut on = self.signal.picks.contains(&key);
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
            ui.painter().rect_filled(r, 2, key_color(key));
            if ui.checkbox(&mut on, label).changed() {
                if on {
                    self.signal.picks.push(key);
                } else {
                    self.signal.picks.retain(|k| *k != key);
                }
            }
            if let Some(n) = note {
                ui.label(RichText::new(n).color(t.dim).size(11.0));
            }
        });
    }

    fn signal_channels(&mut self, ui: &mut Ui) {
        let t = self.theme();
        let names: Vec<(usize, String, bool)> = self
            .session
            .inputs
            .channels
            .iter()
            .enumerate()
            .map(|(i, c)| (i, c.name.clone(), c.is_empty_signal()))
            .collect();
        let kinds: Vec<RatioKind> = self
            .red
            .upb
            .as_ref()
            .map_or_else(Vec::new, |u| RatioKind::ALL.iter().copied().filter(|k| u.ratio(*k).is_some()).collect());
        let te_channels: Vec<usize> = self.red.te.as_ref().map_or_else(Vec::new, |te| {
            te.calibrations.iter().enumerate().filter(|(_, c)| c.is_some()).map(|(i, _)| i).collect()
        });
        section(ui, &t, "Channels", |ui| {
            let mut tab = self.signal.channel_tab;
            theme::segmented(
                ui,
                &t,
                &mut tab,
                &[(ChannelTab::Input, "Input"), (ChannelTab::Intermediate, "Intermediate"), (ChannelTab::Output, "Output")],
            );
            self.signal.channel_tab = tab;
            ui.add_space(4.0);
            egui::ScrollArea::vertical().id_salt(("channel_list", tab as u8)).max_height(290.0).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                match tab {
                    ChannelTab::Input => {
                        for (i, name, empty) in &names {
                            self.pick_row(ui, SeriesKey::Raw(*i), name, empty.then_some("no signal"));
                        }
                    }
                    ChannelTab::Intermediate => {
                        egui::CollapsingHeader::new(RichText::new("Ratios").strong()).id_salt("int_ratios").default_open(true).show(ui, |ui| {
                            if kinds.is_empty() {
                                ui.label(RichText::new("Ratios appear once a primary reference is set (U–Pb scheme).").color(t.dim).size(11.5));
                            } else {
                                self.pick_row(ui, SeriesKey::BeamSeconds, "Beam seconds", None);
                            }
                            for k in &kinds {
                                self.pick_row(ui, SeriesKey::RatioRaw(*k), &format!("{} raw", k.key()), None);
                            }
                            for k in &kinds {
                                if *k != RatioKind::R76 {
                                    self.pick_row(ui, SeriesKey::RatioDownhole(*k), &format!("{} down-hole", k.key()), None);
                                }
                            }
                        });
                        egui::CollapsingHeader::new(RichText::new("Contents (counts minus baseline)").strong()).id_salt("int_contents").default_open(true).show(ui, |ui| {
                            self.pick_row(ui, SeriesKey::TotalBeam, "Total beam", None);
                            for (i, name, _) in &names {
                                self.pick_row(ui, SeriesKey::Subtracted(*i), name, None);
                            }
                        });
                    }
                    ChannelTab::Output => {
                        egui::CollapsingHeader::new(RichText::new("Ratios and ages").strong()).id_salt("out_ratios").default_open(true).show(ui, |ui| {
                            if kinds.is_empty() {
                                ui.label(RichText::new("Ratios and ages appear once a primary reference is set (U–Pb scheme).").color(t.dim).size(11.5));
                            }
                            for k in &kinds {
                                self.pick_row(ui, SeriesKey::RatioFinal(*k), &format!("{} final", k.key()), None);
                            }
                            for k in &kinds {
                                self.pick_row(ui, SeriesKey::Age(*k), &format!("{} age", k.key()), None);
                            }
                        });
                        egui::CollapsingHeader::new(RichText::new("Contents (ppm)").strong()).id_salt("out_contents").default_open(true).show(ui, |ui| {
                            if te_channels.is_empty() {
                                ui.label(RichText::new("Contents appear with the trace-element scheme and a calibration reference.").color(t.dim).size(11.5));
                            }
                            for c in &te_channels {
                                let name = names[*c].1.clone();
                                self.pick_row(ui, SeriesKey::Ppm(*c), &name, None);
                            }
                        });
                    }
                }
            });
        });
    }

    /// Selections the list shows, in time order.
    pub fn list_rows(&self) -> Vec<usize> {
        self.session
            .selections
            .iter()
            .enumerate()
            .filter(|(_, s)| match self.signal.list_group {
                Some(g) => s.group == g,
                None => self.session.group_of(s).is_some_and(|g| g.kind != GroupKind::Baseline),
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn selection_list(&mut self, ui: &mut Ui) {
        let t = self.theme();
        if self.signal.list_group.is_some_and(|g| self.session.group(g).is_none()) {
            self.signal.list_group = None;
        }
        let groups: Vec<(u64, String)> = self.session.groups.iter().map(|g| (g.id, g.name.clone())).collect();
        let rows = self.list_rows();
        let list_height = (ui.available_height() * 0.42).clamp(150.0, 360.0);
        t.card().inner_margin(egui::Margin::same(8)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let text = self
                    .signal
                    .list_group
                    .and_then(|c| groups.iter().find(|g| g.0 == c))
                    .map_or("All analyses", |g| g.1.as_str())
                    .to_string();
                egui::ComboBox::from_id_salt("list_group").selected_text(text).width(130.0).show_ui(ui, |ui| {
                    if ui.selectable_label(self.signal.list_group.is_none(), "All analyses").clicked() {
                        self.signal.list_group = None;
                    }
                    for (id, name) in &groups {
                        if ui.selectable_label(self.signal.list_group == Some(*id), name).clicked() {
                            self.signal.list_group = Some(*id);
                            self.signal.active_group = Some(*id);
                        }
                    }
                });
                ui.label(RichText::new(format!("{}", rows.len())).color(t.dim).size(12.0));
                ui.checkbox(&mut self.signal.follow, "Zoom").on_hover_text("Zoom to each analysis as you go through the list");
            });
            ui.add_space(2.0);
            const ROW: f32 = 22.0;
            let mut area = egui::ScrollArea::vertical()
                .id_salt("selection_rows")
                .max_height(list_height)
                .auto_shrink([false, true]);
            if let Some(i) = self.signal.scroll_to.take() {
                area = area.vertical_scroll_offset((i as f32 * ROW - list_height / 2.0 + ROW).max(0.0));
            }
            let mut clicked: Option<(u64, egui::Modifiers)> = None;
            let mut move_to: Option<(Vec<u64>, u64)> = None;
            area.show_rows(ui, ROW, rows.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for row in range {
                    let index = rows[row];
                    let s = &self.session.selections[index];
                    let result = self.red.selections.get(index);
                    let group = self.session.group_of(s);
                    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW), egui::Sense::click());
                    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &s.label));
                    let on = self.selected == Some(s.id);
                    if on {
                        ui.painter().rect_filled(rect, 3, t.accent.linear_multiply(if t.dark { 0.3 } else { 0.22 }));
                    } else if self.signal.marked.contains(&s.id) {
                        ui.painter().rect_filled(rect, 3, t.accent.linear_multiply(if t.dark { 0.16 } else { 0.12 }));
                    } else if response.hovered() {
                        ui.painter().rect_filled(rect, 3, t.raised);
                    }
                    let color = group.map_or(t.dim, |g| group_color(g.color));
                    ui.painter().rect_filled(
                        Rect::from_center_size(rect.left_center() + egui::vec2(9.0, 0.0), egui::vec2(9.0, 9.0)),
                        2,
                        color,
                    );
                    let flags = result.map_or(0, |r| r.flags.len());
                    let (detail, detail_color) = if s.excluded {
                        ("left out".to_string(), t.dim)
                    } else {
                        let age = result.and_then(|r| r.upb.as_ref()).map(|u| fmt_pm(u.a68.age, u.a68.prop2s));
                        let text = age.unwrap_or_else(|| format!("{:.0} s", s.end - s.start));
                        (text, if flags > 0 { t.warn } else { t.dim })
                    };
                    let clip = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
                    clip.text(
                        rect.right_center() - egui::vec2(5.0, 0.0),
                        Align2::RIGHT_CENTER,
                        &detail,
                        FontId::proportional(12.0),
                        detail_color,
                    );
                    let label_clip = Rect::from_min_max(rect.min, Pos2::new(rect.max.x - 100.0, rect.max.y)).intersect(ui.clip_rect());
                    ui.painter().with_clip_rect(label_clip).text(
                        rect.left_center() + egui::vec2(18.0, 0.0),
                        Align2::LEFT_CENTER,
                        &s.label,
                        FontId::proportional(12.5),
                        if s.excluded { t.dim } else { t.text },
                    );
                    let flag_text = result.map_or(String::new(), |r| r.flags.join(", "));
                    let response = if flag_text.is_empty() { response } else { response.on_hover_text(flag_text) };
                    if response.clicked() {
                        clicked = Some((s.id, ui.input(|i| i.modifiers)));
                    }
                    let sid = s.id;
                    response.context_menu(|ui| {
                        let targets: Vec<u64> = if self.signal.marked.contains(&sid) { self.signal.marked.iter().copied().collect() } else { vec![sid] };
                        ui.label(RichText::new(if targets.len() == 1 { "Move this analysis to".to_string() } else { format!("Move {} analyses to", targets.len()) }).strong());
                        for (gid, name) in &groups {
                            if ui.button(name).clicked() {
                                move_to = Some((targets.clone(), *gid));
                                ui.close();
                            }
                        }
                    });
                }
            });
            if let Some((id, m)) = clicked {
                if m.command {
                    if !self.signal.marked.remove(&id) {
                        self.signal.marked.insert(id);
                    }
                    self.signal.mark_anchor = Some(id);
                } else if m.shift {
                    let ids: Vec<u64> = rows.iter().map(|i| self.session.selections[*i].id).collect();
                    let a = self.signal.mark_anchor.or(self.selected).and_then(|a| ids.iter().position(|x| *x == a));
                    let b = ids.iter().position(|x| *x == id);
                    if let (Some(a), Some(b)) = (a, b) {
                        for x in &ids[a.min(b)..=a.max(b)] {
                            self.signal.marked.insert(*x);
                        }
                    }
                } else {
                    self.signal.marked.clear();
                    self.signal.marked.insert(id);
                    self.signal.mark_anchor = Some(id);
                }
                self.selected = Some(id);
                if self.signal.follow && !m.command && !m.shift {
                    self.zoom_to_selected();
                }
            }
            let n = self.signal.marked.len();
            if n > 1 {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{n} marked")).strong());
                    let target = self.signal.move_target.and_then(|g| groups.iter().find(|x| x.0 == g)).map_or("group…".to_string(), |g| g.1.clone());
                    egui::ComboBox::from_id_salt("move_target").selected_text(target).width(100.0).show_ui(ui, |ui| {
                        for (gid, name) in &groups {
                            ui.selectable_value(&mut self.signal.move_target, Some(*gid), name);
                        }
                    });
                    if ui.add_enabled(self.signal.move_target.is_some(), egui::Button::new("Move")).on_hover_text("Move the marked analyses to this group").clicked() {
                        if let Some(g) = self.signal.move_target {
                            move_to = Some((self.signal.marked.iter().copied().collect(), g));
                        }
                    }
                    if ui.small_button("Clear").clicked() {
                        self.signal.marked.clear();
                    }
                });
            }
            ui.label(RichText::new("Ctrl- or Shift-click to mark several; right-click to move them to another group.").color(t.dim).size(11.0));
            if let Some((ids, g)) = move_to {
                let name = groups.iter().find(|x| x.0 == g).map(|x| x.1.clone()).unwrap_or_default();
                let mut k = 0;
                for s in self.session.selections.iter_mut() {
                    if ids.contains(&s.id) && s.group != g {
                        s.group = g;
                        k += 1;
                    }
                }
                if k > 0 {
                    self.changed = true;
                    self.say(format!("Moved {k} analyses to {name}."), false);
                }
                self.signal.marked.clear();
            }
        });
    }

    fn signal_groups(&mut self, ui: &mut Ui) {
        let t = self.theme();
        let rows: Vec<(u64, String, GroupKind, [u8; 3], usize, Option<String>)> = self
            .session
            .groups
            .iter()
            .map(|g| {
                let n = self.session.selections.iter().filter(|s| s.group == g.id).count();
                (g.id, g.name.clone(), g.kind, g.color, n, g.rm.clone())
            })
            .collect();
        section(ui, &t, "Selection groups", |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for (id, name, kind, color, n, _) in &rows {
                let on = self.signal.active_group == Some(*id);
                let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), egui::Sense::click());
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
                if on {
                    ui.painter().rect_filled(rect, 3, t.raised);
                    ui.painter().rect_stroke(rect, 3, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
                }
                ui.painter().rect_filled(
                    Rect::from_center_size(rect.left_center() + egui::vec2(11.0, 0.0), egui::vec2(10.0, 10.0)),
                    2,
                    group_color(*color),
                );
                ui.painter().text(
                    rect.left_center() + egui::vec2(22.0, 0.0),
                    Align2::LEFT_CENTER,
                    name,
                    FontId::proportional(13.0),
                    t.text,
                );
                let kind_text = match kind {
                    GroupKind::Baseline => "BL",
                    GroupKind::ReferenceMaterial => "RM",
                    GroupKind::Sample => "",
                };
                ui.painter().text(
                    rect.right_center() - egui::vec2(6.0, 0.0),
                    Align2::RIGHT_CENTER,
                    format!("{kind_text} {n}"),
                    FontId::proportional(11.5),
                    t.dim,
                );
                if response.clicked() {
                    self.signal.active_group = Some(*id);
                    self.signal.list_group = Some(*id);
                    self.signal.tab = SignalTab::Selections;
                }
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.small_button("Add group").clicked() {
                    let mut k = 1;
                    let mut name = "New group".to_string();
                    while self.session.groups.iter().any(|g| g.name == name) {
                        k += 1;
                        name = format!("New group {k}");
                    }
                    let id = self.session.ensure_group(&name, GroupKind::Sample);
                    self.signal.active_group = Some(id);
                    self.changed = true;
                }
                if let Some(active) = self.signal.active_group {
                    let used = self.session.selections.iter().any(|s| s.group == active);
                    let remove = egui::Button::new("Remove").small();
                    if ui
                        .add_enabled(!used, remove)
                        .on_disabled_hover_text("Move or delete its selections first")
                        .clicked()
                    {
                        self.session.groups.retain(|g| g.id != active);
                        self.signal.active_group = self.session.groups.first().map(|g| g.id);
                        self.changed = true;
                    }
                }
            });
            if let Some(active) = self.signal.active_group {
                let names: Vec<String> = self.session.rms.iter().map(|m| m.name.clone()).collect();
                if let Some(g) = self.session.groups.iter_mut().find(|g| g.id == active) {
                    ui.add_space(4.0);
                    let mut changed = false;
                    ui.horizontal(|ui| {
                        ui.add_sized([62.0, 22.0], egui::Label::new(RichText::new("Name").color(t.dim)));
                        changed |= ui.add(egui::TextEdit::singleline(&mut g.name).desired_width(118.0)).lost_focus();
                    });
                    ui.horizontal(|ui| {
                        ui.add_sized([62.0, 22.0], egui::Label::new(RichText::new("Colour").color(t.dim)));
                        let mut c = g.color;
                        if ui.color_edit_button_srgb(&mut c).on_hover_text("Colour of this group in the signal, lists and plots").changed() {
                            g.color = c;
                            changed = true;
                        }
                    });
                    if g.kind != GroupKind::Baseline {
                        ui.horizontal(|ui| {
                            ui.add_sized([62.0, 22.0], egui::Label::new(RichText::new("Mineral").color(t.dim)));
                            let shown = g.mineral.clone().unwrap_or_else(|| "automatic".to_string());
                            egui::ComboBox::from_id_salt("group_mineral").selected_text(shown).width(110.0).show_ui(ui, |ui| {
                                if ui.selectable_label(g.mineral.is_none(), "automatic (from the reference, else monazite)").clicked() {
                                    g.mineral = None;
                                    changed = true;
                                }
                                for m in MINERALS {
                                    if ui.selectable_label(g.mineral.as_deref() == Some(m), m).clicked() {
                                        g.mineral = Some(m.to_string());
                                        changed = true;
                                    }
                                }
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.add_sized([62.0, 22.0], egui::Label::new(RichText::new("Reference").color(t.dim)));
                            let text = g.rm.clone().unwrap_or_else(|| "None: sample".to_string());
                            egui::ComboBox::from_id_salt("group_rm")
                                .selected_text(text)
                                .width(118.0)
                                .show_ui(ui, |ui| {
                                    if ui.selectable_label(g.rm.is_none(), "None: sample").clicked() {
                                        g.rm = None;
                                        g.kind = GroupKind::Sample;
                                        changed = true;
                                    }
                                    for n in &names {
                                        if ui.selectable_label(g.rm.as_deref() == Some(n), n).clicked() {
                                            g.rm = Some(n.clone());
                                            g.kind = GroupKind::ReferenceMaterial;
                                            changed = true;
                                        }
                                    }
                                });
                        });
                    }
                    self.changed |= changed;
                }
            }
        });
    }

    fn signal_toolbar(&mut self, ui: &mut Ui) {
        let t = self.theme();
        ui.horizontal_wrapped(|ui| {
            if ui.button("Fit all").clicked() {
                self.signal.needs_fit = true;
            }
            if ui.button("Previous spot").on_hover_text("Go to the previous analysis in the list").clicked() {
                self.step_selection(-1);
            }
            if ui.button("Next spot").on_hover_text("Go to the next analysis in the list").clicked() {
                self.step_selection(1);
            }
            ui.separator();
            let mut log = self.signal.log_y;
            if theme::segmented(ui, &t, &mut log, &[(true, "Log"), (false, "Linear")]) {
                self.signal.log_y = log;
            }
            ui.separator();
            ui.checkbox(&mut self.signal.show_baseline, "Show baseline curve");
            let te_on = self.session.settings.te.enabled;
            ui.separator();
            ui.label(RichText::new("Cut-outs").color(t.dim));
            let mut mode = self.session.settings.te.inclusions;
            let name = |m: InclusionMode| match m {
                InclusionMode::ByHand => "by hand",
                InclusionMode::Automatic => "automatic",
                InclusionMode::Both => "both",
            };
            egui::ComboBox::from_id_salt("incl_mode_signal")
                .selected_text(name(mode))
                .width(96.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut mode, InclusionMode::ByHand, "by hand");
                    ui.add_enabled_ui(te_on || self.session.settings.upb.enabled, |ui| {
                        ui.selectable_value(&mut mode, InclusionMode::Automatic, "automatic");
                        ui.selectable_value(&mut mode, InclusionMode::Both, "both");
                    })
                    .response
                    .on_disabled_hover_text("Automatic removal needs a reduction scheme");
                })
                .response
                .on_hover_text("Cut-outs (inclusion correction): drawn by hand, the automatic spike removal for trace elements, or both (also on Corrections)");
            if mode != self.session.settings.te.inclusions {
                self.session.settings.te.inclusions = mode;
                if te_on && !mode.by_hand() {
                    self.signal.cut_mode = false;
                }
                self.changed = true;
            }
            if !te_on || self.session.settings.te.inclusions.by_hand() {
                ui.separator();
                let label = if self.signal.cut_mode { RichText::new("Drawing cut-outs").color(egui::Color32::WHITE).strong() } else { RichText::new("Draw cut-out") };
                if ui
                    .add(egui::Button::new(label).fill(if self.signal.cut_mode { egui::Color32::from_rgb(170, 40, 40) } else { t.raised }))
                    .on_hover_text("Drag over an inclusion inside the selected analysis: the elements ticked in its panel (right) leave those sweeps out. Click again to go back to drawing selections.")
                    .clicked()
                {
                    self.signal.cut_mode = !self.signal.cut_mode;
                }
            }
            if let Some(g) = self.signal.active_group.and_then(|g| self.session.group(g)) {
                ui.separator();
                ui.label(RichText::new("Drawing in").color(t.dim));
                tag(ui, &g.name, group_color(g.color));
            }
        });
    }

    fn step_selection(&mut self, direction: i32) {
        let rows = self.list_rows();
        if rows.is_empty() {
            return;
        }
        let position = self
            .selected
            .and_then(|id| rows.iter().position(|i| self.session.selections[*i].id == id));
        let next = match position {
            Some(p) => (p as i32 + direction).rem_euclid(rows.len() as i32) as usize,
            None => 0,
        };
        self.selected = Some(self.session.selections[rows[next]].id);
        self.signal.scroll_to = Some(next);
        if self.signal.follow {
            self.zoom_to_selected();
        }
    }

    pub fn zoom_to_selected(&mut self) {
        if let Some(s) = self.selected.and_then(|id| self.session.selections.iter().find(|s| s.id == id)) {
            let pad = ((s.end - s.start) * 1.2).max(10.0);
            self.signal.x = (s.start - pad, s.end + pad);
        }
    }

    /// The main plot and one strip per ratio, age or content, all on one time axis.
    fn signal_plots(&mut self, ui: &mut Ui) {
        let inputs = self.session.inputs.clone();
        if inputs.time.is_empty() {
            ui.label("No data yet. Open a data folder on the Data page.");
            return;
        }
        let first = inputs.time[0];
        let last = *inputs.time.last().expect("not empty");
        if self.signal.needs_fit {
            let pad = (last - first) * 0.01;
            self.signal.x = (first - pad, last + pad);
            self.signal.needs_fit = false;
        }
        const STRIP_MIN: f32 = 64.0;
        const STRIP_MAX: f32 = 130.0;
        const MAIN_MIN: f32 = 190.0;
        let total = (ui.available_height() - 22.0).max(MAIN_MIN + 40.0);
        let width = ui.available_width();
        let main = self.picked(true);
        let strips = self.picked(false);
        let for_strips = total - MAIN_MIN - plot::MARGIN_BOTTOM;
        let room = (for_strips / STRIP_MIN).floor().max(0.0) as usize;
        let shown = strips.len().min(room);
        let strip_height = if shown > 0 {
            (for_strips / shown as f32).clamp(STRIP_MIN, STRIP_MAX)
        } else {
            0.0
        };
        let extra = if shown > 0 { plot::MARGIN_BOTTOM } else { 0.0 };
        let main_height = total - strip_height * shown as f32 - extra;
        let hover_x = self.signal.hover_x.take();
        ui.spacing_mut().item_spacing.y = 0.0;
        self.plot_one(ui, 0, &main, egui::vec2(width, main_height), self.signal.log_y, "CPS", shown == 0, hover_x);
        for (i, strip) in strips.iter().take(shown).enumerate() {
            let is_last = i + 1 == shown;
            let height = strip_height + if is_last { extra } else { 0.0 };
            let label = strip.2.clone();
            self.plot_one(ui, i + 1, std::slice::from_ref(strip), egui::vec2(width, height), false, &label, is_last, hover_x);
        }
        if strips.len() > shown {
            self.say(format!("{} ticked channels don't fit under the signal. Untick some, or make the window taller.", strips.len() - shown), true);
        }
        ui.spacing_mut().item_spacing.y = 6.0;
    }

    #[allow(clippy::too_many_arguments)]
    fn plot_one(
        &mut self,
        ui: &mut Ui,
        index: usize,
        traces: &[(SeriesKey, Color32, String)],
        size: egui::Vec2,
        log_y: bool,
        y_label: &str,
        show_x: bool,
        hover_x: Option<f64>,
    ) {
        let t = self.theme();
        let inputs = self.session.inputs.clone();
        let top = if index == 0 { 8.0 } else { 3.0 };
        let bottom = if show_x { plot::MARGIN_BOTTOM } else { 5.0 };
        let (response, painter, rect) = plot::allocate_with(ui, size, top, bottom);
        let response = response.on_hover_cursor(egui::CursorIcon::Crosshair);
        let key = {
            let mut parts = vec![
                self.signal.x.0.to_bits(),
                self.signal.x.1.to_bits(),
                rect.width().to_bits() as u64,
                self.revision,
                log_y as u64,
            ];
            for (k, _, _) in traces {
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                std::hash::Hash::hash(k, &mut hasher);
                parts.push(std::hash::Hasher::finish(&hasher));
            }
            hash_key(&parts)
        };
        let fresh = self.signal.caches.get(&index).is_none_or(|c| c.key != key || c.width != rect.width());
        if fresh {
            let gap = (inputs.sweep_seconds() * 4.0).max(1e-6);
            let mut lines = Vec::new();
            let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
            for (k, color, _) in traces {
                let values = self.series(*k);
                let segs = decimate(&inputs.time, &values, self.signal.x, rect.width(), gap);
                for seg in &segs {
                    for (_, v) in seg {
                        if !log_y || *v > 0.0 {
                            lo = lo.min(*v);
                            hi = hi.max(*v);
                        }
                    }
                }
                lines.push((*color, segs));
            }
            let y = if log_y {
                let top = if hi.is_finite() { 10f64.powf(hi.log10().ceil() + 0.1) } else { 1e6 };
                let bottom = if lo.is_finite() { 10f64.powf(lo.log10().floor()).max(1.0) } else { 1.0 };
                (bottom, top.max(bottom * 10.0))
            } else if lo.is_finite() && index > 0 {
                // Ratios from weak sweeps have long tails: use a robust range.
                let mut all: Vec<f64> = lines.iter().flat_map(|l| l.1.iter().flatten().map(|p| p.1)).collect();
                all.sort_by(f64::total_cmp);
                let (a, b) = (all[all.len() / 50], all[all.len() - 1 - all.len() / 50]);
                plot::pad(a, b, 0.15)
            } else if lo.is_finite() {
                plot::pad(lo.min(0.0), hi, 0.05)
            } else {
                (0.0, 1.0)
            };
            self.signal.caches.insert(index, PlotCache { key, y, lines, width: rect.width() });
        }
        let cache = self.signal.caches.get(&index).cloned().expect("just made");
        let f = Frame2 { rect, x: self.signal.x, y: cache.y, log_y };
        plot::axes_with(&painter, &t, &f, "Time (s)", y_label, show_x);
        let clip = painter.with_clip_rect(rect);
        self.paint_selections(&clip, &t, &f, index == 0);
        for (color, segs) in &cache.lines {
            for seg in segs {
                let points: Vec<Pos2> = seg.iter().map(|(x, y)| f.pos(*x, y.max(if f.log_y { 1.0 } else { f64::MIN }))).collect();
                clip.add(Shape::line(points, Stroke::new(1.1, *color)));
            }
        }
        if index == 0 && self.signal.show_baseline {
            let raw: Vec<usize> = self
                .signal
                .picks
                .iter()
                .filter_map(|k| match k {
                    SeriesKey::Raw(c) => Some(*c),
                    _ => None,
                })
                .collect();
            for c in raw {
                if let Some(curve) = self.red.baseline.get(c).and_then(|b| b.as_ref()) {
                    let n = rect.width() as usize / 4;
                    let points: Vec<Pos2> = (0..=n)
                        .map(|i| {
                            let x = f.x.0 + (f.x.1 - f.x.0) * i as f64 / n as f64;
                            f.pos(x, curve.eval(x).max(if f.log_y { 1.0 } else { f64::MIN }))
                        })
                        .collect();
                    clip.add(Shape::dashed_line(&points, Stroke::new(1.3, series_color(c)), 6.0, 4.0));
                }
            }
        }
        if index == 0 {
            if traces.is_empty() {
                painter.text(rect.center(), Align2::CENTER_CENTER, "Tick a channel on the left to see it here", FontId::proportional(13.0), t.dim);
            }
            self.signal_legend(&painter, &t, &f, traces);
        } else if cache.lines.iter().all(|l| l.1.is_empty()) {
            painter.text(rect.center(), Align2::CENTER_CENTER, "Nothing to show in this time range", FontId::proportional(12.0), t.dim);
        }
        self.plot_interaction(ui, &response, &f);
        if let Some(x) = hover_x {
            let px = f.px(x);
            if px >= rect.left() && px <= rect.right() {
                clip.line_segment([Pos2::new(px, rect.top()), Pos2::new(px, rect.bottom())], Stroke::new(1.0, t.dim.linear_multiply(0.4)));
            }
        }
        // Readout at the pointer.
        if let Some(p) = response.hover_pos() {
            if rect.contains(p) && self.signal.drag.is_none() {
                let tx = f.x_at(p.x);
                self.signal.hover_x = Some(tx);
                let i = inputs.time.partition_point(|v| *v < tx).min(inputs.time.len() - 1);
                let mut lines = vec![format!("{:.1} s", inputs.time[i])];
                for (k, _, label) in traces.iter().take(8) {
                    let values = self.series(*k);
                    lines.push(format!("{label}: {}", theme::fmt_sig(values[i], 4)));
                }
                if let Some(s) = self.selection_at(tx) {
                    lines.push(format!("Selection: {}", s.label));
                }
                plot::hover_box(&painter, &t, rect, p, &lines);
            }
        }
    }

    fn selection_at(&self, x: f64) -> Option<&Selection> {
        self.session.selections.iter().find(|s| x >= s.start && x <= s.end)
    }

    fn paint_selections(&self, clip: &egui::Painter, t: &theme::Theme, f: &Frame2, labels: bool) {
        let rect = f.rect;
        for s in &self.session.selections {
            if s.end < f.x.0 || s.start > f.x.1 {
                continue;
            }
            let Some(g) = self.session.group_of(s) else { continue };
            let (x0, x1) = (f.px(s.start), f.px(s.end));
            let c = group_color(g.color);
            let selected = self.selected == Some(s.id);
            let alpha = if s.excluded {
                0.05
            } else if selected {
                0.32
            } else if g.kind == GroupKind::Baseline {
                0.10
            } else {
                0.18
            };
            let band = Rect::from_min_max(Pos2::new(x0, rect.top()), Pos2::new(x1, rect.bottom()));
            clip.rect_filled(band, 0, c.linear_multiply(alpha));
            if selected {
                clip.rect_stroke(band, 0, Stroke::new(1.5, c), egui::StrokeKind::Inside);
            }
            if s.excluded {
                clip.line_segment([band.left_top(), band.right_bottom()], Stroke::new(1.0, t.alarm.linear_multiply(0.5)));
            }
            if selected {
                // cut-outs drawn by hand: red bands with their elements
                for c in &s.cutouts {
                    let r = Rect::from_min_max(Pos2::new(f.px(c.start), rect.top()), Pos2::new(f.px(c.end), rect.bottom()));
                    // hatched, so it stands apart from the analysis band whatever the group colour
                    let ink = if t.dark { egui::Color32::from_white_alpha(150) } else { egui::Color32::from_black_alpha(140) };
                    clip.rect_filled(r, 0, if t.dark { egui::Color32::from_black_alpha(90) } else { egui::Color32::from_white_alpha(110) });
                    let pc = clip.with_clip_rect(r.intersect(rect));
                    let mut x = r.left() - r.height();
                    while x < r.right() {
                        pc.line_segment([Pos2::new(x, r.bottom()), Pos2::new(x + r.height(), r.top())], Stroke::new(1.0, ink.linear_multiply(0.45)));
                        x += 9.0;
                    }
                    clip.extend(Shape::dashed_line(&[r.left_top(), r.left_bottom()], Stroke::new(1.5, ink), 5.0, 3.0));
                    clip.extend(Shape::dashed_line(&[r.right_top(), r.right_bottom()], Stroke::new(1.5, ink), 5.0, 3.0));
                    if labels {
                        let txt = format!("cut: {}", c.channels.join(" "));
                        let g = clip.layout_no_wrap(txt, FontId::proportional(11.0), ink);
                        let at = Pos2::new(r.left() + 3.0, rect.top() + 34.0);
                        clip.rect_filled(Rect::from_min_size(at, g.size()).expand(2.0), 2, if t.dark { egui::Color32::from_black_alpha(200) } else { egui::Color32::from_white_alpha(220) });
                        clip.galley(at, g, ink);
                    }
                }
                // sweeps the automatic removal left out: ticks at the bottom
                if let Some(res) = self.red.result_for(s.id) {
                    for cut in res.cuts.iter().filter(|c| c.auto) {
                        for i in &cut.sweeps {
                            if let Some(tm) = self.session.inputs.time.get(*i) {
                                let x = f.px(*tm);
                                clip.line_segment([Pos2::new(x, rect.bottom() - 9.0), Pos2::new(x, rect.bottom())], Stroke::new(2.0, t.warn));
                            }
                        }
                    }
                }
            }
            if labels && x1 - x0 > 42.0 && g.kind != GroupKind::Baseline {
                clip.text(
                    Pos2::new(x0 + 3.0, rect.top() + 20.0),
                    Align2::LEFT_TOP,
                    &s.label,
                    FontId::proportional(11.0),
                    c,
                );
            }
        }
    }

    fn signal_legend(&self, painter: &egui::Painter, t: &theme::Theme, f: &Frame2, traces: &[(SeriesKey, Color32, String)]) {
        let mut x = f.rect.left() + 50.0;
        let y = f.rect.top() - 2.0 + 12.0;
        let font = FontId::proportional(11.5);
        for (_, color, label) in traces.iter().take(10) {
            let short = label.trim_end_matches(" (CPS)").to_string();
            let g = painter.layout_no_wrap(short, font.clone(), t.text);
            let w = g.size().x;
            if x + w + 26.0 > f.rect.right() {
                break;
            }
            painter.rect_filled(Rect::from_center_size(Pos2::new(x, y), egui::vec2(12.0, 3.0)), 1, *color);
            painter.galley(Pos2::new(x + 9.0, y - g.size().y / 2.0), g, t.text);
            x += w + 26.0;
        }
    }

    fn plot_interaction(&mut self, ui: &Ui, response: &egui::Response, f: &Frame2) {
        const EDGE: f32 = 6.0;
        let min_len = (self.session.inputs.sweep_seconds() * 3.0).max(0.01);
        // Zoom and pan with the wheel.
        if response.hovered() {
            let (scroll, zoom, shift) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers.shift));
            let span = f.x.1 - f.x.0;
            let mut x = f.x;
            let mut moved = false;
            if (zoom - 1.0).abs() > 1e-3 {
                if let Some(p) = response.hover_pos() {
                    let c = f.x_at(p.x);
                    let k = 1.0 / zoom as f64;
                    x = (c + (x.0 - c) * k, c + (x.1 - c) * k);
                    moved = true;
                }
            }
            let dx = if shift { scroll.y } else { scroll.x + scroll.y };
            if dx.abs() > 0.01 {
                let d = -(dx as f64) / f.rect.width() as f64 * span;
                x = (x.0 + d, x.1 + d);
                moved = true;
            }
            if moved {
                self.signal.x = x;
            }
        }
        let hover_x = response.hover_pos().map(|p| p.x);
        // What is under the pointer?
        let hit = |this: &TraceApp, px: f32| -> Option<DragKind> {
            for s in this.session.selections.iter().rev() {
                let (a, b) = (f.px(s.start), f.px(s.end));
                if (px - a).abs() <= EDGE && b - a > EDGE {
                    return Some(DragKind::Start(s.id));
                }
                if (px - b).abs() <= EDGE && b - a > EDGE {
                    return Some(DragKind::End(s.id));
                }
            }
            None
        };
        if let Some(px) = hover_x {
            if self.signal.drag.is_none() && hit(self, px).is_some() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
        }
        if response.drag_started_by(egui::PointerButton::Primary) {
            if let Some(p) = response.interact_pointer_pos() {
                let press = p - response.drag_delta();
                let x = f.x_at(press.x);
                let kind = if self.signal.cut_mode { None } else { hit(self, press.x) }.or_else(|| {
                    if self.signal.cut_mode {
                        return None;
                    }
                    if ui.input(|i| i.modifiers.alt) {
                        return Some(DragKind::Pan { last: press.x });
                    }
                    self.selection_at(x)
                        .filter(|s| self.selected == Some(s.id))
                        .map(|s| DragKind::Move { id: s.id, grab: x - s.start })
                });
                self.signal.drag = Some(kind.unwrap_or(DragKind::New { anchor: x }));
                self.dragging = true;
            }
        }
        if let (Some(kind), Some(p)) = (self.signal.drag, response.interact_pointer_pos()) {
            let x = f.x_at(p.x);
            match kind {
                DragKind::New { anchor } => {
                    let (a, b) = (anchor.min(x), anchor.max(x));
                    let band = Rect::from_min_max(Pos2::new(f.px(a), f.rect.top()), Pos2::new(f.px(b), f.rect.bottom()));
                    ui.painter().rect_filled(band, 0, self.theme().accent.linear_multiply(0.2));
                }
                DragKind::Start(id) => {
                    if let Some(s) = self.session.selections.iter_mut().find(|s| s.id == id) {
                        s.start = x.min(s.end - min_len);
                    }
                    self.stale = true;
                }
                DragKind::End(id) => {
                    if let Some(s) = self.session.selections.iter_mut().find(|s| s.id == id) {
                        s.end = x.max(s.start + min_len);
                    }
                    self.stale = true;
                }
                DragKind::Move { id, grab } => {
                    self.stale = true;
                    if let Some(s) = self.session.selections.iter_mut().find(|s| s.id == id) {
                        let len = s.end - s.start;
                        s.start = x - grab;
                        s.end = s.start + len;
                    }
                }
                DragKind::Pan { last } => {
                    let d = (p.x - last) as f64 / f.rect.width() as f64 * (f.x.1 - f.x.0);
                    self.signal.x = (f.x.0 - d, f.x.1 - d);
                    self.signal.drag = Some(DragKind::Pan { last: p.x });
                }
            }
        }
        if response.drag_stopped() {
            if let (Some(kind), Some(p)) = (self.signal.drag.take(), response.interact_pointer_pos()) {
                let x = f.x_at(p.x);
                match kind {
                    DragKind::New { anchor } => {
                        let (a, b) = (anchor.min(x), anchor.max(x));
                        if self.signal.cut_mode {
                            self.add_cutout(a, b);
                        } else if b - a >= min_len {
                            self.add_selection(a, b);
                        }
                    }
                    DragKind::Pan { .. } => {}
                    _ => {
                        self.session.sort_selections();
                        self.changed = true;
                    }
                }
            }
            self.dragging = false;
        }
        if response.clicked() {
            if let Some(p) = response.interact_pointer_pos() {
                self.selected = self.selection_at(f.x_at(p.x)).map(|s| s.id);
                // the list on the right shows the clicked selection's group (baselines too)
                // and scrolls to it
                if let Some(g) = self.selected.and_then(|id| self.session.selections.iter().find(|s| s.id == id)).map(|s| s.group) {
                    self.signal.list_group = Some(g);
                    self.signal.active_group = Some(g);
                }
                let rows = self.list_rows();
                self.signal.scroll_to = self
                    .selected
                    .and_then(|id| rows.iter().position(|i| self.session.selections[*i].id == id));
            }
        }
        if response.double_clicked() {
            if self.selected.is_some() {
                self.zoom_to_selected();
            } else {
                self.signal.needs_fit = true;
            }
        }
        let focused = ui.ctx().memory(|m| m.focused().is_some());
        if !focused && self.selected.is_some() {
            let delete = ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace));
            if delete {
                let id = self.selected.take();
                self.session.selections.retain(|s| Some(s.id) != id);
                self.changed = true;
            }
        }
    }

    /// Makes the part of a spot under one of its cut-outs an analysis of its
    /// own (label `_incl`, group "<group> inclusions", all channels). The spot
    /// itself stays one analysis over its whole range, with its cut-out, so the
    /// cut elements still leave that part out and every other element keeps it.
    pub fn split_off(&mut self, id: u64, cut: usize) {
        let Some(host) = self.session.selections.iter().find(|s| s.id == id).cloned() else { return };
        let Some(c) = host.cutouts.get(cut).cloned() else { return };
        let label = format!("{}_incl", host.label);
        if self.session.selections.iter().any(|s| s.label == label && (s.start - c.start).abs() < 1e-6 && (s.end - c.end).abs() < 1e-6) {
            self.say(format!("{label} is already there."), true);
            return;
        }
        let group_name = self.session.group(host.group).map(|g| g.name.clone()).unwrap_or_default();
        let incl_group = self.session.ensure_group(&format!("{group_name} inclusions"), GroupKind::Sample);
        // the new group starts with the spot group's internal standard and mineral (change them if the inclusion differs)
        let (is_ppm, mineral) = self.session.group(host.group).map(|g| (g.is_ppm, g.mineral.clone())).unwrap_or((None, None));
        if let Some(g) = self.session.groups.iter_mut().find(|g| g.id == incl_group) {
            if g.is_ppm.is_none() {
                g.is_ppm = is_ppm;
            }
            if g.mineral.is_none() {
                g.mineral = mineral;
            }
        }
        let mut incl = host.clone();
        incl.id = self.session.next_id();
        incl.group = incl_group;
        incl.label = label.clone();
        incl.start = c.start.max(host.start);
        incl.end = c.end.min(host.end);
        incl.cutouts.clear();
        incl.source = SelectionSource::Manual;
        self.session.selections.push(incl);
        self.session.sort_selections();
        self.changed = true;
        self.say(format!("{label} is now an analysis of its own (group {group_name} inclusions, all elements); {} stays one analysis with its cut-out. Set the inclusion's internal-standard content and mineral if it is a different mineral.", host.label), false);
    }

    /// Copies a cut-out to every marked analysis, at the same time after its start.
    pub fn copy_cutout_to_marked(&mut self, id: u64, cut: usize) {
        let Some(host) = self.session.selections.iter().find(|s| s.id == id).cloned() else { return };
        let Some(c) = host.cutouts.get(cut).cloned() else { return };
        let (a, b) = (c.start - host.start, c.end - host.start);
        let marked: Vec<u64> = self.signal.marked.iter().copied().filter(|m| *m != id).collect();
        let mut n = 0;
        for s in self.session.selections.iter_mut().filter(|s| marked.contains(&s.id)) {
            let (x, y) = ((s.start + a).max(s.start), (s.start + b).min(s.end));
            if y > x && !s.cutouts.iter().any(|o| (o.start - x).abs() < 1e-6 && (o.end - y).abs() < 1e-6 && o.channels == c.channels) {
                s.cutouts.push(Cutout { start: x, end: y, channels: c.channels.clone() });
                n += 1;
            }
        }
        if n > 0 {
            self.changed = true;
        }
        self.say(format!("Cut-out copied to {n} marked analyses."), n == 0);
    }

    /// A cut-out (inclusion) in the selected analysis for the chosen elements.
    fn add_cutout(&mut self, start: f64, end: f64) {
        let Some(id) = self.selected else {
            self.say("Select the analysis first, then drag over its inclusion.", true);
            return;
        };
        if self.signal.cut_channels.is_empty() {
            self.say("Choose the elements of the cut-out first (in the selection's panel on the right).", true);
            return;
        }
        let channels: Vec<String> = self.signal.cut_channels.iter().cloned().collect();
        let Some(s) = self.session.selections.iter_mut().find(|s| s.id == id) else { return };
        let (a, b) = (start.max(s.start), end.min(s.end));
        if b <= a {
            self.say("A cut-out has to lie inside the selected analysis.", true);
            return;
        }
        s.cutouts.push(Cutout { start: a, end: b, channels: channels.clone() });
        let label = s.label.clone();
        self.changed = true;
        self.say(format!("{label}: {} left out between {a:.1} and {b:.1} s.", channels.join(", ")), false);
    }

    fn add_selection(&mut self, start: f64, end: f64) {
        let Some(group) = self.signal.active_group.or(self.session.groups.first().map(|g| g.id)) else {
            let id = self.session.ensure_group("New group", GroupKind::Sample);
            self.signal.active_group = Some(id);
            return self.add_selection(start, end);
        };
        let id = self.session.next_id();
        let name = self.session.group(group).map_or("Selection", |g| g.name.as_str()).to_string();
        let number = self.session.selections.iter().filter(|s| s.group == group).count() + 1;
        self.session.selections.push(Selection {
            id,
            group,
            label: format!("{name} {number}"),
            start,
            end,
            comment: String::new(),
            excluded: false,
            source: SelectionSource::Manual,
            laser_on: None,
            is_ppm: None,
            cutouts: Vec::new(),
            x: None,
            y: None,
            spot_size: None,
        });
        self.session.sort_selections();
        self.selected = Some(id);
        self.changed = true;
    }

    fn signal_inspector(&mut self, ui: &mut Ui) {
        let t = self.theme();
        let Some(id) = self.selected else {
            section(ui, &t, "Selection", |ui| {
                ui.label(RichText::new("Click a row above or a selection on the plot to see its results and edit it.").color(t.dim));
            });
            return;
        };
        let Some(index) = self.session.selections.iter().position(|s| s.id == id) else {
            self.selected = None;
            return;
        };
        let groups: Vec<(u64, String)> = self.session.groups.iter().map(|g| (g.id, g.name.clone())).collect();
        let (is_f, is_unit) = self.is_unit();
        let te_on = self.session.settings.te.enabled;
        // only U–Pb: cut-outs by hand on the raw U, Th and Pb signals
        let upb_only = !te_on && self.session.settings.upb.enabled;
        let by_hand = if te_on { self.session.settings.te.inclusions.by_hand() } else { upb_only };
        let channel_names: Vec<String> = self.session.inputs.channels.iter().map(|c| c.name.clone()).collect();
        let mut cut_channels = self.signal.cut_channels.clone();
        let mut cut_mode = self.signal.cut_mode;
        let time = &self.session.inputs.time;
        // (text, time span of an automatic cut)
        let auto_lines: Vec<(String, Option<(String, f64, f64)>)> = self
            .red
            .result_for(id)
            .map(|r| {
                r.cuts
                    .iter()
                    .map(|c| {
                        let name = channel_names.get(c.channel).cloned().unwrap_or_default();
                        let span = if c.auto {
                            let lo = c.sweeps.iter().filter_map(|i| time.get(*i)).copied().fold(f64::INFINITY, f64::min);
                            let hi = c.sweeps.iter().filter_map(|i| time.get(*i)).copied().fold(f64::NEG_INFINITY, f64::max);
                            lo.is_finite().then(|| (name.clone(), lo, hi))
                        } else {
                            None
                        };
                        (format!("{name}: {} sweeps left out ({})", c.sweeps.len(), if c.auto { "automatic" } else { "by hand" }), span)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut changed = false;
        let mut delete = false;
        let mut split: Option<usize> = None;
        let mut copy_cut: Option<usize> = None;
        let n_marked = self.signal.marked.len();
        section(ui, &t, "Selection", |ui| {
            let s = &mut self.session.selections[index];
            form_row(ui, &t, "Label", |ui| {
                changed |= ui.add(egui::TextEdit::singleline(&mut s.label).desired_width(110.0)).lost_focus();
            });
            form_row(ui, &t, "Group", |ui| {
                let current = groups.iter().find(|g| g.0 == s.group).map_or("?", |g| g.1.as_str()).to_string();
                egui::ComboBox::from_id_salt("sel_group").selected_text(current).width(100.0).show_ui(ui, |ui| {
                    for (g, name) in &groups {
                        if ui.selectable_label(s.group == *g, name).clicked() {
                            s.group = *g;
                            changed = true;
                        }
                    }
                });
            });
            form_row(ui, &t, "Start (s)", |ui| {
                changed |= ui.add(egui::DragValue::new(&mut s.start).speed(0.1).max_decimals(2)).changed();
            });
            form_row(ui, &t, "End (s)", |ui| {
                changed |= ui.add(egui::DragValue::new(&mut s.end).speed(0.1).max_decimals(2)).changed();
            });
            let mut included = !s.excluded;
            if ui.checkbox(&mut included, "Use in calculations").changed() {
                s.excluded = !included;
                changed = true;
            }
            let mut has_value = s.is_ppm.is_some();
            form_row(ui, &t, &format!("Int. std ({is_unit})"), |ui| {
                if ui.checkbox(&mut has_value, "").on_hover_text("Own value for this spot (otherwise the group value)").changed() {
                    s.is_ppm = if has_value { Some(100000.0) } else { None };
                    changed = true;
                }
                if let Some(v) = &mut s.is_ppm {
                    let mut shown = *v / is_f;
                    let sp = shown.abs().max(1e-3) * 1e-3;
                    if ui.add(egui::DragValue::new(&mut shown).speed(sp).max_decimals(4).range(0.0..=1e6)).changed() {
                        *v = shown * is_f;
                        changed = true;
                    }
                }
            });
            if by_hand {
                ui.add_space(4.0);
                ui.label(RichText::new("Inclusion correction").strong());
                if upb_only {
                    ui.label(RichText::new("A cut-out on Pb206, Pb207, Pb208, U238 or Th232 leaves its sweeps out of the ratios using that channel.").color(t.dim).size(11.0));
                }
                let mut remove: Option<usize> = None;
                for (k, c) in s.cutouts.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{:.1}–{:.1} s: {}", c.start, c.end, c.channels.join(", "))).size(12.0).color(t.alarm));
                        if ui.small_button("x").on_hover_text("Remove this cut-out").clicked() {
                            remove = Some(k);
                        }
                        if n_marked > 1
                            && ui
                                .small_button("Copy to marked")
                                .on_hover_text("Add this cut-out to every marked analysis, at the same time after its start (Ctrl- or Shift-click analyses in the list to mark them)")
                                .clicked()
                        {
                            copy_cut = Some(k);
                        }
                        if ui
                            .small_button("Split off")
                            .on_hover_text("Make this part an analysis of its own as well (label _incl, all elements); the spot stays one analysis with this cut-out")
                            .clicked()
                        {
                            split = Some(k);
                        }
                    });
                }
                if let Some(k) = remove {
                    s.cutouts.remove(k);
                    changed = true;
                }
                ui.horizontal_wrapped(|ui| {
                    let shown = if cut_channels.is_empty() { "choose elements".to_string() } else { cut_channels.iter().cloned().collect::<Vec<_>>().join(", ") };
                    egui::ComboBox::from_id_salt("cut_channels").selected_text(shown).width(150.0).show_ui(ui, |ui| {
                        let offered: Vec<&String> = channel_names.iter().filter(|n| !upb_only || ["Pb", "U", "Th"].iter().any(|e| n.starts_with(e))).collect();
                        let mut all = !offered.is_empty() && offered.iter().all(|n| cut_channels.contains(*n));
                        if ui.checkbox(&mut all, RichText::new("Select all").strong()).changed() {
                            if all {
                                cut_channels.extend(offered.iter().map(|n| n.to_string()));
                            } else {
                                cut_channels.clear();
                            }
                        }
                        ui.separator();
                        for n in offered {
                            let mut on = cut_channels.contains(n);
                            if ui.checkbox(&mut on, n).changed() {
                                if on {
                                    cut_channels.insert(n.clone());
                                } else {
                                    cut_channels.remove(n);
                                }
                            }
                        }
                    });
                    let label = if cut_mode { RichText::new("Drawing…").color(egui::Color32::WHITE).strong() } else { RichText::new("Draw a cut-out") };
                    if ui.add(egui::Button::new(label).fill(if cut_mode { egui::Color32::from_rgb(170, 40, 40) } else { t.raised })).on_hover_text("Then drag over the inclusion on the plot").clicked() {
                        cut_mode = !cut_mode;
                    }
                });
            }
            if te_on {
                for (line, span) in &auto_lines {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(line).size(11.5).color(t.warn));
                        if let Some((name, a, b)) = span {
                            if ui.small_button("Split off").on_hover_text("Make the part with this inclusion an analysis of its own as well (all elements); the spot stays as it is").clicked() {
                                // all elements cut automatically over about the same time go with it
                                let names: Vec<String> = auto_lines
                                    .iter()
                                    .filter_map(|(_, x)| x.as_ref())
                                    .filter(|(_, c, d)| *c < *b + 0.5 && *d > *a - 0.5)
                                    .map(|(n, _, _)| n.clone())
                                    .collect();
                                s.cutouts.push(Cutout { start: *a, end: *b, channels: if names.is_empty() { vec![name.clone()] } else { names } });
                                split = Some(s.cutouts.len() - 1);
                            }
                        }
                    });
                }
            }
            ui.label(RichText::new("Comment").color(t.dim));
            changed |= ui.add(egui::TextEdit::singleline(&mut s.comment).desired_width(f32::INFINITY)).lost_focus();
            ui.horizontal(|ui| {
                if ui.small_button("Show in results").clicked() {
                    self.results.jump_to = Some(id);
                    self.page = crate::app::Page::Results;
                }
                if ui.small_button("Delete").clicked() {
                    delete = true;
                }
            });
        });
        self.signal.cut_channels = cut_channels;
        self.signal.cut_mode = cut_mode;
        if let Some(k) = split {
            self.split_off(id, k);
            return;
        }
        if let Some(k) = copy_cut {
            self.copy_cutout_to_marked(id, k);
        }
        if delete {
            self.session.selections.retain(|s| s.id != id);
            self.selected = None;
            self.changed = true;
            return;
        }
        self.changed |= changed;
        if let Some(r) = self.red.result_for(id).cloned() {
            let group_is_baseline = self.session.group_of(&self.session.selections[index]).is_some_and(|g| g.kind == GroupKind::Baseline);
            if group_is_baseline {
                return;
            }
            section(ui, &t, "Result", |ui| {
                if !r.flags.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        for f in &r.flags {
                            tag(ui, f, t.warn);
                        }
                    });
                }
                if let Some(u) = &r.upb {
                    egui::Grid::new("sel_result").num_columns(2).spacing([8.0, 2.0]).show(ui, |ui| {
                        let mut row = |name: &str, value: String| {
                            ui.label(RichText::new(name).color(t.dim).size(12.0));
                            ui.label(RichText::new(value).size(12.5));
                            ui.end_row();
                        };
                        row("206/238 age", format!("{} Ma", fmt_pm(u.a68.age, u.a68.prop2s)));
                        row("207/235 age", format!("{} Ma", fmt_pm(u.a75.age, u.a75.prop2s)));
                        row("207/206 age", format!("{} Ma", fmt_pm(u.a76.age, u.a76.prop2s)));
                        if let Some(a) = u.a82 {
                            row("208/232 age", format!("{} Ma", fmt_pm(a.age, a.prop2s)));
                        }
                        row("Discordance", format!("{:.1} %", u.discordance_pct));
                        row("Th/U", theme::fmt_sig(u.th_u, 3));
                    });
                    ui.label(RichText::new("Analytical + excess 2s").color(t.dim).size(11.0));
                } else {
                    ui.label(RichText::new("No age for this selection.").color(t.dim));
                }
            });
        }
    }

    fn signal_auto(&mut self, ui: &mut Ui) {
        let t = self.theme();
        let has_log = !self.session.inputs.laser.is_empty();
        section(ui, &t, "Automatic selections", |ui| {
            ui.label(
                RichText::new(if has_log {
                    "Made from the laser log. Crops are in seconds."
                } else {
                    "Made from the signal, because there is no laser log. Crops are in seconds."
                })
                .color(t.dim)
                .size(12.0),
            );
            let a = &mut self.signal.auto;
            form_row(ui, &t, "Spot start crop", |ui| {
                ui.add(egui::DragValue::new(&mut a.crop_start_s).speed(0.1).range(0.0..=30.0));
            });
            form_row(ui, &t, "Spot end crop", |ui| {
                ui.add(egui::DragValue::new(&mut a.crop_end_s).speed(0.1).range(0.0..=30.0));
            });
            form_row(ui, &t, "Baseline start", |ui| {
                ui.add(egui::DragValue::new(&mut a.baseline_crop_start_s).speed(0.1).range(0.0..=60.0))
                    .on_hover_text("Seconds left out at the start of each baseline: after a laser firing (washout) and at the start of each data file.");
            });
            form_row(ui, &t, "Baseline end", |ui| {
                ui.add(egui::DragValue::new(&mut a.baseline_crop_end_s).speed(0.1).range(0.0..=60.0));
            });
            if ui.button("Remake all selections").on_hover_text("Replaces every selection and group. Undo brings them back.").clicked() {
                self.pending_remake = true;
            }
            if ui
                .button("Import selections…")
                .on_hover_text("A CSV table of kind, label, group, start and end. Replaces the current selections. Undo brings them back.")
                .clicked()
            {
                if let Some(path) = rfd::FileDialog::new().add_filter("CSV", &["csv", "txt"]).pick_file() {
                    let result = std::fs::read_to_string(&path)
                        .map_err(|e| e.to_string())
                        .and_then(|text| project::apply_selection_table(&mut self.session, &text));
                    match result {
                        Ok(count) => {
                            self.selected = None;
                            self.changed = true;
                            self.say(format!("Imported {count} selections."), false);
                        }
                        Err(e) => self.say(e, true),
                    }
                }
            }
            if self.pending_remake {
                ui.colored_label(t.warn, "This replaces all selections, groups and their settings.");
                ui.horizontal(|ui| {
                    if ui.button("Replace them").clicked() {
                        let options = self.signal.auto;
                        // Keep the reference-material choices that still fit.
                        let upb_primary_name = self.session.settings.upb.primary_group.and_then(|g| self.session.group(g)).map(|g| g.name.clone());
                        let te_primary_name = self.session.settings.te.primary_group.and_then(|g| self.session.group(g)).map(|g| g.name.clone());
                        let count = project::auto_select(&mut self.session, &options);
                        let find = |s: &Session, n: Option<String>| n.and_then(|n| s.groups.iter().find(|g| g.name == n).map(|g| g.id));
                        self.session.settings.upb.primary_group = find(&self.session, upb_primary_name);
                        self.session.settings.te.primary_group = find(&self.session, te_primary_name);
                        self.session.apply_defaults();
                        self.signal.active_group = self.session.groups.first().map(|g| g.id);
                        self.selected = None;
                        self.pending_remake = false;
                        self.changed = true;
                        // baselines shorter than the minimum are left out: say so
                        let n_bl = self.session.selections.iter().filter(|s| self.session.group_of(s).is_some_and(|g| g.kind == GroupKind::Baseline)).count();
                        let n_spots = self.session.selections.len() - n_bl;
                        if n_spots > 0 && n_bl * 2 < n_spots {
                            self.say(
                                format!(
                                    "Made {count} selections, but only {n_bl} baselines for {n_spots} analyses: the gaps before the firings are too short for Baseline start {:.1} s + Baseline end {:.1} s + at least {:.0} s. Lower Baseline start or end.",
                                    options.baseline_crop_start_s, options.baseline_crop_end_s, options.min_baseline_s
                                ),
                                true,
                            );
                        } else {
                            self.say(format!("Made {count} selections."), false);
                        }
                    }
                    if ui.button("Keep them").clicked() {
                        self.pending_remake = false;
                    }
                });
            }
        });
    }
}
