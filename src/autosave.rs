// SPDX-License-Identifier: GPL-3.0-or-later
//! Autosave and recovery. While a project has unsaved changes, a copy (with
//! the laser session) is written every few minutes into the settings folder
//! (`autosave/`). Saving, or closing with "Don't save", removes it; a copy
//! that is still there at the next start is offered for recovery.

use crate::app::ChemApp;
use egui::{Context, RichText};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const EVERY: Duration = Duration::from_secs(180);

#[derive(Default)]
pub struct AutoState {
    pub last: Option<Instant>,
    pub checked: bool,
    /// Our own copy (one per running program).
    pub file: Option<PathBuf>,
    /// A copy left behind by an earlier run: (file, saved at, project name).
    pub offer: Option<(PathBuf, String, String)>,
    pub off: bool,
}

pub fn dir(app: &ChemApp) -> PathBuf {
    app.settings.dir.join("autosave")
}

fn own_file(app: &ChemApp) -> PathBuf {
    dir(app).join(format!("recovery-{}.chemchron.json", std::process::id()))
}

/// Copies left by earlier runs, newest first: (file, saved at, project).
pub fn leftovers(d: &Path, skip: Option<&Path>) -> Vec<(PathBuf, String, String)> {
    let mut v: Vec<(PathBuf, String, String)> = std::fs::read_dir(d)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("recovery-")) && Some(p.as_path()) != skip)
        .filter_map(|p| {
            let t = std::fs::read_to_string(&p).ok()?;
            let v: Value = serde_json::from_str(&t).ok()?;
            let a = v.get("autosave")?;
            let when = a.get("time").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let name = a.get("project").and_then(|x| x.as_str()).unwrap_or("Untitled project").to_string();
            Some((p, when, name))
        })
        .collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v
}

/// The value written as the backup copy.
pub fn backup_value(app: &ChemApp) -> Value {
    let mut v = app.project_value();
    if v.get("laser").is_none_or(|l| l.is_null()) && !app.laser.is_demo() && !app.laser.is_empty() {
        v["laser"] = serde_json::to_value(&app.laser.session).unwrap_or(Value::Null);
    }
    if !app.settings.laser_in_project {
        v["laser_sessions"] = Value::Array(app.laser_waiting.iter().filter_map(|w| serde_json::to_value(&w.session).ok()).collect());
    }
    v["autosave"] = json!({
        "time": crate::longterm::timestamp(),
        "project": app.project_name(),
        "path": app.project_path.as_ref().map(|p| p.to_string_lossy().to_string()),
    });
    v
}

/// Writes the backup copy now.
pub fn write_now(app: &mut ChemApp) -> Result<PathBuf, String> {
    let p = own_file(app);
    std::fs::create_dir_all(dir(app)).map_err(|e| e.to_string())?;
    let text = serde_json::to_string(&backup_value(app)).map_err(|e| e.to_string())?;
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &p).map_err(|e| e.to_string())?;
    app.autosave.file = Some(p.clone());
    Ok(p)
}

/// Removes our backup copy (after saving, or when the changes are thrown away).
pub fn clear(app: &mut ChemApp) {
    if let Some(p) = app.autosave.file.take() {
        let _ = std::fs::remove_file(p);
    }
}

/// Called every frame.
pub fn tick(app: &mut ChemApp, ctx: &Context) {
    if app.autosave.off {
        return;
    }
    if !app.autosave.checked {
        app.autosave.checked = true;
        app.autosave.offer = leftovers(&dir(app), None).into_iter().next();
    }
    if app.unsaved() {
        let due = app.autosave.last.is_none_or(|t| t.elapsed() >= EVERY);
        if app.autosave.last.is_none() {
            // the first copy a few minutes after the first change
            app.autosave.last = Some(Instant::now());
        } else if due {
            app.autosave.last = Some(Instant::now());
            if let Err(e) = write_now(app) {
                eprintln!("autosave: {e}");
            }
        }
        ctx.request_repaint_after(EVERY);
    } else {
        app.autosave.last = None;
        clear(app);
    }
    offer_window(app, ctx);
}

/// Opens a backup copy as the current project (marked unsaved, under its original file name).
pub fn recover(app: &mut ChemApp, file: &Path) {
    let original = std::fs::read_to_string(file)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.get("autosave").and_then(|a| a.get("path")).and_then(|p| p.as_str()).map(PathBuf::from));
    app.open_project(file);
    let f = file.to_string_lossy().to_string();
    app.settings.recent_projects.retain(|p| *p != f);
    app.project_path = original;
    app.data.unsaved = true;
    let _ = std::fs::remove_file(file);
    app.say("Recovered the unsaved work. Save it to keep it.", false);
}

fn offer_window(app: &mut ChemApp, ctx: &Context) {
    let Some((file, when, name)) = app.autosave.offer.clone() else { return };
    let t = app.theme();
    let mut done = false;
    egui::Modal::new(egui::Id::unique("cc_recover")).show(ctx, |ui| {
        ui.set_width(400.0);
        ui.label(RichText::new("Recover unsaved work?").strong());
        ui.label(RichText::new(format!("ChemChron was closed without saving last time. A backup copy of “{name}” from {when} is there.")).color(t.dim));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button(RichText::new("Recover").strong()).clicked() {
                recover(app, &file);
                done = true;
            }
            if ui.button("Throw it away").clicked() {
                let _ = std::fs::remove_file(&file);
                done = true;
            }
            if ui.button("Later").on_hover_text("Keep the copy and ask again at the next start").clicked() {
                done = true;
            }
        });
    });
    if done {
        app.autosave.offer = None;
    }
}
