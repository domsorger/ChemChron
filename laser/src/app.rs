// SPDX-License-Identifier: GPL-3.0-or-later
//! Application state and the frame around the pages.

use crate::theme::Theme;
use crate::ui;
use eframe::Frame;
use egui::{Color32, Context, Margin, RichText, Stroke, Ui};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use chemchron_laser_core::drs::{self, Reduction, SeriesKey};
use chemchron_laser_core::model::*;
use chemchron_laser_core::{project, synthetic};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Page {
    Data,
    Signal,
    Corrections,
    Results,
    Concordia,
    Ages,
    Elements,
    Map,
    Quality,
    References,
    Export,
}

impl Page {
    pub const ALL: [Page; 11] = [
        Page::Data,
        Page::Signal,
        Page::Corrections,
        Page::Results,
        Page::Concordia,
        Page::Ages,
        Page::Elements,
        Page::Map,
        Page::Quality,
        Page::References,
        Page::Export,
    ];
    pub fn title(&self) -> &'static str {
        match self {
            Page::Data => "Data",
            Page::Signal => "Signal & selections",
            Page::Corrections => "Corrections",
            Page::Results => "Results",
            Page::Concordia => "Concordia",
            Page::Ages => "Ages and ratios",
            Page::Elements => "Elements",
            Page::Map => "Spot map",
            Page::Quality => "Quality",
            Page::References => "Reference materials",
            Page::Export => "Export",
        }
    }
    pub fn from_name(name: &str) -> Option<Page> {
        let n = name.to_lowercase();
        Page::ALL.iter().copied().find(|p| {
            p.title().to_lowercase().starts_with(&n) || format!("{p:?}").to_lowercase() == n
        })
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum StepState {
    Ok,
    Attention,
    Problem,
    Idle,
}

/// Ratios sent to Combined results.
/// Systematic term of the lab (long-term log or manual, with or without the
/// decay constants), 2σ % per age type: ²⁰⁶Pb/²³⁸U, ²⁰⁷Pb/²³⁵U, ²⁰⁷Pb/²⁰⁶Pb, ²⁰⁸Pb/²³²Th.
#[derive(Clone, Debug, PartialEq)]
pub struct LabSystematic {
    pub pct: [f64; 4],
    pub source: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SendRatios {
    #[default]
    Measured,
    /// Common-Pb corrected (the method chosen on Corrections).
    CommonPb,
    /// Common-Pb corrected and 206Pb/238U corrected for 230Th disequilibrium.
    CommonPbTh230,
}

pub struct TraceApp {
    pub session: Session,
    pub red: Reduction,
    pub page: Page,
    pub dark: bool,
    applied_dark: Option<bool>,
    /// Session as of the last recorded edit.
    committed: Session,
    last_edit_time: f64,
    undo: Vec<Session>,
    redo: Vec<Session>,
    /// A page changed the session this frame.
    pub changed: bool,
    /// The results must be recalculated once the current drag ends.
    pub stale: bool,
    /// Set by the "from EPMA" button (internal standard); ChemChron fills the contents.
    pub request_is_from_epma: bool,
    /// Set by "Send to Combined results"; ChemChron links the results.
    pub request_push: bool,
    /// Set by "Demo session" inside ChemChron: the demo with EPMA data too.
    pub request_demo: bool,
    /// Results revision last sent to Combined results (ChemChron).
    pub sent_revision: Option<u64>,
    /// The lab's systematic term (2σ %, for ²⁰⁶Pb/²³⁸U, ²⁰⁷Pb/²³⁵U, ²⁰⁷Pb/²⁰⁶Pb and
    /// ²⁰⁸Pb/²³²Th ages), from the long-term log or manual, set by ChemChron; the
    /// primary's accepted-value uncertainty is added from the session itself.
    pub lab_systematic: Option<LabSystematic>,
    /// Which ratios go to Combined results.
    pub send_ratios: SendRatios,
    /// Counts recorded edits (ChemChron's one undo history follows it).
    pub edits: u64,
    pub dragging: bool,
    pub unsaved: bool,
    pub session_path: Option<PathBuf>,
    pub selected: Option<u64>,
    pub series_cache: HashMap<SeriesKey, Arc<Vec<f64>>>,
    pub revision: u64,
    pub notice: Option<(String, bool)>,
    pub notice_time: f64,
    pub import_job: Option<Receiver<Result<Session, String>>>,
    pub pending_remake: bool,
    confirm_close: bool,
    allow_close: bool,
    // Page states.
    pub signal: ui::signal::SignalState,
    pub corrections: ui::corrections::CorrectionsState,
    pub results: ui::results::ResultsState,
    pub concordia: ui::concordia::ConcordiaState,
    pub ages: ui::ages::AgesState,
    pub elements: ui::elements::ElementsState,
    pub map: ui::map::MapState,
    pub references: ui::references::ReferencesState,
    pub export: ui::export::ExportState,
    pub hidden_groups: BTreeSet<u64>,
    screenshot: Option<ScreenshotJob>,
    frame_count: u64,
    /// Shown inside ChemChron: no title, theme switch or close prompt of its own.
    pub embedded: bool,
}

struct ScreenshotJob {
    path: PathBuf,
    requested: bool,
    done: Arc<Mutex<Option<Result<(), String>>>>,
}

impl TraceApp {
    pub fn new(ctx: &Context) -> TraceApp {
        let session = synthetic::demo_session(1);
        let red = drs::reduce(&session);
        let mut app = TraceApp {
            committed: session.clone(),
            last_edit_time: f64::NEG_INFINITY,
            session,
            red,
            page: Page::Signal,
            dark: true,
            applied_dark: None,
            undo: Vec::new(),
            redo: Vec::new(),
            changed: false,
            stale: false,
            request_is_from_epma: false,
            request_push: false,
            request_demo: false,
            sent_revision: None,
            lab_systematic: None,
            send_ratios: SendRatios::Measured,
            edits: 0,
            dragging: false,
            unsaved: false,
            session_path: None,
            selected: None,
            series_cache: HashMap::new(),
            revision: 1,
            notice: None,
            notice_time: 0.0,
            import_job: None,
            pending_remake: false,
            confirm_close: false,
            allow_close: false,
            signal: Default::default(),
            corrections: Default::default(),
            results: Default::default(),
            concordia: Default::default(),
            ages: Default::default(),
            map: Default::default(),
            elements: Default::default(),
            references: Default::default(),
            export: Default::default(),
            hidden_groups: BTreeSet::new(),
            screenshot: None,
            frame_count: 0,
            embedded: false,
        };
        app.after_session_replaced();
        app.apply_theme(ctx);
        app
    }

    pub fn request_screenshot(&mut self, path: PathBuf) {
        self.screenshot = Some(ScreenshotJob {
            path,
            requested: false,
            done: Arc::new(Mutex::new(None)),
        });
    }

    pub fn theme(&self) -> Theme {
        Theme::get(self.dark)
    }

    fn apply_theme(&mut self, ctx: &Context) {
        if self.applied_dark != Some(self.dark) {
            self.theme().apply(ctx);
            self.applied_dark = Some(self.dark);
        }
    }

    /// No laser data open (ChemChron starts like this).
    pub fn is_empty(&self) -> bool {
        self.session.inputs.time.is_empty()
    }

    /// An empty session: nothing open yet.
    pub fn start_empty(&mut self) {
        let rms = self.session.rms.clone();
        self.session = Session::new("No laser data", SessionSource::Files { paths: Vec::new(), laser_log: None }, Inputs::default(), rms);
        self.undo.clear();
        self.redo.clear();
        self.committed = self.session.clone();
        self.session_path = None;
        self.unsaved = false;
        self.recalculate();
        self.after_session_replaced();
        self.page = Page::Data;
    }

    /// The built-in simulated session (to try the program).
    pub fn load_demo(&mut self) {
        self.load_demo_part(chemchron_laser_core::model::DemoPart::Combined);
    }

    /// One of the simulated sessions: both methods, U–Pb only or trace elements only.
    pub fn load_demo_part(&mut self, part: chemchron_laser_core::model::DemoPart) {
        self.take_session(Ok(synthetic::demo_session_part(1, part)));
        self.undo.clear();
        self.redo.clear();
        self.committed = self.session.clone();
        self.unsaved = false;
        self.page = Page::Signal;
    }

    pub fn is_demo(&self) -> bool {
        matches!(self.session.source, SessionSource::Synthetic { .. })
    }

    /// Every page starts over after a new session comes in.
    pub fn after_session_replaced(&mut self) {
        self.selected = None;
        self.committed = self.session.clone();
        self.last_edit_time = f64::NEG_INFINITY;
        self.undo.clear();
        self.redo.clear();
        self.unsaved = false;
        self.series_cache.clear();
        self.revision += 1;
        self.signal.reset_for(&self.session);
        self.ages.group = None;
        self.corrections = Default::default();
        self.elements = Default::default();
        self.concordia = Default::default();
        self.results = Default::default();
        self.references = Default::default();
        self.hidden_groups.clear();
        for g in self.session.groups.iter_mut() {
            if g.kind == GroupKind::Baseline && g.color == chemchron_laser_core::model::OLD_BASELINE_COLOR {
                g.color = chemchron_laser_core::model::BASELINE_COLOR;
            }
        }
        for g in &self.session.groups {
            let glass = self.session.rm_of(g.id).is_some_and(|m| m.matrix == "Glass");
            if glass {
                self.hidden_groups.insert(g.id);
            }
        }
    }

    pub fn recalculate(&mut self) {
        self.red = drs::reduce(&self.session);
        self.series_cache.clear();
        self.revision += 1;
        self.stale = false;
    }

    fn record_edit(&mut self, now: f64) {
        let before = std::mem::replace(&mut self.committed, self.session.clone());
        // Rapid changes, like dragging a number, undo together as one step.
        if now - self.last_edit_time > 0.6 || self.undo.is_empty() {
            self.undo.push(before);
            self.edits += 1;
            if self.undo.len() > 200 {
                self.undo.remove(0);
            }
        }
        self.last_edit_time = now;
        self.redo.clear();
        self.unsaved = true;
        self.stale = true;
    }

    /// Uses the lab's excess rule (no undo step: it is a lab setting) and recalculates.
    pub fn set_excess_rule(&mut self, rule: chemchron_laser_core::model::ExcessRule) {
        if self.session.settings.upb.excess_rule == rule {
            return;
        }
        self.session.settings.upb.excess_rule = rule.clone();
        self.committed.settings.upb.excess_rule = rule;
        self.recalculate();
    }

    /// Sets a group's colour without an undo step (the lab's colour for this group name).
    pub fn set_group_colour_quiet(&mut self, id: u64, c: [u8; 3]) {
        for s in [&mut self.session, &mut self.committed] {
            if let Some(g) = s.groups.iter_mut().find(|g| g.id == id) {
                g.color = c;
            }
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) {
        self.last_edit_time = f64::NEG_INFINITY;
        if let Some(previous) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.session, previous));
            self.committed = self.session.clone();
            self.unsaved = true;
            self.recalculate();
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.session, next));
            self.committed = self.session.clone();
            self.unsaved = true;
            self.recalculate();
        }
    }

    pub fn say(&mut self, text: impl Into<String>, problem: bool) {
        self.notice = Some((text.into(), problem));
        self.notice_time = 0.0;
    }

    pub fn series(&mut self, key: SeriesKey) -> Arc<Vec<f64>> {
        if let Some(v) = self.series_cache.get(&key) {
            return v.clone();
        }
        let v = Arc::new(drs::series(&self.session, &self.red, key));
        self.series_cache.insert(key, v.clone());
        v
    }

    /// Open data or a session right away, used at start-up from the command line.
    pub fn open_blocking(&mut self, paths: &[PathBuf]) {
        let result = load_any(paths);
        self.take_session(result);
    }

    fn take_session(&mut self, result: Result<Session, String>) {
        match result {
            Ok(session) => {
                let name = session.name.clone();
                self.session = session;
                self.recalculate();
                self.after_session_replaced();
                if self.session.selections.is_empty() {
                    self.say(
                        format!("Opened {name}, but no signal stood out to select. Draw selections by hand."),
                        true,
                    );
                } else {
                    self.say(format!("Opened {name}."), false);
                }
            }
            Err(e) => self.say(e, true),
        }
    }

    /// Read files on another thread so the window stays responsive.
    pub fn start_import(&mut self, paths: Vec<PathBuf>) {
        let (tx, rx) = channel();
        self.import_job = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(load_any(&paths));
        });
    }

    pub fn start_new_demo(&mut self) {
        self.session = synthetic::demo_session(1);
        self.session_path = None;
        self.recalculate();
        self.after_session_replaced();
        self.say("Opened the demo session with simulated data.", false);
    }

    pub fn save(&mut self, path: Option<PathBuf>) {
        let target = path.or_else(|| self.session_path.clone()).or_else(|| {
            rfd::FileDialog::new()
                .set_file_name(format!("{}.laser.json", sanitize(&self.session.name)))
                .add_filter("ChemChron laser session", &["json"])
                .save_file()
        });
        if let Some(target) = target {
            match project::save_session(&self.session, &target) {
                Ok(()) => {
                    self.say(format!("Saved {}", target.display()), false);
                    self.session_path = Some(target);
                    self.unsaved = false;
                }
                Err(e) => self.say(e, true),
            }
        }
    }

    pub fn ui_root(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.frame_count += 1;
        self.apply_theme(&ctx);
        self.poll_import();
        self.shortcuts(&ctx);
        let t = self.theme();
        egui::Panel::top("top")
            .exact_size(46.0)
            .frame(egui::Frame::NONE.fill(t.panel).inner_margin(Margin::symmetric(12, 7)).stroke(Stroke::new(1.0, t.border)))
            .show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("status")
            .exact_size(26.0)
            .frame(egui::Frame::NONE.fill(t.panel).inner_margin(Margin::symmetric(12, 4)).stroke(Stroke::new(1.0, t.border)))
            .show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("nav")
            .exact_size(186.0)
            .frame(egui::Frame::NONE.fill(t.panel).inner_margin(Margin::symmetric(8, 10)).stroke(Stroke::new(1.0, t.border)))
            .show(ui, |ui| self.navigation(ui));
        if !self.page_visible(self.page) {
            self.page = Page::Corrections;
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(t.bg).inner_margin(Margin::same(10)))
            .show(ui, |ui| match self.page {
                p if p != Page::Data && self.is_empty() => {
                    let t = self.theme();
                    ui.add_space(40.0);
                    ui.vertical_centered(|ui| {
                        ui.heading("No laser data yet");
                        ui.label(RichText::new("Open a data folder or a saved laser session on the Data page.").color(t.dim));
                        if ui.button("Go to Data").clicked() {
                            self.page = Page::Data;
                        }
                    });
                }
                Page::Data => self.page_data(ui),
                Page::Signal => self.page_signal(ui),
                Page::Corrections => self.page_corrections(ui),
                Page::Results => self.page_results(ui),
                Page::Concordia => self.page_concordia(ui),
                Page::Ages => self.page_ages(ui),
                Page::Elements => self.page_elements(ui),
                Page::Map => self.page_map(ui),
                Page::Quality => self.page_quality(ui),
                Page::References => self.page_references(ui),
                Page::Export => self.page_export(ui),
            });
        if !self.embedded {
            self.close_dialog(&ctx);
        }
        if self.changed {
            self.changed = false;
            let now = ctx.input(|i| i.time);
            self.record_edit(now);
        }
        // While a selection is dragged the results follow live when the
        // reduction is quick (the live concordia on the Signal page).
        if self.stale && (!self.dragging || self.red.elapsed_ms < 120.0) {
            self.recalculate();
            ctx.request_repaint();
        }
        self.drive_screenshot(&ctx);
    }

    fn poll_import(&mut self) {
        if let Some(rx) = &self.import_job {
            if let Ok(result) = rx.try_recv() {
                self.import_job = None;
                self.take_session(result);
            }
        }
    }

    fn shortcuts(&mut self, ctx: &Context) {
        if self.embedded {
            return; // ChemChron routes undo, redo and save to the part in view
        }
        let typing = ctx.memory(|m| m.focused().is_some());
        let (undo, redo, save) = ctx.input_mut(|i| {
            let redo = i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Z);
            let undo = i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z);
            let save = i.consume_key(egui::Modifiers::COMMAND, egui::Key::S);
            (undo, redo, save)
        });
        if !typing {
            if undo {
                self.undo();
            }
            if redo {
                self.redo();
            }
        }
        if save {
            self.save(None);
        }
    }

    fn top_bar(&mut self, ui: &mut Ui) {
        let t = self.theme();
        ui.horizontal_centered(|ui| {
            if self.embedded {
                ui.label(RichText::new("Laser session").color(t.dim));
            } else {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
                let p = ui.painter();
                p.circle_stroke(rect.center(), 9.0, Stroke::new(2.0, t.accent));
                p.line_segment(
                    [rect.center() + egui::vec2(-6.0, 4.0), rect.center() + egui::vec2(6.0, -4.0)],
                    Stroke::new(2.0, t.accent),
                );
                ui.label(RichText::new("ChemChron LA-ICP-MS").strong().size(15.0));
                ui.separator();
            }
            ui.label(RichText::new(&self.session.name).color(if self.embedded { t.text } else { t.dim }));
            if self.is_demo() {
                crate::theme::tag(ui, "Simulated data", t.warn);
            }
            if self.unsaved {
                ui.label(RichText::new("• unsaved").color(t.dim).size(12.0));
            }
            if self.embedded && self.sent_revision.is_some_and(|r| r != self.revision) {
                crate::theme::tag(ui, "changed since sent to Combined results", t.warn);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !self.embedded {
                    let label = if self.dark { "Light" } else { "Dark" };
                    if ui.button(label).on_hover_text("Switch colour theme").clicked() {
                        self.dark = !self.dark;
                    }
                    ui.separator();
                }
                // inside ChemChron, Undo and Redo sit in the main header (one history)
                if !self.embedded {
                    if ui.add_enabled(self.can_redo(), egui::Button::new("Redo")).clicked() {
                        self.redo();
                    }
                    if ui
                        .add_enabled(self.can_undo(), egui::Button::new("Undo"))
                        .on_hover_text("Undo the last change to selections or settings")
                        .clicked()
                    {
                        self.undo();
                    }
                    ui.separator();
                }
                let save_label = if self.embedded { "Save laser session" } else { "Save" };
                if ui.button(save_label).on_hover_text("Save this laser session as a readable text file").clicked() {
                    self.save(None);
                }
                if ui.button("Open…").on_hover_text("Open a data folder, data files or a saved session").clicked() {
                    self.page = Page::Data;
                }
            });
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let t = self.theme();
        ui.horizontal_centered(|ui| {
            if self.import_job.is_some() {
                ui.add(egui::Spinner::new().size(13.0));
                ui.label(RichText::new("Reading files…").size(12.0));
            } else if let Some((text, problem)) = self.notice.clone() {
                let color = if problem { t.alarm } else { t.dim };
                ui.label(RichText::new(text).color(color).size(12.0));
            } else {
                let n = self.session.selections.len();
                ui.label(
                    RichText::new(format!(
                        "{} sweeps · {} channels · {} selections · reduced in {:.0} ms",
                        self.session.inputs.time.len(),
                        self.session.inputs.channels.len(),
                        n,
                        self.red.elapsed_ms
                    ))
                    .color(t.dim)
                    .size(12.0),
                );
            }
        });
    }

    pub fn step_state(&self, page: Page) -> StepState {
        match page {
            Page::Data => {
                if self.session.inputs.time.is_empty() {
                    StepState::Problem
                } else {
                    StepState::Ok
                }
            }
            Page::Signal => {
                if self.session.selections.is_empty() {
                    StepState::Problem
                } else {
                    StepState::Ok
                }
            }
            Page::Corrections => {
                if self.red.upb.is_none() && self.red.te.is_none() {
                    StepState::Problem
                } else {
                    StepState::Ok
                }
            }
            Page::Results | Page::Concordia | Page::Ages => {
                if self.red.upb.is_some() {
                    StepState::Ok
                } else {
                    StepState::Idle
                }
            }
            Page::Elements => {
                if self.red.te.is_some() {
                    StepState::Ok
                } else {
                    StepState::Idle
                }
            }
            Page::Map => {
                if self.has_positions() {
                    StepState::Ok
                } else {
                    StepState::Idle
                }
            }
            Page::Quality => {
                if self.red.qa.alarms() > 0 {
                    StepState::Problem
                } else if self.red.qa.warnings() > 0 || !self.red.warnings.is_empty() {
                    StepState::Attention
                } else {
                    StepState::Ok
                }
            }
            Page::References => {
                let unconfirmed = self
                    .session
                    .groups
                    .iter()
                    .filter_map(|g| self.session.rm_of(g.id))
                    .any(|m| !m.confirmed);
                if unconfirmed {
                    StepState::Attention
                } else {
                    StepState::Ok
                }
            }
            Page::Export => StepState::Idle,
        }
    }

    /// Pages shown in the step list: only those of the chosen reduction
    /// scheme; inside ChemChron the reference list is in Settings.
    pub fn page_visible(&self, page: Page) -> bool {
        let st = &self.session.settings;
        match page {
            Page::References => !self.embedded,
            Page::Concordia | Page::Ages => st.upb.enabled,
            Page::Elements => st.te.enabled,
            _ => true,
        }
    }

    fn navigation(&mut self, ui: &mut Ui) {
        let t = self.theme();
        ui.spacing_mut().item_spacing.y = 2.0;
        let mut number = 0;
        let mut separated = false;
        for page in Page::ALL {
            if !self.page_visible(page) {
                continue;
            }
            if matches!(page, Page::References | Page::Export) && !separated {
                separated = true;
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);
            }
            let numbered = !matches!(page, Page::References | Page::Export);
            if numbered {
                number += 1;
            }
            let on = self.page == page;
            let color = match self.step_state(page) {
                StepState::Ok => t.ok,
                StepState::Attention => t.warn,
                StepState::Problem => t.alarm,
                StepState::Idle => t.border,
            };
            let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), egui::Sense::click());
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, page.title()));
            if on {
                ui.painter().rect_filled(rect, 5, t.raised);
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(rect.min + egui::vec2(0.0, 6.0), egui::vec2(3.0, 20.0)),
                    2,
                    t.accent,
                );
            } else if response.hovered() {
                ui.painter().rect_filled(rect, 5, t.raised.linear_multiply(0.5));
            }
            let text_color = if on { t.text } else { t.dim };
            let label = if numbered { format!("{number}  {}", page.title()) } else { page.title().to_string() };
            ui.painter().text(
                rect.left_center() + egui::vec2(12.0, 0.0),
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::proportional(13.5),
                text_color,
            );
            ui.painter().circle_filled(rect.right_center() - egui::vec2(12.0, 0.0), 3.5, color);
            if response.clicked() {
                self.page = page;
            }
        }
    }

    fn close_dialog(&mut self, ctx: &Context) {
        let close_asked = ctx.input(|i| i.viewport().close_requested());
        if close_asked && self.unsaved && !self.allow_close && !self.is_demo() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
        }
        if self.confirm_close {
            let t = self.theme();
            egui::Modal::new(egui::Id::unique("confirm_close")).show(ctx, |ui| {
                ui.set_width(340.0);
                ui.label(RichText::new("Save this session before closing?").strong());
                ui.label(RichText::new("Your raw files are safe either way. Selections and settings since the last save would be lost.").color(t.dim));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        self.save(None);
                        if !self.unsaved {
                            self.allow_close = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        self.confirm_close = false;
                    }
                    if ui.button("Don't save").clicked() {
                        self.allow_close = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        self.confirm_close = false;
                    }
                    if ui.button("Cancel").clicked() {
                        self.confirm_close = false;
                    }
                });
            });
        }
    }

    fn drive_screenshot(&mut self, ctx: &Context) {
        let frame = self.frame_count;
        let Some(job) = &mut self.screenshot else { return };
        ctx.request_repaint();
        if frame >= 120 && !job.requested {
            job.requested = true;
            let path = job.path.clone();
            let done = job.done.clone();
            let callback = egui::ScreenshotCallback::new(move |image: Arc<egui::ColorImage>| {
                *done.lock().expect("screenshot state") = Some(save_png(&path, &image));
            });
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(callback));
        }
        let finished = job.done.lock().expect("screenshot state").take();
        if let Some(result) = finished {
            match result {
                Ok(()) => {
                    self.allow_close = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    self.screenshot = None;
                }
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            }
        }
    }
}

impl eframe::App for TraceApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut Frame) {
        self.ui_root(ui);
    }
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let c: Color32 = self.theme().bg;
        [
            c.r() as f32 / 255.0,
            c.g() as f32 / 255.0,
            c.b() as f32 / 255.0,
            1.0,
        ]
    }
}

fn save_png(path: &Path, image: &egui::ColorImage) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    let [w, h] = image.size;
    let mut bytes = Vec::with_capacity(w * h * 4);
    for p in &image.pixels {
        bytes.extend_from_slice(&[p.r(), p.g(), p.b(), 255]);
    }
    image::save_buffer(path, &bytes, w as u32, h as u32, image::ColorType::Rgba8)
        .map_err(|e| format!("{}: {e}", path.display()))
}

pub fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

/// A saved session, or data files and folders.
pub fn load_any(paths: &[PathBuf]) -> Result<Session, String> {
    if let [one] = paths {
        if one.is_file() && one.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")) {
            let loaded = project::load_session(one)?;
            let mut session = loaded.session;
            if !loaded.changed_files.is_empty() {
                let mut inputs = (*session.inputs).clone();
                inputs.warnings.extend(loaded.changed_files);
                session.inputs = Arc::new(inputs);
            }
            return Ok(session);
        }
    }
    project::session_from_paths(paths, None)
}
