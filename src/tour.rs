// SPDX-License-Identifier: GPL-3.0-or-later
//! `--screenshot-tour <folder>`: opens every part and page in turn, dark and
//! light, saves a picture of each into the folder and closes. Used to check
//! the window on another computer (see the take_screenshots scripts).

use crate::app::{ChemApp, Part};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy)]
pub enum Shot {
    Laser(chemchron_laser::app::Page),
    Epma(crate::epma::Page, bool),
    Combine(crate::combine::Page),
    Settings(crate::global::Page),
}

pub struct Tour {
    pub dir: PathBuf,
    pub steps: Vec<(String, Shot, bool)>,
    pub at: usize,
    pub frames: u32,
    pub pending: Option<Arc<Mutex<Option<Result<(), String>>>>>,
    pub keep_dark: bool,
    pub written: Vec<String>,
}

fn slug(s: &str) -> String {
    let v: String = s.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
    v.split('_').filter(|x| !x.is_empty()).collect::<Vec<_>>().join("_")
}

pub fn plan() -> Vec<(String, Shot)> {
    use crate::combine::Page as C;
    use crate::epma::Page as E;
    let mut v: Vec<(String, Shot)> = Vec::new();
    for p in chemchron_laser::app::Page::ALL {
        if p != chemchron_laser::app::Page::References {
            v.push((format!("laser_{}", slug(p.title())), Shot::Laser(p)));
        }
    }
    for (p, n) in [(E::Data, "data"), (E::Geochronology, "geochronology"), (E::Composition, "composition")] {
        v.push((format!("epma_{n}"), Shot::Epma(p, false)));
        if p != E::Data {
            v.push((format!("epma_{n}_references"), Shot::Epma(p, true)));
        }
    }
    for (p, n) in [
        (C::Data, "data"),
        (C::Summary, "summary"),
        (C::Isochron, "isochron"),
        (C::WeightedMean, "weighted_mean"),
        (C::Concordia, "concordia"),
        (C::Ree, "ree"),
        (C::Composition, "composition"),
        (C::Compare, "chemical_vs_upb"),
        (C::AgeDist, "age_distribution"),
        (C::LongTerm, "long_term"),
        (C::Export, "export"),
    ] {
        v.push((format!("combined_{n}"), Shot::Combine(p)));
    }
    for p in crate::global::Page::ALL {
        v.push((format!("settings_{}", slug(&format!("{p:?}"))), Shot::Settings(p)));
    }
    v
}

pub fn start(app: &mut ChemApp, dir: PathBuf) {
    let mut steps = Vec::new();
    for dark in [true, false] {
        for (k, (n, s)) in plan().into_iter().enumerate() {
            steps.push((format!("{}{:02}_{n}_{}", if dark { "a" } else { "b" }, k + 1, if dark { "dark" } else { "light" }), s, dark));
        }
    }
    app.tour = Some(Tour { dir, steps, at: 0, frames: 0, pending: None, keep_dark: app.settings.dark, written: Vec::new() });
}

pub fn apply(app: &mut ChemApp, s: Shot, dark: bool) {
    app.dark = dark;
    match s {
        Shot::Laser(p) => {
            app.part = Part::Laser;
            app.laser.page = p;
        }
        Shot::Epma(p, refs) => {
            app.part = Part::Epma;
            app.epma.page = p;
            app.epma.show_refs = refs;
        }
        Shot::Combine(p) => {
            app.part = Part::Combine;
            app.combine.page = p;
            app.combine.wm_upb = false;
        }
        Shot::Settings(p) => {
            app.part = Part::Settings;
            app.global.page = p;
        }
    }
}

/// Called every frame while the tour runs; true when it is over.
pub fn drive(app: &mut ChemApp, ctx: &egui::Context) -> bool {
    let Some(mut tour) = app.tour.take() else { return false };
    ctx.request_repaint();
    if tour.at >= tour.steps.len() {
        let index: String = tour.written.iter().map(|n| format!("{n}\n")).collect();
        let _ = std::fs::write(tour.dir.join("list.txt"), index);
        app.dark = tour.keep_dark;
        app.settings.dark = tour.keep_dark;
        app.save_settings_quiet();
        println!("Saved {} pictures in {}", tour.written.len(), tour.dir.display());
        return true;
    }
    let (name, shot, dark) = tour.steps[tour.at].clone();
    if tour.frames == 0 {
        apply(app, shot, dark);
    }
    tour.frames += 1;
    if tour.frames == 30 && tour.pending.is_none() {
        let p = tour.dir.join(format!("{name}.png"));
        let done: Arc<Mutex<Option<Result<(), String>>>> = Arc::new(Mutex::new(None));
        let d = done.clone();
        let cb = egui::ScreenshotCallback::new(move |image: Arc<egui::ColorImage>| {
            *d.lock().unwrap() = Some(crate::app::save_png(&p, &image));
        });
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(cb));
        tour.pending = Some(done);
    }
    let finished = tour.pending.as_ref().and_then(|d| d.lock().unwrap().take());
    if let Some(r) = finished {
        match r {
            Ok(()) => tour.written.push(format!("{name}.png")),
            Err(e) => eprintln!("{name}: {e}"),
        }
        tour.pending = None;
        tour.frames = 0;
        tour.at += 1;
    } else if tour.frames > 600 {
        eprintln!("{name}: no picture, skipped");
        tour.pending = None;
        tour.frames = 0;
        tour.at += 1;
    }
    app.tour = Some(tour);
    false
}
