// SPDX-License-Identifier: GPL-3.0-or-later
//! Regression test: the EPMA and Combined calculations on the synthetic demo
//! files, compared with stored reference results (`tests/golden/`).

use chemchron_core::concordia::{concordia_age, discordia, Pt};
use chemchron_core::constants::*;
use chemchron_core::epma::process_csv_file;
use chemchron_core::isochron::compute_isochron;
use chemchron_core::overview::{flag_overview, reference_overview, UpbOptions};
use chemchron_core::pipeline;
use chemchron_core::processing::{Context, PerMineral};
use chemchron_core::row::*;
use chemchron_core::settings::Settings;
use chemchron_core::stats::weighted_mean_for_rows;
use chemchron_core::upbstats::{concordia_data, isotopic_weighted_mean, UpbMineralUncertainty};
use serde_json::Value;
use std::path::{Path, PathBuf};

fn golden(name: &str) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(name);
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn epma_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../testdata/epma")
}

/// Compares two values; numbers to a relative tolerance. Many stored values
/// are rounded to the shown decimals while the program keeps full precision:
/// a value also agrees when it matches to the decimals stored (within one
/// unit of the last digit, as intermediate steps were rounded there too).
fn same(a: &Value, b: &Value, tol: f64) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let shown = x.to_string();
            let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
            if (x - y).abs() <= tol * x.abs().max(y.abs()).max(1e-12) || (x - y).abs() < 1e-12 {
                return true;
            }
            let k = shown.split_once('.').map(|(_, f)| f.trim_end_matches(|c: char| !c.is_ascii_digit()).len()).unwrap_or(0);
            k <= 6 && !shown.contains('e') && (x - y).abs() <= 10f64.powi(-(k as i32)) * (1.0 + 1e-9)
        }
        (Value::Null, Value::String(s)) | (Value::String(s), Value::Null) => s.is_empty(),
        _ => a == b,
    }
}

fn compare_rows(py: &[Value], rs: &[Row], what: &str, skip: &[&str]) -> usize {
    assert_eq!(py.len(), rs.len(), "{what}: row count");
    let mut bad = 0;
    let mut compared = 0;
    for (i, (p, r)) in py.iter().zip(rs).enumerate() {
        let p = p.as_object().unwrap();
        for (k, pv) in p {
            if skip.contains(&k.as_str()) {
                continue;
            }
            let rv = r.get(k).cloned().unwrap_or(Value::Null);
            compared += 1;
            // the excess is estimated from unrounded ages (the stored results used rounded
            // ones): it and the uncertainties built on it agree to 0.1 %
            let tol = if matches!(k.as_str(), "Excess_2s_rel_pct" | "Age_2s_Ma_point" | "Age_Rel_2s_pct_point" | "Age_2s_Ma_total" | "Age_Rel_2s_pct_total") { 1e-3 } else { 1e-9 };
            if !same(pv, &rv, tol) {
                if bad < 25 {
                    eprintln!("{what} row {i} ({}) field {k}: stored {pv} now {rv}", text(r, "Label"));
                }
                bad += 1;
            }
        }
    }
    eprintln!("{what}: {compared} values compared");
    assert!(py.is_empty() || compared > py.len() * 5, "{what}: too few values compared");
    bad
}

fn refs() -> chemchron_core::reference::Refs {
    golden("refs.json").as_object().unwrap().clone()
}

#[test]
fn imports_match_the_reference_results() {
    let g = golden("imports.json");
    let refs = refs();
    let decay = DecayConstants::steiger_jaeger_1977();
    for (name, path, lt, ex) in [
        ("demo_epma_S1.csv", epma_dir().join("examples/demo_epma_S1.csv"), PerMineral::None, PerMineral::None),
        ("demo_epma_S2.csv", epma_dir().join("examples/demo_epma_S2.csv"), PerMineral::None, PerMineral::None),
        ("epma_export_two_points.csv", epma_dir().join("tests/data/epma_export_two_points.csv"), PerMineral::All(2.0), PerMineral::All(1.5)),
    ] {
        let ctx = |act: &[String]| Context {
            decay: &decay,
            refs: &refs,
            active: act.to_vec(),
            long_term: lt.clone(),
            excess: ex.clone(),
            dp: DecimalPlaces::default(),
            thresholds: MineralIdThresholds::default(),
        };
        let imp = process_csv_file(&path, &ctx).unwrap();
        let p = &g[name];
        let active: Vec<String> = p["active"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect();
        assert_eq!(active, imp.active, "{name} active oxides");
        let skipped: Vec<String> = p["skipped"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect();
        assert_eq!(skipped, imp.skipped, "{name} messages");
        let bad = compare_rows(p["rows"].as_array().unwrap(), &imp.results, name, &["Row_ID"]);
        assert_eq!(bad, 0, "{name}: {bad} fields differ");
    }
}

fn demo_settings() -> Settings {
    let mut s = Settings { dir: std::env::temp_dir().join("chemchron-test-none"), ..Default::default() };
    // the stored results were calculated with ²³⁸U/²³⁵U = 137.88
    s.decay = DecayConstants::steiger_jaeger_1977();
    let mut nested = serde_json::Map::new();
    nested.insert("Monazite".into(), Value::Object(refs()));
    s.refs_nested = nested;
    s.upb.uncertainty_by_mineral.insert("Monazite".into(), UpbMineralUncertainty { excess_source: "project".into(), ..Default::default() });
    s
}

#[test]
fn demo_project_matches_the_reference_results() {
    let s = demo_settings();
    let proj = chemchron_core::settings::load_project(&epma_dir().join("examples/demo_project.chemchron.json")).unwrap();
    let mut rows = pipeline::reprocess_all(&proj.rows, &proj.active_oxides, &s);
    let models = pipeline::refresh(&mut rows, &s);
    let g = golden("demo_rows.json");
    // The U–Pb excess rule differs from the one of the stored results (reference
    // spots are not removed as discordant, the 207Pb/206Pb of materials younger
    // than 1000 Ma is left out): the fields built on that excess are not
    // compared; everything else is.
    let skip = [
        "UPb_PointKey", "UPb_Ref_Accepted_Ma", "UPb_Ref_Accepted_2s_Ma", "UPb_Ref_Diff_Ma", "UPb_Ref_Diff_pct", "UPb_Ref_Within_2s",
        "UPb_r86_2s_point", "UPb_r76_2s_point", "UPb_rho_TW_point", "UPb_r82_2s_point", "UPb_ConcDist_point_sigma", "UPb_Excess_2s_pct",
        "UPb_t68_2s_point_Ma", "UPb_t75_2s_point_Ma", "UPb_t76_2s_point_Ma", "UPb_t82_2s_point_Ma",
    ];
    let bad = compare_rows(g["rows"].as_array().unwrap(), &rows, "demo", &skip);
    assert_eq!(bad, 0, "demo project: {bad} fields differ");

    let sm = golden("demo_summary.json");
    for (m, v) in sm["model"].as_object().unwrap() {
        let r = &models.chem[m];
        // from unrounded ages (the stored results: rounded ones), so to 0.1 %
        let e = v["excess_pct"].as_f64().unwrap();
        assert!((r.excess_pct - e).abs() <= 1e-3 * e.abs().max(1e-9), "{m} excess {} vs {e}", r.excess_pct);
        assert!((r.systematic_pct - v["systematic_pct"].as_f64().unwrap()).abs() < 1e-9, "{m} systematic");
    }
    let opts = UpbOptions { all_age_types: true, ..pipeline::upb_options(&s, true) };
    let refs = s.refs();
    let ro = reference_overview(&rows, &refs, &s.decimals, true, 2.0, Some(&opts));
    let fo = flag_overview(&rows, &s.decimals, false, 2.0, Some(&opts));
    // U–Pb ages are compared with the material's one reference age; the stored
    // results had separate U–Pb accepted ages (blank in the demo), so these differ.
    let mut upb_ref: Vec<String> = ["UPb_Accepted_Ma", "UPb_Accepted_2s_Ma", "UPb_Diff_Ma", "UPb_Diff_pct", "UPb_Within_2s"].iter().map(|s| s.to_string()).collect();
    for t in ["t68", "t75", "t76", "t82"] {
        for k in ["Accepted_Ma", "Accepted_2s_Ma", "Diff_Ma", "Diff_pct", "Within_2s"] {
            upb_ref.push(format!("UPbSum_{t}_{k}"));
        }
    }
    // ... and the means built on the 207Pb/206Pb excess (young materials left
    // out of it, reference spots not removed as discordant)
    for t in ["t75", "t76"] {
        for k in ["WM_Ma", "WM_2s_Ma", "MSWD"] {
            upb_ref.push(format!("UPbSum_{t}_{k}"));
        }
    }
    let upb_ref: Vec<&str> = upb_ref.iter().map(|s| s.as_str()).collect();
    assert_eq!(compare_rows(sm["refs"].as_array().unwrap(), &ro, "ref overview", &upb_ref), 0);
    let pb_pb: Vec<String> = ["t75", "t76"].iter().flat_map(|t| ["WM_Ma", "WM_2s_Ma", "MSWD"].map(|k| format!("UPbSum_{t}_{k}"))).collect();
    let pb_pb: Vec<&str> = pb_pb.iter().map(|s| s.as_str()).collect();
    assert_eq!(compare_rows(sm["flags"].as_array().unwrap(), &fo, "flag overview", &pb_pb), 0);

    let crit = s.upb.criteria();
    for (flag, e) in sm["groups"].as_object().unwrap() {
        let grp: Vec<&Row> = rows.iter().filter(|r| text(r, "Flag") == *flag && flag_ok(r)).collect();
        let chem: Vec<&Row> = grp.iter().copied().filter(|r| !truthy(r, "No_EPMA")).collect();
        for form in ["Th", "U"] {
            for rej in [false, true] {
                let iso = compute_isochron(&chem, &s.decay, form == "Th", rej, 2.0);
                let p = &e[format!("iso_{form}_{}", if rej { "True" } else { "False" })];
                match iso {
                    None => assert!(p.is_null(), "{flag} isochron {form}"),
                    Some(i) => {
                        // (the stored results rounded the oxides and the excess; the program keeps every decimal)
                        let close = |a: f64, k: &str| (a - p[k].as_f64().unwrap()).abs() <= 1e-5 * a.abs().max(1.0);
                        // uncertainties and MSWD rest on the excess (estimated from unrounded ages now)
                        let close_u = |a: f64, k: &str| (a - p[k].as_f64().unwrap()).abs() <= 1e-3 * a.abs().max(1.0);
                        assert_eq!(i.n, p["n"].as_u64().unwrap() as usize, "{flag} {form} {rej} n");
                        assert!(close(i.slope, "slope"), "{flag} {form} slope");
                        assert!(close(i.age_ma.unwrap(), "age_Ma"), "{flag} {form} age {:?} vs {}", i.age_ma, p["age_Ma"]);
                        assert!(close_u(i.age_2s_ma.unwrap(), "age_2s_Ma"), "{flag} {form} 2s {:?} vs {}", i.age_2s_ma, p["age_2s_Ma"]);
                        assert!(close_u(i.mswd.unwrap(), "mswd"), "{flag} {form} mswd");
                        assert_eq!(i.age_2s_method, p["age_2s_method"].as_str().unwrap());
                    }
                }
            }
        }
        for rej in [false, true] {
            let w = weighted_mean_for_rows(&grp, rej, 2.0);
            let p = &e[format!("wm_{}", if rej { "True" } else { "False" })];
            match w {
                None => assert!(p.is_null()),
                Some(w) => {
                    assert!((w.mean - p["mean_age_ma"].as_f64().unwrap()).abs() < 1e-5 * w.mean, "{flag} wm");
                    assert!((w.total_2s - p["total_2s_ma"].as_f64().unwrap()).abs() < 1e-3 * w.total_2s, "{flag} wm 2s");
                    assert!((w.p_value - p["p_value"].as_f64().unwrap()).abs() < 1e-3, "{flag} wm p");
                }
            }
        }
        // (t76 means rest on the current 207Pb/206Pb excess rule and are not compared)
        for t in ["t68", "t82"] {
            let w = isotopic_weighted_mean(&grp, t, 1.3, false, 2.0, &crit);
            let p = &e[format!("iwm_{t}")];
            match w {
                None => assert!(p.is_null(), "{flag} {t} iwm missing in the stored results"),
                Some(w) => {
                    assert_eq!(w.wm.n, p["n"].as_u64().unwrap() as usize, "{flag} {t} n");
                    // the stored results averaged ages rounded to 2 decimals; the program averages unrounded ones
                    assert!((w.wm.mean - p["mean_age_ma"].as_f64().unwrap()).abs() < 1e-5 * w.wm.mean, "{flag} {t} iwm {} vs {}", w.wm.mean, p["mean_age_ma"]);
                    assert!((w.wm.total_2s - p["total_2s_ma"].as_f64().unwrap()).abs() < 1e-3 * w.wm.total_2s, "{flag} {t} iwm 2s");
                }
            }
        }
        // the concordia and discordia maths, on the stored point uncertainties
        // (the current 207Pb/206Pb excess rule differs from theirs)
        let py_by_id: std::collections::BTreeMap<String, &Value> = g["rows"].as_array().unwrap().iter().map(|r| (r["Row_ID"].as_str().unwrap_or("").to_string(), r)).collect();
        let grp_py: Vec<Row> = grp
            .iter()
            .map(|r| {
                let mut c = (*r).clone();
                if let Some(p) = py_by_id.get(&text(r, "Row_ID")) {
                    for k in ["UPb_r86_2s_point", "UPb_r76_2s_point", "UPb_rho_TW_point", "UPb_ConcDist_point_sigma"] {
                        if let Some(v) = p.get(k) {
                            c.insert(k.to_string(), v.clone());
                        }
                    }
                }
                c
            })
            .collect();
        let grp_py: Vec<&Row> = grp_py.iter().collect();
        let pts: Vec<Pt> = concordia_data(&grp_py, false).into_iter().map(|(_, x, sx, y, sy, r)| (x, sx, y, sy, r)).collect();
        let ca = if pts.is_empty() { None } else { concordia_age(&pts, &s.decay, None) };
        let p = &e["concordia"];
        match ca {
            None => assert!(p.is_null()),
            Some(c) => {
                assert!((c.age_ma - p["age_ma"].as_f64().unwrap()).abs() < 1e-5 * c.age_ma, "{flag} concordia {} vs {}", c.age_ma, p["age_ma"]);
                assert!((c.age_2s_ma - p["age_2s_ma"].as_f64().unwrap()).abs() < 1e-3 * c.age_2s_ma, "{flag} conc 2s {} vs {}", c.age_2s_ma, p["age_2s_ma"]);
                assert!((c.mswd - p["mswd"].as_f64().unwrap()).abs() < 1e-4 * c.mswd.max(1.0));
            }
        }
        for (key, anchor) in [("discordia", None), ("discordia0", Some(0.0))] {
            let d = if pts.len() >= if anchor.is_some() { 2 } else { 3 } { discordia(&pts, &s.decay, anchor) } else { None };
            let p = &e[key];
            match d {
                None => assert!(p.is_null(), "{flag} {key}"),
                Some(d) => {
                    let pi = p["intercepts"].as_array().unwrap();
                    assert_eq!(d.intercepts.len(), pi.len(), "{flag} {key} intercepts");
                    for (a, b) in d.intercepts.iter().zip(pi) {
                        assert!((a.age_ma - b["age_ma"].as_f64().unwrap()).abs() < 1e-6 * a.age_ma.max(1.0), "{flag} {key} {} vs {}", a.age_ma, b["age_ma"]);
                        assert!((a.age_2s_ma - b["age_2s_ma"].as_f64().unwrap()).abs() < 1e-4 * a.age_2s_ma.max(1.0));
                    }
                }
            }
        }
    }
}

fn flag_ok(r: &Row) -> bool {
    flag(r, "Include_In_Average", true)
}
