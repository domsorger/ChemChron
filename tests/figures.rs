// SPDX-License-Identifier: GPL-3.0-or-later
//! Writes the main Combined-results figures of the demo project as PDF, SVG
//! and EPS (target/figure-check/), so their look can be checked as pictures.

use chemchron::app::ChemApp;
use chemchron::plots::{self, Bar, Colors};
use chemchron::vector::{self, Format};
use chemchron_core::row::*;
use std::path::Path;

fn app() -> ChemApp {
    let dir = std::env::temp_dir().join(format!("chemchron-fig-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var("CHEMCHRON_CONFIG_DIR", &dir);
    let ctx = egui::Context::default();
    let mut app = ChemApp::new(&ctx);
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/epma/examples/demo_project.chemchron.json");
    app.open_project(&p);
    app
}

#[test]
fn combined_figures_are_written_in_every_format() {
    let app = app();
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/figure-check");
    std::fs::create_dir_all(&out).unwrap();
    let gc = app.group_colors();
    let samples: Vec<&Row> = app.data.rows.iter().filter(|r| !truthy(r, "Is_Reference") && flag(r, "Include_In_Average", true)).collect();
    let colors = Colors::for_rows(&gc, &samples);
    // weighted mean of the first sample group
    let g = text(samples[0], "Flag");
    let grp: Vec<&Row> = samples.iter().copied().filter(|r| text(r, "Flag") == g).collect();
    let wm = chemchron_core::stats::weighted_mean_for_rows(&grp, true, 2.0).unwrap();
    let mut bars: Vec<Bar> = grp
        .iter()
        .enumerate()
        .filter_map(|(i, r)| chemchron_core::stats::row_age_sigma(r).map(|(a, s)| Bar { id: i, group: g.clone(), label: text(r, "Label"), age: a, s2: 2.0 * s, used: !wm.rejected_idx.contains(&i), color: colors.of(r) }))
        .collect();
    let figs = vec![
        ("weighted_mean", plots::weighted_mean(&mut bars, Some((wm.mean, wm.total_2s, wm.mswd, wm.p_value)), &format!("Sample Group: {g}"), "Age (Ma)", true, egui::Color32::RED)),
        ("ternary", plots::ternary(&samples, ["X_monazite", "X_cheralite", "X_huttonite"], ["Mnz", "Cher", "Ht"], "Mnz-Cher-Ht ternary", &colors, egui::Color32::from_rgb(0x44, 0x44, 0x44)).0),
        ("binary", plots::binary(&samples, "ThO2_wt%", "UO2_wt%", "ThO2 (wt%)", "UO2 (wt%)", "ThO2 (wt%) vs UO2 (wt%)", &colors, false).0),
        ("ree", {
            let norm = app.settings.norm.by_name(&app.settings.norm.active).clone();
            plots::ree_diagram(&samples, &norm, false, chemchron_core::ree::Source::ALL[0], true, false, "Sample Groups", &colors, &|r: &Row| text(r, "Flag"))
        }),
    ];
    for (name, f) in figs {
        assert!(!f.legend.is_empty(), "{name} has a legend");
        for fmt in Format::ALL {
            let bytes = vector::write(&f, fmt, 120.0, 90.0, 8.0, true);
            assert!(bytes.len() > 500, "{name}.{}", fmt.ext());
            std::fs::write(out.join(format!("{name}.{}", fmt.ext())), bytes).unwrap();
        }
    }
}

#[test]
fn the_icon_is_the_logo_on_a_rounded_square() {
    let px = chemchron::icon::rgba(64);
    assert_eq!(px.len(), 64 * 64 * 4);
    let at = |x: usize, y: usize| &px[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4];
    assert_eq!(at(0, 0)[3], 0, "corners are transparent");
    assert_eq!(at(32, 32)[3], 255, "the middle is opaque");
    let p = std::env::temp_dir().join(format!("cc_icon_{}.png", std::process::id()));
    chemchron::icon::write_png(&p, 128).unwrap();
    assert!(std::fs::metadata(&p).unwrap().len() > 500);
    let _ = std::fs::remove_file(p);
}
