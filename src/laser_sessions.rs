// SPDX-License-Identifier: GPL-3.0-or-later
//! Several laser sessions in one project. The open session is the one the
//! LA-ICP-MS part shows; the others wait here and are saved with the project
//! (`"laser_sessions"`). A strip above the LA-ICP-MS part switches between
//! them, adds a new one or removes one.

use crate::app::ChemApp;
use chemchron_laser_core::model::Session;
use egui::{RichText, Ui};
use std::path::PathBuf;

/// A laser session that isn't open right now.
#[derive(Clone)]
pub struct Waiting {
    pub session: Session,
    pub unsaved: bool,
    pub path: Option<PathBuf>,
}

impl Waiting {
    pub fn name(&self) -> String {
        session_name(&self.session)
    }
}

pub fn session_name(s: &Session) -> String {
    if s.name.trim().is_empty() { "Laser session".into() } else { s.name.clone() }
}

/// The open session, taken out of the laser part (which is left empty).
fn take_open(app: &mut ChemApp) -> Option<Waiting> {
    if app.laser.is_empty() {
        return None;
    }
    Some(Waiting { session: app.laser.session.clone(), unsaved: app.laser.unsaved, path: app.laser.session_path.clone() })
}

fn put_open(app: &mut ChemApp, w: Waiting) {
    app.laser.session = w.session;
    app.laser.recalculate();
    app.laser.after_session_replaced();
    app.laser.unsaved = w.unsaved;
    app.laser.session_path = w.path;
    app.laser.sent_revision = None;
}

/// Opens waiting session `k`; the open one waits in its place.
pub fn switch_to(app: &mut ChemApp, k: usize) {
    if k >= app.laser_waiting.len() {
        return;
    }
    let next = app.laser_waiting.remove(k);
    if let Some(cur) = take_open(app) {
        app.laser_waiting.insert(k, cur);
    }
    let name = next.name();
    put_open(app, next);
    app.say(format!("Opened the laser session {name}."), false);
}

/// Keeps the open session in the project and starts an empty one (open data files next).
pub fn add_new(app: &mut ChemApp) {
    if let Some(cur) = take_open(app) {
        app.laser_waiting.push(cur);
    }
    app.laser.start_empty();
    app.laser.session_path = None;
    app.laser.unsaved = false;
    app.laser.sent_revision = None;
    app.laser.page = chemchron_laser::app::Page::Data;
}

/// Removes the open session from the project; the next waiting one opens.
pub fn remove_open(app: &mut ChemApp) {
    let name = session_name(&app.laser.session);
    if app.laser_waiting.is_empty() {
        app.laser.start_empty();
        app.laser.session_path = None;
    } else {
        let next = app.laser_waiting.remove(0);
        put_open(app, next);
    }
    app.laser.unsaved = false;
    app.data.unsaved = true;
    app.say(format!("Removed the laser session {name} from the project (its data files are untouched)."), false);
}

/// The strip above the LA-ICP-MS part.
pub fn strip(app: &mut ChemApp, ui: &mut Ui) {
    let t = app.theme();
    if app.laser.is_demo() && app.laser_waiting.is_empty() {
        return;
    }
    let mut pick: Option<usize> = None;
    let mut add = false;
    let mut remove = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new("Laser session").color(t.dim));
        let open = if app.laser.is_empty() { "(new, no data yet)".to_string() } else { session_name(&app.laser.session) };
        egui::ComboBox::from_id_salt("laser_session_pick").selected_text(open.clone()).width(260.0).show_ui(ui, |ui| {
            let _ = ui.selectable_label(true, open);
            for (k, w) in app.laser_waiting.iter().enumerate() {
                if ui.selectable_label(false, w.name()).clicked() {
                    pick = Some(k);
                }
            }
        });
        let n = app.laser_waiting.len() + usize::from(!app.laser.is_empty());
        ui.label(RichText::new(format!("{n} in this project")).color(t.dim).size(12.0));
        add = ui
            .add_enabled(!app.laser.is_empty(), egui::Button::new("Add another session"))
            .on_hover_text("Keep this session in the project and start a new one (e.g. another day); open its data files next.")
            .clicked();
        remove = ui
            .add_enabled(!app.laser.is_empty() || !app.laser_waiting.is_empty(), egui::Button::new("Remove from project"))
            .on_hover_text("Take the open session out of the project. Results already sent to Combined results stay there.")
            .clicked();
        if !app.settings.laser_in_project && !app.laser_waiting.is_empty() {
            ui.label(RichText::new("Sessions are kept in the project file only when \"Keep the laser session in the project\" is on (Project menu).").color(t.warn).size(12.0));
        }
    });
    if let Some(k) = pick {
        switch_to(app, k);
    }
    if add {
        add_new(app);
    }
    if remove {
        remove_open(app);
    }
}
