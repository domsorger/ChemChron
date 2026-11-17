// SPDX-License-Identifier: GPL-3.0-or-later
//! The demo: the simulated laser session and, for the same spots, simulated
//! EPMA analyses (same labels, compositions from the same "true" materials,
//! PbO from the true age), linked in Combined results. Every part of
//! ChemChron can be tried with it.

use crate::app::{ChemApp, Part};
use chemchron_core::constants::{oxide_element_fraction, DecayConstants};
use chemchron_core::row::*;
use chemchron_laser_core::synthetic::{true_ppm, truths};

/// Small reproducible scatter (± `rel`, relative).
fn wiggle(k: usize, salt: u64, rel: f64) -> f64 {
    let mut x = (k as u64).wrapping_mul(6364136223846793005).wrapping_add(salt.wrapping_mul(1442695040888963407));
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51afd7ed558ccd);
    x ^= x >> 33;
    let u = (x % 10_000) as f64 / 10_000.0; // 0..1
    1.0 + rel * (2.0 * u - 1.0)
}

/// EPMA table (CSV text) for the sample and reference spots of the demo laser session.
pub fn demo_epma_csv(labels: &[(String, String)]) -> String {
    let d = DecayConstants::default();
    let oxides: [(&str, &str); 14] = [
        ("P2O5", "P"), ("SiO2", ""), ("CaO", "Ca"), ("Y2O3", "Y"), ("La2O3", "La"), ("Ce2O3", "Ce"), ("Pr2O3", "Pr"),
        ("Nd2O3", "Nd"), ("Sm2O3", "Sm"), ("Gd2O3", "Gd"), ("Dy2O3", "Dy"), ("ThO2", ""), ("UO2", ""), ("PbO", ""),
    ];
    let mut out = String::from("Label");
    for (o, _) in &oxides {
        out.push_str(&format!(",{o}"));
    }
    out.push('\n');
    for (k, (label, material)) in labels.iter().enumerate() {
        let Some(t) = truths().into_iter().find(|t| t.name == material) else { continue };
        let th = t.th * wiggle(k, 1, 0.08);
        let u = t.u * wiggle(k, 2, 0.08);
        let pb = chemchron_core::age::total_radiogenic_pb_ppm(th, u, t.age_ma * 1e6, &d) * wiggle(k, 3, 0.012);
        out.push_str(label);
        for (o, el) in &oxides {
            let ppm = match *o {
                "ThO2" => th,
                "UO2" => u,
                "PbO" => pb,
                "SiO2" => 1500.0 * wiggle(k, 4, 0.3),
                _ => true_ppm(material, el).unwrap_or(0.0) * wiggle(k, 5 + el.len() as u64, 0.04),
            };
            let wt = ppm / 1e4 / oxide_element_fraction(o);
            out.push_str(&format!(",{wt:.4}"));
        }
        out.push('\n');
    }
    out
}

/// Opens the demo in every part: two laser sessions as most labs run them (one
/// for U–Pb, one for trace elements, on the same grains), EPMA analyses of the
/// same spots, and the results of both sessions linked in Combined results.
pub fn load(app: &mut ChemApp) {
    use chemchron_laser_core::model::DemoPart;
    app.new_project();
    // the analyses of both sessions (the same sample spots; references of each)
    let mut labels: Vec<(String, String)> = Vec::new();
    for part in [DemoPart::UPb, DemoPart::TraceElements] {
        let s = chemchron_laser_core::synthetic::demo_session_part(1, part);
        for sel in &s.selections {
            let Some(g) = s.group_of(sel) else { continue };
            if truths().iter().any(|t| t.name == g.name) && !labels.iter().any(|(l, _)| *l == sel.label) {
                labels.push((sel.label.clone(), g.name.clone()));
            }
        }
    }
    let path = std::env::temp_dir().join(format!("chemchron_demo_epma_{}.csv", std::process::id()));
    if std::fs::write(&path, demo_epma_csv(&labels)).is_err() {
        app.say("The demo EPMA data could not be written to the temporary folder.", true);
        return;
    }
    crate::epma::import_files(app, vec![path.clone()], false);
    let _ = std::fs::remove_file(&path);
    // sample groups as in the laser sessions
    for r in app.data.rows.iter_mut() {
        if truthy(r, "Is_Reference") {
            continue;
        }
        let l = text(r, "Label");
        if let Some((_, g)) = labels.iter().find(|(x, _)| *x == l) {
            set(r, "Flag", jstr(g));
        }
    }
    app.data.refresh(&app.settings);
    // trace elements first: sent to Combined results, then kept in the project
    app.laser.load_demo_part(DemoPart::TraceElements);
    crate::combine::link_laser_now(app);
    crate::laser_sessions::add_new(app);
    // the U–Pb session stays open
    app.laser.load_demo_part(DemoPart::UPb);
    crate::combine::link_laser_now(app);
    app.data.clear_history();
    app.part = Part::Laser;
    app.say("Demo: a simulated U–Pb session and a trace-element session (switch above), and simulated EPMA analyses of the same spots, linked in Combined results.", false);
}
