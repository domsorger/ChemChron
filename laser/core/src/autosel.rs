// SPDX-License-Identifier: GPL-3.0-or-later
//! Automatic selections, from the laser log or from the signal itself.

use crate::model::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug)]
pub struct AutoOptions {
    /// Seconds dropped after the laser fires: wash-in.
    pub crop_start_s: f64,
    /// Seconds dropped before the laser stops.
    pub crop_end_s: f64,
    /// Seconds dropped before a baseline starts: washout after a firing, or
    /// the settling at the start of a data file (each spot often is its own file).
    pub baseline_crop_start_s: f64,
    /// Seconds dropped between the end of a baseline and the next firing.
    pub baseline_crop_end_s: f64,
    /// Shortest sample selection kept, in seconds.
    pub min_seconds: f64,
    /// Shortest baseline kept, in seconds.
    pub min_baseline_s: f64,
}

impl Default for AutoOptions {
    fn default() -> Self {
        AutoOptions {
            crop_start_s: 3.0,
            crop_end_s: 1.0,
            baseline_crop_start_s: 2.0,
            baseline_crop_end_s: 2.0,
            min_seconds: 5.0,
            min_baseline_s: 3.0,
        }
    }
}

fn is_separator(c: char) -> bool {
    matches!(c, '_' | '-' | ' ' | '.')
}

/// Drop a run number from the front: "003_44069" and "001NIST610".
fn strip_run_prefix(label: &str) -> &str {
    let digits = label.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits >= 2 && digits < label.len() {
        let rest = &label[digits..];
        let next = rest.chars().next().unwrap_or('x');
        if is_separator(next) {
            let tail = rest.trim_start_matches(is_separator);
            // "003_44069": the run number is the shorter of two numbers.
            // "44069_1": here the material comes first, so keep the whole label.
            let numeric_tail = !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit());
            if numeric_tail && tail.len() <= digits {
                return label;
            }
            return tail;
        }
        if next.is_alphabetic() {
            return rest;
        }
    }
    label
}

fn split_tail_number(label: &str) -> Option<(&str, &str)> {
    let digits = label.chars().rev().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 || digits == label.len() {
        return None;
    }
    let cut = label.len() - digits;
    Some((&label[..cut], &label[cut..]))
}

/// Work out sample groups from labels. A trailing run number is dropped only
/// where the labels show a series: "M44_1..M44_20" becomes M44 and
/// "m01..m86" becomes m, while "KM03-72" and "KM03-73" stay apart and
/// "NIST610" or "A27" keep their numbers.
pub fn group_names(labels: &[String]) -> Vec<String> {
    group_names_known(labels, &|_| false)
}

/// The same, and a series whose name is a known reference material (e.g.
/// "612_1", "612_2" for NIST 612) is grouped however few spots it has.
pub fn group_names_known(labels: &[String], known: &dyn Fn(&str) -> bool) -> Vec<String> {
    let stripped: Vec<&str> = labels.iter().map(|l| strip_run_prefix(l)).collect();
    let mut series: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for s in &stripped {
        if let Some((stem, number)) = split_tail_number(s) {
            let clean = stem.trim_end_matches(is_separator);
            series.entry(clean.to_string()).or_default().insert(number.to_string());
        }
    }
    stripped
        .iter()
        .map(|s| {
            if let Some((stem, _)) = split_tail_number(s) {
                let stem_clean = stem.trim_end_matches(is_separator);
                let distinct = series.get(stem_clean).map_or(0, BTreeSet::len);
                if !stem_clean.is_empty() {
                    let has_separator = stem.len() != stem_clean.len();
                    if (has_separator && distinct >= 3) || (!has_separator && distinct >= 5) || (has_separator && known(stem_clean)) {
                        return stem_clean.to_string();
                    }
                }
            }
            s.to_string()
        })
        .collect()
}

/// Selections from the laser log: one per firing, and baselines in the quiet
/// stretches between firings where sweeps were recorded.
pub fn from_log(session: &mut Session, opts: &AutoOptions) -> usize {
    let inputs = session.inputs.clone();
    if inputs.laser.is_empty() {
        return 0;
    }
    session.selections.clear();
    session.groups.clear();
    let baseline = session.ensure_group("Baseline", GroupKind::Baseline);
    let to_index = |unix: f64, offset: f64| unix + offset - inputs.epoch_unix;
    let offset = session.clock_offset_s;
    let labels: Vec<String> = inputs.laser.iter().map(|s| s.label.clone()).collect();
    let rms = session.rms.clone();
    let names = group_names_known(&labels, &|n| rms.iter().any(|m| m.matches_label(n)));
    let segments = inputs.segments();
    let mut count = 0;
    let mut firings: Vec<(f64, f64)> = Vec::new();
    // A log can repeat one name for many firings. Number those, so every
    // selection can be told apart in results and exports.
    let mut totals: BTreeMap<&str, usize> = BTreeMap::new();
    for l in &labels {
        *totals.entry(l.as_str()).or_default() += 1;
    }
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for (spot, name) in inputs.laser.iter().zip(&names) {
        let number = seen.entry(spot.label.as_str()).or_default();
        *number += 1;
        let unique = if totals.get(spot.label.as_str()).copied().unwrap_or(0) > 1 {
            format!("{}_{number}", spot.label)
        } else {
            spot.label.clone()
        };
        let on = to_index(spot.on, offset);
        let off = to_index(spot.off, offset);
        firings.push((on, off));
        let start = on + opts.crop_start_s;
        let end = off - opts.crop_end_s;
        let range = inputs.range(start, end);
        if range.is_empty() || (range.len() as f64) * inputs.sweep_seconds() < opts.min_seconds {
            continue;
        }
        let group = session.ensure_group(if name.is_empty() { "Unnamed" } else { name }, GroupKind::Sample);
        let id = session.next_id();
        session.selections.push(Selection {
            id,
            group,
            label: unique,
            start,
            end,
            comment: String::new(),
            excluded: false,
            source: SelectionSource::LaserLog,
            laser_on: Some(on),
            is_ppm: None,
            cutouts: Vec::new(),
            x: spot.x.is_finite().then_some(spot.x),
            y: spot.y.is_finite().then_some(spot.y),
            spot_size: spot.spot_size.is_finite().then_some(spot.spot_size),
        });
        count += 1;
    }
    for segment in segments {
        let seg_start = inputs.time[segment.start];
        let seg_end = inputs.time[segment.end - 1];
        // Quiet stretches inside this run of sweeps.
        let mut cursor = seg_start;
        let mut quiet = Vec::new();
        for (on, off) in &firings {
            if *off < seg_start || *on > seg_end {
                continue;
            }
            if *on > cursor {
                quiet.push((cursor, *on));
            }
            cursor = cursor.max(*off);
        }
        if cursor < seg_end {
            quiet.push((cursor, seg_end));
        }
        for (a, b) in quiet {
            let start = a + opts.baseline_crop_start_s;
            let end = b - opts.baseline_crop_end_s;
            if end - start >= opts.min_baseline_s {
                let id = session.next_id();
                session.selections.push(new_baseline(id, baseline, start, end, SelectionSource::LaserLog));
                count += 1;
            }
        }
    }
    session.sort_selections();
    count
}

fn new_baseline(id: u64, group: u64, start: f64, end: f64, source: SelectionSource) -> Selection {
    Selection {
        id,
        group,
        label: format!("Baseline {id}"),
        start,
        end,
        comment: String::new(),
        excluded: false,
        source,
        laser_on: None,
        is_ppm: None,
        cutouts: Vec::new(),
        x: None,
        y: None,
        spot_size: None,
    }
}

/// Sum of all channels but mercury, per sweep.
pub fn total_beam(inputs: &Inputs) -> Vec<f64> {
    let mut total = vec![0.0; inputs.time.len()];
    for c in &inputs.channels {
        if c.isotope.as_ref().is_some_and(|i| i.element == "Hg") {
            continue;
        }
        for (t, v) in total.iter_mut().zip(&c.values) {
            if v.is_finite() {
                *t += v;
            }
        }
    }
    total
}

/// Five-point box smoothing.
pub fn box5(values: &[f64]) -> Vec<f64> {
    let n = values.len();
    (0..n)
        .map(|i| {
            let a = i.saturating_sub(2);
            let b = (i + 3).min(n);
            values[a..b].iter().sum::<f64>() / (b - a) as f64
        })
        .collect()
}

/// Otsu's threshold on log10 values: the split between quiet and firing sweeps.
pub fn otsu_threshold(values: &[f64]) -> f64 {
    let logs: Vec<f64> = values
        .iter()
        .filter(|v| v.is_finite())
        .map(|v| (v.max(0.0) + 1.0).log10())
        .collect();
    if logs.is_empty() {
        return 0.0;
    }
    let lo = logs.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = logs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if hi <= lo {
        return 10f64.powf(hi);
    }
    const BINS: usize = 128;
    let mut hist = [0usize; BINS];
    for v in &logs {
        let b = (((v - lo) / (hi - lo)) * (BINS as f64 - 1.0)) as usize;
        hist[b] += 1;
    }
    let total = logs.len() as f64;
    let sum_all: f64 = hist.iter().enumerate().map(|(i, h)| i as f64 * *h as f64).sum();
    let (mut w0, mut sum0, mut best, mut split) = (0.0, 0.0, -1.0, 0usize);
    for (i, h) in hist.iter().enumerate() {
        w0 += *h as f64;
        if w0 == 0.0 || w0 == total {
            continue;
        }
        sum0 += i as f64 * *h as f64;
        let w1 = total - w0;
        let m0 = sum0 / w0;
        let m1 = (sum_all - sum0) / w1;
        let between = w0 * w1 * (m0 - m1) * (m0 - m1);
        if between > best {
            best = between;
            split = i;
        }
    }
    10f64.powf(lo + (split as f64 + 0.5) / BINS as f64 * (hi - lo))
}

/// Selections from the signal: runs above a threshold found from the data,
/// named after the source file. Used when there is no laser log.
pub fn from_signal(session: &mut Session, opts: &AutoOptions) -> usize {
    let inputs = session.inputs.clone();
    if inputs.time.is_empty() {
        return 0;
    }
    session.selections.clear();
    session.groups.clear();
    let baseline = session.ensure_group("Baseline", GroupKind::Baseline);
    let smooth = box5(&total_beam(&inputs));
    let threshold = otsu_threshold(&smooth);
    let dt = inputs.sweep_seconds().max(1e-6);
    let min_sweeps = (opts.min_seconds / dt) as usize;
    let mut found: Vec<(usize, usize, usize)> = Vec::new();
    for segment in inputs.segments() {
        let mut i = segment.start;
        while i < segment.end {
            if smooth[i] > threshold {
                let start = i;
                while i < segment.end && smooth[i] > threshold {
                    i += 1;
                }
                if i - start >= min_sweeps {
                    found.push((segment.start, start, i));
                }
            } else {
                i += 1;
            }
        }
    }
    // Name each run after its source file; a file with several runs gets
    // numbered labels.
    let file_of = |sweep: usize| -> Option<&SourceFile> {
        session
            .files
            .iter()
            .find(|f| sweep >= f.first_sweep && sweep < f.first_sweep + f.n_sweeps)
    };
    let labels: Vec<String> = found
        .iter()
        .enumerate()
        .map(|(k, (_, start, _))| file_of(*start).map_or_else(|| format!("Spot {}", k + 1), |f| f.label.clone()))
        .collect();
    let rms = session.rms.clone();
    let names = group_names_known(&labels, &|n| rms.iter().any(|m| m.matches_label(n)));
    let mut count = 0;
    let mut last_end_time: Option<(usize, f64)> = None;
    for (k, (seg_start, start, end)) in found.iter().enumerate() {
        let rise = inputs.time[*start];
        let fall = inputs.time[*end - 1];
        let group = session.ensure_group(&names[k], GroupKind::Sample);
        let id = session.next_id();
        session.selections.push(Selection {
            id,
            group,
            label: labels[k].clone(),
            start: rise + opts.crop_start_s,
            end: fall - opts.crop_end_s,
            comment: String::new(),
            excluded: false,
            source: SelectionSource::Signal,
            laser_on: None,
            is_ppm: None,
            cutouts: Vec::new(),
            x: None,
            y: None,
            spot_size: None,
        });
        count += 1;
        let a = match last_end_time {
            Some((seg, t)) if seg == *seg_start => t,
            _ => inputs.time[*seg_start],
        } + opts.baseline_crop_start_s;
        let b = rise - opts.baseline_crop_end_s;
        if b - a >= opts.min_baseline_s {
            let id = session.next_id();
            session.selections.push(new_baseline(id, baseline, a, b, SelectionSource::Signal));
            count += 1;
        }
        last_end_time = Some((*seg_start, fall));
    }
    session.sort_selections();
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn element_names_group_into_series() {
        let l = labels(&["M44_1", "M44_2", "M44_10", "MBA_1", "MBA_2", "MBA-7", "A27_1", "A27_2", "A27_3"]);
        let g = group_names(&l);
        assert_eq!(g[0], "M44");
        assert_eq!(g[2], "M44");
        assert_eq!(g[3], "MBA");
        assert_eq!(g[6], "A27");
    }

    #[test]
    fn log_comments_keep_material_names_and_series_collapse() {
        let mut l = labels(&["M44", "MBA", "A27"]);
        for i in 1..=12 {
            l.push(format!("m{i:02}"));
        }
        let g = group_names(&l);
        assert_eq!(g[0], "M44");
        assert_eq!(g[2], "A27");
        assert_eq!(g[3], "m");
        assert_eq!(g[14], "m");
    }

    #[test]
    fn agilent_run_prefixes_are_dropped() {
        let l = labels(&["001NIST610", "003_44069", "016KM03-72", "017KM03-73", "006Weinsberg", "44069_1", "44069_2", "44069_3"]);
        let g = group_names(&l);
        assert_eq!(g[5], "44069");
        assert_eq!(g[7], "44069");
        assert_eq!(g[0], "NIST610");
        assert_eq!(g[1], "44069");
        assert_eq!(g[2], "KM03-72");
        assert_eq!(g[3], "KM03-73");
        assert_eq!(g[4], "Weinsberg");
    }

    #[test]
    fn glass_spots_group_even_when_few() {
        let l = labels(&["001_612_1", "002_612_2", "003_BCR2", "004GA1_1_1", "005GA1_1_2", "024_612_1", "025_612_2"]);
        let g = group_names_known(&l, &|n| n == "612");
        assert_eq!(g[0], "612");
        assert_eq!(g[1], "612");
        assert_eq!(g[6], "612");
        assert_eq!(g[2], "BCR2");
        let plain = group_names(&l);
        assert_eq!(plain[0], "612_1", "unchanged without the reference list");
    }

    #[test]
    fn threshold_separates_two_levels() {
        let mut v = vec![100.0; 100];
        v.extend(vec![100000.0; 100]);
        let t = otsu_threshold(&v);
        assert!(t > 100.0 && t < 100000.0, "{t}");
    }
}
