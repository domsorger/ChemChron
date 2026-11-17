// SPDX-License-Identifier: GPL-3.0-or-later
//! Importers. Files are only ever read. Each one is recorded with its SHA-256,
//! so a saved session can prove it was reduced from the same raw data.
//!
//! Read directly: Thermo Element `.FIN2`, Agilent MassHunter per-spot `.csv`
//! ("Intensity Vs Time"), ESI laser logs, and generic time-resolved text with a
//! time column. Nu Plasma II `.run` files are not read yet.

use crate::model::*;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Fin2,
    AgilentCsv,
    LaserLog,
    GenericText,
    Other,
}

/// A clock time as written in a file. A numeric date such as 11/08/2026 can be
/// day-first or month-first; it is kept undecided until the whole batch is seen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stamp {
    Exact(f64),
    Numeric { a: u32, b: u32, year: i64, seconds: f64 },
}

#[derive(Clone, Debug)]
pub struct ParsedFile {
    pub path: PathBuf,
    pub label: String,
    pub format: &'static str,
    pub stamp: Option<Stamp>,
    pub stamp_text: Option<String>,
    pub time: Vec<f64>,
    pub names: Vec<String>,
    pub columns: Vec<Vec<f64>>,
}

/// Days since 1970-01-01 for a civil date.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn hms(text: &str) -> Option<f64> {
    let mut it = text.trim().split(':');
    let h: f64 = it.next()?.trim().parse().ok()?;
    let m: f64 = it.next()?.trim().parse().ok()?;
    let s: f64 = it.next().unwrap_or("0").trim().parse().ok()?;
    Some(h * 3600.0 + m * 60.0 + s)
}

/// "2025-01-21 14:15:07.886" as seconds since 1970.
pub fn parse_iso(text: &str) -> Option<f64> {
    let t = text.trim().replace('T', " ");
    let (date, time) = t.split_once(' ')?;
    let mut d = date.split('-');
    let y: i64 = d.next()?.parse().ok()?;
    let m: u32 = d.next()?.parse().ok()?;
    let day: u32 = d.next()?.parse().ok()?;
    Some(days_from_civil(y, m, day) as f64 * 86400.0 + hms(time)?)
}

/// "Tuesday, January 21, 2025 13:16:45" as seconds since 1970.
pub fn parse_long_date(text: &str) -> Option<f64> {
    const MONTHS: [&str; 12] = [
        "january", "february", "march", "april", "may", "june", "july", "august", "september",
        "october", "november", "december",
    ];
    let parts: Vec<&str> = text.split(',').map(str::trim).collect();
    if parts.len() < 3 {
        return None;
    }
    let mut md = parts[1].split_whitespace();
    let month_name = md.next()?.to_lowercase();
    let month = MONTHS.iter().position(|m| *m == month_name)? as u32 + 1;
    let day: u32 = md.next()?.parse().ok()?;
    let mut yt = parts[2].split_whitespace();
    let year: i64 = yt.next()?.parse().ok()?;
    let time = yt.next()?;
    Some(days_from_civil(year, month, day) as f64 * 86400.0 + hms(time)?)
}

fn parse_numeric_date(text: &str) -> Option<Stamp> {
    let mut words = text.split_whitespace();
    let date = words.next()?;
    let time = words.next()?;
    let mut d = date.split('/');
    let a: u32 = d.next()?.parse().ok()?;
    let b: u32 = d.next()?.parse().ok()?;
    let year: i64 = d.next()?.parse().ok()?;
    Some(Stamp::Numeric { a, b, year, seconds: hms(time)? })
}

fn numeric_value(a: u32, b: u32, year: i64, seconds: f64, day_first: bool) -> Option<f64> {
    let (d, m) = if day_first { (a, b) } else { (b, a) };
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(days_from_civil(year, m, d) as f64 * 86400.0 + seconds)
}

fn split_cells(line: &str) -> Vec<&str> {
    let sep = if line.contains('\t') {
        '\t'
    } else if line.contains(',') {
        ','
    } else if line.contains(';') {
        ';'
    } else {
        return line.split_whitespace().collect();
    };
    line.split(sep).map(|c| c.trim().trim_matches('"')).collect()
}

fn clean_channel_name(raw: &str) -> String {
    let t = raw.trim().trim_matches('"');
    let t = t.split('(').next().unwrap_or(t).trim();
    t.to_string()
}

fn numeric_row(cells: &[&str]) -> Option<Vec<f64>> {
    if cells.len() < 2 {
        return None;
    }
    let mut out = Vec::with_capacity(cells.len());
    for c in cells {
        if c.is_empty() {
            out.push(f64::NAN);
        } else {
            out.push(c.parse::<f64>().ok()?);
        }
    }
    if out[0].is_nan() {
        return None;
    }
    Some(out)
}

fn file_label(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

/// Parse the block of a time-resolved text file: header row, then numeric rows
/// until the first row that is not numeric.
fn parse_block(text: &str, path: &Path, format: &'static str) -> Result<ParsedFile, String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut first_data = None;
    for (i, line) in lines.iter().enumerate() {
        let cells = split_cells(line);
        if numeric_row(&cells).is_some() && i > 0 {
            let header = split_cells(lines[i - 1]);
            if header.len() == cells.len() && numeric_row(&header).is_none() {
                first_data = Some(i);
                break;
            }
        }
    }
    let first = first_data.ok_or_else(|| {
        format!(
            "{}: no table of numbers with a header row was found.",
            path.display()
        )
    })?;
    let header = split_cells(lines[first - 1]);
    let time_head = header[0].to_lowercase();
    let time_factor = if time_head.contains("[ms]") || time_head.contains("(ms)") {
        0.001
    } else if time_head.contains("min") {
        60.0
    } else {
        1.0
    };
    let names: Vec<String> = header[1..].iter().map(|c| clean_channel_name(c)).collect();
    let mut time = Vec::new();
    let mut columns = vec![Vec::new(); names.len()];
    for line in &lines[first..] {
        let cells = split_cells(line);
        if cells.len() != header.len() {
            break;
        }
        let Some(row) = numeric_row(&cells) else { break };
        time.push(row[0] * time_factor);
        for (c, v) in columns.iter_mut().zip(&row[1..]) {
            c.push(*v);
        }
    }
    Ok(ParsedFile {
        path: path.to_path_buf(),
        label: file_label(path),
        format,
        stamp: None,
        stamp_text: None,
        time,
        names,
        columns,
    })
}

pub fn parse_fin2(text: &str, path: &Path) -> Result<ParsedFile, String> {
    let mut p = parse_block(text, path, "Thermo Element FIN2")?;
    let line = text.lines().nth(1).unwrap_or("").trim().to_string();
    p.stamp = parse_long_date(&line).map(Stamp::Exact);
    p.stamp_text = Some(line);
    Ok(p)
}

pub fn parse_agilent(text: &str, path: &Path) -> Result<ParsedFile, String> {
    let mut p = parse_block(text, path, "Agilent MassHunter CSV")?;
    for line in text.lines().take(6) {
        if let Some(rest) = line.trim().strip_prefix("Acquired") {
            let rest = rest.trim_start_matches([' ', ':']).trim();
            let stamp_part = rest.split(" using").next().unwrap_or(rest).trim();
            p.stamp = parse_numeric_date(stamp_part).or_else(|| parse_iso(stamp_part).map(Stamp::Exact));
            p.stamp_text = Some(stamp_part.to_string());
        }
    }
    Ok(p)
}

pub fn parse_generic(text: &str, path: &Path) -> Result<ParsedFile, String> {
    parse_block(text, path, "Text table")
}

fn read_text(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(String::from_utf8_lossy(&bytes).trim_start_matches('\u{feff}').to_string())
}

/// Look at the top of a file to decide what it is.
pub fn classify(path: &Path) -> FileKind {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    if name.starts_with("~$") || name.starts_with('.') {
        return FileKind::Other;
    }
    if ext == "fin2" {
        return FileKind::Fin2;
    }
    if !matches!(ext.as_str(), "csv" | "txt" | "asc") {
        return FileKind::Other;
    }
    let Ok(text) = read_text(path) else { return FileKind::Other };
    let top: Vec<&str> = text.lines().take(8).collect();
    let joined = top.join("\n").to_lowercase();
    if joined.contains("laser state") || (joined.contains("laser") && joined.contains("timestamp")) {
        return FileKind::LaserLog;
    }
    if joined.contains("intensity vs time") {
        return FileKind::AgilentCsv;
    }
    if joined.contains("acq. date-time") || joined.contains("acquisition parameters") {
        return FileKind::Other;
    }
    let mut prev_text = false;
    for line in top {
        let cells = split_cells(line);
        if numeric_row(&cells).is_some() && prev_text {
            return FileKind::GenericText;
        }
        prev_text = cells.len() >= 2 && numeric_row(&cells).is_none();
    }
    FileKind::Other
}

/// What a folder or list of paths contains.
#[derive(Clone, Debug, Default)]
pub struct Gathered {
    pub ms_files: Vec<PathBuf>,
    pub laser_log: Option<PathBuf>,
    pub skipped: Vec<PathBuf>,
}

/// Walk folders and sort files into mass-spectrometer data and laser log.
pub fn gather(paths: &[PathBuf]) -> Gathered {
    let mut stack: Vec<PathBuf> = paths.to_vec();
    let mut out = Gathered::default();
    while let Some(path) = stack.pop() {
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "AcqData") {
                continue;
            }
            if let Ok(entries) = std::fs::read_dir(&path) {
                for e in entries.flatten() {
                    stack.push(e.path());
                }
            }
            continue;
        }
        match classify(&path) {
            FileKind::Fin2 | FileKind::AgilentCsv | FileKind::GenericText => out.ms_files.push(path),
            FileKind::LaserLog => out.laser_log = Some(path),
            FileKind::Other => out.skipped.push(path),
        }
    }
    // An Element folder also holds .ASC and .TXT copies of the same spots.
    if out.ms_files.iter().any(|p| classify(p) == FileKind::Fin2) {
        out.ms_files.retain(|p| classify(p) == FileKind::Fin2);
    }
    // An Agilent batch keeps each spot in its .d folder and again under Calc.
    let in_d_folder = |p: &PathBuf| {
        p.parent()
            .and_then(|d| d.extension())
            .is_some_and(|e| e.eq_ignore_ascii_case("d"))
    };
    if out.ms_files.iter().any(in_d_folder) {
        let (keep, drop): (Vec<PathBuf>, Vec<PathBuf>) = out.ms_files.drain(..).partition(in_d_folder);
        out.ms_files = keep;
        out.skipped.extend(drop);
    }
    out.ms_files.sort();
    out
}

fn resolve_stamps(files: &mut [ParsedFile], warnings: &mut Vec<String>) {
    // Pick day-first or month-first, whichever makes the batch span the
    // shorter time. Only impossible dates rule a reading out.
    let numeric: Vec<(u32, u32, i64, f64)> = files
        .iter()
        .filter_map(|f| match f.stamp {
            Some(Stamp::Numeric { a, b, year, seconds }) => Some((a, b, year, seconds)),
            _ => None,
        })
        .collect();
    if numeric.is_empty() {
        return;
    }
    let span = |day_first: bool| -> Option<f64> {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for (a, b, y, s) in &numeric {
            let v = numeric_value(*a, *b, *y, *s, day_first)?;
            lo = lo.min(v);
            hi = hi.max(v);
        }
        Some(hi - lo)
    };
    let day_first = match (span(true), span(false)) {
        (Some(d), Some(m)) => {
            if d != m {
                d <= m
            } else {
                if numeric.iter().any(|(a, b, ..)| a != b) {
                    warnings.push(
                        "Numeric dates could be read day-first or month-first. Day-first was used; \
                         this does not change the time differences within this batch."
                            .to_string(),
                    );
                }
                true
            }
        }
        (Some(_), None) => true,
        (None, Some(_)) => false,
        (None, None) => true,
    };
    for f in files.iter_mut() {
        if let Some(Stamp::Numeric { a, b, year, seconds }) = f.stamp {
            f.stamp = numeric_value(a, b, year, seconds, day_first).map(Stamp::Exact);
        }
    }
}

/// Combine parsed files into one index time, sorted by acquisition time.
pub fn assemble(mut files: Vec<ParsedFile>) -> (Inputs, Vec<usize>) {
    let mut warnings = Vec::new();
    resolve_stamps(&mut files, &mut warnings);
    let stamp_of = |f: &ParsedFile| match f.stamp {
        Some(Stamp::Exact(v)) => Some(v),
        _ => None,
    };
    let all_stamped = files.iter().all(|f| stamp_of(f).is_some());
    let mut order: Vec<usize> = (0..files.len()).collect();
    if all_stamped {
        order.sort_by(|a, b| {
            stamp_of(&files[*a])
                .unwrap_or(0.0)
                .total_cmp(&stamp_of(&files[*b]).unwrap_or(0.0))
        });
    } else if files.len() > 1 {
        warnings.push(
            "Not every file states its acquisition time. Files were joined in name order with a short gap."
                .to_string(),
        );
    }
    let epoch_unix = if all_stamped {
        order.first().and_then(|i| stamp_of(&files[*i])).unwrap_or(0.0)
    } else {
        0.0
    };
    let mut names: Vec<String> = Vec::new();
    for &i in &order {
        for n in &files[i].names {
            if !names.contains(n) {
                names.push(n.clone());
            }
        }
    }
    let mut time: Vec<f64> = Vec::new();
    let mut values: Vec<Vec<f64>> = vec![Vec::new(); names.len()];
    let mut clamped = 0usize;
    for &i in &order {
        let f = &files[i];
        let base = if all_stamped {
            stamp_of(f).unwrap_or(0.0) - epoch_unix
        } else {
            time.last().copied().unwrap_or(0.0) + 10.0
        };
        for (k, t) in f.time.iter().enumerate() {
            let mut value = base + t;
            if let Some(last) = time.last() {
                if value <= *last {
                    // Whole-second file stamps can overlap by a fraction of a
                    // second. Sweeps are moved just far enough to stay in order.
                    value = last + 1e-3;
                    clamped += 1;
                }
            }
            time.push(value);
            for (c, name) in values.iter_mut().zip(&names) {
                let v = f
                    .names
                    .iter()
                    .position(|n| n == name)
                    .map_or(f64::NAN, |j| f.columns[j][k]);
                c.push(v);
            }
        }
    }
    if clamped > 0 {
        warnings.push(format!(
            "{clamped} sweeps overlapped the end of the previous file and were moved forward to stay in time order."
        ));
    }
    let channels = names
        .iter()
        .zip(values)
        .map(|(name, values)| InputChannel {
            name: name.clone(),
            isotope: parse_isotope(name),
            values,
        })
        .collect();
    (
        Inputs {
            epoch_unix,
            time,
            channels,
            laser: Vec::new(),
            warnings,
        },
        order,
    )
}

fn find_col(header: &[String], needles: &[&str]) -> Option<usize> {
    header.iter().position(|h| needles.iter().any(|n| h.contains(n)))
}

/// Parse a laser log. ESI logs list events with a "Laser State" column; a
/// simple table with name, start and end columns also works.
pub fn parse_laser_log(text: &str) -> Result<Vec<LaserSpot>, String> {
    let text = text.trim_start_matches('\u{feff}');
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let head = lines.next().ok_or("The laser log is empty.")?;
    let header: Vec<String> = head.split(',').map(|c| c.trim().to_lowercase()).collect();
    let col_time = find_col(&header, &["timestamp"]);
    let col_state = find_col(&header, &["laser state"]);
    let col_comment = find_col(&header, &["comment", "sample", "name"]);
    let col_x = header.iter().position(|h| h.starts_with("x(") || h == "x");
    let col_y = header.iter().position(|h| h.starts_with("y(") || h == "y");
    let col_rate = find_col(&header, &["rep. rate", "rep rate"]);
    let col_size = find_col(&header, &["spot size"]);
    let col_fluence = find_col(&header, &["fluence"]);
    let number = |cells: &[&str], col: Option<usize>| -> f64 {
        col.and_then(|c| cells.get(c)).and_then(|v| v.trim().parse().ok()).unwrap_or(f64::NAN)
    };
    let mut spots = Vec::new();
    if let (Some(ct), Some(cs)) = (col_time, col_state) {
        let mut label = String::new();
        let mut open: Option<LaserSpot> = None;
        for line in lines {
            let cells: Vec<&str> = line.split(',').collect();
            let Some(t) = cells.get(ct).and_then(|v| parse_iso(v)) else { continue };
            if let Some(c) = col_comment.and_then(|c| cells.get(c)) {
                if !c.trim().is_empty() {
                    label = c.trim().to_string();
                }
            }
            let on = cells.get(cs).is_some_and(|s| s.trim().eq_ignore_ascii_case("on"));
            match (&mut open, on) {
                (None, true) => {
                    open = Some(LaserSpot {
                        label: label.clone(),
                        on: t,
                        off: t,
                        x: number(&cells, col_x),
                        y: number(&cells, col_y),
                        spot_size: number(&cells, col_size),
                        rep_rate: number(&cells, col_rate),
                        fluence: number(&cells, col_fluence),
                    });
                }
                (Some(spot), false) => {
                    spot.off = t;
                    if let Some(s) = open.take() {
                        spots.push(s);
                    }
                }
                _ => {}
            }
        }
        if let Some(s) = open.take() {
            spots.push(s);
        }
    } else {
        let col_start = find_col(&header, &["start"]);
        let col_end = find_col(&header, &["end"]);
        let (Some(cs), Some(ce)) = (col_start, col_end) else {
            return Err(
                "This laser log has neither Timestamp and Laser State columns nor start and end columns."
                    .to_string(),
            );
        };
        for line in lines {
            let cells: Vec<&str> = line.split(',').collect();
            let read = |c: usize| -> Option<f64> {
                let v = cells.get(c)?.trim();
                v.parse::<f64>().ok().or_else(|| parse_iso(v))
            };
            let (Some(on), Some(off)) = (read(cs), read(ce)) else { continue };
            spots.push(LaserSpot {
                label: col_comment
                    .and_then(|c| cells.get(c))
                    .map_or_else(String::new, |s| s.trim().to_string()),
                on,
                off,
                x: number(&cells, col_x),
                y: number(&cells, col_y),
                spot_size: number(&cells, col_size),
                rep_rate: number(&cells, col_rate),
                fluence: number(&cells, col_fluence),
            });
        }
    }
    if spots.is_empty() {
        return Err("No laser firing was found in this log.".to_string());
    }
    Ok(spots)
}

/// Sum over channels other than mercury, on a log scale, per sweep. It rises
/// wherever the laser fires, so it can be compared with the log.
fn log_beam(inputs: &Inputs) -> Vec<f64> {
    let n = inputs.time.len();
    let mut total = vec![0.0; n];
    for c in &inputs.channels {
        if c.isotope.as_ref().is_some_and(|i| i.element == "Hg") {
            continue;
        }
        for (t, v) in total.iter_mut().zip(&c.values) {
            if v.is_finite() && *v > 0.0 {
                *t += v;
            }
        }
    }
    total.iter().map(|v| (v + 1.0).log10()).collect()
}

/// Result of matching the two clocks.
#[derive(Clone, Debug)]
pub struct ClockFit {
    pub offset_s: f64,
    /// Difference of mean log10(beam) with the laser on and off, in decades.
    pub contrast: f64,
    /// Share of firings that fall on recorded sweeps.
    pub coverage: f64,
}

/// Find the shift of the laser clock that lines up firings with the signal:
/// a 1 s grid over +/- 2 hours, refined to 0.05 s. The result is stored in
/// the session and can be overruled by hand.
pub fn align_clock(inputs: &Inputs, spots: &[LaserSpot]) -> Option<ClockFit> {
    if spots.is_empty() || inputs.time.len() < 10 {
        return None;
    }
    let beam = log_beam(inputs);
    let mut prefix = Vec::with_capacity(beam.len() + 1);
    prefix.push(0.0);
    for v in &beam {
        prefix.push(prefix.last().unwrap() + v);
    }
    let total = *prefix.last().unwrap();
    let n = beam.len() as f64;
    let dt = inputs.sweep_seconds().max(1e-3);
    let expected: f64 = spots.iter().map(|s| (s.off - s.on).max(0.0) / dt).sum::<f64>().max(1.0);
    let score = |offset: f64| -> (f64, f64, f64) {
        let (mut inside, mut count) = (0.0, 0.0);
        for s in spots {
            let a = inputs.time.partition_point(|t| *t < s.on + offset - inputs.epoch_unix);
            let b = inputs.time.partition_point(|t| *t < s.off + offset - inputs.epoch_unix);
            if b > a {
                inside += prefix[b] - prefix[a];
                count += (b - a) as f64;
            }
        }
        if count < 1.0 || count >= n {
            return (f64::NEG_INFINITY, 0.0, 0.0);
        }
        let contrast = inside / count - (total - inside) / (n - count);
        let coverage = (count / expected).min(1.0);
        (contrast * coverage, contrast, coverage)
    };
    let centre = inputs.epoch_unix - spots[0].on + inputs.time[0];
    let mut best = (f64::NEG_INFINITY, centre);
    let mut k = -7200.0;
    while k <= 7200.0 {
        let s = score(centre + k).0;
        if s > best.0 {
            best = (s, centre + k);
        }
        k += 1.0;
    }
    let coarse = best.1;
    let mut k = -2.0;
    while k <= 2.0 {
        let s = score(coarse + k).0;
        if s > best.0 {
            best = (s, coarse + k);
        }
        k += 0.05;
    }
    let (_, contrast, coverage) = score(best.1);
    Some(ClockFit {
        offset_s: best.1,
        contrast,
        coverage,
    })
}

pub fn sha256_of(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn source_record(path: &Path, label: &str, format: &str, stamp: Option<String>) -> Result<SourceFile, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(SourceFile {
        path: path.display().to_string(),
        sha256: sha256_of(&bytes),
        bytes: bytes.len() as u64,
        label: label.to_string(),
        format: format.to_string(),
        stamp,
        first_sweep: 0,
        n_sweeps: 0,
    })
}

/// Everything an import produced.
pub struct Imported {
    pub inputs: Inputs,
    pub files: Vec<SourceFile>,
    pub laser_log_file: Option<SourceFile>,
    pub clock: Option<ClockFit>,
}

/// Read mass-spectrometer files and an optional laser log.
pub fn import_files(ms: &[PathBuf], laser_log: Option<&Path>) -> Result<Imported, String> {
    if ms.is_empty() {
        return Err(
            "No data files were found there. ChemChron reads Thermo Element FIN2, Agilent MassHunter CSV and text tables with a time column. Nu Plasma .run files aren't read yet."
                .to_string(),
        );
    }
    let mut parsed = Vec::with_capacity(ms.len());
    let mut problems = Vec::new();
    for path in ms {
        let result = read_text(path).and_then(|text| match classify(path) {
            FileKind::Fin2 => parse_fin2(&text, path),
            FileKind::AgilentCsv => parse_agilent(&text, path),
            _ => parse_generic(&text, path),
        });
        match result {
            Ok(p) => parsed.push(p),
            Err(e) => problems.push(e),
        }
    }
    if parsed.is_empty() {
        return Err(problems.join("\n"));
    }
    let (mut inputs, order) = assemble(parsed.clone());
    inputs.warnings.extend(problems);
    let mut files = Vec::with_capacity(order.len());
    let mut first = 0;
    for &i in &order {
        let p = &parsed[i];
        let mut rec = source_record(&p.path, &p.label, p.format, p.stamp_text.clone())?;
        rec.first_sweep = first;
        rec.n_sweeps = p.time.len();
        first += p.time.len();
        files.push(rec);
    }
    let mut laser_log_file = None;
    let mut clock = None;
    if let Some(path) = laser_log {
        let text = read_text(path)?;
        let spots = parse_laser_log(&text)?;
        laser_log_file = Some(source_record(path, &file_label(path), "Laser log", None)?);
        clock = align_clock(&inputs, &spots);
        inputs.laser = spots;
    }
    Ok(Imported {
        inputs,
        files,
        laser_log_file,
        clock,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIN2: &str = "Finnigan MAT ELEMENT Raw Data\r\nTuesday, January 21, 2025 13:16:45\r\nx.FIN\r\n3\r\n0\r\n0,0\r\nCPS\r\nTime,Pb206,U238\r\n0.276000,133.3,112.0\r\n0.550000,311.3,785.3\r\n0.824000,66.5,561.0\r\n";

    const AGILENT: &str = "D:\\Agilent\\x.d\nIntensity Vs Time,CPS\nAcquired      : 11/08/2026 13:57:51 using Batch x.b\nTime [Sec],Hg202,U238\n0.1649,1000.04,0.00\n0.2999,2000.15,33.33\n\n\n          Printed:11/08/2026 13:59:15\n";

    #[test]
    fn element_file_reads_clock_and_columns() {
        let p = parse_fin2(FIN2, Path::new("A27_1.FIN2")).unwrap();
        assert_eq!(p.names, ["Pb206", "U238"]);
        assert_eq!(p.time.len(), 3);
        assert!((p.columns[1][1] - 785.3).abs() < 1e-9);
        let want = parse_iso("2025-01-21 13:16:45").unwrap();
        assert_eq!(p.stamp, Some(Stamp::Exact(want)));
    }

    #[test]
    fn agilent_file_stops_at_the_footer_and_keeps_its_date_undecided() {
        let p = parse_agilent(AGILENT, Path::new("001NIST610.csv")).unwrap();
        assert_eq!(p.time.len(), 2);
        assert_eq!(p.names, ["Hg202", "U238"]);
        assert!(matches!(p.stamp, Some(Stamp::Numeric { a: 11, b: 8, year: 2026, .. })));
    }

    #[test]
    fn dates_are_read_the_way_that_keeps_the_batch_together() {
        let mk = |text: &str| {
            let mut p = parse_agilent(AGILENT, Path::new("a.csv")).unwrap();
            p.stamp = parse_numeric_date(text);
            p
        };
        // 31/08 then 01/09 only makes sense day-first.
        let (inp, _) = assemble(vec![mk("31/08/2026 23:59:00"), mk("01/09/2026 00:01:00")]);
        assert!(inp.time.last().unwrap() < &200.0, "{:?}", inp.time);
    }

    #[test]
    fn files_are_sorted_by_clock_time_and_channels_united() {
        let mut a = parse_fin2(FIN2, Path::new("late.FIN2")).unwrap();
        a.stamp = Some(Stamp::Exact(1000.0));
        let mut b = parse_fin2(FIN2, Path::new("early.FIN2")).unwrap();
        b.stamp = Some(Stamp::Exact(900.0));
        b.names = vec!["Pb206".into(), "Th232".into()];
        let (inp, order) = assemble(vec![a, b]);
        assert_eq!(order, vec![1, 0]);
        assert_eq!(inp.channels.len(), 3);
        assert!(inp.time.windows(2).all(|w| w[1] > w[0]));
        // U238 was not in the earlier file.
        let u = inp.channel_index("U238").unwrap();
        assert!(inp.channels[u].values[0].is_nan());
        assert!(inp.channels[u].values[3].is_finite());
    }

    #[test]
    fn esi_log_becomes_named_firings() {
        let log = "\u{feff}Timestamp, Sequence Number, SubPoint Number, Vertix Number, Comment, X(um), Y(um), Intended X(um), Intended Y(um), Scan Velocity (um/s), Laser State, Laser Rep. Rate (Hz), Spot Type, Spot Size (um)\n\
2025-01-21 14:15:07.886,1,1,,M44,108362,89941,,,,Off,0,Wheel,10.0\n\
2025-01-21 14:15:29.472,,,,,108362,89941,,,,On,5,Wheel,10.0\n\
2025-01-21 14:15:51.552,,,,,108362,89941,,,,Off,0,Wheel,10.0\n\
2025-01-21 14:15:55.767,2,1,,m01,108400,89941,,,,Off,0,Wheel,10.0\n\
2025-01-21 14:16:17.000,,,,,108400,89941,,,,On,5,Wheel,10.0\n\
2025-01-21 14:16:39.000,,,,,108400,89941,,,,Off,0,Wheel,10.0\n";
        let spots = parse_laser_log(log).unwrap();
        assert_eq!(spots.len(), 2);
        assert_eq!(spots[0].label, "M44");
        assert_eq!(spots[1].label, "m01");
        assert!((spots[0].off - spots[0].on - 22.08).abs() < 1e-6);
        assert_eq!(spots[0].rep_rate, 5.0);
        assert_eq!(spots[0].spot_size, 10.0);
    }

    #[test]
    fn clock_offset_is_recovered() {
        let time: Vec<f64> = (0..4000).map(|i| i as f64 * 0.25).collect();
        let truth = -3657.3;
        let epoch = 1_700_000_000.0;
        let mut spots = Vec::new();
        let mut values = vec![100.0; time.len()];
        for k in 0..6 {
            let on_index = epoch + 30.0 + k as f64 * 150.0;
            spots.push(LaserSpot {
                label: format!("s{k}"),
                on: on_index - truth,
                off: on_index - truth + 22.0 + (k % 3) as f64,
                x: 0.0,
                y: 0.0,
                spot_size: 10.0,
                rep_rate: 5.0,
                fluence: 1.0,
            });
            let start = 30.0 + k as f64 * 150.0;
            let end = start + 22.0 + (k % 3) as f64;
            for (i, t) in time.iter().enumerate() {
                if *t >= start && *t < end {
                    values[i] = 1e6;
                }
            }
        }
        let inputs = Inputs {
            epoch_unix: epoch,
            time,
            channels: vec![InputChannel {
                name: "U238".into(),
                isotope: parse_isotope("U238"),
                values,
            }],
            laser: Vec::new(),
            warnings: Vec::new(),
        };
        let fit = align_clock(&inputs, &spots).unwrap();
        assert!((fit.offset_s - truth).abs() < 0.3, "{}", fit.offset_s);
        assert!(fit.contrast > 3.0);
    }
}
