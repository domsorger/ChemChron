// SPDX-License-Identifier: GPL-3.0-or-later
//! Opening data and saving sessions. A session file is readable JSON holding
//! decisions only. Raw files stay where they are, referenced by path and hash,
//! and are checked again when the session is opened.

use crate::autosel::{from_log, from_signal, AutoOptions};
use crate::geochron::DecayConstants;
use crate::io;
use crate::model::*;
use crate::refmat;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Build a new session from files or folders: read everything, line up the
/// clocks and make selections.
pub fn session_from_paths(paths: &[PathBuf], log_override: Option<&Path>) -> Result<Session, String> {
    let gathered = io::gather(paths);
    let log_path = log_override.map(Path::to_path_buf).or(gathered.laser_log);
    let imported = io::import_files(&gathered.ms_files, log_path.as_deref())?;
    let name = paths
        .first()
        .and_then(|p| p.file_name())
        .map_or_else(|| "New session".to_string(), |n| n.to_string_lossy().to_string());
    let source = SessionSource::Files {
        paths: gathered.ms_files.iter().map(|p| p.display().to_string()).collect(),
        laser_log: log_path.as_ref().map(|p| p.display().to_string()),
    };
    let rms = refmat::builtin(&DecayConstants::default());
    let mut session = Session::new(&name, source, imported.inputs, rms);
    session.files = imported.files;
    session.laser_log_file = imported.laser_log_file;
    if let Some(fit) = &imported.clock {
        session.clock_offset_s = fit.offset_s;
        session.clock_note = format!(
            "Found automatically: laser-on signal stands {:.2} decades above the rest; {:.0} % of firings fall on recorded sweeps.",
            fit.contrast,
            fit.coverage * 100.0
        );
    }
    session.settings.upb.beam = if session.inputs.laser.is_empty() {
        BeamMethod::SignalJump
    } else {
        BeamMethod::LaserLog
    };
    auto_select(&mut session, &AutoOptions::default());
    session.apply_defaults();
    Ok(session)
}

/// Make selections from the log when there is one, else from the signal.
pub fn auto_select(session: &mut Session, options: &AutoOptions) -> usize {
    if session.inputs.laser.is_empty() {
        from_signal(session, options)
    } else {
        from_log(session, options)
    }
}

/// Replace selections with a table of `kind,label,group,start,end`, for example
/// exported from another program, so both can be compared on the same windows.
/// Kind is "baseline" or anything else for an analysis. Start and end are clock
/// times like 2025-01-21 13:14:26.276 on the mass spectrometer's clock, or
/// seconds of index time. Returns the number of selections made.
pub fn apply_selection_table(session: &mut Session, text: &str) -> Result<usize, String> {
    let inputs = session.inputs.clone();
    let mut rows = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cells: Vec<&str> = line.split(',').map(str::trim).collect();
        if cells.len() < 5 {
            return Err(format!("Line {}: expected kind, label, group, start and end.", index + 1));
        }
        let read = |v: &str| v.parse::<f64>().ok().or_else(|| io::parse_iso(v).map(|t| t - inputs.epoch_unix));
        let (Some(start), Some(end)) = (read(cells[3]), read(cells[4])) else {
            if index == 0 {
                continue;
            }
            return Err(format!("Line {}: can't read the times \"{}\" and \"{}\".", index + 1, cells[3], cells[4]));
        };
        rows.push((cells[0].eq_ignore_ascii_case("baseline"), cells[1].to_string(), cells[2].to_string(), start, end));
    }
    if rows.is_empty() {
        return Err("The table has no selections.".to_string());
    }
    let windows = rows
        .into_iter()
        .map(|(baseline, label, group, start, end)| Window { baseline, label, group, start, end })
        .collect();
    Ok(apply_windows(session, windows))
}

/// One selection window, in index time.
pub struct Window {
    pub baseline: bool,
    pub label: String,
    pub group: String,
    pub start: f64,
    pub end: f64,
}

/// Replace all selections with these windows, matching each analysis to its laser firing.
pub fn apply_windows(session: &mut Session, windows: Vec<Window>) -> usize {
    let inputs = session.inputs.clone();
    let offset = session.clock_offset_s;
    let firing_on: Vec<f64> = inputs.laser.iter().map(|s| s.on + offset - inputs.epoch_unix).collect();
    session.selections.clear();
    let count = windows.len();
    for w in windows {
        let kind = if w.baseline { GroupKind::Baseline } else { GroupKind::Sample };
        let group = session.ensure_group(if w.baseline { "Baseline" } else { &w.group }, kind);
        let id = session.next_id();
        // The firing this analysis belongs to started shortly before or just after its start.
        let laser_on = if w.baseline {
            None
        } else {
            firing_on
                .iter()
                .copied()
                .min_by(|a, b| (a - w.start).abs().total_cmp(&(b - w.start).abs()))
                .filter(|on| (on - w.start).abs() < 30.0)
        };
        session.selections.push(Selection {
            id,
            group,
            label: w.label,
            start: w.start,
            end: w.end,
            comment: String::new(),
            excluded: false,
            source: SelectionSource::Manual,
            laser_on,
            is_ppm: None,
            cutouts: Vec::new(),
            x: None,
            y: None,
            spot_size: None,
        });
    }
    session.sort_selections();
    count
}

/// Internal-standard values per selection from a table of label and ppm.
/// Returns how many selections got a value, and labels that matched nothing.
/// Internal-standard values per selection from a table of selection label and
/// value, matched by exact label; `to_ppm` turns the table's unit into element ppm.
pub fn apply_is_table(session: &mut Session, text: &str) -> Result<(usize, Vec<String>), String> {
    apply_is_table_in(session, text, 1.0)
}

pub fn apply_is_table_in(session: &mut Session, text: &str, to_ppm: f64) -> Result<(usize, Vec<String>), String> {
    let mut applied = 0;
    let mut unmatched = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() {
            continue;
        }
        let cells: Vec<&str> = line.split([',', ';', '\t']).map(str::trim).collect();
        if cells.len() < 2 {
            return Err(format!("Line {}: expected a selection label and a value.", index + 1));
        }
        let Ok(value) = cells[1].parse::<f64>() else {
            if index == 0 {
                continue;
            }
            return Err(format!("Line {}: \"{}\" is not a number.", index + 1, cells[1]));
        };
        let mut found = false;
        for s in session.selections.iter_mut().filter(|s| s.label == cells[0]) {
            s.is_ppm = Some(value * to_ppm);
            found = true;
            applied += 1;
        }
        if !found {
            unmatched.push(cells[0].to_string());
        }
    }
    Ok((applied, unmatched))
}

pub fn save_session(session: &Session, path: &Path) -> Result<(), String> {
    let mut copy = session.clone();
    copy.software_version = crate::VERSION.to_string();
    let text = serde_json::to_string_pretty(&copy).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Result of opening a saved session.
pub struct Loaded {
    pub session: Session,
    /// Files whose contents no longer match their recorded hash, or that are gone.
    pub changed_files: Vec<String>,
}

pub fn load_session(path: &Path) -> Result<Loaded, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let session: Session =
        serde_json::from_str(&text).map_err(|e| format!("This is not a readable session file: {e}"))?;
    let mut dirs = Vec::new();
    if let Some(d) = path.parent() {
        dirs.push(d.to_path_buf());
        if let Some(up) = d.parent() {
            dirs.push(up.to_path_buf());
        }
    }
    reopen_session(session, &dirs)
}

/// Re-reads the raw data of a deserialised session. Files that are no longer
/// where the session says are looked for in `search_dirs` (same relative
/// place, or anywhere below with the same name and contents).
pub fn reopen_session(mut session: Session, search_dirs: &[PathBuf]) -> Result<Loaded, String> {
    if session.format == OLD_SESSION_FORMAT {
        session.format = SESSION_FORMAT.to_string();
    }
    if session.format != SESSION_FORMAT {
        return Err(format!("Unknown session format \"{}\".", session.format));
    }
    let mut changed = relocate_session(&mut session, search_dirs);
    match session.source.clone() {
        SessionSource::Synthetic { seed, part } => {
            session.inputs = crate::synthetic::demo_session_part(seed, part).inputs;
        }
        SessionSource::Files { paths, laser_log } => {
            let list: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
            for record in session.files.iter().chain(session.laser_log_file.iter()) {
                match std::fs::read(&record.path) {
                    Ok(bytes) if io::sha256_of(&bytes) == record.sha256 => {}
                    Ok(_) => changed.push(format!("{} has changed since this session was saved.", record.path)),
                    Err(_) => changed.push(format!("{} can't be read.", record.path)),
                }
            }
            let log = laser_log.as_ref().map(PathBuf::from);
            let imported = io::import_files(&list, log.as_deref()).map_err(|e| {
                format!("{e} The data may have moved: open the session again and choose \"Find moved data\" to point to the new folder.")
            })?;
            session.inputs = Arc::new(imported.inputs);
        }
    }
    Ok(Loaded { session, changed_files: changed })
}

/// Where a missing file is now: the same trailing path below one of `dirs`,
/// else a file with the same name and (when known) contents up to 5 levels below.
pub fn find_moved(original: &str, sha256: Option<&str>, dirs: &[PathBuf]) -> Option<PathBuf> {
    let orig = Path::new(original);
    if orig.exists() {
        return None;
    }
    let parts: Vec<String> = orig.components().filter_map(|c| match c {
        std::path::Component::Normal(s) => Some(s.to_string_lossy().to_string()),
        _ => None,
    }).collect();
    let matches = |p: &Path| -> bool {
        match sha256 {
            Some(h) if p.is_file() => std::fs::read(p).map(|b| io::sha256_of(&b) == h).unwrap_or(false),
            _ => p.exists(),
        }
    };
    for d in dirs {
        for k in (1..=parts.len().min(6)).rev() {
            let mut cand = d.clone();
            for part in &parts[parts.len() - k..] {
                cand.push(part);
            }
            if matches(&cand) {
                return Some(cand);
            }
        }
    }
    let name = parts.last()?;
    fn walk(dir: &Path, name: &str, depth: usize, ok: &dyn Fn(&Path) -> bool) -> Option<PathBuf> {
        if depth > 5 {
            return None;
        }
        let entries = std::fs::read_dir(dir).ok()?;
        let mut subdirs = Vec::new();
        for e in entries.flatten() {
            let p = e.path();
            if p.file_name().is_some_and(|n| n.to_string_lossy() == name) && ok(&p) {
                return Some(p);
            }
            if p.is_dir() {
                subdirs.push(p);
            }
        }
        subdirs.into_iter().find_map(|s| walk(&s, name, depth + 1, ok))
    }
    dirs.iter().find_map(|d| walk(d, name, 0, &matches))
}

/// Points the session's file paths to where moved files are now. Returns notes.
pub fn relocate_session(session: &mut Session, dirs: &[PathBuf]) -> Vec<String> {
    if dirs.is_empty() {
        return Vec::new();
    }
    let mut map: Vec<(String, String)> = Vec::new();
    let try_one = |p: &str, sha: Option<&str>, map: &mut Vec<(String, String)>| {
        if map.iter().any(|m| m.0 == p) {
            return;
        }
        if let Some(n) = find_moved(p, sha, dirs) {
            map.push((p.to_string(), n.to_string_lossy().to_string()));
        }
    };
    for f in session.files.iter().chain(session.laser_log_file.iter()) {
        try_one(&f.path, Some(&f.sha256), &mut map);
    }
    if let SessionSource::Files { paths, laser_log } = &session.source {
        for p in paths.iter().chain(laser_log.iter()) {
            try_one(p, None, &mut map);
        }
    }
    if map.is_empty() {
        return Vec::new();
    }
    let fix = |p: &mut String| {
        if let Some((_, n)) = map.iter().find(|m| m.0 == *p) {
            *p = n.clone();
        }
    };
    for f in session.files.iter_mut().chain(session.laser_log_file.iter_mut()) {
        fix(&mut f.path);
    }
    if let SessionSource::Files { paths, laser_log } = &mut session.source {
        for p in paths.iter_mut() {
            fix(p);
        }
        if let Some(l) = laser_log {
            fix(l);
        }
    }
    vec![format!("{} moved file(s) found in their new place; save the session to keep the new locations.", map.len())]
}
