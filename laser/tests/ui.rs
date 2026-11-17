// SPDX-License-Identifier: GPL-3.0-or-later
//! Drives the real window code headlessly: clicks, drags and keys, then checks
//! what changed in the session and results.

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use chemchron_laser::app::{Page, TraceApp};
use chemchron_laser_core::model::*;
use chemchron_laser_core::drs::upb::RatioKind;

fn harness<'a>() -> Harness<'a, TraceApp> {
    let ctx = egui::Context::default();
    let app = TraceApp::new(&ctx);
    Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .with_max_steps(40)
        // Form controls don't all have spoken names yet; this suite tests behaviour.
        .with_accessibility_check(false)
        .build_ui_state(|ui, app: &mut TraceApp| app.ui_root(ui), app)
}

fn settle(h: &mut Harness<'_, TraceApp>) {
    h.run_steps(4);
}

#[test]
fn every_page_opens_from_the_navigation() {
    let mut h = harness();
    settle(&mut h);
    for page in Page::ALL {
        h.get_by_label(page.title()).click();
        settle(&mut h);
        assert_eq!(h.state().page, page);
    }
}

#[test]
fn every_page_survives_empty_data() {
    let mut h = harness();
    {
        let app = h.state_mut();
        app.session.selections.clear();
        app.session.groups.clear();
        app.session.settings.upb.primary_group = None;
        app.session.settings.te.primary_group = None;
        app.recalculate();
        app.after_session_replaced();
    }
    settle(&mut h);
    for page in Page::ALL {
        h.state_mut().page = page;
        settle(&mut h);
    }
}

fn plot_points(h: &Harness<'_, TraceApp>) -> (egui::Pos2, egui::Pos2) {
    // The plot fills the middle of the page; these two points are inside it,
    // and the first spots of the demo session are not under them on screen.
    let app = h.state();
    assert_eq!(app.page, Page::Signal);
    (egui::pos2(700.0, 400.0), egui::pos2(900.0, 400.0))
}

#[test]
fn dragging_on_the_plot_draws_a_selection_and_undo_takes_it_back() {
    let mut h = harness();
    settle(&mut h);
    // zoom to an empty stretch between the first glass spot (ends 39 s) and the next baseline
    h.state_mut().signal.x = (40.0, 52.0);
    settle(&mut h);
    let before = h.state().session.selections.len();
    let (a, b) = plot_points(&h);
    h.hover_at(a);
    h.step();
    h.drag_at(a);
    h.step();
    h.hover_at(egui::pos2(800.0, 400.0));
    h.step();
    h.hover_at(b);
    h.step();
    h.drop_at(b);
    settle(&mut h);
    assert_eq!(h.state().session.selections.len(), before + 1, "a new selection");
    let id = h.state().selected.expect("the new selection is selected");
    let s = h.state().session.selections.iter().find(|s| s.id == id).unwrap();
    assert_eq!(s.source, SelectionSource::Manual);
    assert!(s.end > s.start);
    h.get_by_label("Undo").click();
    settle(&mut h);
    assert_eq!(h.state().session.selections.len(), before, "undo removes it");
    h.get_by_label("Redo").click();
    settle(&mut h);
    assert_eq!(h.state().session.selections.len(), before + 1, "redo restores it");
}

#[test]
fn next_spot_moves_through_selections_and_zooms() {
    let mut h = harness();
    settle(&mut h);
    h.get_by_label("Next spot").click();
    settle(&mut h);
    let first = h.state().selected.expect("a selection");
    h.get_by_label("Next spot").click();
    settle(&mut h);
    let second = h.state().selected.expect("a selection");
    assert_ne!(first, second);
    h.get_by_label("Previous spot").click();
    settle(&mut h);
    assert_eq!(h.state().selected, Some(first));
    assert!(h.state().signal.x.1 - h.state().signal.x.0 < 200.0);
}

#[test]
fn delete_key_removes_the_selected_selection() {
    let mut h = harness();
    settle(&mut h);
    let before = h.state().session.selections.len();
    h.get_by_label("Next spot").click();
    settle(&mut h);
    h.key_press(egui::Key::Delete);
    settle(&mut h);
    assert_eq!(h.state().session.selections.len(), before - 1);
    assert!(h.state().selected.is_none());
}

#[test]
fn leaving_an_analysis_out_changes_results_and_marks_the_session_unsaved() {
    let mut h = harness();
    h.state_mut().page = Page::Results;
    let target = h
        .state()
        .session
        .selections
        .iter()
        .find(|s| s.label == "Sample B_1")
        .map(|s| s.id)
        .unwrap();
    let before = h.state().red.qa.groups.iter().find(|g| g.name == "Sample B").unwrap().n;
    h.state_mut().selected = Some(target);
    settle(&mut h);
    h.get_by_label("Leave out").click();
    settle(&mut h);
    assert!(h.state().unsaved);
    let after = h.state().red.qa.groups.iter().find(|g| g.name == "Sample B").unwrap().n;
    assert_eq!(after, before - 1);
    h.get_by_label("Use in calculations").click();
    settle(&mut h);
    let restored = h.state().red.qa.groups.iter().find(|g| g.name == "Sample B").unwrap().n;
    assert_eq!(restored, before);
}

#[test]
fn choosing_a_common_lead_method_recalculates() {
    let mut h = harness();
    settle(&mut h);
    let first = |app: &TraceApp| app.red.selections.iter().find_map(|s| s.upb.as_ref()).map(|u| u.common.is_some());
    assert_eq!(first(h.state()), Some(false));
    h.state_mut().session.settings.upb.common_method = CommonPbMethod::Pb204;
    h.state_mut().changed = true;
    settle(&mut h);
    assert_eq!(first(h.state()), Some(true));
    h.get_by_label("Undo").click();
    settle(&mut h);
    assert_eq!(first(h.state()), Some(false));
}

#[test]
fn results_views_and_uncertainty_choices_switch() {
    let mut h = harness();
    h.state_mut().page = Page::Results;
    settle(&mut h);
    h.get_by_label("Ratios").click();
    settle(&mut h);
    h.get_all_by_label("Elements").last().expect("the results view button").click();
    settle(&mut h);
    assert!(h.state().results.view == chemchron_laser::ui::results::ResultsView::Elements);
    h.get_by_label("Total").click();
    settle(&mut h);
    assert!(h.state().results.uncertainty == chemchron_laser::ui::results::Uncertainty::Total);
    h.get_by_label("Flagged only").click();
    settle(&mut h);
    assert!(h.state().results.flagged_only);
}

#[test]
fn theme_switch_keeps_working() {
    let mut h = harness();
    settle(&mut h);
    h.get_by_label("Light").click();
    settle(&mut h);
    assert!(!h.state().dark);
    h.get_by_label("Dark").click();
    settle(&mut h);
    assert!(h.state().dark);
}

#[test]
fn real_element_session_opens_on_every_page() {
    let Some(path) = chemchron_laser_core::testdata::session_folder("LAICPMS_monazite_session") else {
        return;
    };
    let mut h = harness();
    h.state_mut().open_blocking(&[path]);
    settle(&mut h);
    assert!(!h.state().is_demo());
    for page in Page::ALL {
        h.state_mut().page = page;
        settle(&mut h);
    }
}

#[test]
fn arrow_keys_go_through_the_list_within_a_group() {
    let mut h = harness();
    settle(&mut h);
    h.key_press(egui::Key::ArrowDown);
    settle(&mut h);
    let first = h.state().selected.expect("the first analysis");
    h.key_press(egui::Key::ArrowDown);
    settle(&mut h);
    let second = h.state().selected.expect("the next analysis");
    assert_ne!(first, second);
    h.key_press(egui::Key::ArrowUp);
    settle(&mut h);
    assert_eq!(h.state().selected, Some(first));

    // Clicking a group on the left narrows the list to it, baselines too.
    h.get_by_label("Baseline").click();
    settle(&mut h);
    let baseline = h.state().session.groups.iter().find(|g| g.name == "Baseline").unwrap().id;
    let rows = h.state().list_rows();
    assert!(rows.len() > 50);
    assert!(rows.iter().all(|i| h.state().session.selections[*i].group == baseline));
    h.key_press(egui::Key::ArrowDown);
    settle(&mut h);
    let id = h.state().selected.unwrap();
    let chosen = h.state().session.selections.iter().find(|s| s.id == id).unwrap();
    assert_eq!(chosen.group, baseline, "stepping stays in the group");
    let view = h.state().signal.x;
    assert!(view.1 - view.0 < 200.0, "it zooms to the selection");
}

#[test]
fn clicking_a_row_selects_that_analysis() {
    let mut h = harness();
    settle(&mut h);
    h.get_by_label("44069_1").click();
    settle(&mut h);
    let id = h.state().selected.expect("a selection");
    let s = h.state().session.selections.iter().find(|s| s.id == id).unwrap();
    assert_eq!(s.label, "44069_1");
}

#[test]
fn ticking_channels_adds_and_removes_strips() {
    let mut h = harness();
    settle(&mut h);
    let key = chemchron_laser_core::drs::SeriesKey::TotalBeam;
    assert!(!h.state().signal.picks.contains(&key));
    h.get_by_label("Intermediate").click();
    settle(&mut h);
    h.get_by_label("Total beam").click();
    settle(&mut h);
    assert!(h.state().signal.picks.contains(&key), "total beam joins the main plot");
    h.get_by_label("Output").click();
    settle(&mut h);
    let age = "207Pb/206Pb age";
    h.get_by_label(age).click();
    settle(&mut h);
    let picked = chemchron_laser_core::drs::SeriesKey::Age(chemchron_laser_core::drs::upb::RatioKind::R76);
    assert!(h.state().signal.picks.contains(&picked));
    assert!(!picked.is_counts(), "ages get their own strip");
    h.get_by_label(age).click();
    settle(&mut h);
    assert!(!h.state().signal.picks.contains(&picked));
}

#[test]
fn many_strips_do_not_break_the_page() {
    use chemchron_laser_core::drs::{upb::RatioKind, SeriesKey};
    let mut h = harness();
    {
        let app = h.state_mut();
        for k in RatioKind::ALL {
            app.signal.picks.push(SeriesKey::RatioRaw(k));
            app.signal.picks.push(SeriesKey::RatioDownhole(k));
            app.signal.picks.push(SeriesKey::Age(k));
        }
        app.signal.picks.push(SeriesKey::BeamSeconds);
    }
    settle(&mut h);
    assert!(h.state().notice.as_ref().is_some_and(|n| n.0.contains("don't fit")));
}


#[test]
fn marked_analyses_move_to_another_group_and_undo_brings_them_back() {
    let mut h = harness();
    h.state_mut().page = Page::Signal;
    settle(&mut h);
    let (ids, target) = {
        let app = h.state();
        let first = app.session.selections.iter().find(|s| app.session.group_of(s).is_some_and(|g| g.kind != GroupKind::Baseline)).unwrap();
        let ids: Vec<u64> = app.session.selections.iter().filter(|s| s.group == first.group).take(2).map(|s| s.id).collect();
        let target = app.session.groups.iter().find(|g| g.kind != GroupKind::Baseline && g.id != first.group).unwrap().id;
        (ids, target)
    };
    {
        let app = h.state_mut();
        app.signal.marked = ids.iter().copied().collect();
        app.selected = Some(ids[0]);
    }
    settle(&mut h);
    // the bar under the list offers the move
    assert!(h.query_by_label("2 marked").is_some());
    h.state_mut().signal.move_target = Some(target);
    settle(&mut h);
    h.get_by_label("Move").click();
    settle(&mut h);
    for id in &ids {
        assert_eq!(h.state().session.selections.iter().find(|s| s.id == *id).unwrap().group, target);
    }
    h.state_mut().undo();
    settle(&mut h);
    assert_ne!(h.state().session.selections.iter().find(|s| s.id == ids[0]).unwrap().group, target);
}

#[test]
fn reduction_scheme_turns_the_schemes_on_and_off() {
    let mut h = harness();
    h.state_mut().page = Page::Corrections;
    settle(&mut h);
    h.get_by_label("Trace elements only").click();
    settle(&mut h);
    assert!(!h.state().session.settings.upb.enabled && h.state().session.settings.te.enabled);
    assert!(h.state().red.upb.is_none());
    h.get_by_label("U–Pb geochronology and trace elements").click();
    settle(&mut h);
    assert!(h.state().session.settings.upb.enabled && h.state().red.upb.is_some());
}

#[test]
fn live_concordia_follows_the_selected_analysis() {
    let mut h = harness();
    h.state_mut().page = Page::Signal;
    let id = {
        let app = h.state();
        app.session.selections.iter().zip(&app.red.selections).find(|(_, r)| r.upb.is_some()).map(|(s, _)| s.id).unwrap()
    };
    h.state_mut().selected = Some(id);
    settle(&mut h);
    assert!(h.query_by_label("Live concordia").is_some());
}

#[test]
fn cut_outs_are_drawn_and_listed_on_the_signal_page() {
    let mut h = harness();
    h.state_mut().page = Page::Signal;
    let id = h.state().session.selections.iter().find(|s| s.label.starts_with("Sample A")).unwrap().id;
    h.state_mut().selected = Some(id);
    h.state_mut().zoom_to_selected();
    settle(&mut h);
    // the toolbar switch and the panel controls are there
    h.get_by_label("Draw cut-out").click();
    settle(&mut h);
    assert!(h.state().signal.cut_mode);
    {
        let app = h.state_mut();
        app.signal.cut_channels.insert("La139".into());
        let s = app.session.selections.iter_mut().find(|s| s.id == id).unwrap();
        let mid = (s.start + s.end) / 2.0;
        s.cutouts.push(Cutout { start: mid - 1.0, end: mid + 1.0, channels: vec!["La139".into()] });
        app.changed = true;
    }
    settle(&mut h);
    assert!(h.query_all_by_label_contains("La139").count() > 0);
    // automatic mode shows its own controls on Corrections
    h.state_mut().session.settings.te.inclusions = InclusionMode::Both;
    h.state_mut().page = Page::Corrections;
    settle(&mut h);
    assert!(h.query_by_label("Spike above").is_some());
}



#[test]
fn an_inclusion_is_split_off_into_its_own_analysis() {
    let mut h = harness();
    settle(&mut h);
    let (id, label, a, b) = {
        let app = h.state();
        let y = app.session.inputs.channel_index("Y89").unwrap();
        let s = app
            .session
            .selections
            .iter()
            .find(|s| app.red.result_for(s.id).is_some_and(|r| r.cuts.iter().any(|c| c.channel == y && c.auto)))
            .expect("the demo inclusion");
        let r = app.red.result_for(s.id).unwrap();
        let c = r.cuts.iter().find(|c| c.channel == y).unwrap();
        let t = &app.session.inputs.time;
        (s.id, s.label.clone(), t[*c.sweeps.first().unwrap()], t[*c.sweeps.last().unwrap()])
    };
    let n = h.state().session.selections.len();
    {
        let app = h.state_mut();
        app.session.selections.iter_mut().find(|s| s.id == id).unwrap().cutouts.push(Cutout { start: a, end: b, channels: vec!["Y89".into()] });
        app.split_off(id, 0);
    }
    settle(&mut h);
    let app = h.state();
    let incl = app.session.selections.iter().find(|s| s.label == format!("{label}_incl")).expect("inclusion analysis");
    assert!((incl.start - a).abs() < 1e-9 && (incl.end - b).abs() < 1e-9);
    assert!(app.session.group(incl.group).unwrap().name.ends_with(" inclusions"));
    let host = app.session.selections.iter().find(|s| s.id == id).unwrap();
    assert!(host.start < a && host.end > b, "the spot stays one analysis over its whole range");
    assert_eq!(host.cutouts.len(), 1, "with its cut-out");
    assert!(incl.cutouts.is_empty(), "the inclusion analysis keeps all elements");
    assert_eq!(app.session.selections.len(), n + 1);
    // the inclusion part has the high Y, the host part not
    let y = app.session.inputs.channel_index("Y89").unwrap();
    let yi = app.red.result_for(incl.id).and_then(|r| r.te[y].as_ref()).map(|v| v.ppm).unwrap_or(0.0);
    let yh = app.red.result_for(id).and_then(|r| r.te[y].as_ref()).map(|v| v.ppm).unwrap_or(f64::NAN);
    assert!(yi > yh * 2.0, "inclusion Y {yi} vs host {yh}");
}

#[test]
fn inclusion_mode_and_cut_outs_on_the_signal_page() {
    let mut h = harness();
    h.state_mut().page = Page::Signal;
    settle(&mut h);
    assert!(h.query_by_label("Cut-outs").is_some(), "mode switch in the toolbar");
    // automatic only: no drawing button
    h.state_mut().session.settings.te.inclusions = InclusionMode::Automatic;
    settle(&mut h);
    assert!(h.query_by_label("Draw cut-out").is_none());
    // U–Pb only: cut-outs by hand on the raw signals
    h.state_mut().session.settings.te.enabled = false;
    h.state_mut().changed = true;
    settle(&mut h);
    assert!(h.query_by_label("Draw cut-out").is_some());
    assert!(h.query_by_label("Cut-outs").is_some(), "the switch stays with U–Pb only");
}

#[test]
fn clicking_a_baseline_on_the_plot_shows_it_in_the_list() {
    let mut h = harness();
    settle(&mut h);
    let (id, group, label, start, end) = {
        let s = &h.state().session;
        let bl = s.groups.iter().find(|g| g.kind == GroupKind::Baseline).unwrap().id;
        let x = s.selections.iter().filter(|x| x.group == bl).nth(2).unwrap();
        (x.id, bl, x.label.clone(), x.start, x.end)
    };
    assert_eq!(h.state().signal.list_group, None, "the list starts with all analyses (no baselines)");
    // zoom into that baseline, so a click in the plot lands on it
    h.state_mut().signal.x = (start + 0.2, end - 0.2);
    settle(&mut h);
    h.hover_at(egui::pos2(800.0, 400.0));
    h.step();
    // a click: press and release on the same spot
    h.drag_at(egui::pos2(800.0, 400.0));
    h.step();
    h.drop_at(egui::pos2(800.0, 400.0));
    settle(&mut h);
    assert_eq!(h.state().selected, Some(id));
    assert_eq!(h.state().signal.list_group, Some(group), "the list switches to the baseline group");
    assert!(h.query_by_label(&label).is_some(), "and shows {label}");
}

#[test]
fn secondary_reference_materials_join_the_excess() {
    let h = harness();
    let model = h.state().red.upb.as_ref().expect("the demo has U-Pb");
    let r68 = model.ratio(RatioKind::R68).unwrap();
    assert!(r68.n_secondary > 0, "Trebilcock analyses enter the 206/238 excess");
    assert!(r68.excess.is_finite() && r68.excess_primary.is_finite());
    // its 207/206 (a young monazite: Pb–Pb age too imprecise) is left out of that ratio only
    let r76 = model.ratio(RatioKind::R76).unwrap();
    assert!(r76.left_out.iter().any(|(n, _)| n == "Trebilcock"), "{:?}", r76.left_out);
    assert_eq!(r76.n_secondary, 0);
    let r75 = model.ratio(RatioKind::R75).unwrap();
    assert!(r75.left_out.is_empty() && r75.n_secondary > 0, "207/235 of Trebilcock is used");
}
