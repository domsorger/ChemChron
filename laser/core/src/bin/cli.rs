// SPDX-License-Identifier: GPL-3.0-or-later
//! Scripted reduction without any window, for batch runs and checks:
//! chemchron-laser-cli <files or folders...> [--primary GROUP] [--te-primary GROUP]
//!     [--material GROUP=MATERIAL] [--beam log|jump] [--dhf MODEL] [--csv out.csv] [--xlsx out.xlsx]
//!     [--group NAME] [--demo] [--session out.json] [--rm-csv values.csv]
//!     [--selections table.csv] [--outlier-limit K]

use std::path::PathBuf;
use chemchron_laser_core::drs::{reduce, upb::RatioKind};
use chemchron_laser_core::model::*;
use chemchron_laser_core::{export, project, synthetic};

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut paths = Vec::new();
    let mut args = std::env::args().skip(1);
    let (mut primary, mut te_primary, mut csv, mut xlsx, mut out_session) = (None, None, None, None, None);
    let mut mapping: Vec<(String, String)> = Vec::new();
    let mut show_group: Option<String> = None;
    let mut demo = false;
    let mut beam = None;
    let mut dhf = None;
    let mut rm_csv = None;
    let mut selection_table = None;
    let mut outlier_limit = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--primary" => primary = args.next(),
            "--te-primary" => te_primary = args.next(),
            "--csv" => csv = args.next(),
            "--xlsx" => xlsx = args.next(),
            "--session" => out_session = args.next(),
            "--group" => show_group = args.next(),
            "--beam" => beam = args.next(),
            "--dhf" => dhf = args.next(),
            "--rm-csv" => rm_csv = args.next(),
            "--selections" => selection_table = args.next(),
            "--outlier-limit" => outlier_limit = args.next(),
            "--demo" => demo = true,
            "--material" => {
                let v = args.next().ok_or("--material needs GROUP=MATERIAL")?;
                let (g, m) = v.split_once('=').ok_or("--material needs GROUP=MATERIAL")?;
                mapping.push((g.to_string(), m.to_string()));
            }
            _ => paths.push(PathBuf::from(a)),
        }
    }
    let mut session = if demo {
        synthetic::demo_session(1)
    } else {
        project::session_from_paths(&paths, None)?
    };
    if let Some(path) = selection_table {
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
        project::apply_selection_table(&mut session, &text)?;
    }
    if let Some(k) = outlier_limit {
        session.settings.outlier_k = k.parse().map_err(|_| "--outlier-limit needs a number")?;
    }
    if let Some(path) = rm_csv {
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
        chemchron_laser_core::refmat::merge_csv(&mut session.rms, &text)?;
    }
    for (group, material) in mapping {
        let id = session.ensure_group(&group, GroupKind::Sample);
        let g = session.groups.iter_mut().find(|g| g.id == id).expect("just made");
        g.rm = Some(material);
        g.kind = GroupKind::ReferenceMaterial;
    }
    let find = |s: &Session, n: &str| s.groups.iter().find(|g| g.name == n).map(|g| g.id);
    if let Some(p) = primary {
        session.settings.upb.primary_group = find(&session, &p);
    }
    if let Some(p) = te_primary {
        session.settings.te.primary_group = find(&session, &p);
    }
    match beam.as_deref() {
        Some("log") => session.settings.upb.beam = BeamMethod::LaserLog,
        Some("jump") => session.settings.upb.beam = BeamMethod::SignalJump,
        _ => {}
    }
    if let Some(name) = &dhf {
        let model = match name.as_str() {
            "none" => DhfModel::None,
            "linear" => DhfModel::Linear,
            "exp" => DhfModel::Exponential,
            "explin" => DhfModel::ExponentialLinear,
            "spline" => DhfModel::Spline,
            _ => return Err(format!("Unknown down-hole model {name}")),
        };
        session.settings.upb.dhf_r68 = model;
        session.settings.upb.dhf_r75 = model;
        session.settings.upb.dhf_r82 = model;
    }
    session.apply_defaults();
    let red = reduce(&session);
    let dt = session.inputs.sweep_seconds();
    println!(
        "{} sweeps, {} channels, {:.3} s per sweep, {} files, {} firings in the log",
        session.inputs.time.len(),
        session.inputs.channels.len(),
        dt,
        session.files.len(),
        session.inputs.laser.len()
    );
    if !session.clock_note.is_empty() {
        println!("Clock offset {:.2} s. {}", session.clock_offset_s, session.clock_note);
    }
    println!("Reduced in {:.0} ms", red.elapsed_ms);
    for g in &session.groups {
        let n = session.selections.iter().filter(|s| s.group == g.id).count();
        println!("Group {:<12} {:?} n={n} rm={:?}", g.name, g.kind, g.rm);
    }
    for w in &red.warnings {
        println!("Warning: {w}");
    }
    if let Some(u) = &red.upb {
        println!("Primary analyses {}", u.primary_n);
        for kind in RatioKind::ALL {
            if let Some(r) = u.ratio(kind) {
                let dhf = r.dhf.as_ref().map_or("none".to_string(), |f| {
                    format!(
                        "{} a={:.4} b={:.4} c={:.4} m={:.5} suggested={}",
                        f.model.label(),
                        f.a,
                        f.b,
                        f.c,
                        f.m,
                        f.suggested.label()
                    )
                });
                println!(
                    "{:<12} accepted {:.5}, excess {:.3} %, MSWD {:.2}; {dhf}",
                    kind.key(),
                    r.accepted,
                    r.excess * 100.0,
                    r.mswd_after
                );
            }
        }
    }
    for g in &red.qa.groups {
        let c = g.concordia.as_ref().map_or("-".to_string(), |c| {
            format!("{:.1} ± {:.1} Ma (MSWD {:.2}, n={})", c.age, c.unc2s, c.mswd, c.n)
        });
        let w = g.wm_a68.as_ref().map_or("-".to_string(), |w| {
            format!("{:.1} ± {:.1} Ma (MSWD {:.2})", w.mean, w.unc2s_mswd, w.mswd)
        });
        let w82 = g.wm_a82.as_ref().map_or("-".to_string(), |w| {
            format!("{:.1} ± {:.1} Ma (MSWD {:.2})", w.mean, w.unc2s_mswd, w.mswd)
        });
        println!("{:<12} n={:<3} concordia {c}; 206/238 {w}; 208/232 {w82}", g.name, g.n);
    }
    let flagged: usize = red.selections.iter().filter(|s| !s.flags.is_empty()).count();
    println!("Flagged analyses: {flagged}");
    if let Some(name) = show_group {
        for (s, r) in session.selections.iter().zip(&red.selections) {
            if session.group_of(s).is_some_and(|g| g.name == name) {
                if let Some(u) = &r.upb {
                    println!(
                        "{:<8} t={:7.1} beam={:6.2} 68={:.5}±{:.5} 75={:.4}±{:.4} 76={:.5} a68={:.1}±{:.1} a76={:.0} disc={:+.1}% f206={:+.2}% {:?}",
                        s.label,
                        s.start,
                        s.start - u.beam_start,
                        u.r68.mean,
                        u.r68.prop2s,
                        u.r75.mean,
                        u.r75.prop2s,
                        u.r76.mean,
                        u.a68.age,
                        u.a68.prop2s,
                        u.a76.age,
                        u.discordance_pct,
                        u.f206_pct_204,
                        r.flags
                    );
                }
            }
        }
    }
    if let Some(path) = csv {
        export::write_results_csv(&session, &red, std::path::Path::new(&path))?;
        println!("Wrote {path}");
    }
    if let Some(path) = xlsx {
        export::write_xlsx(&session, &red, std::path::Path::new(&path))?;
        println!("Wrote {path}");
    }
    if let Some(path) = out_session {
        project::save_session(&session, std::path::Path::new(&path))?;
        println!("Wrote {path}");
    }
    Ok(())
}
