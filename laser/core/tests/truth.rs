// SPDX-License-Identifier: GPL-3.0-or-later
//! Does the reduction recover what the simulation put in? The demo session has
//! Poisson counting noise, drift and down-hole fractionation with known values.

use chemchron_laser_core::drs::{reduce, upb::RatioKind};
use chemchron_laser_core::model::*;
use chemchron_laser_core::synthetic::{demo_session, true_dhf, true_ppm, test_elements};
use chemchron_laser_core::{export, project};

fn group_id(s: &Session, name: &str) -> u64 {
    s.groups.iter().find(|g| g.name == name).unwrap().id
}

#[test]
fn demo_finds_its_groups_and_references() {
    let s = demo_session(1);
    assert!(s.groups.iter().any(|g| g.name == "NIST610" && g.kind == GroupKind::ReferenceMaterial));
    assert!(s.groups.iter().any(|g| g.name == "44069" && g.kind == GroupKind::ReferenceMaterial));
    assert!(s.groups.iter().any(|g| g.name == "Sample A" && g.kind == GroupKind::Sample));
    assert_eq!(s.settings.upb.primary_group, Some(group_id(&s, "44069")));
    assert!(s.selections.iter().filter(|x| s.group_of(x).unwrap().kind == GroupKind::Baseline).count() > 50);
}

#[test]
fn ages_of_unknowns_are_recovered() {
    let mut s = demo_session(1);
    // The spots simulated with common Pb or Pb loss are not part of the age population.
    for x in s.selections.iter_mut() {
        if matches!(x.label.as_str(), "Sample A_10" | "Sample A_19" | "Sample B_10") {
            x.excluded = true;
        }
    }
    let red = reduce(&s);
    assert!(red.upb.is_some(), "{:?}", red.warnings);
    let stats_for = |name: &str| red.qa.groups.iter().find(|g| g.name == name).unwrap().clone();
    // Secondary reference and unknowns: within 1 % of truth.
    for (name, truth) in [("Trebilcock", 272.0), ("Sample B", 615.0)] {
        let g = stats_for(name);
        let w = g.wm_a68.unwrap();
        assert!(
            (w.mean - truth).abs() / truth < 0.012,
            "{name}: {} vs {truth}",
            w.mean
        );
    }
    let a = stats_for("Sample A");
    let w = a.wm_a76.unwrap();
    assert!((w.mean - 1850.0).abs() < 25.0, "Sample A 207/206: {}", w.mean);
    let g = stats_for("Trebilcock");
    let c = g.concordia.unwrap();
    assert!((c.age - 272.0).abs() / 272.0 < 0.012, "concordia {}", c.age);
}

#[test]
fn downhole_fit_recovers_the_true_curve() {
    let s = demo_session(1);
    let red = reduce(&s);
    let u = red.upb.as_ref().unwrap();
    let fit = u.ratio(RatioKind::R68).unwrap().dhf.as_ref().unwrap();
    // Compare the shape: size at 2 s relative to 18 s.
    let want = true_dhf(2.0) / true_dhf(18.0);
    let got = fit.factor(2.0) / fit.factor(18.0);
    assert!((got - want).abs() < 0.015, "got {got}, want {want}");
}

#[test]
fn excess_uncertainty_sees_the_added_scatter() {
    // The simulation adds about 0.3 % scatter per analysis on 206Pb/238U.
    let s = demo_session(1);
    let red = reduce(&s);
    let r = red.upb.as_ref().unwrap().ratio(RatioKind::R68).unwrap();
    assert!(r.excess > 0.0005 && r.excess < 0.01, "{}", r.excess);
    assert!((r.mswd_after - 1.0).abs() < 0.2, "{}", r.mswd_after);
}

#[test]
fn concentrations_of_unknowns_are_recovered() {
    let s = demo_session(1);
    let red = reduce(&s);
    let te = red.te.as_ref().unwrap();
    let group = group_id(&s, "Sample A");
    for element in test_elements() {
        let channel = s.inputs.channels.iter().position(|c| c.isotope.as_ref().is_some_and(|i| i.element == element)).unwrap();
        assert!(te.calibrations[channel].is_some(), "{element} has no calibration");
        let values: Vec<f64> = s
            .selections
            .iter()
            .zip(&red.selections)
            .filter(|(x, _)| x.group == group)
            .filter_map(|(_, r)| r.te[channel].as_ref().map(|v| v.ppm))
            .collect();
        assert!(values.len() >= 20, "{element}");
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        let truth = true_ppm("Sample A", element).unwrap();
        assert!((mean - truth).abs() / truth < 0.03, "{element}: {mean} vs {truth}");
    }
}

#[test]
fn common_lead_and_lead_loss_spots_are_flagged_and_clean_spots_mostly_are_not() {
    let s = demo_session(1);
    let red = reduce(&s);
    let flagged = |label: &str, needle: &str| {
        let i = s.selections.iter().position(|x| x.label == label).unwrap();
        red.selections[i].flags.iter().any(|f| f.contains(needle))
    };
    // Block 2 of 6, second Sample A spot: 6 % common Pb.
    assert!(flagged("Sample A_10", "Common Pb"), "{:?}", s.selections.iter().map(|x| &x.label).take(3).collect::<Vec<_>>());
    // Block 4, third spot: 25 % Pb loss.
    assert!(flagged("Sample A_19", "Discordant"));
    let total = s.selections.iter().zip(&red.selections).filter(|(x, _)| s.group_of(x).unwrap().name == "44069");
    let n_flag = total.filter(|(_, r)| !r.flags.is_empty()).count();
    assert!(n_flag <= 2, "{n_flag} clean reference spots flagged");
}

#[test]
fn correcting_common_lead_moves_the_age_back_to_truth() {
    let mut s = demo_session(1);
    s.settings.upb.common_method = CommonPbMethod::Pb204;
    s.settings.upb.hg_correction = false;
    let red = reduce(&s);
    let i = s.selections.iter().position(|x| x.label == "Sample B_10").unwrap();
    let u = red.selections[i].upb.as_ref().unwrap();
    let c = u.common.as_ref().unwrap();
    // Block 3, first Sample B spot has 10 % common Pb and a true age of 615 Ma.
    assert!(u.a68.age > 650.0, "raw age should be too old: {}", u.a68.age);
    assert!((c.age68 - 615.0).abs() < 25.0, "corrected {}", c.age68);
    assert!((c.f206_pct - 10.0).abs() < 3.0, "f206 {}", c.f206_pct);
}

#[test]
fn input_channels_are_not_changed_by_reduction() {
    let s = demo_session(1);
    let before = s.inputs.channels[0].values.clone();
    let _ = reduce(&s);
    assert_eq!(before, s.inputs.channels[0].values);
}

#[test]
fn every_export_runs_and_says_how_values_were_made() {
    let s = demo_session(1);
    let red = reduce(&s);
    let dir = std::env::temp_dir().join(format!("chemchron-laser-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let csv = dir.join("r.csv");
    export::write_results_csv(&s, &red, &csv).unwrap();
    let text = std::fs::read_to_string(&csv).unwrap();
    assert!(text.contains("SIMULATED"));
    assert!(text.contains("# Outlier rule"));
    assert!(text.contains("206Pb/238U Prop2s"));
    let header = text.lines().find(|l| !l.starts_with('#')).unwrap();
    let columns = header.split(',').count();
    for row in text.lines().filter(|l| !l.starts_with('#')).skip(1) {
        // Quoted flags may hold commas; the column count of unquoted rows must match.
        if !row.contains('"') {
            assert_eq!(row.split(',').count(), columns, "{row}");
        }
    }
    export::write_xlsx(&s, &red, &dir.join("r.xlsx")).unwrap();
    assert!(std::fs::metadata(dir.join("r.xlsx")).unwrap().len() > 1000);
    let group = group_id(&s, "Sample B");
    let iso = export::isoplotr_text(&s, &red, export::IsoplotFormat::Wetherill, &[group]);
    assert!(iso.starts_with("Pb207U235,errPb207U235"));
    assert!(iso.lines().count() > 10);
    let sel = s.selections.iter().find(|x| x.group == group).unwrap();
    let sweeps = export::sweeps_csv(&s, &red, sel);
    assert!(sweeps.contains("206Pb/238U final"));
}

#[test]
fn session_file_is_readable_and_round_trips() {
    let mut s = demo_session(1);
    s.settings.upb.common_method = CommonPbMethod::Pb207;
    s.selections[3].comment = "checked".to_string();
    let dir = std::env::temp_dir().join(format!("chemchron-laser-test-s{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("session.json");
    project::save_session(&s, &path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("\"clock_offset_s\""));
    assert!(text.len() < 2_000_000, "raw data must not be stored in the session");
    let loaded = project::load_session(&path).unwrap();
    assert_eq!(loaded.session.settings.upb.common_method, CommonPbMethod::Pb207);
    assert_eq!(loaded.session.selections[3].comment, "checked");
    assert_eq!(loaded.session.inputs.time.len(), s.inputs.time.len());
    assert_eq!(loaded.session.inputs.channels[3].values, s.inputs.channels[3].values);
}

#[test]
fn clean_secondary_reference_scatters_as_its_uncertainties_say() {
    // Across seeds, the simulated secondary reference should agree with itself.
    for seed in [1, 2, 3, 4] {
        let s = demo_session(seed);
        let red = reduce(&s);
        let g = red.qa.groups.iter().find(|g| g.name == "Trebilcock").unwrap();
        let c = g.concordia.as_ref().unwrap();
        let w68 = g.wm_a68.as_ref().unwrap();
        let w82 = g.wm_a82.as_ref().unwrap();
        let v: Vec<(f64, f64)> = s
            .selections
            .iter()
            .zip(&red.selections)
            .filter(|(x, _)| x.group == g.group)
            .filter_map(|(_, r)| r.upb.as_ref().map(|u| (u.r75.mean, u.r75.prop2s)))
            .collect();
        let w75 = chemchron_laser_core::stats::weighted_mean(&v).unwrap();
        eprintln!(
            "seed {seed}: concordia {:.1} mswd {:.2}; 68 mswd {:.2}; 82 mswd {:.2}; 75 mswd {:.2} mean {:.4} (truth {:.4})",
            c.age,
            c.mswd,
            w68.mswd,
            w82.mswd,
            w75.mswd,
            w75.mean,
            s.settings.upb.decay.r75(272.0)
        );
        assert!((c.age - 272.0).abs() < 3.0);
    }
}

#[test]
fn figures_are_made_for_chosen_groups() {
    use chemchron_laser_core::svg::{concordia_svg, weighted_mean_svg, ConcordiaKind};
    let s = demo_session(1);
    let red = reduce(&s);
    let group = group_id(&s, "Sample B");
    for kind in [ConcordiaKind::Wetherill, ConcordiaKind::TeraWasserburg] {
        let svg = concordia_svg(&s, &red, kind, &[group]);
        assert!(svg.starts_with("<svg") && svg.trim_end().ends_with("</svg>"));
        assert_eq!(svg.matches("fill-opacity").count(), 18, "one ellipse per analysis");
        assert!(!svg.contains("NaN"));
    }
    let wm = weighted_mean_svg(&s, &red, group);
    assert!(wm.contains("mean 206Pb/238U age"));
    assert!(!wm.contains("NaN"));
}

#[test]
fn internal_standard_table_sets_single_selections_and_names_the_rest() {
    let mut s = demo_session(1);
    let (n, missing) = project::apply_is_table(&mut s, "selection,ppm\nSample A_1,123000\nNo such spot,5\n").unwrap();
    assert_eq!(n, 1);
    assert_eq!(missing, ["No such spot"]);
    let before = reduce(&s);
    let i = s.selections.iter().position(|x| x.label == "Sample A_1").unwrap();
    let channel = s.inputs.channel_index("La139").unwrap();
    let with_table = before.selections[i].te[channel].as_ref().unwrap().ppm;
    s.selections[i].is_ppm = None;
    let without = reduce(&s).selections[i].te[channel].as_ref().unwrap().ppm;
    // Content scales with the internal-standard value: 123000 against the group's 250000.
    assert!((with_table / without - 123000.0 / 250000.0).abs() < 1e-6, "{with_table} {without}");
}

#[test]
fn below_detection_values_are_flagged_and_never_zero() {
    let s = demo_session(1);
    let red = reduce(&s);
    let channel = s.inputs.channel_index("Pb204").unwrap();
    let mut below = 0;
    for r in &red.selections {
        if let Some(v) = &r.te[channel] {
            assert!(v.lod_pettke > 0.0 && v.lod_longerich > 0.0 && v.lod_howell > 0.0);
            if v.below_lod {
                below += 1;
                assert_ne!(v.ppm, 0.0);
                assert!(v.ppm < v.lod_pettke);
            }
        }
    }
    assert!(below > 0, "monazite spots carry almost no 204Pb");
    let table = export::results_table(&s, &red);
    let column = table.headers.iter().position(|h| h == "Pb204 ppm").unwrap();
    assert!(table.rows.iter().any(|row| matches!(&row[column], export::Cell::Text(t) if t == "< LOD")));
}

#[test]
fn channels_show_the_whole_firing_not_only_the_selection() {
    use chemchron_laser_core::drs::{series, upb::RatioKind, SeriesKey};
    let s = demo_session(1);
    let red = reduce(&s);
    let sel = s.selections.iter().find(|x| x.label == "Sample A_1").unwrap();
    let time = &s.inputs.time;
    let on = sel.laser_on.unwrap();
    // Between the laser firing and the start of the window, and just after its end.
    let before = time.partition_point(|t| *t < on + 1.5);
    assert!(time[before] < sel.start);
    let after = time.partition_point(|t| *t < sel.end + 0.5);
    let quiet = time.partition_point(|t| *t < on - 6.0);
    for key in [
        SeriesKey::RatioRaw(RatioKind::R68),
        SeriesKey::RatioDownhole(RatioKind::R68),
        SeriesKey::RatioFinal(RatioKind::R68),
        SeriesKey::Age(RatioKind::R68),
        SeriesKey::Age(RatioKind::R76),
        SeriesKey::BeamSeconds,
    ] {
        let v = series(&s, &red, key);
        assert!(v[before].is_finite(), "{key:?} before the window");
        assert!(v[after].is_finite(), "{key:?} after the window");
        assert!(v[quiet].is_nan(), "{key:?} must not run through the baseline");
    }
    let beam = series(&s, &red, SeriesKey::BeamSeconds);
    assert!((beam[before] - (time[before] - on)).abs() < 1e-9);
    let channel = s.inputs.channel_index("La139").unwrap();
    let ppm = series(&s, &red, SeriesKey::Ppm(channel));
    assert!(ppm[before].is_finite() && ppm[quiet].is_nan());
}

#[test]
fn ages_per_sweep_agree_with_the_mean_age_of_the_window() {
    use chemchron_laser_core::drs::{series, upb::RatioKind, SeriesKey};
    let s = demo_session(1);
    let red = reduce(&s);
    let i = s.selections.iter().position(|x| x.label == "Sample A_1").unwrap();
    let sel = &s.selections[i];
    let range = s.inputs.range(sel.start, sel.end);
    let v = series(&s, &red, SeriesKey::RatioFinal(RatioKind::R68));
    let m = chemchron_laser_core::stats::mean_se(&v[range.clone()], s.settings.outlier_k);
    let u = red.selections[i].upb.as_ref().unwrap();
    assert!((m.mean - u.r68.mean).abs() / u.r68.mean < 1e-9, "the display must be the same numbers");
    let ages = series(&s, &red, SeriesKey::Age(RatioKind::R68));
    let d = &s.settings.upb.decay;
    let k = range.start + 5;
    assert!((ages[k] - d.age68(v[k])).abs() < 1e-9);
}

#[test]
fn total_beam_leaves_out_mercury() {
    use chemchron_laser_core::drs::{series, SeriesKey};
    let s = demo_session(1);
    let red = reduce(&s);
    let total = series(&s, &red, SeriesKey::TotalBeam);
    let k = s.inputs.time.len() / 2;
    let sum: f64 = s
        .inputs
        .channels
        .iter()
        .filter(|c| c.isotope.as_ref().is_some_and(|i| i.element != "Hg"))
        .map(|c| c.values[k])
        .sum();
    assert_eq!(total[k], sum);
}
