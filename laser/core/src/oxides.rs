// SPDX-License-Identifier: GPL-3.0-or-later
//! Oxides used to give internal-standard contents as oxide wt% (as an EPMA
//! measures them): the element share of each oxide by mass.

/// (oxide, element, cations, oxygens)
pub const OXIDES: [(&str, &str, f64, f64); 18] = [
    ("CaO", "Ca", 1.0, 1.0),
    ("P2O5", "P", 2.0, 5.0),
    ("SiO2", "Si", 1.0, 2.0),
    ("Ce2O3", "Ce", 2.0, 3.0),
    ("La2O3", "La", 2.0, 3.0),
    ("Nd2O3", "Nd", 2.0, 3.0),
    ("Y2O3", "Y", 2.0, 3.0),
    ("TiO2", "Ti", 1.0, 2.0),
    ("ZrO2", "Zr", 1.0, 2.0),
    ("ThO2", "Th", 1.0, 2.0),
    ("UO2", "U", 1.0, 2.0),
    ("Al2O3", "Al", 2.0, 3.0),
    ("FeO", "Fe", 1.0, 1.0),
    ("MgO", "Mg", 1.0, 1.0),
    ("MnO", "Mn", 1.0, 1.0),
    ("Na2O", "Na", 2.0, 1.0),
    ("K2O", "K", 2.0, 1.0),
    ("HfO2", "Hf", 1.0, 2.0),
];

fn atomic_mass(element: &str) -> Option<f64> {
    Some(match element {
        "Ca" => 40.078,
        "P" => 30.974,
        "Si" => 28.086,
        "Ce" => 140.116,
        "La" => 138.905,
        "Nd" => 144.242,
        "Y" => 88.906,
        "Ti" => 47.867,
        "Zr" => 91.224,
        "Th" => 232.038,
        "U" => 238.029,
        "Al" => 26.982,
        "Fe" => 55.845,
        "Mg" => 24.305,
        "Mn" => 54.938,
        "Na" => 22.990,
        "K" => 39.098,
        "Hf" => 178.49,
        _ => return None,
    })
}

/// Mass share of the element in its oxide (CaO: 0.7147).
pub fn element_fraction(oxide: &str) -> Option<f64> {
    let (_, el, n, o) = OXIDES.iter().find(|x| x.0 == oxide)?;
    let m = atomic_mass(el)?;
    Some(n * m / (n * m + o * 15.999))
}

/// The usual oxide of an element (Ca: CaO).
pub fn default_oxide(element: &str) -> Option<&'static str> {
    OXIDES.iter().find(|x| x.1 == element).map(|x| x.0)
}

/// Element ppm per 1 wt% of the oxide.
pub fn ppm_per_wt_pct(oxide: &str) -> Option<f64> {
    element_fraction(oxide).map(|f| f * 1e4)
}

#[cfg(test)]
mod tests {
    #[test]
    fn oxide_shares() {
        assert!((super::element_fraction("CaO").unwrap() - 0.7147).abs() < 1e-3);
        assert!((super::element_fraction("P2O5").unwrap() - 0.4364).abs() < 1e-3);
        assert_eq!(super::default_oxide("Ce"), Some("Ce2O3"));
    }
}
