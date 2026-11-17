// SPDX-License-Identifier: GPL-3.0-or-later
//! About window: what ChemChron is, how to work with it, uncertainty
//! calculations, mineral abbreviations, and "About ChemChron".

use crate::app::ChemApp;
use egui::{Context, RichText};

const TOPICS: [&str; 4] = ["Workflow", "Uncertainty calculations", "Mineral abbreviations", "About ChemChron"];

const COPYRIGHT: &str = "Copyright 2026 Dominik Sorger, Department of Earth Sciences, University of Graz";

const LICENSE_TEXT: &str = "This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version (GPL-3.0-or-later).

This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details: https://www.gnu.org/licenses/gpl-3.0.html";

const SOURCE_CODE: &str = "https://github.com/domsorger/ChemChron";

const DESCRIPTION: &str = "ChemChron is a program for the geochronology and geochemistry of accessory minerals such as monazite, xenotime and zircon. It brings three kinds of work together in one window and one project file:

• LA-ICP-MS data reduction: from the raw time-resolved signal of a laser-ablation ICP-MS session to U–Th–Pb ratios, ages and trace-element contents, with baseline, down-hole and drift corrections, common-Pb and 230Th corrections, internal standards and quality checks.

• EPMA chemical dating: electron-microprobe Th–U–Pb ages with their uncertainty model, mineral formulae and end-members, chemical isochrons and weighted means, checked against reference materials.

• Combined results: chemical ages, U–Pb ages and trace elements of the same analyses linked by label, with summaries, concordia, weighted means, REE and composition plots, long-term logs, exports and publication figures (PDF, SVG, EPS). Every part also works on its own.";

const WORKFLOW: &str = "ChemChron has three parts — LA-ICP-MS, EPMA and Combined results — switched with the buttons at the top, and Settings (top right) for everything lab-wide. One project file keeps all of it; the program starts with an empty project. Files can be dropped on the window: EPMA tables (CSV, Excel, ODS) or a project in EPMA and Combined results, data folders or files in LA-ICP-MS.

LA-ICP-MS. Open a data folder (Element FIN2, Agilent CSV, ESI laser logs) or try the demo session (simulated laser and EPMA data with a known truth). Check the selections (Signal & selections: Ctrl- or Shift-click marks several, right-click moves them to another group; each group has a colour and a mineral). Choose the data reduction scheme (U–Pb and trace elements, U–Pb only, trace elements only) and set the corrections: baseline and outlier sweeps, down-hole and primary reference, common Pb and 230Th, the internal standard (as oxide wt% or element ppm, per group or per spot, imported from a table or taken from the EPMA analysis with the same label). Cut-outs remove inclusions: drawn by hand for chosen elements (and for U–Pb on the Pb, U and Th signals), found automatically for trace elements, or both; a cut-out part can be split off as an analysis of its own. The live concordia follows the selected analysis while you drag its selection. Results, Concordia, Ages and ratios, Elements and Quality are quick checks; Export > Send to Combined results passes the results on.

EPMA. Import microprobe exports or ChemChron CSV tables (each file is one EPMA session), paste a table from the clipboard or add single points; assign sample groups and reference materials and look at chemical ages and mineral formulae in the Geochronology and Mineral composition tables.

Combined results. Everything linked by analysis label (case and leading zeros of the last number ignored): Data (Geochronology and Mineral composition), the Summary (ages, or group composition means), chemical isochron, weighted mean (chemical or U–Pb), concordia, REE diagram (each analysis, median with IQR or min–max), composition plots, chemical against U–Pb ages, the long-term logs and the exports. Every plot also works with only chemical ages, only U–Pb ages or only trace elements. Save figure… sets width, aspect ratio, font size and title and writes PDF, SVG or EPS.

Tables. Click a column group's name to fold it; double-click a label, comment or session to edit it (comments and sessions go to all selected rows); Show hides sample groups, reference materials, analyses left out, PbO below d.l. or discordant U–Pb; Sort … then … sorts on two columns; the buttons above the table and the right-click menu assign groups, references and minerals, switch between included and left out, and plot the selection (in table order).

Settings. Working directory, window colours, sample groups and their colours, the one reference-material list (code, alternative codes that identify it in labels, one reference age, laser values; reference-material files in JSON can be imported), decay constants (one set for every part), EPMA settings, the uncertainty models (chemical and U–Pb), U–Pb criteria and the laser quality limits. Reset window layout reopens folded sections and resets column widths.

Undo and Redo (Ctrl+Z, Ctrl+Shift+Z) take back the latest change wherever it was made: analyses, laser session or lab settings.";

const UNCERTAINTY: &str = "Chemical ages. Each analysis has an analytical 2σ from its Th, U and Pb counting errors (or 1 % Th/U, 2 % Pb when the import has none), propagated through the total-Pb age equation. Analytical + excess adds a random excess per analysis (from the reference materials of its EPMA session: the excess that brings their pooled MSWD to 1); the total adds the systematic term (from the long-term log once it holds 3 sessions, until then the manual value; optionally the decay constants). Weighted means and isochrons are weighted on analytical + excess, expanded by √MSWD when the chi-square test rejects the fit (P < 0.05, as IsoplotR), and get the systematic term once. Reference analyses are compared with their accepted age using the total.

Isochrons. PbO against ThO2* or UO2* (Suzuki & Dunkley 2014), York regression; x* and the age are iterated to convergence; the age uncertainty comes from the curvature of the full total-Pb misfit, independent of the Th or U form. Outlier rejection is leave-one-out, never below 3 points.

Isotopic ages. Every analysis has its analytical 2σ (the sweeps) and the session's excess (analytical + excess). The excess is set in Settings > Uncertainty models > U–Pb for the whole program: from the session's reference materials (the primary and the secondaries together, MSWD 1; reference spots are not tested for discordance, and the ²⁰⁷Pb/²⁰⁶Pb of secondaries younger than 1000 Ma is left out, as their Pb–Pb age is too imprecise), or manual values; single ratios of a material can be left out by hand. Means and concordia ages are weighted on analytical + excess, expanded by √MSWD when P < 0.05, and get the systematic term, which is part of every total: from the U–Pb long-term log (all logged reference materials, except the Pb–Pb ages of those younger than 1000 Ma, once it holds 3 sessions; until then the manual value), the accepted-value uncertainty of the primary reference material, and optionally the decay constants. The LA-ICP-MS part shows the same totals. Each laser spot counts once even when copied to several EPMA analyses of a grain. Discordant (|disc.| above the threshold whose 95 % ellipse misses the concordia, or above the hard limit) and imprecise spots are left out of means. Concordia ages follow Ludwig (1998), optionally with decay-constant errors; discordia lines York (2004). Decay constants: λ238U and λ235U Jaffey et al. (1971), λ232Th Le Roux & Glendenin (1963), 238U/235U = 137.818 (Hiess et al. 2012).";

const MINERALS: &str = "Mnz monazite · Xtm xenotime · Thr thorite · Urn uraninite · Tho thorianite · Zrn zircon (Warr 2021, IMA–CNMNC symbols). End-members: Mnz monazite, Cher cheralite, Ht huttonite.";

pub fn window(app: &mut ChemApp, ctx: &Context) {
    let mut open = app.help_open;
    egui::Window::new("About ChemChron").open(&mut open).default_size([680.0, 560.0]).show(ctx, |ui| {
        ui.horizontal(|ui| {
            for (i, t) in TOPICS.iter().enumerate() {
                ui.selectable_value(&mut app.help_topic, i, *t);
            }
        });
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| match app.help_topic {
            0 => {
                ui.label(WORKFLOW);
            }
            1 => {
                ui.label(UNCERTAINTY);
            }
            2 => {
                ui.label(MINERALS);
            }
            _ => {
                ui.horizontal(|ui| {
                    let tex = ui.ctx().load_texture("about_icon", egui::ColorImage::from_rgba_unmultiplied([96, 96], &crate::icon::rgba(96)), Default::default());
                    ui.image((tex.id(), egui::vec2(64.0, 64.0)));
                    ui.vertical(|ui| {
                        ui.label(RichText::new(format!("ChemChron {}", env!("CARGO_PKG_VERSION"))).strong().size(20.0));
                        ui.label(COPYRIGHT);
                    });
                });
                ui.add_space(8.0);
                ui.label(DESCRIPTION);
                ui.add_space(8.0);
                ui.label(RichText::new(LICENSE_TEXT).size(12.0));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Source code:").size(12.0));
                    ui.hyperlink(SOURCE_CODE);
                });
                ui.add_space(8.0);
                ui.label(RichText::new(format!("Settings folder: {}", app.settings.dir.display())).size(12.0));
            }
        });
    });
    app.help_open = open;
}
