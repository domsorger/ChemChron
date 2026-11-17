// SPDX-License-Identifier: GPL-3.0-or-later
//! Exports for publication: per-selection results with full metadata, Excel,
//! IsoplotR-ready input, and every sweep of a selection for hand checks.

use crate::drs::qa::QaStatus;
use crate::drs::upb::{sweeps_for, RatioKind};
use crate::drs::Reduction;
use crate::model::*;
use std::fmt::Write as _;
use std::path::Path;

/// A value for a table: numbers stay numbers in Excel.
#[derive(Clone, Debug)]
pub enum Cell {
    Text(String),
    Num(f64),
    Empty,
}

impl Cell {
    fn csv(&self) -> String {
        match self {
            Cell::Text(s) => {
                if s.contains([',', '"', '\n']) {
                    format!("\"{}\"", s.replace('"', "\"\""))
                } else {
                    s.clone()
                }
            }
            Cell::Num(v) => format_number(*v),
            Cell::Empty => String::new(),
        }
    }
}

fn format_number(v: f64) -> String {
    if !v.is_finite() {
        return String::new();
    }
    if v == 0.0 {
        return "0".to_string();
    }
    format!("{:.6e}", v)
        .parse::<f64>()
        .map_or_else(|_| v.to_string(), |rounded| rounded.to_string())
}

fn num(v: f64) -> Cell {
    if v.is_finite() {
        Cell::Num(v)
    } else {
        Cell::Empty
    }
}

pub struct Table {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<Cell>>,
}

/// Which columns the results hold.
pub fn results_table(session: &Session, red: &Reduction) -> Table {
    let mut headers: Vec<String> = [
        "Selection", "Group", "Type", "Reference material", "Source", "Left out", "Start (s)", "End (s)",
        "Laser on (s)", "Beam start (s)", "X (um)", "Y (um)", "Spot (um)", "Sweeps", "Flags",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let has_upb = red.upb.is_some();
    if has_upb {
        for key in ["206Pb/238U", "207Pb/235U", "207Pb/206Pb", "208Pb/232Th"] {
            for suffix in ["", " Int2SE", " Prop2s", " RMInc2s", " n", " outliers"] {
                headers.push(format!("{key}{suffix}"));
            }
        }
        headers.extend(
            [
                "238U/206Pb", "238U/206Pb Prop2s", "rho 206Pb/238U v 207Pb/235U",
                "rho 207Pb/206Pb v 238U/206Pb",
                "Age 206Pb/238U (Ma)", "Int2SE", "Prop2s", "RMInc2s",
                "Age 207Pb/235U (Ma)", "Int2SE", "Prop2s", "RMInc2s",
                "Age 207Pb/206Pb (Ma)", "Int2SE", "Prop2s", "RMInc2s",
                "Age 208Pb/232Th (Ma)", "Int2SE", "Prop2s", "RMInc2s",
                "Discordance (%)", "Th/U (count rates)", "f206 from 204Pb (%)",
                "Common-Pb method", "f206 corrected (%)", "206Pb/238U corr", "2s",
                "207Pb/235U corr", "2s", "208Pb/232Th corr", "2s",
                "Age 206/238 corr (Ma)", "2s", "Age 207/235 corr (Ma)", "2s", "Age 208/232 corr (Ma)", "2s",
                "Age 206/238 230Th corr (Ma)", "2s",
            ]
            .iter()
            .map(|s| s.to_string()),
        );
    }
    let te_channels: Vec<usize> = red
        .te
        .as_ref()
        .map_or_else(Vec::new, |t| {
            t.calibrations
                .iter()
                .enumerate()
                .filter(|(_, c)| c.is_some())
                .map(|(i, _)| i)
                .collect()
        });
    for &c in &te_channels {
        let name = &session.inputs.channels[c].name;
        for suffix in ["ppm", "Int2SE", "Prop2s", "RMInc2s", "LOD", "n", "outliers"] {
            headers.push(format!("{name} {suffix}"));
        }
    }
    let mut rows = Vec::new();
    for (s, r) in session.selections.iter().zip(&red.selections) {
        let Some(group) = session.group_of(s) else { continue };
        if group.kind == GroupKind::Baseline {
            continue;
        }
        let mut row = vec![
            Cell::Text(s.label.clone()),
            Cell::Text(group.name.clone()),
            Cell::Text(match group.kind {
                GroupKind::ReferenceMaterial => "Reference material",
                GroupKind::Sample => "Sample",
                GroupKind::Baseline => "Baseline",
            }
            .to_string()),
            Cell::Text(group.rm.clone().unwrap_or_default()),
            Cell::Text(format!("{:?}", s.source)),
            Cell::Text(if s.excluded { "yes" } else { "" }.to_string()),
            num(s.start),
            num(s.end),
            s.laser_on.map_or(Cell::Empty, num),
            r.upb.as_ref().map_or(Cell::Empty, |u| num(u.beam_start)),
            s.x.map_or(Cell::Empty, num),
            s.y.map_or(Cell::Empty, num),
            s.spot_size.map_or(Cell::Empty, num),
            Cell::Num(r.range.len() as f64),
            Cell::Text(r.flags.join("; ")),
        ];
        if has_upb {
            match &r.upb {
                Some(u) => {
                    for ratio in [Some(&u.r68), Some(&u.r75), Some(&u.r76), u.r82.as_ref()] {
                        match ratio {
                            Some(v) => row.extend([
                                num(v.mean), num(v.int2se), num(v.prop2s), num(v.rminc2s),
                                Cell::Num(v.n as f64), Cell::Num(v.n_outliers as f64),
                            ]),
                            None => row.extend(std::iter::repeat_n(Cell::Empty, 6)),
                        }
                    }
                    row.extend([num(u.r38), num(u.r38_prop2s), num(u.rho_68_75), num(u.rho_76_38)]);
                    for a in [Some(u.a68), Some(u.a75), Some(u.a76), u.a82] {
                        match a {
                            Some(a) => row.extend([num(a.age), num(a.int2s), num(a.prop2s), num(a.rminc2s)]),
                            None => row.extend(std::iter::repeat_n(Cell::Empty, 4)),
                        }
                    }
                    row.extend([num(u.discordance_pct), num(u.th_u), num(u.f206_pct_204)]);
                    match &u.common {
                        Some(c) => row.extend([
                            Cell::Text(c.method.to_string()),
                            num(c.f206_pct), num(c.r68), num(c.r68_2s), num(c.r75), num(c.r75_2s),
                            num(c.r82), num(c.r82_2s), num(c.age68), num(c.age68_2s),
                            num(c.age75), num(c.age75_2s), num(c.age82), num(c.age82_2s),
                        ]),
                        None => row.extend(std::iter::repeat_n(Cell::Empty, 14)),
                    }
                    match u.a68_th230 {
                        Some(a) => row.extend([num(a.age), num(a.prop2s)]),
                        None => row.extend([Cell::Empty, Cell::Empty]),
                    }
                }
                None => row.extend(std::iter::repeat_n(Cell::Empty, 24 + 4 + 16 + 3 + 14 + 2)),
            }
        }
        let lod_method = session.settings.te.lod;
        for &c in &te_channels {
            match r.te.get(c).and_then(|v| v.as_ref()) {
                Some(v) if v.below_lod => row.extend([
                    Cell::Text("< LOD".to_string()),
                    Cell::Empty,
                    Cell::Empty,
                    Cell::Empty,
                    num(v.lod(lod_method)),
                    Cell::Num(v.n as f64),
                    Cell::Num(v.n_outliers as f64),
                ]),
                Some(v) => row.extend([
                    num(v.ppm), num(v.int2se), num(v.prop2s), num(v.rminc2s), num(v.lod(lod_method)),
                    Cell::Num(v.n as f64), Cell::Num(v.n_outliers as f64),
                ]),
                None => row.extend(std::iter::repeat_n(Cell::Empty, 7)),
            }
        }
        rows.push(row);
    }
    Table { headers, rows }
}

fn median_of<'a>(values: impl Iterator<Item = &'a f64>) -> f64 {
    let v: Vec<f64> = values.copied().collect();
    crate::stats::median(&v)
}

/// Facts about the session, the method and every setting: enough to repeat
/// the reduction and to fill a publication metadata table.
pub fn metadata_rows(session: &Session, red: &Reduction) -> Vec<(String, String)> {
    let mut m: Vec<(String, String)> = Vec::new();
    let mut push = |k: &str, v: String| m.push((k.to_string(), v));
    let s = &session.settings;
    push("Software", format!("ChemChron {}", crate::VERSION));
    push("Session", session.name.clone());
    if matches!(session.source, SessionSource::Synthetic { .. }) {
        push("Data", "SIMULATED DEMO DATA, not measurements".to_string());
    }
    push("Laboratory", session.metadata.laboratory.clone());
    push("Operator", session.metadata.operator.clone());
    push("Mass spectrometer", session.metadata.instrument.clone());
    push("Laser system", session.metadata.laser.clone());
    push("Carrier gas", session.metadata.carrier_gas.clone());
    push("Notes", session.metadata.notes.clone());
    let inputs = &session.inputs;
    let spots = &inputs.laser;
    if !spots.is_empty() {
        let rep = median_of(spots.iter().map(|s| &s.rep_rate));
        let size = median_of(spots.iter().map(|s| &s.spot_size));
        let flu: Vec<f64> = spots.iter().map(|s| s.fluence).filter(|f| *f > 0.0).collect();
        push("Repetition rate (Hz)", format!("{rep}"));
        push("Spot size (um)", format!("{size}"));
        if !flu.is_empty() {
            push("Fluence (J/cm2)", format!("{:.2}", crate::stats::median(&flu)));
        }
    }
    push("Sweep time (s)", format!("{:.4}", inputs.sweep_seconds()));
    push("Sweeps in session", inputs.time.len().to_string());
    push(
        "Channels",
        inputs.channels.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", "),
    );
    push("Raw data files", session.files.len().to_string());
    if let Some(l) = &session.laser_log_file {
        push("Laser log", format!("{} (sha256 {})", l.path, l.sha256));
        push("Clock offset (s), laser log to mass spectrometer", format!("{:.2}", session.clock_offset_s));
        push("Clock offset method", session.clock_note.clone());
    }
    push(
        "Outlier rule",
        if s.outlier_k > 0.0 {
            format!(
                "Sweeps beyond {} robust standard deviations (1.4826 x median absolute deviation) from the median are rejected, up to 3 passes. They are counted in the outlier columns.",
                s.outlier_k
            )
        } else {
            "No outlier rejection".to_string()
        },
    );
    if let Some(g) = session.groups.iter().find(|g| g.kind == GroupKind::Baseline) {
        push("Baseline interpolation", g.interp.label());
    }
    push(
        "Uncertainty types",
        "Int2SE (analytical): 2 standard errors over the sweeps of the selection. Prop2s (analytical + excess): Int2SE plus the session's excess uncertainty in quadrature (from the primary and the secondary reference materials together, MSWD 1). RMInc2s: Prop2s plus the accepted-value uncertainty of the primary in quadrature. Long-term and decay-constant terms are added in ChemChron's Combined results and weighted means.".to_string(),
    );
    if let Some(te) = &red.te {
        let name = te.is_channel.map_or_else(
            || "None (semi-quantitative)".to_string(),
            |i| inputs.channels[i].name.clone(),
        );
        push("Trace elements: internal standard", name);
        if let Some(g) = s.te.primary_group.and_then(|g| session.group(g)) {
            push("Trace elements: calibration reference", g.name.clone());
        }
        push("Trace elements: limit of detection", s.te.lod.label().to_string());
        push(
            "Trace elements: dwell per channel (s)",
            format!("{:.5}{}", te.dwell_s, if te.dwell_estimated { " (estimated as sweep time / channels)" } else { "" }),
        );
    }
    if let Some(u) = &red.upb {
        if let Some(g) = session.group(u.primary_group) {
            push("U-Th-Pb: primary reference", format!("{} ({} analyses)", g.name, u.primary_n));
        }
        push(
            "U-Th-Pb: beam seconds from",
            match s.upb.beam {
                BeamMethod::LaserLog => "Laser log".to_string(),
                BeamMethod::SignalJump => "Jump in the smoothed index channel".to_string(),
            },
        );
        if let Some(i) = s.upb.index_channel.as_ref() {
            push("U-Th-Pb: index channel", i.clone());
        }
        let d = s.upb.decay;
        push(
            "Decay constants (1/yr)",
            format!("238U {:e}, 235U {:e}, 232Th {:e}, 230Th {:e}", d.l238, d.l235, d.l232, d.l230),
        );
        push("238U/235U", format!("{} (207Pb/235U is calculated from 207Pb/238U with this value)", d.u85));
        for kind in RatioKind::ALL {
            let Some(r) = u.ratio(kind) else { continue };
            if let Some(f) = &r.dhf {
                push(
                    &format!("{}: down-hole model", kind.key()),
                    format!("{} (a={:.5}, b={:.5}, c={:.5}, m={:.6}; first {} s of beam left out of the fit)", f.model.label(), f.a, f.b, f.c, f.m, s.upb.dhf_trim_s),
                );
            }
            push(&format!("{}: accepted value", kind.key()), format!("{:.6} (2s rel {:.3} %)", r.accepted, r.accepted_rel2s * 100.0));
            push(&format!("{}: excess uncertainty", kind.key()), format!("{:.3} % (1 sigma; primary alone {:.3} %, jack-knifed against {}; {} secondary analyses)", r.excess * 100.0, r.excess_primary * 100.0, r.curve.kind.label(), r.n_secondary));
        }
        push("Common-Pb correction", s.upb.common_method.label().to_string());
        if s.upb.common_method != CommonPbMethod::None {
            push(
                "Common-Pb composition",
                match s.upb.common_composition {
                    CommonPbComposition::StaceyKramers => "Stacey & Kramers (1975) at the 206Pb/238U age".to_string(),
                    CommonPbComposition::User(a, b, c) => format!("206/204 {a}, 207/204 {b}, 208/204 {c}"),
                },
            );
        }
        if s.upb.hg_correction {
            push("204Hg correction", format!("204Hg = {} x 202Hg", s.upb.hg_204_202));
        }
        push(
            "230Th correction",
            if s.upb.th230_enabled {
                "Schaerer (1984), with Th/U of the melt or fluid set per group and Th/U of the mineral from count rates".to_string()
            } else {
                "Off".to_string()
            },
        );
        push("Discordance", "Below 1000 Ma: 1 - (206/238 age) / (207/235 age). Above: 1 - (206/238 age) / (207/206 age).".to_string());
    }
    for rm in &session.rms {
        let used = session.groups.iter().any(|g| g.rm.as_deref() == Some(&rm.name));
        if used {
            let status = if rm.confirmed { "" } else { " [NOT CONFIRMED]" };
            push(&format!("Reference material {}", rm.name), format!("{}{status}", rm.source));
        }
    }
    for w in &red.warnings {
        push("Warning", w.clone());
    }
    m
}

pub fn write_results_csv(session: &Session, red: &Reduction, path: &Path) -> Result<(), String> {
    let mut out = String::new();
    for (k, v) in metadata_rows(session, red) {
        let _ = writeln!(out, "# {k}: {v}");
    }
    let t = results_table(session, red);
    let _ = writeln!(out, "{}", t.headers.iter().map(|h| Cell::Text(h.clone()).csv()).collect::<Vec<_>>().join(","));
    for row in &t.rows {
        let _ = writeln!(out, "{}", row.iter().map(Cell::csv).collect::<Vec<_>>().join(","));
    }
    std::fs::write(path, out).map_err(|e| format!("{}: {e}", path.display()))
}

/// One Excel workbook: metadata, results, reference-material checks, group summaries.
pub fn write_xlsx(session: &Session, red: &Reduction, path: &Path) -> Result<(), String> {
    use rust_xlsxwriter::{Format, Workbook};
    let err = |e: rust_xlsxwriter::XlsxError| e.to_string();
    let mut book = Workbook::new();
    let bold = Format::new().set_bold();
    {
        let sheet = book.add_worksheet().set_name("Metadata").map_err(err)?;
        sheet.write_string_with_format(0, 0, "Item", &bold).map_err(err)?;
        sheet.write_string_with_format(0, 1, "Value", &bold).map_err(err)?;
        for (i, (k, v)) in metadata_rows(session, red).iter().enumerate() {
            sheet.write_string(i as u32 + 1, 0, k).map_err(err)?;
            sheet.write_string(i as u32 + 1, 1, v).map_err(err)?;
        }
        sheet.set_column_width(0, 44).map_err(err)?;
        sheet.set_column_width(1, 110).map_err(err)?;
    }
    {
        let t = results_table(session, red);
        let sheet = book.add_worksheet().set_name("Results").map_err(err)?;
        for (c, h) in t.headers.iter().enumerate() {
            sheet.write_string_with_format(0, c as u16, h, &bold).map_err(err)?;
        }
        for (r, row) in t.rows.iter().enumerate() {
            for (c, cell) in row.iter().enumerate() {
                match cell {
                    Cell::Text(s) => {
                        sheet.write_string(r as u32 + 1, c as u16, s).map_err(err)?;
                    }
                    Cell::Num(v) => {
                        sheet.write_number(r as u32 + 1, c as u16, *v).map_err(err)?;
                    }
                    Cell::Empty => {}
                }
            }
        }
        sheet.set_freeze_panes(1, 1).map_err(err)?;
    }
    {
        let sheet = book.add_worksheet().set_name("QA-QC").map_err(err)?;
        let head = ["Group", "Reference material", "Quantity", "Measured", "2s", "Accepted", "2s", "Difference (%)", "MSWD", "n", "Status"];
        for (c, h) in head.iter().enumerate() {
            sheet.write_string_with_format(0, c as u16, *h, &bold).map_err(err)?;
        }
        for (i, r) in red.qa.rows.iter().enumerate() {
            let row = i as u32 + 1;
            sheet.write_string(row, 0, &r.group).map_err(err)?;
            sheet.write_string(row, 1, &r.material).map_err(err)?;
            sheet.write_string(row, 2, &r.quantity).map_err(err)?;
            for (c, v) in [(3, r.measured), (4, r.measured_2s), (5, r.accepted), (6, r.accepted_2s), (7, r.diff_pct), (8, r.mswd), (9, r.n as f64)] {
                if v.is_finite() {
                    sheet.write_number(row, c, v).map_err(err)?;
                }
            }
            sheet.write_string(row, 10, qa_label(r.status)).map_err(err)?;
        }
    }
    {
        let sheet = book.add_worksheet().set_name("Group ages").map_err(err)?;
        let head = [
            "Group", "Analyses", "Concordia age (Ma)", "2s", "MSWD", "p",
            "Mean 206/238 age (Ma)", "2s", "MSWD", "Mean 207/206 age (Ma)", "2s", "MSWD", "Mean 208/232 age (Ma)", "2s", "MSWD",
        ];
        for (c, h) in head.iter().enumerate() {
            sheet.write_string_with_format(0, c as u16, *h, &bold).map_err(err)?;
        }
        for (i, g) in red.qa.groups.iter().enumerate() {
            let row = i as u32 + 1;
            sheet.write_string(row, 0, &g.name).map_err(err)?;
            sheet.write_number(row, 1, g.n as f64).map_err(err)?;
            let c = g.concordia.as_ref();
            let wm = |w: &Option<crate::stats::WeightedMean>| w.as_ref().map_or([f64::NAN; 3], |w| [w.mean, w.unc2s_mswd, w.mswd]);
            let (a, b, d) = (wm(&g.wm_a68), wm(&g.wm_a76), wm(&g.wm_a82));
            let values = [
                c.map_or(f64::NAN, |c| c.age), c.map_or(f64::NAN, |c| c.unc2s), c.map_or(f64::NAN, |c| c.mswd), c.map_or(f64::NAN, |c| c.p),
                a[0], a[1], a[2], b[0], b[1], b[2], d[0], d[1], d[2],
            ];
            for (k, v) in values.iter().enumerate() {
                if v.is_finite() {
                    sheet.write_number(row, k as u16 + 2, *v).map_err(err)?;
                }
            }
        }
    }
    book.save(path).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn qa_label(s: QaStatus) -> &'static str {
    match s {
        QaStatus::Pass => "Pass",
        QaStatus::Warn => "Warning",
        QaStatus::Alarm => "Alarm",
        QaStatus::Primary => "Primary (agrees by construction)",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IsoplotFormat {
    /// x = 207Pb/235U, y = 206Pb/238U.
    Wetherill,
    /// x = 238U/206Pb, y = 207Pb/206Pb.
    TeraWasserburg,
}

/// CSV for IsoplotR's U-Pb input with propagated uncertainties at 1 sigma, its default.
pub fn isoplotr_text(session: &Session, red: &Reduction, format: IsoplotFormat, groups: &[u64]) -> String {
    let mut out = String::new();
    out.push_str(match format {
        IsoplotFormat::Wetherill => "Pb207U235,errPb207U235,Pb206U238,errPb206U238,rhoXY\n",
        IsoplotFormat::TeraWasserburg => "U238Pb206,errU238Pb206,Pb207Pb206,errPb207Pb206,rhoXY\n",
    });
    for (s, r) in session.selections.iter().zip(&red.selections) {
        if s.excluded || !groups.contains(&s.group) {
            continue;
        }
        let Some(u) = &r.upb else { continue };
        let row = match format {
            IsoplotFormat::Wetherill => [u.r75.mean, u.r75.prop2s / 2.0, u.r68.mean, u.r68.prop2s / 2.0, u.rho_68_75],
            IsoplotFormat::TeraWasserburg => [u.r38, u.r38_prop2s / 2.0, u.r76.mean, u.r76.prop2s / 2.0, u.rho_76_38],
        };
        if row.iter().all(|v| v.is_finite()) {
            let cells: Vec<String> = row.iter().map(|v| format!("{v:.7}")).collect();
            let _ = writeln!(out, "{}", cells.join(","));
        }
    }
    out
}

/// Every sweep of one selection at every step, so any number can be recalculated by hand.
pub fn sweeps_csv(session: &Session, red: &Reduction, selection: &Selection) -> String {
    let inputs = &session.inputs;
    let range = inputs.range(selection.start, selection.end);
    let mut headers = vec!["Time (s)".to_string()];
    for c in &inputs.channels {
        headers.push(format!("{} raw CPS", c.name));
        headers.push(format!("{} baseline", c.name));
    }
    let table = sweeps_for(session, red, selection);
    if table.is_some() {
        headers.push("Beam seconds".to_string());
        for kind in RatioKind::ALL {
            headers.push(format!("{} raw", kind.key()));
            headers.push(format!("{} down-hole corrected", kind.key()));
            headers.push(format!("{} final", kind.key()));
            headers.push(format!("{} kept", kind.key()));
        }
    }
    let mut out = String::new();
    let _ = writeln!(out, "# Selection {} from ChemChron {}", selection.label, crate::VERSION);
    let _ = writeln!(out, "{}", headers.join(","));
    for (row, i) in range.clone().enumerate() {
        let t = inputs.time[i];
        let mut cells = vec![format_number(t)];
        for (c, channel) in inputs.channels.iter().enumerate() {
            cells.push(format_number(channel.values[i]));
            let b = red.baseline.get(c).and_then(|b| b.as_ref()).map_or(0.0, |cv| cv.eval(t));
            cells.push(format_number(b));
        }
        if let Some(tb) = &table {
            cells.push(format_number(tb.tau[row]));
            for kind in RatioKind::ALL {
                let k = kind as usize;
                cells.push(format_number(tb.raw[k][row]));
                cells.push(format_number(tb.downhole[k][row]));
                cells.push(format_number(tb.fin[k][row]));
                cells.push(if tb.keep[k][row] { "yes" } else { "no" }.to_string());
            }
        }
        let _ = writeln!(out, "{}", cells.join(","));
    }
    out
}
