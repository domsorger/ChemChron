// SPDX-License-Identifier: GPL-3.0-or-later
//! The lab's trace-element batch (Agilent 7900, NIST 612 bracketing,
//! garnet unknowns): grouping, calibration and the guard against a bad
//! calibration spot. Skips itself where the batch is not in the project folder.

use chemchron_laser_core::drs::reduce;
use chemchron_laser_core::model::GroupKind;
use chemchron_laser_core::project::session_from_paths;

fn batch() -> Option<std::path::PathBuf> {
    chemchron_laser_core::testdata::session_folder("LAICPMS_trace_session.b")
}

#[test]
fn nist612_spots_group_and_calibrate() {
    let Some(p) = batch() else { return };
    let mut s = session_from_paths(&[p], None).unwrap();
    assert_eq!(s.files.len(), 169);
    let g612 = s.groups.iter().find(|g| g.name == "612").expect("612_1 and 612_2 spots form one group").clone();
    assert_eq!(g612.kind, GroupKind::ReferenceMaterial);
    assert_eq!(g612.rm.as_deref(), Some("NIST612"));
    assert_eq!(s.selections.iter().filter(|x| x.group == g612.id).count(), 16);
    assert_eq!(s.settings.te.is_channel.as_deref(), Some("Si29"), "garnet unknowns: Si, not Ce");
    let red = reduce(&s);
    // 025_612_2 has 30x less U than every other NIST 612 spot
    assert!(red.warnings.iter().any(|w| w.contains("025_612_2")), "{:?}", red.warnings);
    let names: Vec<String> = s.inputs.channels.iter().map(|c| c.name.clone()).collect();
    let ch = |n: &str| names.iter().position(|x| x == n).unwrap();
    for (el, accepted) in [("U238", 37.38), ("Lu175", 36.9), ("Th232", 37.79), ("La139", 35.8), ("Y89", 38.3)] {
        let c = ch(el);
        for (sel, r) in s.selections.iter().zip(&red.selections) {
            if sel.group != g612.id || sel.label == "025_612_2" {
                continue;
            }
            let v = r.te[c].as_ref().unwrap().ppm;
            eprintln!("DEV {} {el} {:+.1} %", sel.label, 100.0 * (v / accepted - 1.0));
            assert!((v / accepted - 1.0).abs() < 0.10, "{} {el}: {v} ppm, accepted {accepted}", sel.label);
        }
    }
    // With an internal-standard content the garnets get concentrations.
    for g in s.groups.iter_mut().filter(|g| g.name.starts_with("GA") || g.name.starts_with("GB")) {
        g.is_ppm = Some(176_000.0);
    }
    let red = reduce(&s);
    let ga = s.groups.iter().find(|g| g.name == "GA1_1").unwrap().id;
    let (sel, r) = s.selections.iter().zip(&red.selections).find(|(x, _)| x.group == ga).unwrap();
    let lu = r.te[ch("Lu175")].as_ref().unwrap();
    assert!(lu.ppm > 0.1 && lu.ppm < 1000.0, "{} Lu {}", sel.label, lu.ppm);
    let y = r.te[ch("Y89")].as_ref().unwrap();
    assert!(y.ppm > 1.0, "{} Y {}", sel.label, y.ppm);
}
