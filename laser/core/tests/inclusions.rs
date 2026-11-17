// SPDX-License-Identifier: GPL-3.0-or-later
//! Inclusions: a spike of one element inside a spot (e.g. a Zr-rich
//! inclusion in garnet) is left out for that element only, by a cut-out
//! drawn by hand or by the automatic spike removal.

use chemchron_laser_core::drs::{reduce, spike_sweeps};
use chemchron_laser_core::model::*;
use chemchron_laser_core::synthetic::demo_session;
use std::sync::Arc;

/// The demo session with La raised 8x over 2 s in the middle of one sample spot.
fn with_inclusion() -> (Session, u64, usize, (f64, f64)) {
    let mut s = demo_session(1);
    // without the general outlier rule, which also catches short spikes,
    // and without the automatic correction (the demo has it on)
    s.settings.outlier_k = 0.0;
    s.settings.te.inclusions = InclusionMode::ByHand;
    let sel = s.selections.iter().find(|x| x.label.starts_with("Sample A")).unwrap().clone();
    let la = s.inputs.channel_index("La139").unwrap();
    let mid = (sel.start + sel.end) / 2.0;
    let window = (mid - 1.0, mid + 1.0);
    let mut inputs = (*s.inputs).clone();
    for i in 0..inputs.time.len() {
        if inputs.time[i] >= window.0 && inputs.time[i] <= window.1 {
            inputs.channels[la].values[i] *= 8.0;
        }
    }
    s.inputs = Arc::new(inputs);
    (s, sel.id, la, window)
}

fn content(s: &Session, id: u64, ch: usize) -> f64 {
    let red = reduce(s);
    red.result_for(id).unwrap().te[ch].as_ref().map(|v| v.ppm).unwrap_or(f64::NAN)
}

#[test]
fn an_inclusion_is_left_out_for_its_element_only() {
    let mut clean = demo_session(1);
    clean.settings.outlier_k = 0.0;
    clean.settings.te.inclusions = InclusionMode::ByHand;
    let (s, id, la, window) = with_inclusion();
    assert!(clean.settings.te.enabled, "the demo reduces trace elements");
    let ce = s.inputs.channel_index("Nd146").unwrap();
    let truth = content(&clean, id, la);
    let spiked = content(&s, id, la);
    assert!(spiked > truth * 1.3, "the inclusion raises La: {spiked} vs {truth}");

    // by hand: a cut-out over the inclusion for La only
    let mut hand = s.clone();
    hand.settings.te.inclusions = InclusionMode::ByHand;
    hand.selections.iter_mut().find(|x| x.id == id).unwrap().cutouts.push(Cutout { start: window.0 - 0.3, end: window.1 + 0.3, channels: vec!["La139".into()] });
    let la_hand = content(&hand, id, la);
    assert!((la_hand / truth - 1.0).abs() < 0.08, "La back to {la_hand} (truth {truth})");
    // Nd keeps all its sweeps
    let red = reduce(&hand);
    let r = red.result_for(id).unwrap();
    assert!(r.cuts.iter().all(|c| c.channel == la && !c.auto));
    assert_eq!(r.cps[ce].n, reduce(&s).result_for(id).unwrap().cps[ce].n);

    // automatic: the spike is found without a cut-out
    let mut auto = s.clone();
    auto.settings.te.inclusions = InclusionMode::Automatic;
    let la_auto = content(&auto, id, la);
    assert!((la_auto / truth - 1.0).abs() < 0.08, "La automatic {la_auto} (truth {truth})");
    let red = reduce(&auto);
    assert!(red.result_for(id).unwrap().cuts.iter().any(|c| c.channel == la && c.auto));
    // a skipped element is left alone
    auto.settings.te.inclusion_skip.push("La139".into());
    assert!(content(&auto, id, la) > truth * 1.3);
}

#[test]
fn spike_test_leaves_zoning_and_short_spots_alone() {
    // a smooth step (zoning) over half the spot is not cut
    let v: Vec<f64> = (0..40).map(|i| if i < 20 { 100.0 } else { 300.0 }).collect();
    assert!(spike_sweeps(&v, None, 4.0).is_empty());
    // a single spike is, with its neighbours
    let mut w: Vec<f64> = (0..40).map(|i| 100.0 + (i % 3) as f64).collect();
    w[20] = 900.0;
    assert!(spike_sweeps(&w, None, 4.0).is_empty(), "one sweep alone is noise");
    w[21] = 900.0;
    assert_eq!(spike_sweeps(&w, None, 4.0), vec![19, 20, 21, 22]);
    assert!(spike_sweeps(&w[..6], None, 4.0).is_empty());
}

#[test]
fn the_demo_inclusion_is_found_and_corrected() {
    let s = demo_session(1);
    assert_eq!(s.settings.te.inclusions, InclusionMode::Both);
    let red = reduce(&s);
    let y = s.inputs.channel_index("Y89").unwrap();
    let la = s.inputs.channel_index("La139").unwrap();
    // exactly one Sample B spot has automatic Y cuts, inside its inclusion window
    let hit: Vec<&Selection> = s
        .selections
        .iter()
        .filter(|x| x.label.starts_with("Sample B"))
        .filter(|x| red.result_for(x.id).unwrap().cuts.iter().any(|c| c.channel == y && c.auto))
        .collect();
    assert_eq!(hit.len(), 1, "{:?}", hit.iter().map(|x| &x.label).collect::<Vec<_>>());
    let r = red.result_for(hit[0].id).unwrap();
    assert!(r.cuts.iter().all(|c| c.channel != la), "La has no inclusion and keeps its sweeps");
    // its Y content is back near the truth
    let truth = chemchron_laser_core::synthetic::true_ppm("Sample B", "Y").unwrap();
    let got = r.te[y].as_ref().unwrap().ppm;
    assert!((got / truth - 1.0).abs() < 0.12, "Y {got} vs {truth}");
    // without the correction it is clearly too high
    let mut off = s.clone();
    off.settings.te.inclusions = InclusionMode::ByHand;
    off.settings.outlier_k = 0.0;
    let raw = reduce(&off).result_for(hit[0].id).unwrap().te[y].as_ref().unwrap().ppm;
    assert!(raw > got * 1.3, "uncorrected {raw} vs corrected {got}");
}

#[test]
fn a_cut_out_on_a_lead_signal_leaves_its_sweeps_out_of_the_ratios_using_it() {
    let mut s = demo_session(1);
    s.settings.te.enabled = false; // U–Pb only
    let sel = s.selections.iter().find(|x| x.label.starts_with("Sample A")).unwrap().clone();
    let before = reduce(&s);
    let u0 = before.result_for(sel.id).unwrap().upb.clone().unwrap();
    let mid = (sel.start + sel.end) / 2.0;
    s.selections.iter_mut().find(|x| x.id == sel.id).unwrap().cutouts.push(Cutout { start: mid - 1.0, end: mid + 1.0, channels: vec!["Pb206".into()] });
    let after = reduce(&s);
    let u1 = after.result_for(sel.id).unwrap().upb.clone().unwrap();
    assert!(u1.r68.n < u0.r68.n, "206/238 loses the cut sweeps: {} -> {}", u0.r68.n, u1.r68.n);
    assert!(u1.r76.n < u0.r76.n, "207/206 too");
    assert_eq!(u1.r75.n, u0.r75.n, "207/235 does not use Pb206");
    assert_eq!(u1.r82.as_ref().map(|r| r.n), u0.r82.as_ref().map(|r| r.n));
}

#[test]
fn automatic_u_pb_cut_outs_remove_a_common_lead_inclusion() {
    let mut s = demo_session(1);
    s.settings.te.enabled = false;
    s.settings.outlier_k = 0.0;
    let sel = s.selections.iter().find(|x| x.label.starts_with("Sample A")).unwrap().clone();
    let mid = (sel.start + sel.end) / 2.0;
    let (p204, p207) = (s.inputs.channel_index("Pb204").unwrap(), s.inputs.channel_index("Pb207").unwrap());
    let mut inputs = (*s.inputs).clone();
    for i in 0..inputs.time.len() {
        if (inputs.time[i] - mid).abs() <= 1.0 {
            inputs.channels[p207].values[i] *= 2.5;
            inputs.channels[p204].values[i] *= 30.0;
        }
    }
    s.inputs = Arc::new(inputs);
    s.settings.te.inclusions = InclusionMode::ByHand;
    let raw = reduce(&s).result_for(sel.id).unwrap().upb.clone().unwrap();
    s.settings.te.inclusions = InclusionMode::Automatic;
    let red = reduce(&s);
    let r = red.result_for(sel.id).unwrap();
    let fixed = r.upb.clone().unwrap();
    assert!(r.cuts.iter().any(|c| c.auto && c.channel == p207), "found by 207/206");
    assert!(fixed.r76.n < raw.r76.n);
    let truth = 1850.0;
    assert!((fixed.a76.age - truth).abs() < (raw.a76.age - truth).abs(), "207/206 age closer to the truth: {} vs {}", fixed.a76.age, raw.a76.age);
}
