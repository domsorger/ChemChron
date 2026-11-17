// SPDX-License-Identifier: GPL-3.0-or-later
//! Measurement sessions and the per-import detection-limit rule.

use crate::row::*;

pub const EPMA_SESSION: &str = "EPMA_Session";
pub const UPB_SESSION: &str = "UPb_Session";
pub const UNASSIGNED: &str = "(unassigned)";
pub const DETECTION_RULE_VERSION: &str = "per_import_v1";

pub fn session_name_from_path(path: &str) -> String {
    let p = std::path::Path::new(path);
    p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

pub fn epma_session(row: &Row) -> String {
    text(row, EPMA_SESSION).trim().to_string()
}

pub fn upb_session(row: &Row) -> String {
    text(row, UPB_SESSION).trim().to_string()
}

pub fn display_session(name: &str) -> &str {
    if name.is_empty() {
        UNASSIGNED
    } else {
        name
    }
}

/// Sorted session names; the unassigned one ("") last.
pub fn sessions_in(rows: &[Row], upb: bool) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for row in rows {
        let s = if upb {
            if !crate::upb::has_upb(row) {
                continue;
            }
            upb_session(row)
        } else {
            if truthy(row, "No_EPMA") {
                continue;
            }
            epma_session(row)
        };
        if !out.contains(&s) {
            out.push(s);
        }
    }
    out.sort_by(|a, b| (a.is_empty(), a).cmp(&(b.is_empty(), b)));
    out
}

pub fn fill_missing_upb_sessions(rows: &mut [Row]) -> usize {
    let mut n = 0;
    for row in rows.iter_mut() {
        if !upb_session(row).is_empty() || !crate::upb::has_upb(row) {
            continue;
        }
        let src = text(row, "UPb_Source");
        if let Some(last) = src.split("; ").filter(|s| !s.is_empty()).last() {
            let name = session_name_from_path(last);
            set(row, UPB_SESSION, jstr(name));
            n += 1;
        }
    }
    n
}

pub fn below_dl_set(row: &Row) -> Vec<String> {
    text(row, "Below_DL").split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

/// Marks as below detection limit every oxide measured somewhere in this
/// import but without a positive value in a row. Returns flags added.
pub fn apply_detection_rule(rows: &mut [Row], oxides: &[String]) -> usize {
    let mut uniq: Vec<String> = Vec::new();
    for o in oxides {
        if !uniq.contains(o) {
            uniq.push(o.clone());
        }
    }
    let measured: Vec<String> = uniq
        .into_iter()
        .filter(|ox| rows.iter().any(|r| num_loose(r, &format!("{ox}_wt%")).unwrap_or(0.0) > 0.0))
        .collect();
    let mut added = 0;
    for row in rows.iter_mut() {
        let mut flags = below_dl_set(row);
        for ox in &measured {
            let v = num_loose(row, &format!("{ox}_wt%"));
            if v.is_none_or(|v| v <= 0.0) && !flags.contains(ox) {
                flags.push(ox.clone());
                added += 1;
            }
        }
        if !flags.is_empty() {
            set(row, "Below_DL", jstr(flags.join(",")));
        }
    }
    added
}
