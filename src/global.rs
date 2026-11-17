// SPDX-License-Identifier: GPL-3.0-or-later
//! Settings for the whole program (top bar > Settings), all in one place:
//! working directory and its files, window colours and figures, the one
//! reference-material list, decay constants, EPMA settings, the uncertainty
//! models (chemical and U–Pb), U–Pb criteria and the laser defaults.
//! Choices that belong to one laser session's data (primary reference,
//! baseline, down-hole model, common Pb) stay in the Laser steps.

use crate::app::{frame_part, nav, ChemApp};
use crate::settings_pages::Which;
use egui::{RichText, Ui};
use chemchron_laser::theme::{section, segmented};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Page {
    #[default]
    General,
    References,
    Decay,
    Epma,
    Uncertainty,
    Upb,
    Laser,
}

impl Page {
    pub const ALL: [Page; 7] = [Page::General, Page::References, Page::Decay, Page::Epma, Page::Uncertainty, Page::Upb, Page::Laser];

    pub fn title(&self) -> &'static str {
        match self {
            Page::General => "General",
            Page::References => "Reference materials",
            Page::Decay => "Decay constants",
            Page::Epma => "EPMA",
            Page::Uncertainty => "Uncertainty models",
            Page::Upb => "U–Pb & Combine",
            Page::Laser => "Laser",
        }
    }

    pub fn from_name(n: &str) -> Option<Page> {
        let n = n.to_lowercase();
        Some(match n.as_str() {
            "general" | "settings" | "workdir" | "directory" => Page::General,
            "references" | "reference" | "refs" => Page::References,
            "decay" => Page::Decay,
            "epma" => Page::Epma,
            "uncertainty" => Page::Uncertainty,
            "upb" | "combine" => Page::Upb,
            "laser" => Page::Laser,
            _ => return None,
        })
    }
}

#[derive(Default)]
pub struct GlobalState {
    pub page: Page,
    /// Uncertainty models: false = chemical ages, true = U–Pb.
    pub upb_uncertainty: bool,
}

pub fn show(app: &mut ChemApp, ui: &mut Ui) {
    frame_part(app, ui, "global", nav_ui, |app, ui| match app.global.page {
        Page::General => page_general(app, ui),
        Page::References => crate::refs_page::page_references(app, ui),
        Page::Decay => crate::settings_pages::page_settings(app, ui, Which::Decay),
        Page::Epma => crate::settings_pages::page_settings(app, ui, Which::Epma),
        Page::Uncertainty => page_uncertainty(app, ui),
        Page::Upb => crate::longterm::page_upb_settings(app, ui, false),
        Page::Laser => page_laser(app, ui),
    });
}

fn nav_ui(app: &mut ChemApp, ui: &mut Ui) {
    let t = app.theme();
    let pages: Vec<(Page, &str, Option<egui::Color32>, bool)> = Page::ALL.iter().map(|p| (*p, p.title(), None, true)).collect();
    nav(ui, &t, &mut app.global.page, &pages);
    ui.add_space(12.0);
    ui.label(RichText::new("Lab settings for every part and every project, kept in the working directory. Choices for one laser session's data (primary reference, baseline, down-hole model, common Pb) stay in the Laser steps.").color(t.dim).size(11.5));
}

fn page_general(app: &mut ChemApp, ui: &mut Ui) {
    let t = app.theme();
    ui.horizontal(|ui| {
        ui.label(RichText::new("Window colours").color(t.dim));
        let mut dark = app.dark;
        if segmented(ui, &t, &mut dark, &[(false, "Light"), (true, "Dark")]) {
            app.dark = dark;
        }
        ui.separator();
        if ui
            .button("Reset window layout")
            .on_hover_text("Opens every folded section again, puts the columns back to their usual widths and closes extra windows")
            .clicked()
        {
            reset_layout(app, ui.ctx());
        }
    });
    ui.add_space(4.0);
    crate::settings_pages::page_settings(app, ui, Which::General);
}

fn page_uncertainty(app: &mut ChemApp, ui: &mut Ui) {
    let t = app.theme();
    ui.horizontal(|ui| {
        let mut v = app.global.upb_uncertainty;
        if segmented(ui, &t, &mut v, &[(false, "Chemical ages (EPMA)"), (true, "U–Pb (laser)")]) {
            app.global.upb_uncertainty = v;
        }
    });
    ui.add_space(4.0);
    let upb = app.global.upb_uncertainty;
    // Upper part: the model (scrolls on its own); lower part: the log entries of
    // this kind in a fixed share of the page, so neither grows over the other.
    let total = ui.available_height();
    let entries_h = (total * 0.42).clamp(220.0, 460.0);
    let top_h = (total - entries_h - 10.0).max(120.0);
    let w = ui.available_width();
    ui.allocate_ui(egui::vec2(w, top_h), |ui| {
        ui.set_min_height(top_h);
        ui.set_max_height(top_h);
        ui.set_clip_rect(ui.max_rect().intersect(ui.clip_rect()));
        if upb {
            crate::longterm::page_upb_settings(app, ui, true);
        } else {
            crate::settings_pages::page_uncertainty(app, ui);
        }
    });
    ui.add_space(4.0);
    ui.separator();
    ui.allocate_ui(egui::vec2(w, ui.available_height()), |ui| {
        ui.set_clip_rect(ui.max_rect().intersect(ui.clip_rect()));
        crate::longterm::edit_entries(app, ui, upb);
    });
}

fn page_laser(app: &mut ChemApp, ui: &mut Ui) {
    let t = app.theme();
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.heading("Laser");
        section(ui, &t, "Lab-wide laser settings", |ui| {
            ui.label(RichText::new("The laser part uses the lab's reference materials (Settings > Reference materials) and decay constants (Settings > Decay constants). Its uncertainty model for U–Pb is under Settings > Uncertainty models.").color(t.dim).size(12.0));
            ui.add_space(4.0);
            app.laser.lab_settings_ui(ui);
        });
        section(ui, &t, "Per session (in the Laser steps)", |ui| {
            ui.label(RichText::new("Reduction scheme, primary reference material, baseline, down-hole model, common-Pb correction and the trace-element internal standard belong to each session's data and are chosen on LA-ICP-MS > Corrections. They are saved with the laser session.").color(t.dim).size(12.0));
            if ui.button("Go to LA-ICP-MS > Corrections").clicked() {
                app.part = crate::app::Part::Laser;
                app.laser.page = chemchron_laser::app::Page::Corrections;
            }
        });
    });
}

/// Folded sections, column widths and window positions back to the start.
pub fn reset_layout(app: &mut ChemApp, ctx: &egui::Context) {
    chemchron_laser::theme::reset_layout(ctx);
    app.laser.corrections.spots_window = false;
    app.say("Window layout reset.", false);
}
