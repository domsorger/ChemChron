// SPDX-License-Identifier: GPL-3.0-or-later
//! Structural formula (apfu), CSUTPS, ternary end-members, exchange vector and
//! automatic mineral identification.

use crate::constants::*;
use crate::row::*;
use std::collections::BTreeMap;

pub type OxideWt = BTreeMap<String, f64>;
pub type Atoms = BTreeMap<String, f64>;

fn wt(w: &OxideWt, ox: &str) -> f64 {
    w.get(ox).copied().unwrap_or(0.0)
}

fn at(a: &Atoms, key: &str) -> f64 {
    a.get(key).copied().unwrap_or(0.0)
}

fn all_oxides(active: &[String]) -> Vec<String> {
    let mut v: Vec<String> = active.to_vec();
    v.extend(PARENT_OXIDES.iter().map(|s| s.to_string()));
    v
}

pub struct Structural {
    pub atoms: Atoms,
    pub a_site: f64,
    pub b_site: f64,
    pub csutps: f64,
}

pub fn compute_structural_formula(w: &OxideWt, active: &[String], mineral: &str) -> Structural {
    let profile = mineral_profile(mineral);
    let mut atoms = Atoms::new();
    let mut oxygen_sum = 0.0;
    let all = all_oxides(active);
    for ox in &all {
        if !is_structural_oxide(ox, mineral) {
            continue;
        }
        let Some(mm) = oxide_molar_mass(ox) else { continue };
        let moles = wt(w, ox) / mm;
        atoms.insert(af_field(ox), moles * oxide_cations(ox));
        oxygen_sum += moles * oxide_oxygens(ox);
    }
    let factor = if oxygen_sum != 0.0 { profile.oxygen_basis / oxygen_sum } else { 0.0 };
    for v in atoms.values_mut() {
        *v *= factor;
    }
    let (a_site, b_site) = if profile.single_site {
        (atoms.values().sum(), 0.0)
    } else {
        let a: f64 = all
            .iter()
            .filter(|ox| is_a_site(ox) || (QA_ONLY_OXIDES.contains(&ox.as_str()) && is_structural_oxide(ox, mineral)))
            .map(|ox| at(&atoms, &af_field(ox)))
            .sum();
        let b: f64 = all.iter().filter(|ox| is_b_site(ox)).map(|ox| at(&atoms, &af_field(ox))).sum();
        (a, b)
    };
    let csutps = if csutps_mineral(mineral) {
        let den = at(&atoms, "Th_af") + at(&atoms, "U_af") + at(&atoms, "Pb_af") + at(&atoms, "S_af");
        if den > 0.0 {
            (at(&atoms, "Ca_af") + at(&atoms, "Si_af")) / den
        } else {
            f64::NAN
        }
    } else {
        f64::NAN
    };
    Structural { atoms, a_site, b_site, csutps }
}

pub const TERNARY_FIELDS: [&str; 5] = ["X_huttonite", "X_cheralite", "X_monazite", "X_LREE", "X_HREE"];
pub const EXCHANGE_FIELDS: [&str; 2] = ["ThUSi_af", "REEYP_af"];

pub fn ternary_endmembers(atoms: &Atoms, a_site: f64, mineral: &str) -> Vec<(&'static str, f64)> {
    if !csutps_mineral(mineral) || a_site <= 0.0 {
        return TERNARY_FIELDS.iter().map(|f| (*f, f64::NAN)).collect();
    }
    let ca = at(atoms, "Ca_af");
    let thupb = at(atoms, "Th_af") + at(atoms, "U_af") + at(atoms, "Pb_af");
    let y = at(atoms, "Y_af");
    let lree: f64 = REE_LREE_OXIDES.iter().map(|o| at(atoms, &af_field(o))).sum();
    let hree: f64 = REE_HREE_OXIDES.iter().map(|o| at(atoms, &af_field(o))).sum();
    let paired = ca.min(thupb);
    let hutt = thupb - paired;
    let excess_ca = ca - paired;
    vec![
        ("X_huttonite", hutt / a_site),
        ("X_cheralite", 2.0 * paired / a_site),
        ("X_monazite", (lree + hree + y + excess_ca) / a_site),
        ("X_LREE", lree / a_site),
        ("X_HREE", (hree + y) / a_site),
    ]
}

pub fn exchange_vector(atoms: &Atoms, mineral: &str) -> Vec<(&'static str, f64)> {
    if !csutps_mineral(mineral) {
        return EXCHANGE_FIELDS.iter().map(|f| (*f, f64::NAN)).collect();
    }
    let thusi = at(atoms, "Th_af") + at(atoms, "U_af") + at(atoms, "Si_af");
    let ree: f64 = FULL_REE_OXIDES.iter().map(|o| at(atoms, &af_field(o))).sum();
    vec![("ThUSi_af", thusi), ("REEYP_af", ree + at(atoms, "Y_af") + at(atoms, "P_af"))]
}

/// A value with three decimals.
fn f3(v: f64) -> String {
    format!("{v:.3}")
}

pub fn mineral_formula_string(atoms: &Atoms, active: &[String], mineral: &str) -> String {
    let profile = mineral_profile(mineral);
    let selected: Vec<String> = all_oxides(active);
    let has = |o: &str| selected.iter().any(|s| s == o);
    let mut parts = String::new();
    let mut order: Vec<&str> = profile.structural_qa_oxides.iter().copied().filter(|o| has(o)).collect();
    order.extend(formula_order());
    for ox in order {
        if !has(ox) {
            continue;
        }
        let v = at(atoms, &af_field(ox));
        if v >= MINERAL_FORMULA_MIN_APFU_DISPLAY {
            parts.push_str(element_symbol(ox));
            parts.push_str(&f3(v));
        }
    }
    format!("({parts}){}", profile.formula_suffix)
}

pub fn identify_mineral(w: &OxideWt, t: &MineralIdThresholds) -> &'static str {
    let p2o5 = wt(w, "P2O5");
    let sio2 = wt(w, "SiO2");
    let tho2 = wt(w, "ThO2");
    let uo2 = wt(w, "UO2");
    if p2o5 >= t.phosphate_p2o5_min && sio2 < t.phosphate_sio2_max {
        let lree: f64 = REE_LREE_OXIDES.iter().map(|o| wt(w, o)).sum();
        let hree_y: f64 = REE_HREE_OXIDES.iter().map(|o| wt(w, o)).sum::<f64>() + wt(w, "Y2O3");
        return if hree_y > lree { "Xenotime" } else { "Monazite" };
    }
    if sio2 >= t.thorite_sio2_min && tho2 >= t.thorite_tho2_min && p2o5 < t.thorite_p2o5_max {
        return "Thorite";
    }
    if tho2 + uo2 >= t.oxide_thpu_min && p2o5 < t.oxide_p2o5_max && sio2 < t.oxide_sio2_max {
        return if tho2 >= uo2 { "Thorianite" } else { "Uraninite" };
    }
    UNCLASSIFIED
}

pub fn oxide_wt_from_processed(row: &Row, active: &[String]) -> OxideWt {
    all_oxides(active).into_iter().map(|o| {
        let v = num(row, &format!("{o}_wt%")).unwrap_or(0.0);
        (o, v)
    }).collect()
}

/// Writes the mineral-dependent fields of a processed row for `mineral`.
pub fn write_mineral_fields(row: &mut Row, w: &OxideWt, active: &[String], mineral: &str, dp: &DecimalPlaces) {
    let s = compute_structural_formula(w, active, mineral);
    set(row, "Mineral", jstr(mineral));
    set(row, "Mineral_Formula", jstr(mineral_formula_string(&s.atoms, active, mineral)));
    set(row, "CSUTPS", jround(s.csutps, dp.csutps));
    for (f, v) in ternary_endmembers(&s.atoms, s.a_site, mineral) {
        set(row, f, jround(v, dp.csutps));
    }
    for (f, v) in exchange_vector(&s.atoms, mineral) {
        set(row, f, jround(v, dp.apfu));
    }
    set(row, "A_site_sum", jnum(s.a_site));
    set(row, "B_site_sum", jnum(s.b_site));
    for ox in all_oxides(active) {
        if !is_structural_oxide(&ox, mineral) {
            row.remove(&af_field(&ox));
            continue;
        }
        set(row, &af_field(&ox), jnum(at(&s.atoms, &af_field(&ox))));
    }
}

/// After a manual mineral change: recompute only the mineral-dependent fields.
pub fn recompute_mineral_formula_for_row(row: &mut Row, active: &[String], mineral: &str, dp: &DecimalPlaces) {
    let w = oxide_wt_from_processed(row, active);
    write_mineral_fields(row, &w, active, mineral, dp);
}
