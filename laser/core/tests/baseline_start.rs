// SPDX-License-Identifier: GPL-3.0-or-later
//! The "Baseline start" of the automatic selections moves every baseline's
//! start, also where each spot is its own data file.

use chemchron_laser_core::autosel::AutoOptions;
use chemchron_laser_core::model::GroupKind;
use chemchron_laser_core::project;

fn baseline_starts(name: &str, crop: f64) -> Option<Vec<f64>> {
    let data = chemchron_laser_core::testdata::session_folder(name)?;
    let mut s = project::session_from_paths(&[data], None).ok()?;
    let opts = AutoOptions { baseline_crop_start_s: crop, ..Default::default() };
    project::auto_select(&mut s, &opts);
    let bl: Vec<u64> = s.groups.iter().filter(|g| g.kind == GroupKind::Baseline).map(|g| g.id).collect();
    Some(s.selections.iter().filter(|x| bl.contains(&x.group)).map(|x| x.start).collect())
}

#[test]
fn baseline_start_moves_every_baseline() {
    for name in ["LAICPMS_trace_session.b", "LAICPMS_monazite_session"] {
        let (Some(a), Some(b)) = (baseline_starts(name, 2.0), baseline_starts(name, 4.0)) else { continue };
        println!("{name}: {} baselines; first starts {:?} / {:?}", a.len(), &a[..a.len().min(3)], &b[..b.len().min(3)]);
        assert_eq!(a.len(), b.len(), "{name}");
        assert!(!a.is_empty(), "{name}");
        for (x, y) in a.iter().zip(&b) {
            assert!((y - x - 2.0).abs() < 0.3, "{name}: baseline start {x} -> {y}");
        }
    }
}
