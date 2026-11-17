// SPDX-License-Identifier: GPL-3.0-or-later
//! Chondrite-normalised REE (+ optional Y) patterns.

use crate::constants::oxide_element_fraction;
use crate::row::*;
use crate::trace::{grain_key, trace_value};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const REE_SEQUENCE: [&str; 14] = ["La", "Ce", "Pr", "Nd", "Sm", "Eu", "Gd", "Tb", "Dy", "Ho", "Er", "Tm", "Yb", "Lu"];
pub const NORMALIZATION_ELEMENTS: [&str; 15] =
    ["Y", "La", "Ce", "Pr", "Nd", "Sm", "Eu", "Gd", "Tb", "Dy", "Ho", "Er", "Tm", "Yb", "Lu"];

pub fn element_oxide(el: &str) -> Option<&'static str> {
    Some(match el {
        "La" => "La2O3",
        "Ce" => "Ce2O3",
        "Pr" => "Pr2O3",
        "Nd" => "Nd2O3",
        "Sm" => "Sm2O3",
        "Eu" => "Eu2O3",
        "Gd" => "Gd2O3",
        "Tb" => "Tb2O3",
        "Dy" => "Dy2O3",
        "Y" => "Y2O3",
        "Ho" => "Ho2O3",
        "Er" => "Er2O3",
        "Tm" => "Tm2O3",
        "Yb" => "Yb2O3",
        "Lu" => "Lu2O3",
        _ => return None,
    })
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NormSet {
    pub name: String,
    #[serde(default)]
    pub source: String,
    pub values: BTreeMap<String, f64>,
    #[serde(default)]
    pub preset: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NormSettings {
    pub active: String,
    pub sets: Vec<NormSet>,
}

pub fn presets() -> Vec<NormSet> {
    let ms: [(&str, f64); 15] = [
        ("Y", 1.57), ("La", 0.237), ("Ce", 0.613), ("Pr", 0.0928), ("Nd", 0.457), ("Sm", 0.148), ("Eu", 0.0563),
        ("Gd", 0.199), ("Tb", 0.0361), ("Dy", 0.246), ("Ho", 0.0546), ("Er", 0.160), ("Tm", 0.0247), ("Yb", 0.161),
        ("Lu", 0.0246),
    ];
    let boy: [(&str, f64); 14] = [
        ("La", 0.31), ("Ce", 0.808), ("Pr", 0.122), ("Nd", 0.6), ("Sm", 0.195), ("Eu", 0.0735), ("Gd", 0.259),
        ("Tb", 0.0474), ("Dy", 0.322), ("Ho", 0.0718), ("Er", 0.21), ("Tm", 0.0324), ("Yb", 0.209), ("Lu", 0.0322),
    ];
    vec![
        NormSet {
            name: "CI chondrite (McDonough & Sun 1995)".into(),
            source: "McDonough, W.F., Sun, S.-s. (1995) The composition of the Earth. Chemical Geology 120, 223-253. doi:10.1016/0009-2541(94)00140-4".into(),
            values: ms.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
            preset: true,
        },
        NormSet {
            name: "Chondrite (Boynton 1984)".into(),
            source: "Boynton, W.V. (1984) Cosmochemistry of the rare earth elements: meteorite studies. In: Henderson, P. (ed.) Rare Earth Element Geochemistry. Elsevier, Amsterdam, 63-114. Recommended chondrite values.".into(),
            values: boy.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
            preset: true,
        },
    ]
}

impl Default for NormSettings {
    fn default() -> Self {
        let p = presets();
        NormSettings { active: p[0].name.clone(), sets: p }
    }
}

/// Stored settings with every preset present.
pub fn merge_normalization_settings(stored: Option<&serde_json::Value>) -> NormSettings {
    let mut s = NormSettings::default();
    let Some(obj) = stored.and_then(|v| v.as_object()) else { return s };
    let mut sets = Vec::new();
    if let Some(arr) = obj.get("sets").and_then(|v| v.as_array()) {
        for e in arr {
            let (Some(name), Some(vals)) = (e.get("name").and_then(|v| v.as_str()), e.get("values").and_then(|v| v.as_object())) else {
                continue;
            };
            if name.is_empty() {
                continue;
            }
            let mut values = BTreeMap::new();
            for (el, v) in vals {
                let x = v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()));
                if let Some(x) = x {
                    if NORMALIZATION_ELEMENTS.contains(&el.as_str()) && x > 0.0 {
                        values.insert(el.clone(), x);
                    }
                }
            }
            sets.push(NormSet {
                name: name.into(),
                source: e.get("source").and_then(|v| v.as_str()).unwrap_or("").into(),
                values,
                preset: e.get("preset").and_then(|v| v.as_bool()).unwrap_or(false),
            });
        }
    }
    for p in presets() {
        if !sets.iter().any(|s: &NormSet| s.name == p.name) {
            sets.push(p);
        }
    }
    let active = obj.get("active").and_then(|v| v.as_str()).unwrap_or("");
    s.active = if sets.iter().any(|x| x.name == active) { active.into() } else { s.active };
    s.sets = sets;
    s
}

impl NormSettings {
    pub fn by_name(&self, name: &str) -> &NormSet {
        self.sets.iter().find(|s| s.name == name).unwrap_or(&self.sets[0])
    }
}

pub fn element_sequence(include_y: bool) -> Vec<&'static str> {
    let mut s = REE_SEQUENCE.to_vec();
    if include_y {
        let i = s.iter().position(|e| *e == "Ho").unwrap();
        s.insert(i, "Y");
    }
    s
}

/// Element ppm from the row's oxide wt% (None when not determined or below d.l.).
pub fn element_ppm(row: &Row, el: &str) -> Option<f64> {
    let ox = element_oxide(el)?;
    if crate::sessions::below_dl_set(row).iter().any(|s| s == ox) {
        return None;
    }
    let key = format!("{ox}_wt%");
    let v = if row.contains_key(&key) { num_loose(row, &key) } else { num_loose(row, ox) }?;
    if !v.is_finite() || v <= 0.0 {
        return None;
    }
    Some(v * 1e4 * oxide_element_fraction(ox))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Source {
    #[serde(rename = "epma")]
    Epma,
    #[serde(rename = "imported")]
    Imported,
    #[serde(rename = "epma_fill")]
    EpmaFill,
    #[default]
    #[serde(rename = "imported_fill")]
    ImportedFill,
}

impl Source {
    pub const ALL: [Source; 4] = [Source::ImportedFill, Source::EpmaFill, Source::Epma, Source::Imported];
    pub fn label(&self) -> &'static str {
        match self {
            Source::ImportedFill => "LA-ICP-MS, EPMA where LA-ICP-MS missing",
            Source::EpmaFill => "EPMA, LA-ICP-MS where EPMA missing",
            Source::Epma => "EPMA only",
            Source::Imported => "LA-ICP-MS only",
        }
    }
}

/// (ppm, imported?) of one element.
pub fn element_value(row: &Row, el: &str, src: Source) -> Option<(f64, bool)> {
    let e = || element_ppm(row, el).map(|v| (v, false));
    let i = || trace_value(row, el).map(|v| (v, true));
    match src {
        Source::Epma => e(),
        Source::Imported => i(),
        Source::EpmaFill => e().or_else(i),
        Source::ImportedFill => i().or_else(e),
    }
}

pub fn normalized_pattern(row: &Row, norm: &BTreeMap<String, f64>, els: &[&str], src: Source) -> Vec<Option<(f64, bool)>> {
    els.iter()
        .map(|el| match (element_value(row, el, src), norm.get(*el)) {
            (Some((v, imp)), Some(r)) if *r != 0.0 => Some((v / r, imp)),
            _ => None,
        })
        .collect()
}

pub fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let pos = (sorted.len() - 1) as f64 * q;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo as f64)
}

#[derive(Clone, Debug, Default)]
pub struct GroupPattern {
    pub median: Vec<Option<f64>>,
    pub q1: Vec<Option<f64>>,
    pub q3: Vec<Option<f64>>,
    pub min: Vec<Option<f64>>,
    pub max: Vec<Option<f64>>,
    pub n: Vec<usize>,
    /// Some value at this element came from imported (laser) ppm data.
    pub imported: Vec<bool>,
}

pub fn group_pattern(rows: &[&Row], norm: &BTreeMap<String, f64>, els: &[&str], src: Source) -> GroupPattern {
    let pats: Vec<(String, Vec<Option<(f64, bool)>>)> =
        rows.iter().map(|r| (grain_key(&text(r, "Label")), normalized_pattern(r, norm, els, src))).collect();
    let mut g = GroupPattern::default();
    for k in 0..els.len() {
        let mut vals = Vec::new();
        let mut seen: Vec<(String, i64)> = Vec::new();
        let mut any_imported = false;
        for (grain, p) in &pats {
            let Some((v, imported)) = p[k] else { continue };
            any_imported |= imported;
            if imported {
                let key = (grain.clone(), (round_to(v, 9) * 1e9) as i64);
                if seen.contains(&key) {
                    continue;
                }
                seen.push(key);
            }
            vals.push(v);
        }
        g.n.push(vals.len());
        g.imported.push(any_imported);
        if vals.is_empty() {
            g.median.push(None);
            g.q1.push(None);
            g.q3.push(None);
            g.min.push(None);
            g.max.push(None);
        } else {
            vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
            g.median.push(Some(percentile(&vals, 0.5)));
            g.q1.push(Some(percentile(&vals, 0.25)));
            g.q3.push(Some(percentile(&vals, 0.75)));
            g.min.push(vals.first().copied());
            g.max.push(vals.last().copied());
        }
    }
    g
}

/// (i, j, bridged) segments joining determined values.
pub fn pattern_segments(values: &[Option<f64>]) -> Vec<(usize, usize, bool)> {
    let idx: Vec<usize> = values.iter().enumerate().filter(|(_, v)| v.is_some()).map(|(i, _)| i).collect();
    idx.windows(2).map(|w| (w[0], w[1], w[1] - w[0] > 1)).collect()
}
