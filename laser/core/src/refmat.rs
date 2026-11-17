// SPDX-License-Identifier: GPL-3.0-or-later
//! Reference materials: accepted values with uncertainties, editable per
//! session and importable from a CSV file. The built-in list is a small
//! starting set of published values; each entry stays unconfirmed until the
//! user checks it against the certificate, because the accepted value decides
//! every result.

use crate::geochron::DecayConstants;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const K_R68: &str = "206Pb/238U";
pub const K_R75: &str = "207Pb/235U";
pub const K_R76: &str = "207Pb/206Pb";
pub const K_R82: &str = "208Pb/232Th";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RmValue {
    pub value: f64,
    /// Two sigma uncertainty in the same unit; 0 when unknown.
    pub unc2s: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReferenceMaterial {
    pub name: String,
    /// Names that also match this material in sample labels.
    pub aliases: Vec<String>,
    pub matrix: String,
    pub source: String,
    /// True once somebody has checked the values against a certificate.
    pub confirmed: bool,
    /// Isotope ratios under their ratio key; element contents in ppm under the element symbol.
    pub values: BTreeMap<String, RmValue>,
}

impl ReferenceMaterial {
    pub fn get(&self, key: &str) -> Option<&RmValue> {
        self.values.get(key)
    }

    /// Does this label name the material, ignoring case and punctuation?
    pub fn matches_label(&self, label: &str) -> bool {
        let wanted = squash(label);
        if wanted.is_empty() {
            return false;
        }
        std::iter::once(&self.name)
            .chain(self.aliases.iter())
            .any(|n| squash(n) == wanted)
    }
}

fn squash(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Ratio values from a concordant age and its 2 sigma, in Ma.
fn ages(
    d: &DecayConstants,
    t68: f64,
    u68: f64,
    t76: f64,
    u76: f64,
    t82: Option<(f64, f64)>,
) -> BTreeMap<String, RmValue> {
    let mut v = BTreeMap::new();
    let r68 = d.r68(t68);
    let s68 = (d.r68(t68 + u68) - r68).abs();
    v.insert(K_R68.to_string(), RmValue { value: r68, unc2s: s68 });
    let r76 = d.r76(t76);
    let s76 = (d.r76(t76 + u76) - r76).abs();
    v.insert(K_R76.to_string(), RmValue { value: r76, unc2s: s76 });
    let r75 = r76 * r68 * d.u85;
    let rel = ((s68 / r68).powi(2) + (s76 / r76).powi(2)).sqrt();
    v.insert(K_R75.to_string(), RmValue { value: r75, unc2s: r75 * rel });
    if let Some((t, u)) = t82 {
        let r = d.r82(t);
        v.insert(K_R82.to_string(), RmValue { value: r, unc2s: (d.r82(t + u) - r).abs() });
    }
    v
}

/// A monazite reference material with its accepted values.
fn lab(name: &str, aliases: &[&str], source: &str, values: BTreeMap<String, RmValue>) -> ReferenceMaterial {
    ReferenceMaterial {
        name: name.to_string(),
        aliases: aliases.iter().map(|s| s.to_string()).collect(),
        matrix: "Monazite".to_string(),
        source: source.to_string(),
        confirmed: true,
        values,
    }
}

const CHECK: &str = "Check against your certificate.";

fn glass(name: &str, aliases: &[&str], rows: &[(&str, f64)]) -> ReferenceMaterial {
    ReferenceMaterial {
        name: name.to_string(),
        aliases: aliases.iter().map(|s| s.to_string()).collect(),
        matrix: "Glass".to_string(),
        source: format!("Jochum et al. (2011) Geostand. Geoanal. Res. 35. {CHECK}"),
        confirmed: false,
        values: rows
            .iter()
            .map(|(k, v)| (k.to_string(), RmValue { value: *v, unc2s: 0.0 }))
            .collect(),
    }
}

/// The small starting set of reference materials.
pub fn builtin(d: &DecayConstants) -> Vec<ReferenceMaterial> {
    let geo = |name: &str, aliases: &[&str], matrix: &str, cite: &str, values| ReferenceMaterial {
        name: name.to_string(),
        aliases: aliases.iter().map(|s| s.to_string()).collect(),
        matrix: matrix.to_string(),
        source: format!("{cite}. {CHECK}"),
        confirmed: false,
        values,
    };
    vec![
        glass(
            "NIST610",
            &["NIST 610", "SRM610", "SRM 610", "610"],
            &[
                ("P", 413.0), ("Ca", 81500.0), ("Si", 327000.0), ("Ti", 452.0), ("Sr", 515.5),
                ("Y", 462.0), ("Zr", 448.0), ("Nb", 465.0), ("Ba", 453.0), ("La", 440.0),
                ("Ce", 453.0), ("Pr", 448.0), ("Nd", 430.0), ("Sm", 453.0), ("Eu", 447.0),
                ("Gd", 449.0), ("Tb", 437.0), ("Dy", 437.0), ("Ho", 449.0), ("Er", 455.0),
                ("Tm", 435.0), ("Yb", 450.0), ("Lu", 439.0), ("Hf", 435.0), ("Ta", 446.0),
                ("Pb", 426.0), ("Th", 457.2), ("U", 461.5),
            ],
        ),
        glass(
            "NIST612",
            &["NIST 612", "SRM612", "SRM 612", "612"],
            &[
                ("P", 46.6), ("Ca", 85300.0), ("Si", 337000.0), ("Ti", 44.0), ("Sr", 78.4),
                ("Y", 38.3), ("Zr", 37.9), ("Nb", 38.9), ("Ba", 39.3), ("La", 35.8),
                ("Ce", 38.4), ("Pr", 37.9), ("Nd", 35.5), ("Sm", 37.7), ("Eu", 35.0),
                ("Gd", 36.7), ("Tb", 36.0), ("Dy", 36.0), ("Ho", 38.0), ("Er", 38.0),
                ("Tm", 36.0), ("Yb", 39.0), ("Lu", 36.9), ("Hf", 36.7), ("Ta", 37.6),
                ("Pb", 38.57), ("Th", 37.79), ("U", 37.38),
            ],
        ),
        lab(
            "44069",
            &["M44", "M44069", "Mnz44069"],
            "Aleinikoff et al. (2006); 208Pb/232Th calculated as concordant at 425 Ma.",
            {
                let mut v = ages(d, 425.0, 0.4, 425.0, 1.0, Some((425.0, 2.0)));
                for (key, value) in [(K_R68, 0.06811), (K_R75, 0.51948), (K_R76, 0.0553)] {
                    v.insert(key.to_string(), RmValue { value, unc2s: 0.0 });
                }
                v
            },
        ),
        lab(
            "MBA",
            &[],
            "Gonçalves et al. (2016), 507.7 Ma.",
            [
                (K_R68, 0.08294, 0.00021),
                (K_R75, 0.6487, 0.0021),
                (K_R76, 0.056723, 0.000051),
                (K_R82, 0.02464, 0.00084),
            ]
            .iter()
            .map(|(k, v, u)| (k.to_string(), RmValue { value: *v, unc2s: *u }))
            .collect(),
        ),
        lab(
            "MDI",
            &["MI506"],
            "Gonçalves et al. (2016) Chem. Geol.: Itambé ID-TIMS weighted means; best age 506.44 ± 0.73 Ma.",
            [(K_R68, 0.082628, 0.000087), (K_R75, 0.6467, 0.0012), (K_R76, 0.05675, 0.000024)]
                .iter()
                .map(|(k, v, u)| (k.to_string(), RmValue { value: *v, unc2s: *u }))
                .collect(),
        ),
        lab(
            "A27",
            &["A276"],
            "Meriläinen (1976); ratios calculated as concordant at 1911 Ma.",
            ages(d, 1911.0, 0.0, 1911.0, 0.0, None),
        ),
        geo(
            "Trebilcock",
            &["Treb"],
            "Monazite",
            "Tomascak et al. 1996; concordant age assumed",
            ages(d, 272.0, 2.0, 272.0, 2.0, Some((272.0, 2.0))),
        ),
        geo(
            "91500",
            &["Z91500", "Zr91500"],
            "Zircon",
            "Wiedenbeck et al. 1995",
            ages(d, 1062.4, 0.4, 1065.4, 0.3, None),
        ),
        geo(
            "GJ-1",
            &["GJ1"],
            "Zircon",
            "Jackson et al. 2004",
            ages(d, 600.7, 1.1, 608.5, 0.4, None),
        ),
        geo(
            "Plesovice",
            &["Ple", "Pl"],
            "Zircon",
            "Slama et al. 2008",
            ages(d, 337.13, 0.37, 337.13, 1.0, None),
        ),
    ]
}

pub fn find<'a>(db: &'a [ReferenceMaterial], name: &str) -> Option<&'a ReferenceMaterial> {
    db.iter().find(|m| m.name == name)
}

/// Import `material,quantity,value,unc2s[,source]` rows, adding to or updating
/// the list. Imported values count as confirmed: they are the lab's own.
pub fn merge_csv(db: &mut Vec<ReferenceMaterial>, text: &str) -> Result<usize, String> {
    let mut count = 0;
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cells: Vec<&str> = line.split(',').map(str::trim).collect();
        if index == 0 && cells.len() >= 3 && cells[2].parse::<f64>().is_err() {
            continue;
        }
        if cells.len() < 3 {
            return Err(format!("Line {}: expected material, quantity, value, uncertainty.", index + 1));
        }
        let value: f64 = cells[2]
            .parse()
            .map_err(|_| format!("Line {}: \"{}\" is not a number.", index + 1, cells[2]))?;
        let unc2s: f64 = cells.get(3).and_then(|c| c.parse().ok()).unwrap_or(0.0);
        let position = match db.iter().position(|m| m.name == cells[0]) {
            Some(p) => p,
            None => {
                db.push(ReferenceMaterial {
                    name: cells[0].to_string(),
                    aliases: Vec::new(),
                    matrix: String::new(),
                    source: String::new(),
                    confirmed: true,
                    values: BTreeMap::new(),
                });
                db.len() - 1
            }
        };
        let m = &mut db[position];
        m.values.insert(cells[1].to_string(), RmValue { value, unc2s });
        m.confirmed = true;
        m.source = cells.get(4).map_or_else(|| "Imported from a file".to_string(), |s| s.to_string());
        count += 1;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_match_regardless_of_punctuation() {
        let d = DecayConstants::default();
        let db = builtin(&d);
        let m = find(&db, "NIST610").unwrap();
        assert!(m.matches_label("nist-610"));
        assert!(m.matches_label("SRM 610"));
        assert!(!m.matches_label("NIST612"));
        assert!(find(&db, "44069").unwrap().matches_label("M44"));
    }

    #[test]
    fn csv_import_adds_a_lab_value_and_marks_it_confirmed() {
        let mut db = Vec::new();
        let n = merge_csv(
            &mut db,
            "material,quantity,value,unc2s\nA27,206Pb/238U,0.0680,0.0004,Lab sheet 2025\nA27,U,5000,100\n",
        )
        .unwrap();
        assert_eq!(n, 2);
        assert!(db[0].confirmed);
        assert_eq!(db[0].get("U").unwrap().value, 5000.0);
    }

    #[test]
    fn csv_import_reports_a_bad_number_with_its_line() {
        let mut db = Vec::new();
        let e = merge_csv(&mut db, "material,quantity,value\nA27,206Pb/238U,abc\nA27,U,5000\n").unwrap_err();
        assert!(e.contains("Line 2"), "{e}");
    }
}
