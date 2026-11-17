// SPDX-License-Identifier: GPL-3.0-or-later
//! Drives the ChemChron window headlessly: every part and page opens, with
//! the demo project and with no data, and the part switch works.

use chemchron::app::{ChemApp, Part};
use chemchron::{combine, epma};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use std::path::Path;

fn harness<'a>(with_demo: bool) -> Harness<'a, ChemApp> {
    // keep the user's own settings folder untouched
    // each test its own settings folder (tests run side by side)
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let k = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("chemchron-ui-test-{}-{k}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let ctx = egui::Context::default();
    let mut app = {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("CHEMCHRON_CONFIG_DIR", &dir);
        ChemApp::new(&ctx)
    };
    if with_demo {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/epma/examples/demo_project.chemchron.json");
        app.open_project(&p);
        assert!(app.data.rows.len() > 100, "demo project should open: {:?}", app.notice);
    }
    Harness::builder()
        .with_size(egui::vec2(1480.0, 920.0))
        .with_max_steps(40)
        .with_accessibility_check(false)
        .build_ui_state(|ui, app: &mut ChemApp| app.ui_root(ui), app)
}

const EPMA_PAGES: [epma::Page; 3] = [
    epma::Page::Data,
    epma::Page::Geochronology,
    epma::Page::Composition,
];

const COMBINE_PAGES: [combine::Page; 12] = [
    combine::Page::Data,
    combine::Page::Summary,
    combine::Page::Isochron,
    combine::Page::WeightedMean,
    combine::Page::Concordia,
    combine::Page::UpbMean,
    combine::Page::Ree,
    combine::Page::Composition,
    combine::Page::Compare,
    combine::Page::AgeDist,
    combine::Page::LongTerm,
    combine::Page::Export,
];

fn every_page(h: &mut Harness<'_, ChemApp>) {
    h.state_mut().part = Part::Laser;
    h.run_steps(3);
    h.state_mut().part = Part::Epma;
    for p in EPMA_PAGES {
        h.state_mut().epma.page = p;
        h.run_steps(3);
        h.state_mut().epma.show_refs = true;
        h.run_steps(2);
        h.state_mut().epma.show_refs = false;
    }
    h.state_mut().part = Part::Combine;
    for p in COMBINE_PAGES {
        h.state_mut().combine.page = p;
        h.run_steps(3);
    }
}

#[test]
fn every_page_opens_with_the_demo_project() {
    let mut h = harness(true);
    every_page(&mut h);
}

#[test]
fn every_page_opens_without_data() {
    let mut h = harness(false);
    every_page(&mut h);
}

#[test]
fn the_part_buttons_switch_parts() {
    let mut h = harness(false);
    h.run_steps(3);
    h.get_by_label("LA-ICP-MS").click();
    h.run_steps(3);
    assert_eq!(h.state().part, Part::Laser);
    h.get_by_label("Combined results").click();
    h.run_steps(3);
    assert_eq!(h.state().part, Part::Combine);
    h.get_by_label("EPMA").click();
    h.run_steps(3);
    assert_eq!(h.state().part, Part::Epma);
}

#[test]
fn laser_results_link_to_the_table() {
    // the built-in simulated laser session: its spots become rows of their own
    let mut h = harness(false);
    h.state_mut().laser.load_demo();
    combine::link_laser_now(h.state_mut());
    let app = h.state();
    let upb = app.data.rows.iter().filter(|r| chemchron_core::upb::has_upb(r)).count();
    assert!(upb > 10, "laser spots should be linked: {:?}", app.notice);
    assert!(app.data.rows.iter().any(|r| chemchron_core::upb::has_upb(r) && r.get("UPb_t68_Ma").and_then(|v| v.as_f64()).is_some()));
    h.state_mut().part = Part::Combine;
    for p in COMBINE_PAGES {
        h.state_mut().combine.page = p;
        h.run_steps(3);
    }
}

#[test]
fn undo_brings_rows_back() {
    let mut h = harness(true);
    let n = h.state().data.rows.len();
    {
        let app = h.state_mut();
        app.data.checkpoint("Delete rows");
        app.data.rows.truncate(10);
        app.data.refresh(&app.settings.clone());
    }
    h.run_steps(2);
    h.state_mut().undo();
    assert_eq!(h.state().data.rows.len(), n);
}

#[test]
fn exports_write_files() {
    let h = harness(true);
    let dir = std::env::temp_dir().join(format!("chemchron-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let app = h.state();
    chemchron::export::write_csv(app, &dir.join("r.csv")).unwrap();
    chemchron::export::write_xlsx(app, &dir.join("r.xlsx")).unwrap();
    chemchron::export::write_ods(app, &dir.join("r.ods")).unwrap();
    // the workbooks read back
    let t = chemchron_core::table::read_table(&dir.join("r.ods"), Some("Geochronology Samples")).unwrap();
    assert!(t.len() > 50);
    let t = chemchron_core::table::read_table(&dir.join("r.xlsx"), Some("Reference Materials Overview")).unwrap();
    assert!(t.len() >= 3);
    let s = std::fs::read_to_string(dir.join("r.csv")).unwrap();
    assert!(s.contains("GEOCHRONOLOGY SAMPLES"));
}

#[test]
fn light_colours_reach_every_control() {
    let mut h = harness(true);
    h.state_mut().dark = false;
    h.state_mut().part = Part::Epma;
    h.state_mut().epma.page = epma::Page::Geochronology;
    h.run_steps(3);
    assert!(!h.ctx.global_style().visuals.dark_mode, "tables, text boxes and menus use light colours too");
    assert!(!h.state().settings.dark, "the choice is remembered");
}

#[test]
fn row_menu_assigns_groups_and_plots_the_selection() {
    let mut h = harness(true);
    let ctx = h.ctx.clone();
    let app = h.state_mut();
    let samples: Vec<usize> = (0..app.data.rows.len()).filter(|i| !chemchron_core::row::truthy(&app.data.rows[*i], "Is_Reference")).take(3).collect();
    let cols = chemchron_core::columns::age_columns(false, false);
    epma::apply_row_action(app, &ctx, epma::Action::Group(Some("New group".into())), &samples, &cols);
    for i in &samples {
        assert_eq!(chemchron_core::row::text(&app.data.rows[*i], "Flag"), "New group");
    }
    epma::apply_row_action(app, &ctx, epma::Action::Plot(epma::PlotTarget::IsochronTh), &samples, &cols);
    assert_eq!(app.combine.page, combine::Page::Isochron);
    assert_eq!(app.epma.iso_group, epma::GroupPick::Selection);
    assert_eq!(app.epma.geo_table.selected.len(), 3);
    epma::apply_row_action(app, &ctx, epma::Action::Plot(epma::PlotTarget::ConcordiaTw), &samples, &cols);
    assert_eq!(app.part, Part::Combine);
    assert_eq!(app.combine.page, combine::Page::Concordia);
    h.run_steps(3);
    // undo takes the group assignment back
    h.state_mut().part = Part::Epma;
    h.state_mut().undo();
    assert_ne!(chemchron_core::row::text(&h.state().data.rows[samples[0]], "Flag"), "New group");
}

#[test]
fn laser_and_epma_share_one_reference_list() {
    let Some(data) = chemchron_laser_core::testdata::session_folder("LAICPMS_monazite_session") else {
        return;
    };
    let mut h = harness(false);
    h.state_mut().laser.open_blocking(std::slice::from_ref(&data));
    h.run_steps(3);
    let app = h.state();
    // the laser material 44069 sits with the EPMA entry WC425 (label prefix M44)
    let wc = &app.settings.refs_nested["Monazite"]["WC425"];
    assert_eq!(wc["laser"]["name"], "44069");
    assert!(app.settings.refs_nested["Glass"].get("NIST612").is_some());
    let m = app.laser.session.rms.iter().find(|m| m.name == "44069").unwrap();
    assert!(m.matches_label("WC425"));
    // an edit in the laser part reaches the lab file
    let n_values = m.values.len();
    h.state_mut().laser.session.rms.iter_mut().find(|m| m.name == "44069").unwrap().source = "checked against the certificate".into();
    h.run_steps(2);
    assert_eq!(h.state().settings.refs_nested["Monazite"]["WC425"]["laser"]["source"], "checked against the certificate");
    assert_eq!(h.state().settings.refs_nested["Monazite"]["WC425"]["laser"]["values"].as_object().unwrap().len(), n_values);
    let saved = std::fs::read_to_string(h.state().settings.dir.join("reference_materials.json")).unwrap();
    assert!(saved.contains("checked against the certificate"));
}

#[test]
fn settings_part_holds_the_lab_settings_for_all_parts() {
    let mut h = harness(true);
    h.run_steps(3);
    h.get_by_label("Settings").click();
    h.run_steps(3);
    assert_eq!(h.state().part, Part::Settings);
    for p in chemchron::global::Page::ALL {
        h.state_mut().global.page = p;
        for laser in [false, true] {
            h.state_mut().global.upb_uncertainty = laser;
            h.run_steps(3);
        }
    }
    h.state_mut().global.page = chemchron::global::Page::General;
    h.run_steps(3);
    let _ = h.get_by_label("Set working directory…");
}

#[test]
fn every_sample_group_has_one_colour_in_tables_and_plots() {
    let h = harness(true);
    let app = h.state();
    let colors = app.group_colors();
    let mut groups: Vec<String> = app.data.rows.iter().map(|r| chemchron_core::row::text(r, "Flag")).filter(|f| !f.is_empty()).collect();
    groups.sort();
    groups.dedup();
    assert!(!groups.is_empty());
    let t = app.theme();
    let dp = app.settings.decimals;
    let look = chemchron::rowtable::Look { t: &t, dp: &dp, flag_colors: &colors, show_bdl_values: true };
    for g in &groups {
        assert!(colors.contains_key(g), "group {g} has no colour");
        let row = app.data.rows.iter().find(|r| chemchron_core::row::text(r, "Flag") == *g && !chemchron_core::row::truthy(r, "Is_Reference"));
        if let Some(row) = row {
            assert_eq!(chemchron::rowtable::row_tint(row, &look), chemchron::plots::parse_hex(&colors[g]), "row of {g} not tinted in its group colour");
        }
    }
}

#[test]
fn save_figure_window_offers_size_and_vector_formats() {
    let mut h = harness(true);
    h.state_mut().part = Part::Combine;
    h.state_mut().combine.page = combine::Page::WeightedMean;
    h.run_steps(3);
    h.get_by_label("Save figure…").click();
    h.run_steps(3);
    assert!(h.state().fig_export.is_some());
    for l in ["PDF (vector)", "SVG (vector)", "EPS (vector)", "4 : 3", "Custom height"] {
        assert!(h.query_by_label(l).is_some(), "{l}");
    }
    h.get_by_label("EPS (vector)").click();
    h.run_steps(2);
    assert_eq!(h.state().settings.plot_export.format, "eps");
    h.get_by_label("Cancel").click();
    h.run_steps(2);
    assert!(h.state().fig_export.is_none());
}

#[test]
fn column_groups_collapse_and_labels_can_be_edited() {
    let mut h = harness(true);
    h.state_mut().part = Part::Epma;
    h.state_mut().epma.page = epma::Page::Geochronology;
    h.run_steps(3);
    assert!(h.query_by_label("- Chemical age").is_some());
    h.get_by_label("- Chemical age").click();
    h.run_steps(3);
    assert!(h.state().epma.geo_table.collapsed.contains("Chemical age"));
    // editing a label (what a double-click starts)
    let (i, rid) = {
        let a = h.state();
        let i = a.data.rows.iter().position(|r| !chemchron_core::row::truthy(r, "Is_Reference") && !chemchron_core::row::truthy(r, "No_EPMA")).unwrap();
        (i, chemchron_core::row::text(&a.data.rows[i], "Row_ID"))
    };
    h.state_mut().epma.geo_table.editing = Some((rid, "Label".into(), "Renamed_01".into()));
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(chemchron_core::row::text(&h.state().data.rows[i], "Label"), "Renamed_01");
}

#[test]
fn alternative_codes_identify_reference_materials_and_reference_files_join_them() {
    let mut nested: serde_json::Map<String, serde_json::Value> = serde_json::from_value(serde_json::json!({
        "Monazite": {"WC425": {"name": "Grain 44069", "age": 425.0, "age_2s": 3.0, "source": "A", "aliases": ["M44"]}}
    }))
    .unwrap();
    let refs: chemchron_core::reference::Refs = nested["Monazite"].as_object().unwrap().clone();
    assert_eq!(chemchron_core::reference::guess_reference_from_label("M44_03", &refs).as_deref(), Some("WC425"));
    assert_eq!(chemchron_core::reference::guess_reference_from_label("WC425-1", &refs).as_deref(), Some("WC425"));
    assert_eq!(chemchron_core::reference::guess_reference_from_label("M445_1", &refs), None);
    let dir = std::env::temp_dir().join(format!("cc-reference-files-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("M_44069.json");
    std::fs::write(&f, r#"{"Properties": {"Name": "M_44069", "Matrix": "Monazite", "Description": "Monazite 44069, natural standard", "Data Source": "Aleinikoff"},
        "Results": [{"Name": "Age", "Value": 425, "Uncertainty": 0, "Units": "Ma"}, {"Name": "206Pb/238U", "Value": 0.06811, "Uncertainty": 0.0002, "Units": "ratio"}, {"Name": "U", "Value": 2531, "Uncertainty": 0, "Units": "ppm"}]}"#).unwrap();
    let g = dir.join("G_NIST612.json");
    std::fs::write(&g, r#"{"Properties": {"Name": "G_NIST612", "Matrix": "Glass"}, "Results": [{"Name": "Ce", "Value": 38.4, "Uncertainty": 0.7, "Units": "ppm"}, {"Name": "CaO", "Value": 11.9, "Uncertainty": 0.1, "Units": "wt%"}]}"#).unwrap();
    let (added, merged, bad) = chemchron::refsync::import_reference_files(&mut nested, &[f, g]);
    assert!(bad.is_empty());
    assert_eq!(added, vec!["NIST612".to_string()]);
    assert_eq!(merged, vec!["WC425".to_string()], "M_44069 joins WC425, whose name is 'Grain 44069'");
    let wc = &nested["Monazite"]["WC425"];
    assert_eq!(wc["age"], 425.0, "kept");
    assert_eq!(wc["laser"]["values"]["206Pb/238U"]["value"], 0.06811);
    assert!(wc["aliases"].as_array().unwrap().iter().any(|a| a == "M_44069"));
    assert_eq!(nested["Glass"]["NIST612"]["laser"]["values"]["CaO"]["value"], 119000.0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn program_starts_empty_and_new_project_is_blank() {
    let mut h = harness(false);
    h.run_steps(3);
    assert!(h.state().laser.is_empty(), "no demo laser data at start");
    assert!(h.state().data.rows.is_empty());
    h.state_mut().laser.load_demo();
    h.state_mut().new_project();
    assert!(h.state().laser.is_empty() && h.state().data.rows.is_empty());
    h.state_mut().part = Part::Laser;
    h.run_steps(3);
}

#[test]
fn laser_results_are_sent_to_combined_results_from_the_export() {
    let mut h = harness(false);
    h.state_mut().laser.load_demo();
    h.state_mut().part = Part::Laser;
    h.state_mut().laser.page = chemchron_laser::app::Page::Export;
    h.run_steps(3);
    h.get_by_label("Send to Combined results").click();
    h.run_steps(3);
    assert_eq!(h.state().part, Part::Combine);
    assert_eq!(h.state().combine.page, combine::Page::Data);
    assert!(h.state().data.rows.iter().filter(|r| chemchron_core::upb::has_upb(r)).count() > 10);
}

#[test]
fn one_undo_takes_back_the_latest_change_in_any_part() {
    let mut h = harness(true);
    h.run_steps(2);
    // a change of the analyses, then a settings change
    let n = h.state().data.rows.len();
    let rid = chemchron_core::row::text(&h.state().data.rows[0], "Row_ID");
    h.state_mut().data.checkpoint("test delete");
    h.state_mut().data.rows.retain(|r| chemchron_core::row::text(r, "Row_ID") != rid);
    h.run_steps(2);
    let before = h.state().settings.rejection_sigma;
    h.state_mut().settings.rejection_sigma = before + 0.5;
    h.state_mut().save_settings();
    h.run_steps(2);
    // Ctrl+Z: first the settings, then the analyses
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run_steps(2);
    assert_eq!(h.state().settings.rejection_sigma, before);
    assert_eq!(h.state().data.rows.len(), n - 1);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run_steps(2);
    assert_eq!(h.state().data.rows.len(), n);
    h.state_mut().redo();
    assert_eq!(h.state().data.rows.len(), n - 1);
}

#[test]
fn summary_shows_group_composition_means() {
    let mut h = harness(true);
    let (rows, cols) = combine::composition_overview(h.state(), false, true);
    assert!(!rows.is_empty());
    assert!(cols.iter().any(|c| c == "ThO2_wt%") && cols.iter().any(|c| c == "ThO2_wt% 2SD"));
    let g = chemchron_core::row::text(&rows[0], "Flag");
    let members: Vec<f64> = h.state().data.rows.iter().filter(|r| !chemchron_core::row::truthy(r, "Is_Reference") && chemchron_core::row::flag(r, "Include_In_Average", true) && chemchron_core::row::text(r, "Flag") == g && chemchron_core::row::text(r, "Mineral") == chemchron_core::row::text(&rows[0], "Mineral")).filter_map(|r| chemchron_core::row::num(r, "ThO2_wt%")).collect();
    let mean = members.iter().sum::<f64>() / members.len() as f64;
    assert!((chemchron_core::row::num(&rows[0], "ThO2_wt%").unwrap() - mean).abs() < 0.001);
    h.state_mut().part = Part::Combine;
    h.state_mut().combine.page = combine::Page::Summary;
    h.state_mut().combine.summary_view = combine::SummaryView::Composition;
    h.run_steps(3);
}

#[test]
fn demo_brings_laser_and_epma_data_of_the_same_spots() {
    let mut h = harness(false);
    chemchron::demo::load(h.state_mut());
    h.run_steps(3);
    let app = h.state();
    assert!(!app.laser.is_empty());
    let a: Vec<&chemchron_core::row::Row> = app.data.rows.iter().filter(|r| chemchron_core::row::text(r, "Flag") == "Sample A").collect();
    assert!(a.len() >= 20, "Sample A analyses: {}", a.len());
    let ages: Vec<f64> = a.iter().filter_map(|r| chemchron_core::row::num(r, "Age_Ma")).collect();
    let mean = ages.iter().sum::<f64>() / ages.len() as f64;
    assert!((mean - 1850.0).abs() < 40.0, "chemical age of Sample A {mean}");
    assert!(a.iter().filter(|r| chemchron_core::upb::has_upb(r)).count() >= 20, "laser data linked to the EPMA rows");
    assert!(app.data.rows.iter().any(|r| chemchron_core::row::truthy(r, "Is_Reference")), "44069 recognised as reference");
    h.state_mut().part = Part::Combine;
    for p in COMBINE_PAGES {
        h.state_mut().combine.page = p;
        h.run_steps(2);
    }
}

#[test]
fn internal_standard_comes_from_epma_by_exact_label_and_group_mineral_is_sent() {
    let mut h = harness(false);
    chemchron::demo::load(h.state_mut());
    // the trace-element session of the demo
    chemchron::laser_sessions::switch_to(h.state_mut(), 0);
    {
        let app = h.state_mut();
        let te = &mut app.laser.session.settings.te;
        te.use_internal_standard = true;
        te.is_channel = Some("Ca43".into());
        te.is_as_oxide = true;
        te.is_oxide = String::new();
    }
    assert_eq!(h.state().laser.is_unit().1, "CaO wt%");
    let (msg, problem) = chemchron::link::internal_standard_from_epma(h.state_mut());
    assert!(!problem, "{msg}");
    let app = h.state();
    let s = app.laser.session.selections.iter().find(|s| s.label.starts_with("Sample A")).unwrap();
    let row = app.data.rows.iter().find(|r| chemchron_core::row::text(r, "Label") == s.label && !chemchron_core::row::truthy(r, "No_EPMA")).unwrap();
    let cao = chemchron_core::row::num(row, "CaO_wt%").unwrap();
    let expect = cao * chemchron_laser_core::oxides::ppm_per_wt_pct("CaO").unwrap();
    assert!((s.is_ppm.unwrap() - expect).abs() / expect < 0.01, "{} vs {expect}", s.is_ppm.unwrap());
    // a chosen group mineral reaches laser-only rows in Combined results
    let h2 = h.state_mut();
    let g = h2.laser.session.groups.iter().find(|g| g.name == "Sample B").unwrap().id;
    h2.laser.session.groups.iter_mut().find(|x| x.id == g).unwrap().mineral = Some("Xenotime".into());
    h2.data.rows.retain(|r| chemchron_core::row::text(r, "Flag") != "Sample B");
    chemchron::combine::link_laser_now(h2);
    assert!(h2.data.rows.iter().filter(|r| chemchron_core::row::text(r, "Label").starts_with("Sample B")).all(|r| chemchron_core::row::text(r, "Mineral") == "Xenotime"));
}

#[test]
fn a_new_sample_group_can_be_typed_in_the_menu() {
    let mut h = harness(true);
    h.state_mut().part = Part::Epma;
    h.state_mut().epma.page = epma::Page::Geochronology;
    h.run_steps(3);
    let rid = chemchron_core::row::text(h.state().data.rows.iter().find(|r| !chemchron_core::row::truthy(r, "Is_Reference")).unwrap(), "Row_ID");
    h.state_mut().epma.geo_table.selected.insert(rid.clone());
    h.run_steps(3);
    h.get_by_label("Sample group").click();
    h.run_steps(3);
    h.query_all_by_role(egui::accesskit::Role::TextInput).last().expect("the new-group field").click();
    h.run_steps(2);
    h.event(egui::Event::Text("Grp1".into()));
    h.run_steps(2);
    assert_eq!(h.state().epma.menu.new_group, "Grp1", "typing reaches the field (the menu stays open)");
    h.get_by_label("Assign new group").click();
    h.run_steps(3);
    let row = h.state().data.rows.iter().find(|r| chemchron_core::row::text(r, "Row_ID") == rid).unwrap();
    assert_eq!(chemchron_core::row::text(row, "Flag"), "Grp1");
}

#[test]
fn pasted_epma_data_is_imported() {
    let mut h = harness(false);
    h.state_mut().part = Part::Epma;
    h.state_mut().epma.page = epma::Page::Data;
    h.run_steps(3);
    h.get_by_label("Paste from clipboard…").click();
    h.run_steps(3);
    h.query_all_by_role(egui::accesskit::Role::MultilineTextInput).last().unwrap().click();
    h.run_steps(2);
    h.event(egui::Event::Paste("Label\tThO2\tUO2\tPbO\nA_1\t5.0\t0.5\t0.6\nA_2\t6.0\t0.4\t0.7\n".into()));
    h.run_steps(3);
    h.get_by_label("Import").click();
    h.run_steps(3);
    assert_eq!(h.state().data.rows.len(), 2, "{:?}", h.state().notice);
}

#[test]
fn tables_hide_groups_and_sort_on_two_keys() {
    let h = harness(true);
    let app = h.state();
    let rows = &app.data.rows;
    let subset: Vec<usize> = (0..rows.len()).filter(|i| !chemchron_core::row::truthy(&rows[*i], "Is_Reference")).collect();
    let cols = chemchron_core::columns::age_columns(false, false);
    let dp = app.settings.decimals;
    let mut st = chemchron::rowtable::TableState::default();
    let first = chemchron::rowtable::group_of(&rows[subset[0]]);
    st.hidden_groups.insert(first.clone());
    let vis = chemchron::rowtable::visible(rows, &subset, &st, &cols, &dp);
    assert!(!vis.is_empty() && vis.iter().all(|i| chemchron::rowtable::group_of(&rows[*i]) != first));
    st.hidden_groups.clear();
    st.sort = Some(("Flag".into(), true));
    st.sort2 = Some(("Age_Ma".into(), false));
    let vis = chemchron::rowtable::visible(rows, &subset, &st, &cols, &dp);
    for w in vis.windows(2) {
        let (a, b) = (&rows[w[0]], &rows[w[1]]);
        let (fa, fb) = (chemchron_core::row::text(a, "Flag"), chemchron_core::row::text(b, "Flag"));
        assert!(fa <= fb, "groups in order");
        if fa == fb {
            let (x, y) = (chemchron_core::row::num(a, "Age_Ma").unwrap_or(f64::NAN), chemchron_core::row::num(b, "Age_Ma").unwrap_or(f64::NAN));
            if x.is_finite() && y.is_finite() {
                assert!(x >= y, "ages descending within a group");
            }
        }
    }
    st.hide_left_out = true;
    let vis = chemchron::rowtable::visible(rows, &subset, &st, &cols, &dp);
    assert!(vis.iter().all(|i| chemchron_core::row::flag(&rows[*i], "Include_In_Average", true)));
}

#[test]
fn a_group_colour_can_be_typed_as_hex_code() {
    let mut h = harness(true);
    h.state_mut().part = Part::Settings;
    h.state_mut().global.page = chemchron::global::Page::General;
    h.run_steps(3);
    let group = {
        let mut f: Vec<String> = h.state().data.rows.iter().map(|r| chemchron_core::row::text(r, "Flag")).filter(|f| !f.is_empty()).collect();
        f.sort();
        f[0].clone()
    };
    // the first hex field holds the first group's colour
    let field = h.query_all_by_role(egui::accesskit::Role::TextInput).find(|n| n.value().is_some_and(|v| v.starts_with('#'))).expect("a hex field");
    field.click();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.event(egui::Event::Text("#1F77B4".into()));
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(h.state().settings.flag_colors.get(&group).map(|s| s.to_lowercase()), Some("#1f77b4".to_string()));
}

#[test]
fn uncertainty_models_look_alike_and_hold_the_log_entries() {
    let mut h = harness(true);
    h.state_mut().part = Part::Settings;
    h.state_mut().global.page = chemchron::global::Page::Uncertainty;
    for upb in [false, true] {
        h.state_mut().global.upb_uncertainty = upb;
        h.run_steps(3);
        assert!(h.query_by_label(if upb { "Uncertainty model (U–Pb ages)" } else { "Uncertainty model (chemical ages)" }).is_some());
        for title in ["Systematic"] {
            assert!(h.query_all_by_label_contains(title).count() > 0, "{title}");
        }
        assert!(h.query_all_by_label_contains("Long-term log entries").count() > 0, "log entries in the lower part");
        assert!(h.query_all_by_label_contains("In use for").count() > 0);
    }
    // Combined results > Long-term logs no longer edits entries
    h.state_mut().part = Part::Combine;
    h.state_mut().combine.page = combine::Page::LongTerm;
    h.run_steps(3);
    assert!(h.query_all_by_label_contains("Long-term log entries").count() == 0);
}

#[test]
fn long_term_log_entries_can_be_filtered() {
    let mut h = harness(false);
    {
        let app = h.state_mut();
        app.settings.long_term_log.clear();
        for (r, m, s) in [("WC425", "Monazite", "S1"), ("WC425", "Monazite", "S2"), ("DEMO-X320", "Xenotime", "S1")] {
            let mut e = chemchron_core::row::Row::new();
            for (k, v) in [("Reference", r), ("Mineral", m), ("SessionTag", s), ("Label", "x")] {
                chemchron_core::row::set(&mut e, k, chemchron_core::row::jstr(v));
            }
            app.settings.long_term_log.push(e);
        }
        app.part = Part::Settings;
        app.global.page = chemchron::global::Page::Uncertainty;
        app.global.upb_uncertainty = false;
    }
    h.run_steps(3);
    assert!(h.query_by_label("3 of 3 entries shown").is_some());
    h.state_mut().combine.lt_filter.reference = "WC425".into();
    h.run_steps(2);
    assert!(h.query_by_label("2 of 3 entries shown").is_some());
    h.state_mut().combine.lt_filter.session = "S2".into();
    h.run_steps(2);
    assert!(h.query_by_label("1 of 3 entries shown").is_some());
    h.state_mut().combine.lt_filter = Default::default();
    h.state_mut().combine.lt_filter.mineral = "Xenotime".into();
    h.run_steps(2);
    assert!(h.query_by_label("1 of 3 entries shown").is_some());
}

#[test]
fn every_figure_is_exported_into_one_folder() {
    let mut h = harness(true);
    h.state_mut().part = Part::Combine;
    h.run_steps(2);
    let figs = chemchron::plot_settings::all_figures(h.state_mut());
    let names: Vec<&str> = figs.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.iter().any(|n| n.starts_with("isochron_")), "{names:?}");
    assert!(names.iter().any(|n| n.starts_with("weighted_mean_chemical_")), "{names:?}");
    assert!(names.contains(&"ree_diagram") && names.contains(&"age_distribution"), "{names:?}");
    let groups = names.iter().filter(|n| n.starts_with("isochron_")).count();
    assert!(groups >= 2, "one isochron per group: {names:?}");
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/figure-check/all");
    let _ = std::fs::remove_dir_all(&out);
    h.state_mut().settings.plot_export.format = "svg".into();
    let m = chemchron::plot_settings::export_all(h.state_mut(), &out).unwrap();
    let n = std::fs::read_dir(&out).unwrap().count();
    assert_eq!(n, figs.len(), "{m}");
    // the page the user was on stays
    assert_eq!(h.state().combine.page, combine::Page::Data);
}

#[test]
fn plot_choices_are_kept_in_the_project() {
    let mut h = harness(true);
    h.state_mut().part = Part::Combine;
    h.state_mut().combine.page = combine::Page::AgeDist;
    h.run_steps(3);
    {
        let a = h.state_mut();
        a.combine.dist_kind = 2;
        a.combine.dist_bw = 12.5;
        a.combine.conc_wetherill = true;
        a.combine.comp_x = "CaO_wt%".into();
        a.combine.ree_sel.truncate(1);
    }
    let ree_sel = h.state().combine.ree_sel.clone();
    let iso_group = h.state().epma.iso_group.clone();
    let v = h.state().project_value();
    let p = std::env::temp_dir().join(format!("chemchron-plots-{}.chemchron.json", std::process::id()));
    std::fs::write(&p, serde_json::to_string(&v).unwrap()).unwrap();
    h.state_mut().open_project(&p);
    let a = h.state();
    assert_eq!(a.combine.dist_kind, 2);
    assert_eq!(a.combine.dist_bw, 12.5);
    assert!(a.combine.conc_wetherill);
    assert_eq!(a.combine.comp_x, "CaO_wt%");
    assert_eq!(a.combine.ree_sel, ree_sel);
    assert_eq!(a.epma.iso_group, iso_group);
    let _ = std::fs::remove_file(p);
}

#[test]
fn an_autosaved_copy_is_offered_and_recovered() {
    let mut h = harness(true);
    let n = h.state().data.rows.len();
    h.state_mut().data.unsaved = true;
    let file = chemchron::autosave::write_now(h.state_mut()).unwrap();
    assert!(file.exists());
    // a later start finds it
    let found = chemchron::autosave::leftovers(&chemchron::autosave::dir(h.state()), None);
    let (f, when, name) = found.iter().find(|x| x.0 == file).cloned().unwrap();
    assert!(!when.is_empty() && name.contains("demo"), "{name}");
    h.state_mut().new_project();
    assert_eq!(h.state().data.rows.len(), 0);
    chemchron::autosave::recover(h.state_mut(), &f);
    assert_eq!(h.state().data.rows.len(), n);
    assert!(h.state().data.unsaved, "a recovered project still needs saving");
    assert!(!f.exists(), "the copy is used up");
    assert!(h.state().project_path.as_ref().is_some_and(|p| p.to_string_lossy().contains("demo_project")));
}

#[test]
fn age_distribution_page_draws_groups_and_both_age_kinds() {
    let mut h = harness(true);
    h.state_mut().part = Part::Combine;
    h.state_mut().combine.page = combine::Page::AgeDist;
    h.run_steps(3);
    h.get_by_label("Both").click();
    h.run_steps(3);
    assert_eq!(h.state().combine.dist_kind, 2);
    h.state_mut().collect = Some(Vec::new());
    h.run_steps(1);
    let figs = h.state_mut().collect.take().unwrap();
    let (_, fig) = figs.iter().find(|(n, _)| n == "age_distribution").unwrap();
    assert!(fig.legend.len() >= 2, "{:?}", fig.legend);
}

#[test]
fn a_project_holds_several_laser_sessions() {
    let Some(data) = chemchron_laser_core::testdata::session_folder("LAICPMS_monazite_session") else {
        return;
    };
    let mut h = harness(false);
    h.state_mut().settings.laser_in_project = true;
    h.state_mut().laser.open_blocking(std::slice::from_ref(&data));
    h.state_mut().part = Part::Laser;
    h.run_steps(3);
    let first = h.state().laser.session.name.clone();
    let n_first = h.state().laser.session.selections.len();
    h.get_by_label("Add another session").click();
    h.run_steps(2);
    assert!(h.state().laser.is_empty());
    assert_eq!(h.state().laser_waiting.len(), 1);
    // a second session (here the same files again, renamed)
    h.state_mut().laser.open_blocking(std::slice::from_ref(&data));
    h.state_mut().laser.session.name = "second day".into();
    h.state_mut().laser.session.selections.truncate(3);
    h.run_steps(2);
    chemchron::laser_sessions::switch_to(h.state_mut(), 0);
    assert_eq!(h.state().laser.session.name, first);
    assert_eq!(h.state().laser.session.selections.len(), n_first);
    assert_eq!(h.state().laser_waiting[0].session.name, "second day");
    // both are saved with the project and come back
    let v = h.state().project_value();
    assert_eq!(v["laser_sessions"].as_array().unwrap().len(), 1);
    let p = std::env::temp_dir().join(format!("chemchron-sessions-{}.chemchron.json", std::process::id()));
    std::fs::write(&p, serde_json::to_string(&v).unwrap()).unwrap();
    h.state_mut().new_project();
    assert!(h.state().laser_waiting.is_empty());
    h.state_mut().open_project(&p);
    assert_eq!(h.state().laser.session.name, first);
    assert_eq!(h.state().laser_waiting.len(), 1);
    assert_eq!(h.state().laser_waiting[0].session.selections.len(), 3);
    chemchron::laser_sessions::remove_open(h.state_mut());
    assert_eq!(h.state().laser.session.name, "second day");
    assert!(h.state().laser_waiting.is_empty());
    let _ = std::fs::remove_file(p);
}

#[test]
fn the_screenshot_tour_visits_every_page_once() {
    let plan = chemchron::tour::plan();
    let mut names: Vec<&String> = plan.iter().map(|(n, _)| n).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), plan.len(), "picture names are unique");
    assert!(plan.len() >= 30, "{}", plan.len());
    let mut h = harness(true);
    for (_, s) in &plan {
        chemchron::tour::apply(h.state_mut(), *s, false);
        h.run_steps(2);
    }
    // the tour itself steps on even without pictures (here none can be taken)
    chemchron::tour::start(h.state_mut(), std::env::temp_dir().join("chemchron-tour-test"));
    assert_eq!(h.state().tour.as_ref().unwrap().steps.len(), 2 * plan.len());
}

#[test]
fn a_sample_group_has_one_colour_in_laser_and_epma() {
    let mut h = harness(false);
    chemchron::demo::load(h.state_mut());
    h.run_steps(3);
    let lab = |h: &Harness<'_, ChemApp>, n: &str| h.state().settings.flag_colors.get(n).cloned();
    let laser = |h: &Harness<'_, ChemApp>, n: &str| {
        let g = h.state().laser.session.groups.iter().find(|g| g.name == n).unwrap();
        chemchron::plots::to_hex(egui::Color32::from_rgb(g.color[0], g.color[1], g.color[2]))
    };
    for n in ["Sample A", "Sample B"] {
        assert_eq!(lab(&h, n), Some(laser(&h, n)), "{n}");
    }
    // a colour chosen on the Signal page becomes the lab's colour
    let id = h.state().laser.session.groups.iter().find(|g| g.name == "Sample A").unwrap().id;
    h.state_mut().laser.session.groups.iter_mut().find(|g| g.id == id).unwrap().color = [10, 200, 30];
    h.run_steps(2);
    assert_eq!(lab(&h, "Sample A").as_deref(), Some("#0ac81e"));
    // and a colour set in Settings reaches the laser group
    h.state_mut().settings.flag_colors.insert("Sample B".into(), "#123456".into());
    h.run_steps(2);
    assert_eq!(laser(&h, "Sample B"), "#123456");
}

#[test]
fn uncertainty_page_parts_stay_in_place() {
    let mut h = harness(true);
    h.state_mut().part = Part::Settings;
    h.state_mut().global.page = chemchron::global::Page::Uncertainty;
    h.run_steps(3);
    let top = |h: &Harness<'_, ChemApp>| h.get_by_label_contains("Long-term log entries").rect().top();
    let first = top(&h);
    h.run_steps(20);
    assert_eq!(top(&h), first, "the log entries don't creep up");
    let model = h.get_by_label("Uncertainty model (chemical ages)").rect();
    assert!(first > model.bottom() + 200.0, "the model keeps its room: {first} vs {}", model.bottom());
}

#[test]
fn chemical_weighted_mean_uses_rows_marked_in_combined_results() {
    let mut h = harness(true);
    let ids: std::collections::BTreeSet<String> = {
        let a = h.state();
        (0..a.data.rows.len())
            .filter(|i| !chemchron_core::row::truthy(&a.data.rows[*i], "Is_Reference") && chemchron_core::row::num(&a.data.rows[*i], "Age_Ma").is_some())
            .take(4)
            .map(|i| chemchron_core::row::text(&a.data.rows[i], "Row_ID"))
            .collect()
    };
    {
        let a = h.state_mut();
        a.part = Part::Combine;
        a.combine.page = combine::Page::WeightedMean;
        a.combine.wm_upb = false;
        a.combine.table.selected = ids.clone();
        a.epma.wm_group = epma::GroupPick::Selection;
        a.epma.geo_table.selected.clear();
    }
    h.run_steps(2);
    h.state_mut().collect = Some(Vec::new());
    h.run_steps(1);
    let figs = h.state_mut().collect.take().unwrap();
    let (_, fig) = figs.iter().find(|(n, _)| n == "weighted_mean_chemical").expect("a chemical weighted mean");
    assert!(fig.title.contains("/4)"), "the 4 marked rows are plotted: {}", fig.title);
    // the same choice for the chemical isochron
    h.state_mut().epma.iso_group = epma::GroupPick::Selection;
    h.state_mut().combine.page = combine::Page::Isochron;
    h.run_steps(2);
    assert!(h.query_by_label_contains("No analyses").is_none());
}

#[test]
fn combined_concordia_has_reset_view() {
    let mut h = harness(true);
    h.state_mut().part = Part::Combine;
    h.state_mut().combine.page = combine::Page::Concordia;
    h.run_steps(3);
    h.state_mut().combine.conc_view.window = Some(((0.0, 1.0), (0.0, 1.0)));
    h.run_steps(1);
    h.get_by_label("Reset view").click();
    h.run_steps(2);
    assert!(h.state().combine.conc_view.window.is_none());
    // another diagram starts from the whole view again
    h.state_mut().combine.conc_view.window = Some(((0.0, 1.0), (0.0, 1.0)));
    h.run_steps(1);
    h.get_by_label("Wetherill").click();
    h.run_steps(2);
    assert!(h.state().combine.conc_view.window.is_none());
}

#[test]
fn the_upb_excess_setting_reaches_the_laser_part() {
    let mut h = harness(false);
    chemchron::demo::load(h.state_mut());
    h.run_steps(2);
    {
        let m = h.state().laser.red.upb.as_ref().unwrap();
        assert!(!m.ratio(chemchron_laser_core::drs::upb::RatioKind::R68).unwrap().manual, "from the reference materials by default");
    }
    // manual values in Settings > Uncertainty models > U–Pb
    {
        let a = h.state_mut();
        let mut s = a.settings.upb.by_mineral().remove("Monazite").unwrap();
        s.excess_source = "manual".into();
        s.excess_manual_pct.insert("r86".into(), 2.0);
        s.excess_manual_pct.insert("r76".into(), 1.0);
        a.settings.upb.uncertainty_by_mineral.insert("Monazite".into(), s);
    }
    h.run_steps(2);
    let m = h.state().laser.red.upb.as_ref().unwrap();
    let r68 = m.ratio(chemchron_laser_core::drs::upb::RatioKind::R68).unwrap();
    assert!(r68.manual && (r68.excess - 0.01).abs() < 1e-12, "{}", r68.excess);
    let r76 = m.ratio(chemchron_laser_core::drs::upb::RatioKind::R76).unwrap();
    assert!((r76.excess - 0.005).abs() < 1e-12);
    // every U–Pb spot in Combined results has a total 2σ
    chemchron::combine::link_laser_now(h.state_mut());
    h.run_steps(1);
    // Combined results gets the analytical 2σ and the excess apart; together
    // they give the laser part's analytical + excess
    {
        use chemchron_core::row::{num, text, truthy};
        let a = h.state();
        let mut checked = 0;
        for r in a.data.rows.iter().filter(|r| truthy(r, "UPb_Laser_Excess")).take(5) {
            let label = text(r, "Label");
            let Some(i) = a.laser.session.selections.iter().position(|s| chemchron::link::spot_label(&s.label) == label) else { continue };
            let u = a.laser.red.selections[i].upb.as_ref().unwrap();
            let (r86, an, pt) = (num(r, "UPb_r86").unwrap(), num(r, "UPb_r86_2s").unwrap(), num(r, "UPb_r86_2s_point").unwrap());
            assert!(an < pt, "analytical below analytical + excess");
            let rel_laser = u.r68.prop2s / u.r68.mean;
            assert!((pt / r86 - rel_laser).abs() < 1e-6, "{} vs {}", pt / r86, rel_laser);
            assert!((an / r86 - u.r68.int2se / u.r68.mean).abs() < 1e-6, "analytical is the laser's analytical");
            checked += 1;
        }
        assert!(checked > 0);
    }
    let with_total = h.state().data.rows.iter().filter(|r| chemchron_core::row::num(r, "UPb_t68_2s_total_Ma").is_some()).count();
    assert!(with_total > 10, "{with_total}");
}

#[test]
fn laser_spots_keep_their_laser_group_and_the_session_excess_shows_at_once() {
    let Some(data) = chemchron_laser_core::testdata::session_folder("LAICPMS_monazite_session") else {
        return;
    };
    let mut h = harness(false);
    h.state_mut().laser.open_blocking(std::slice::from_ref(&data));
    h.run_steps(3);
    // the settings page shows the session's reference materials before anything is sent
    h.state_mut().part = Part::Settings;
    h.state_mut().global.page = chemchron::global::Page::Uncertainty;
    h.state_mut().global.upb_uncertainty = true;
    h.run_steps(3);
    assert!(h.query_by_label_contains("1. LA-ICP-MS part: open session").is_some());
    {
        let m = h.state().laser.red.upb.as_ref().unwrap();
        let r68 = m.ratio(chemchron_laser_core::drs::upb::RatioKind::R68).unwrap();
        let p = r68.materials.iter().find(|m| m.primary).unwrap();
        assert!(p.mswd < 10.0 && p.excess_2s_pct < 10.0, "primary {:?}", p);
        assert!(r68.materials.len() >= 2, "secondaries listed too");
    }
    // a spot of a sample group whose label looks like a reference code stays a sample
    chemchron::combine::link_laser_now(h.state_mut());
    let a = h.state();
    let m44 = a.data.rows.iter().find(|r| chemchron_core::row::text(r, "Label") == "m44");
    if let Some(r) = m44 {
        assert!(!chemchron_core::row::truthy(r, "Is_Reference"), "m44 is in a sample group of the laser session");
    }
    let wc: Vec<&chemchron_core::row::Row> = a.data.rows.iter().filter(|r| chemchron_core::row::text(r, "Reference") == "WC425").collect();
    assert_eq!(wc.len(), 20, "only the 44069 group's spots are WC425");
}

#[test]
fn the_three_uncertainty_levels_agree_between_laser_part_and_combined_results() {
    let mut h = harness(false);
    chemchron::demo::load(h.state_mut());
    // ages shown without decimals (as in many labs): totals must still come from full values
    h.state_mut().settings.decimals.age = 0;
    h.run_steps(2);
    chemchron::combine::link_laser_now(h.state_mut());
    h.run_steps(1);
    use chemchron_core::row::{num, text, truthy};
    let a = h.state();
    let mut checked = 0;
    for r in a.data.rows.iter().filter(|r| truthy(r, "UPb_Laser_Excess")) {
        let label = text(r, "Label");
        let Some(i) = a.laser.session.selections.iter().position(|s| chemchron::link::spot_label(&s.label) == label) else { continue };
        let Some(u) = a.laser.red.selections[i].upb.as_ref() else { continue };
        let total = u.a68.rminc2s.hypot(a.laser.sys_pct(0) / 100.0 * u.a68.age);
        for (field, laser) in [("UPb_t68_2s_Ma", u.a68.int2s), ("UPb_t68_2s_point_Ma", u.a68.prop2s), ("UPb_t68_2s_total_Ma", total)] {
            // stored at full precision, the same as the laser part ...
            let comb = num(r, field).unwrap();
            assert!((comb - laser).abs() < 1e-6 * laser.max(1.0), "{label} {field}: {comb} vs laser {laser}");
            // ... and rounded only when shown
            let shown = chemchron::rowtable::cell_text(r, field, &a.settings.decimals, true);
            assert_eq!(shown, format!("{:.0}", laser), "{label} {field}");
        }
        checked += 1;
    }
    assert!(checked > 10, "{checked}");
}

#[test]
fn values_keep_every_decimal_and_are_rounded_only_when_shown() {
    let mut h = harness(true);
    h.state_mut().settings.decimals.age = 0;
    h.state_mut().reprocess("test");
    h.run_steps(1);
    let a = h.state();
    let r = a.data.rows.iter().find(|r| chemchron_core::row::num(r, "Age_Ma").is_some()).unwrap();
    let age = chemchron_core::row::num(r, "Age_Ma").unwrap();
    assert!(age.fract() != 0.0, "stored with decimals: {age}");
    assert_eq!(chemchron::rowtable::cell_text(r, "Age_Ma", &a.settings.decimals, true), format!("{age:.0}"), "shown rounded");
    let p = std::env::temp_dir().join(format!("chemchron-full-{}.csv", std::process::id()));
    chemchron::export::write_csv(a, &p).unwrap();
    let text = std::fs::read_to_string(&p).unwrap();
    assert!(text.contains(&age.to_string()), "the export carries every decimal of {age}");
    let _ = std::fs::remove_file(p);
}

#[test]
fn the_demo_has_a_upb_and_a_trace_element_session_linked_to_epma() {
    use chemchron_core::row::{num, text, truthy};
    use chemchron_laser_core::drs::upb::RatioKind;
    let mut h = harness(false);
    chemchron::demo::load(h.state_mut());
    h.run_steps(2);
    let a = h.state();
    // the U–Pb session is open, the trace-element session waits in the project
    assert!(a.laser.red.upb.is_some() && a.laser.red.te.is_none(), "U–Pb only");
    assert_eq!(a.laser_waiting.len(), 1);
    // two verified secondaries next to the primary, both used for the excess
    let m = a.laser.red.upb.as_ref().unwrap();
    let r68 = m.ratio(RatioKind::R68).unwrap();
    for n in ["MBA", "MDI"] {
        let st = r68.materials.iter().find(|x| x.name == n).unwrap_or_else(|| panic!("{n} in the session"));
        assert!(st.left_out.is_none(), "{n} used");
        assert!(a.laser.session.rms.iter().any(|r| r.name == n && r.confirmed), "{n} verified");
    }
    // their ratios come back within 1 % of the accepted values (Quality page)
    for n in ["MBA", "MDI"] {
        let rows: Vec<_> = a.laser.red.qa.rows.iter().filter(|q| q.group == n && q.quantity.contains('/')).collect();
        assert!(!rows.is_empty(), "{n} checked");
        for q in rows {
            assert!(q.diff_pct.abs() < 1.0, "{n} {}: {:+.2} %", q.quantity, q.diff_pct);
        }
    }
    // the same EPMA analysis has the U–Pb and the trace-element results
    let r = a.data.rows.iter().find(|r| text(r, "Label") == "Sample A_1" && !truthy(r, "No_EPMA")).unwrap();
    assert!(num(r, "UPb_t68_Ma").is_some(), "U–Pb linked");
    assert!(r.keys().any(|k| k.starts_with("TE_") || k.ends_with("_ppm")), "trace elements linked: {:?}", r.keys().collect::<Vec<_>>());
    // the reference materials are recognised in EPMA too
    for code in ["WC425", "MB508", "MI506"] {
        assert!(a.data.rows.iter().any(|r| truthy(r, "Is_Reference") && text(r, "Reference") == code && !truthy(r, "No_EPMA")), "{code}");
    }
    // the trace-element session: NIST610 primary, NIST612 secondary
    let te = &a.laser_waiting[0].session;
    assert!(te.groups.iter().any(|g| g.name == "NIST612"));
    assert!(te.settings.te.enabled && !te.settings.upb.enabled);
}
