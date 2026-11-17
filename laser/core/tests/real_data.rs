// SPDX-License-Identifier: GPL-3.0-or-later
//! Runs the importers and the reduction on raw sessions from the lab. See
//! `testdata` for where they are found and when these tests may skip themselves.

use chemchron_laser_core::drs::reduce;
use chemchron_laser_core::project::session_from_paths;
use chemchron_laser_core::testdata::session_folder as folder;

#[test]
fn element_session_with_esi_log() {
    let Some(p) = folder("LAICPMS_monazite_session") else { return };
    let s = session_from_paths(&[p], None).unwrap();
    assert_eq!(s.files.len(), 126);
    assert_eq!(s.inputs.laser.len(), 126);
    assert_eq!(s.inputs.channels.len(), 8);
    // The two clocks differ by about an hour.
    assert!((s.clock_offset_s + 3662.0).abs() < 30.0, "{}", s.clock_offset_s);
    let count = |n: &str| {
        let id = s.groups.iter().find(|g| g.name == n).unwrap().id;
        s.selections.iter().filter(|x| x.group == id).count()
    };
    assert_eq!(count("M44"), 20);
    assert_eq!(count("MBA"), 10);
    assert_eq!(count("A27"), 10);
    assert_eq!(count("m"), 86);
    let m44 = s.groups.iter().find(|g| g.name == "M44").unwrap();
    assert_eq!(m44.rm.as_deref(), Some("44069"));
    assert!(s.selections.iter().any(|x| x.label == "M44_20"), "repeated log names are numbered");
    assert_eq!(s.groups.iter().find(|g| g.name == "MBA").unwrap().rm.as_deref(), Some("MBA"));
    assert_eq!(s.groups.iter().find(|g| g.name == "A27").unwrap().rm.as_deref(), Some("A27"));
    assert_eq!(s.settings.upb.primary_group, Some(m44.id));
    let red = reduce(&s);
    let g = red.qa.groups.iter().find(|g| g.name == "M44").unwrap();
    let c = g.concordia.as_ref().unwrap();
    // Primary reference: comes back at its accepted age by construction.
    assert!((c.age - 424.9).abs() < 2.0, "{}", c.age);
    assert!(c.mswd < 2.0, "{}", c.mswd);
}

#[test]
fn agilent_batch_without_log() {
    let Some(p) = folder("LAICPMS_monazite_batch.b") else { return };
    let s = session_from_paths(&[p], None).unwrap();
    assert_eq!(s.files.len(), 69, "each spot read once");
    assert!(s.inputs.laser.is_empty());
    assert_eq!(s.inputs.channels.len(), 7);
    let total: usize = s
        .groups
        .iter()
        .filter(|g| g.name != "Baseline")
        .map(|g| s.selections.iter().filter(|x| x.group == g.id).count())
        .sum();
    assert_eq!(total, 69, "one analysis per spot");
    let baselines = s.groups.iter().find(|g| g.name == "Baseline").unwrap().id;
    assert_eq!(s.selections.iter().filter(|x| x.group == baselines).count(), 69, "a baseline before each spot");
    let id = s.groups.iter().find(|g| g.name == "44069").unwrap().id;
    assert_eq!(s.settings.upb.primary_group, Some(id));
    assert_eq!(s.selections.iter().filter(|x| x.group == id).count(), 15);
    let red = reduce(&s);
    assert!(red.upb.is_some(), "{:?}", red.warnings);
    let c = red.qa.groups.iter().find(|g| g.name == "44069").unwrap();
    assert!((c.concordia.as_ref().unwrap().age - 424.9).abs() < 3.0);
}
