// SPDX-License-Identifier: GPL-3.0-or-later
//! Constants of the chemical (Th-U-Pb) method.

use serde::{Deserialize, Serialize};

pub const DEFAULT_LONG_TERM_REL_PCT: f64 = 2.0;
/// Fallback 1-sigma relative errors when an import has no measured errors.
pub const DEFAULT_SIGMA_FALLBACK_REL_TH_U: f64 = 0.01;
pub const DEFAULT_SIGMA_FALLBACK_REL_PB: f64 = 0.02;
pub const MINERAL_FORMULA_MIN_APFU_DISPLAY: f64 = 0.005;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DecayConstants {
    pub lambda_232: f64,
    pub lambda_238: f64,
    pub lambda_235: f64,
    #[serde(rename = "U238_frac")]
    pub u238_frac: f64,
    #[serde(rename = "U235_frac")]
    pub u235_frac: f64,
    pub lambda_238_rel_1s_pct: f64,
    pub lambda_235_rel_1s_pct: f64,
    /// Present-day ²³⁸U/²³⁵U; `u235_frac` follows from it (`u238_frac / ratio`).
    /// Files written before it existed get 137.818 (Hiess et al. 2012).
    #[serde(rename = "U238_U235")]
    pub u238_u235: f64,
}

/// Sources of the lab's decay constants, shown in Settings.
pub const DECAY_SOURCES: [(&str, &str); 4] = [
    ("λ²³⁸U", "Jaffey et al. (1971)"),
    ("λ²³⁵U", "Jaffey et al. (1971)"),
    ("λ²³²Th", "Le Roux & Glendenin (1963)"),
    ("²³⁸U/²³⁵U", "Hiess et al. (2012)"),
];

impl DecayConstants {
    /// Steiger & Jäger (1977): ²³⁸U/²³⁵U = 137.88 (0.992745 / 0.0072).
    pub fn steiger_jaeger_1977() -> DecayConstants {
        DecayConstants { u235_frac: 0.0072, u238_u235: 0.992745 / 0.0072, ..Default::default() }
    }
    /// Recomputes the ²³⁵U fraction from the ²³⁸U fraction and the ratio.
    pub fn sync_fractions(&mut self) {
        if self.u238_u235.is_finite() && self.u238_u235 > 1.0 {
            self.u235_frac = self.u238_frac / self.u238_u235;
        }
    }
}

impl Default for DecayConstants {
    fn default() -> Self {
        DecayConstants {
            lambda_232: 4.9475e-11,
            lambda_238: 1.55125e-10,
            lambda_235: 9.8485e-10,
            u238_frac: 0.992745,
            u235_frac: 0.992745 / 137.818,
            lambda_238_rel_1s_pct: 100.0 * 0.000000083 / 0.000155125,
            lambda_235_rel_1s_pct: 100.0 * 0.00000067 / 0.00098485,
            u238_u235: 137.818,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DecimalPlaces {
    pub age: usize,
    pub wt_pct: usize,
    pub apfu: usize,
    pub csutps: usize,
    pub site_sum: usize,
    pub pb_calc: usize,
}

impl Default for DecimalPlaces {
    fn default() -> Self {
        DecimalPlaces { age: 2, wt_pct: 4, apfu: 4, csutps: 4, site_sum: 4, pb_calc: 5 }
    }
}

pub const REQUIRED_COLUMNS: [&str; 4] = ["Label", "ThO2", "UO2", "PbO"];

pub const FULL_REE_OXIDES: [&str; 14] = [
    "La2O3", "Ce2O3", "Pr2O3", "Nd2O3", "Sm2O3", "Eu2O3", "Gd2O3", "Tb2O3", "Dy2O3", "Ho2O3", "Er2O3", "Tm2O3",
    "Yb2O3", "Lu2O3",
];
pub const REE_LREE_OXIDES: [&str; 5] = ["La2O3", "Ce2O3", "Pr2O3", "Nd2O3", "Sm2O3"];
pub const REE_HREE_OXIDES: [&str; 9] =
    ["Eu2O3", "Gd2O3", "Tb2O3", "Dy2O3", "Ho2O3", "Er2O3", "Tm2O3", "Yb2O3", "Lu2O3"];
pub const OTHER_OPTIONAL_OXIDES: [&str; 5] = ["P2O5", "SiO2", "CaO", "Y2O3", "SO3"];
/// Contamination monitors only, never part of the formula (except ZrO2 in zircon).
pub const QA_ONLY_OXIDES: [&str; 2] = ["Al2O3", "ZrO2"];
pub const PARENT_OXIDES: [&str; 3] = ["ThO2", "UO2", "PbO"];

/// Every optional oxide, in ChemChron's column order.
pub fn optional_oxides_master() -> Vec<&'static str> {
    let mut v: Vec<&str> = OTHER_OPTIONAL_OXIDES.to_vec();
    v.extend(FULL_REE_OXIDES);
    v.extend(QA_ONLY_OXIDES);
    v
}

pub fn oxide_molar_mass(oxide: &str) -> Option<f64> {
    Some(match oxide {
        "P2O5" => 141.9445,
        "SiO2" => 60.0843,
        "CaO" => 56.0774,
        "Y2O3" => 225.8099,
        "SO3" => 80.0642,
        "La2O3" => 325.8085,
        "Ce2O3" => 328.2382,
        "Pr2O3" => 329.8129,
        "Nd2O3" => 336.4785,
        "Sm2O3" => 348.7185,
        "Eu2O3" => 351.9259,
        "Gd2O3" => 362.4985,
        "Tb2O3" => 365.8295,
        "Dy2O3" => 373.0026,
        "Ho2O3" => 377.8579,
        "Er2O3" => 382.5159,
        "Tm2O3" => 385.8664,
        "Yb2O3" => 394.1173,
        "Lu2O3" => 397.9314,
        "ThO2" => 264.0367,
        "UO2" => 270.0277,
        "PbO" => 223.1994,
        "Al2O3" => 101.9613,
        "ZrO2" => 123.2228,
        _ => return None,
    })
}

pub fn oxide_cations(oxide: &str) -> f64 {
    if matches!(oxide, "SiO2" | "CaO" | "ThO2" | "UO2" | "PbO" | "SO3" | "ZrO2") {
        1.0
    } else {
        2.0
    }
}

pub fn oxide_oxygens(oxide: &str) -> f64 {
    if oxide == "P2O5" {
        5.0
    } else if oxide.ends_with("O3") {
        3.0
    } else if matches!(oxide, "SiO2" | "ThO2" | "UO2" | "ZrO2") {
        2.0
    } else {
        1.0
    }
}

pub const O_ATOMIC_WEIGHT: f64 = 15.9994;

/// Mass fraction of the oxide that is the cation element.
pub fn oxide_element_fraction(oxide: &str) -> f64 {
    let m = oxide_molar_mass(oxide).unwrap_or(f64::NAN);
    (m - oxide_oxygens(oxide) * O_ATOMIC_WEIGHT) / m
}

pub fn element_symbol(oxide: &str) -> &'static str {
    match oxide {
        "P2O5" => "P",
        "SiO2" => "Si",
        "CaO" => "Ca",
        "Y2O3" => "Y",
        "SO3" => "S",
        "La2O3" => "La",
        "Ce2O3" => "Ce",
        "Pr2O3" => "Pr",
        "Nd2O3" => "Nd",
        "Sm2O3" => "Sm",
        "Eu2O3" => "Eu",
        "Gd2O3" => "Gd",
        "Tb2O3" => "Tb",
        "Dy2O3" => "Dy",
        "Ho2O3" => "Ho",
        "Er2O3" => "Er",
        "Tm2O3" => "Tm",
        "Yb2O3" => "Yb",
        "Lu2O3" => "Lu",
        "ThO2" => "Th",
        "UO2" => "U",
        "PbO" => "Pb",
        "Al2O3" => "Al",
        "ZrO2" => "Zr",
        _ => "?",
    }
}

pub fn af_field(oxide: &str) -> String {
    format!("{}_af", element_symbol(oxide))
}

pub fn formula_order() -> Vec<&'static str> {
    let mut v: Vec<&str> = FULL_REE_OXIDES.to_vec();
    v.extend(["Y2O3", "CaO", "ThO2", "UO2", "PbO", "P2O5", "SiO2", "SO3"]);
    v
}

pub fn is_a_site(oxide: &str) -> bool {
    FULL_REE_OXIDES.contains(&oxide) || matches!(oxide, "Y2O3" | "CaO" | "ThO2" | "UO2" | "PbO")
}

pub fn is_b_site(oxide: &str) -> bool {
    matches!(oxide, "P2O5" | "SiO2" | "SO3")
}

pub const MINERAL_NAMES: [&str; 6] = ["Monazite", "Xenotime", "Thorite", "Uraninite", "Thorianite", "Zircon"];
pub const DEFAULT_MINERAL: &str = "Monazite";
pub const UNCLASSIFIED: &str = "Unclassified";

pub fn mineral_abbreviation(m: &str) -> &str {
    match m {
        "Monazite" => "Mnz",
        "Xenotime" => "Xtm",
        "Thorite" => "Thr",
        "Uraninite" => "Urn",
        "Thorianite" => "Tho",
        "Zircon" => "Zrn",
        other => other,
    }
}

pub fn mineral_from_abbreviation(a: &str) -> Option<&'static str> {
    MINERAL_NAMES.iter().copied().find(|m| mineral_abbreviation(m).eq_ignore_ascii_case(a) || m.eq_ignore_ascii_case(a))
}

pub struct MineralProfile {
    pub oxygen_basis: f64,
    pub single_site: bool,
    pub formula_suffix: &'static str,
    pub structural_qa_oxides: &'static [&'static str],
}

pub fn mineral_profile(m: &str) -> MineralProfile {
    match m {
        "Uraninite" | "Thorianite" => {
            MineralProfile { oxygen_basis: 2.0, single_site: true, formula_suffix: "O2", structural_qa_oxides: &[] }
        }
        "Zircon" => MineralProfile {
            oxygen_basis: 4.0,
            single_site: false,
            formula_suffix: "O4",
            structural_qa_oxides: &["ZrO2"],
        },
        _ => MineralProfile { oxygen_basis: 4.0, single_site: false, formula_suffix: "O4", structural_qa_oxides: &[] },
    }
}

pub fn is_structural_oxide(oxide: &str, mineral: &str) -> bool {
    if !QA_ONLY_OXIDES.contains(&oxide) {
        return true;
    }
    mineral_profile(mineral).structural_qa_oxides.contains(&oxide)
}

pub fn csutps_mineral(m: &str) -> bool {
    matches!(m, "Monazite" | "Xenotime")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MineralIdThresholds {
    pub phosphate_p2o5_min: f64,
    pub phosphate_sio2_max: f64,
    pub thorite_sio2_min: f64,
    pub thorite_tho2_min: f64,
    pub thorite_p2o5_max: f64,
    pub oxide_thpu_min: f64,
    pub oxide_p2o5_max: f64,
    pub oxide_sio2_max: f64,
}

impl Default for MineralIdThresholds {
    fn default() -> Self {
        MineralIdThresholds {
            phosphate_p2o5_min: 15.0,
            phosphate_sio2_max: 5.0,
            thorite_sio2_min: 8.0,
            thorite_tho2_min: 40.0,
            thorite_p2o5_max: 5.0,
            oxide_thpu_min: 50.0,
            oxide_p2o5_max: 5.0,
            oxide_sio2_max: 5.0,
        }
    }
}

/// Element mass fractions used for the wt% to ppm conversion of the age equation.
pub const TH_FRAC: f64 = 232.0 / 264.0377;
pub const U_FRAC: f64 = 238.0 / 270.0277;
pub const PB_FRAC: f64 = 207.2 / 223.1994;
