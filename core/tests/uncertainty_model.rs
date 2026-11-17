// SPDX-License-Identifier: GPL-3.0-or-later
//! The common uncertainty model: the systematic term comes from the long-term
//! log once it holds 3 sessions (else the manual value), and U–Pb means add
//! the primary's accepted-value uncertainty and, when chosen, the decay constants.

use chemchron_core::row::*;
use chemchron_core::uncertainty::between_session_systematic;
use chemchron_core::upbstats::{decay_rel_2s_pct, primary_rm_field, resolve_upb_systematic, systematic_for, UpbMineralUncertainty};
use serde_json::json;
use std::collections::BTreeMap;

fn entry(reference: &str, session: &str, age: f64) -> Row {
    let mut r = Row::new();
    set(&mut r, "Reference", jstr(reference));
    set(&mut r, "SessionTag", jstr(session));
    set(&mut r, "Mineral", jstr("Monazite"));
    set(&mut r, "Age_Ma", jnum(age));
    set(&mut r, "Age_2s_Ma", jnum(4.0));
    set(&mut r, "t68_Ma", jnum(age));
    set(&mut r, "t68_2s_Ma", jnum(4.0));
    r
}

/// Sessions whose means differ by more than their own uncertainties.
fn log(sessions: usize) -> Vec<Row> {
    let mut v = Vec::new();
    for s in 0..sessions {
        let offset = [0.0, 8.0, -7.0, 5.0][s % 4];
        for k in 0..4 {
            v.push(entry("WC425", &format!("S{s}"), 425.0 + offset + (k as f64 - 1.5)));
        }
    }
    v
}

#[test]
fn the_log_sets_the_systematic_term_from_three_sessions() {
    let none = BTreeMap::new();
    assert!(between_session_systematic(&log(2), "Monazite", &[], Some(0.0), &none).0.is_none(), "two sessions are not enough");
    let p = between_session_systematic(&log(3), "Monazite", &[], Some(0.0), &none).0.expect("three sessions");
    assert!(p > 0.5, "{p}");
}

#[test]
fn upb_systematic_falls_back_to_manual_and_adds_primary_and_decay() {
    let mut settings = BTreeMap::new();
    settings.insert("Monazite".to_string(), UpbMineralUncertainty { systematic_pct: 1.3, ..Default::default() });
    assert_eq!(settings["Monazite"].systematic_source, "log", "the log is the default source");
    // too few sessions: the manual value
    let few = resolve_upb_systematic(&settings, &log(2));
    let mut row = Row::new();
    set(&mut row, "Mineral", jstr("Monazite"));
    set(&mut row, "UPb_t68_Ma", jnum(425.0));
    let (p, src) = systematic_for(&few, &[&row], "t68", 1.3);
    assert!((p - 1.3).abs() < 1e-9 && src.contains("manual"), "{p} {src}");
    // the primary's accepted value joins in quadrature
    set(&mut row, &primary_rm_field("t68"), json!(0.6));
    let (p, src) = systematic_for(&few, &[&row], "t68", 1.3);
    assert!((p - 1.3f64.hypot(0.6)).abs() < 1e-9 && src.contains("primary RM"), "{p} {src}");
    // and the decay constants when chosen
    let mut with_decay = few.clone();
    with_decay.decay = Some(chemchron_core::constants::DecayConstants::default());
    let d = decay_rel_2s_pct("t68", 425.0, with_decay.decay.as_ref().unwrap());
    assert!(d > 0.05 && d < 0.5, "{d}");
    let (p2, src) = systematic_for(&with_decay, &[&row], "t68", 1.3);
    assert!((p2 - p.hypot(d)).abs() < 1e-9 && src.contains("decay"), "{p2} {src}");
    // with three sessions the log decides
    let many = resolve_upb_systematic(&settings, &log(3));
    let (base, src) = many.by_mineral["Monazite"]["t68"].clone();
    assert!(src.contains("log") && (base - 1.3).abs() > 1e-6, "{base} {src}");
}

#[test]
fn older_settings_switch_to_the_log() {
    let dir = std::env::temp_dir().join(format!("chemchron-unc-model-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("upb_settings.json"), r#"{"systematic_pct": 1.3, "systematic_source": "manual"}"#).unwrap();
    std::fs::write(dir.join("long_term_uncertainty.json"), r#"{"Monazite": {"systematic_source": "manual", "manual_override_pct": 2.0}}"#).unwrap();
    let s = chemchron_core::settings::Settings::load_from(&dir);
    assert_eq!(s.upb.systematic_source, "log");
    assert_eq!(s.uncertainty["Monazite"].systematic_source, "log");
    assert_eq!(s.upb.model_version, 3);
    // a choice made afterwards is kept
    std::fs::write(dir.join("upb_settings.json"), r#"{"systematic_source": "manual", "model_version": 3}"#).unwrap();
    let s = chemchron_core::settings::Settings::load_from(&dir);
    assert_eq!(s.upb.systematic_source, "manual");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn ratios_left_out_by_hand_leave_their_ages_out_of_the_systematic_term() {
    let s = UpbMineralUncertainty { excluded_ratios: vec!["Trebilcock|r76".into()], ..Default::default() };
    assert!(chemchron_core::upbstats::excluded_for_age(&s, "t76").contains(&"Trebilcock".to_string()));
    assert!(chemchron_core::upbstats::excluded_for_age(&s, "t75").contains(&"Trebilcock".to_string()));
    assert!(!chemchron_core::upbstats::excluded_for_age(&s, "t68").contains(&"Trebilcock".to_string()));
    assert_eq!(UpbMineralUncertainty::default().excess_source, "project");
}

#[test]
fn young_materials_are_left_out_of_the_pb_pb_systematic_term() {
    let mut log = Vec::new();
    for s in 0..3 {
        for k in 0..4 {
            let mut e = entry("Trebilcock", &format!("S{s}"), 272.0);
            set(&mut e, "t76_Ma", jnum(272.0 + 60.0 * ((s * 4 + k) as f64 - 5.5)));
            set(&mut e, "t76_2s_Ma", jnum(40.0));
            log.push(e);
        }
    }
    let (_, mats) = chemchron_core::upbstats::between_session_upb_systematic(&log, "Monazite", "t76", &[]);
    assert!(mats.iter().any(|m| m.reference == "Trebilcock" && m.left_out.is_some()), "{mats:?}");
    let (_, mats) = chemchron_core::upbstats::between_session_upb_systematic(&log, "Monazite", "t68", &[]);
    assert!(mats.iter().all(|m| m.left_out.is_none()));
}
