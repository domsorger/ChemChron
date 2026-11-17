// SPDX-License-Identifier: GPL-3.0-or-later
//! Column sets, display names, blocks and decimals of the EPMA tables and
//! exports.

use crate::constants::{af_field, DecimalPlaces, PARENT_OXIDES, QA_ONLY_OXIDES};

pub fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

pub fn upb_age_columns() -> Vec<String> {
    s(&[
        "UPb_t68_Ma", "UPb_t68_2s_Ma", "UPb_t68_2s_point_Ma", "UPb_t68_2s_total_Ma", "UPb_t75_Ma", "UPb_t75_2s_Ma", "UPb_t75_2s_point_Ma", "UPb_t75_2s_total_Ma",
        "UPb_t76_Ma", "UPb_t76_2s_Ma", "UPb_t76_2s_point_Ma", "UPb_t76_2s_total_Ma", "UPb_t82_Ma", "UPb_t82_2s_Ma", "UPb_t82_2s_point_Ma", "UPb_t82_2s_total_Ma",
        "UPb_Disc76_pct", "UPb_Disc75_pct", "UPb_Include",
    ])
}

pub fn upb_detail_columns() -> Vec<String> {
    s(&[
        "UPb_r86", "UPb_r86_2s", "UPb_r86_2s_point", "UPb_r76", "UPb_r76_2s", "UPb_r76_2s_point", "UPb_rho_TW",
        "UPb_rho_TW_point", "UPb_r68", "UPb_r68_2s", "UPb_r75", "UPb_r75_2s", "UPb_rho_W", "UPb_r82", "UPb_r82_2s",
        "UPb_r82_2s_point", "UPb_Excess_2s_pct", "UPb_RatioConsistency_pct", "UPb_ConcDist_sigma", "UPb_ConcDist_point_sigma",
    ])
}

pub fn upb_ref_columns() -> Vec<String> {
    s(&["UPb_Ref_Accepted_Ma", "UPb_Ref_Accepted_2s_Ma", "UPb_Ref_Diff_Ma", "UPb_Ref_Diff_pct", "UPb_Ref_Within_2s"])
}

const AGE_COMMON: [&str; 13] = [
    "Age_Ma", "Age_2s_Ma", "Age_Rel_2s_pct", "Age_2s_Ma_point", "Age_Rel_2s_pct_point", "Age_2s_Ma_total",
    "Age_Rel_2s_pct_total", "Include_In_Average", "CSUTPS", "Calculated_PbO_wt%", "Pb_difference", "", "",
];

/// Chemical-age columns (EPMA part); with `upb` also the isotopic ones (Combine part).
pub fn age_columns(references: bool, upb: bool) -> Vec<String> {
    let mut v = if references {
        s(&["Label", "Mineral", "Is_Reference", "Reference", "Comment", "EPMA_Session"])
    } else {
        s(&["Label", "Mineral", "Is_Reference", "Flag", "Comment", "EPMA_Session"])
    };
    if upb {
        v.push("UPb_Session".into());
    }
    v.extend(AGE_COMMON.iter().filter(|c| !c.is_empty()).map(|c| c.to_string()));
    if references {
        v.extend(s(&["Ref_Diff_Ma", "Ref_Diff_pct", "Ref_Within_2s"]));
    } else {
        v.extend(s(&["Group_Diff_Ma", "Group_Diff_pct", "Group_Within_2s"]));
    }
    if upb {
        v.extend(upb_age_columns());
        if references {
            v.extend(upb_ref_columns());
        }
        v.extend(upb_detail_columns());
    }
    v
}

pub fn mineral_columns(active: &[String], trace: &[String], references: bool) -> Vec<String> {
    let mut v = if references {
        s(&["Label", "Mineral", "Is_Reference", "Reference", "Comment", "EPMA_Session"])
    } else {
        s(&["Label", "Mineral", "Is_Reference", "Flag", "Comment", "EPMA_Session"])
    };
    let mut ox: Vec<String> = PARENT_OXIDES.iter().map(|x| x.to_string()).collect();
    ox.extend(active.iter().cloned());
    for o in &ox {
        v.push(format!("{o}_wt%"));
        v.push(format!("{o}_2s_wt%"));
        v.push(format!("{o}_DL_wt%"));
    }
    v.extend(crate::trace::trace_columns(trace));
    v.push("Below_DL".into());
    if !trace.is_empty() {
        v.push("TE_Below_DL".into());
    }
    let mut af_ox: Vec<String> = active.to_vec();
    af_ox.extend(PARENT_OXIDES.iter().map(|x| x.to_string()));
    for o in af_ox {
        if !QA_ONLY_OXIDES.contains(&o.as_str()) || o == "ZrO2" {
            v.push(af_field(&o));
        }
    }
    v.extend(s(&["A_site_sum", "B_site_sum", "CSUTPS", "ThUSi_af", "REEYP_af", "X_monazite", "X_huttonite", "X_cheralite", "X_LREE", "X_HREE", "Mineral_Formula"]));
    v
}

pub fn display_name(col: &str) -> String {
    let fixed = match col {
        "Age_2s_Ma" => "Age 2σ analytical (Ma)",
        "Age_Rel_2s_pct" => "Age Rel 2σ analytical (%)",
        "Age_2s_Ma_point" => "Age 2σ analytical + excess (Ma)",
        "Age_Rel_2s_pct_point" => "Age Rel 2σ analytical + excess (%)",
        "Age_2s_Ma_total" => "Age 2σ total (Ma)",
        "Age_Rel_2s_pct_total" => "Age Rel 2σ total (%)",
        "Ref_Accepted_2s_Ma" => "Ref Accepted 2σ (Ma)",
        "Ref_Within_2s" => "Ref Within 2σ",
        "Group_Within_2s" => "Group Within 2σ",
        "2s_Ma" => "2SD (Ma)",
        "Is_Reference" => "Ref/Sample",
        "Include_In_Average" => "Include in Avg",
        "Accepted_Age_Ma" => "Reference Age (Ma)",
        "Accepted_2s_Ma" => "Reference 2σ (Ma)",
        "Flag" => "Sample Group",
        "EPMA_Session" => "EPMA session",
        "UPb_Session" => "U–Pb session",
        "N_logged" => "N (logged)",
        "LongTerm_Mean_Age_Ma" => "Long-term Mean Age (Ma)",
        "LongTerm_2s_Ma" => "Long-term 2SD (Ma)",
        "LongTerm_RelDev_pct" => "Long-term RelDev (%)",
        "LongTerm_Diff_Ma" => "Diff (Ma)",
        "Below_DL" => "Below d.l.",
        "TE_Below_DL" => "Below d.l. (ppm data)",
        "UPb_t68_Ma" => "²⁰⁶Pb/²³⁸U age (Ma)",
        "UPb_t68_2s_Ma" => "²⁰⁶Pb/²³⁸U age 2σ analytical (Ma)",
        "UPb_t75_Ma" => "²⁰⁷Pb/²³⁵U age (Ma)",
        "UPb_t75_2s_Ma" => "²⁰⁷Pb/²³⁵U age 2σ analytical (Ma)",
        "UPb_t76_Ma" => "²⁰⁷Pb/²⁰⁶Pb age (Ma)",
        "UPb_t76_2s_Ma" => "²⁰⁷Pb/²⁰⁶Pb age 2σ analytical (Ma)",
        "UPb_t82_Ma" => "²⁰⁸Pb/²³²Th age (Ma)",
        "UPb_t82_2s_Ma" => "²⁰⁸Pb/²³²Th age 2σ analytical (Ma)",
        "UPb_t68_2s_point_Ma" => "²⁰⁶Pb/²³⁸U age 2σ analytical + excess (Ma)",
        "UPb_t75_2s_point_Ma" => "²⁰⁷Pb/²³⁵U age 2σ analytical + excess (Ma)",
        "UPb_t76_2s_point_Ma" => "²⁰⁷Pb/²⁰⁶Pb age 2σ analytical + excess (Ma)",
        "UPb_t82_2s_point_Ma" => "²⁰⁸Pb/²³²Th age 2σ analytical + excess (Ma)",
        "UPb_t68_2s_total_Ma" => "²⁰⁶Pb/²³⁸U age 2σ total (Ma)",
        "UPb_t75_2s_total_Ma" => "²⁰⁷Pb/²³⁵U age 2σ total (Ma)",
        "UPb_t76_2s_total_Ma" => "²⁰⁷Pb/²⁰⁶Pb age 2σ total (Ma)",
        "UPb_t82_2s_total_Ma" => "²⁰⁸Pb/²³²Th age 2σ total (Ma)",
        "UPb_r86_2s_point" => "²³⁸U/²⁰⁶Pb 2σ analytical + excess",
        "UPb_r76_2s_point" => "²⁰⁷Pb/²⁰⁶Pb 2σ analytical + excess",
        "UPb_rho_TW_point" => "ρ (TW) analytical + excess",
        "UPb_r82_2s_point" => "²⁰⁸Pb/²³²Th 2σ analytical + excess",
        "UPb_Excess_2s_pct" => "Excess added here 2σ (%) 238/206 / 207/206 / 208/232",
        "UPb_ConcDist_point_sigma" => "Concordia distance, analytical + excess (σ)",
        "UPb_Disc76_pct" => "Disc. 1−t68/t76 (%)",
        "UPb_Disc75_pct" => "Disc. 1−t68/t75 (%)",
        "UPb_r86" => "²³⁸U/²⁰⁶Pb",
        "UPb_r86_2s" => "²³⁸U/²⁰⁶Pb 2σ analytical",
        "UPb_r76" => "²⁰⁷Pb/²⁰⁶Pb",
        "UPb_r76_2s" => "²⁰⁷Pb/²⁰⁶Pb 2σ analytical",
        "UPb_rho_TW" => "ρ (TW)",
        "UPb_r68" => "²⁰⁶Pb/²³⁸U",
        "UPb_r68_2s" => "²⁰⁶Pb/²³⁸U 2σ analytical",
        "UPb_r75" => "²⁰⁷Pb/²³⁵U",
        "UPb_r75_2s" => "²⁰⁷Pb/²³⁵U 2σ analytical",
        "UPb_rho_W" => "ρ (Wetherill)",
        "UPb_r82" => "²⁰⁸Pb/²³²Th",
        "UPb_r82_2s" => "²⁰⁸Pb/²³²Th 2σ analytical",
        "UPb_RatioConsistency_pct" => "Ratio consistency (%)",
        "UPb_Include" => "Include in U–Pb avg",
        "UPb_ConcDist_sigma" => "Concordia distance (σ)",
        "UPb_N" => "N (U–Pb)",
        "UPb_N_disc" => "N disc./imprecise (not used)",
        "UPb_Mean_Ma" => "Mean (Ma)",
        "UPb_2s_Ma" => "2SD (Ma)",
        "UPb_Accepted_Ma" => "Reference Age (Ma)",
        "UPb_Accepted_2s_Ma" => "Reference 2σ (Ma)",
        "UPb_Diff_Ma" | "Diff_Ma" => "WM Diff (Ma)",
        "UPb_Diff_pct" | "Diff_pct" => "WM Diff (%)",
        "UPb_WM_Ma" => "Weighted Mean (Ma)",
        "UPb_WM_2s_Ma" => "Weighted Mean 2σ (Ma)",
        "UPb_MSWD" | "MSWD" => "MSWD",
        "UPb_Ref_Accepted_Ma" => "U–Pb Ref Accepted (Ma)",
        "UPb_Ref_Accepted_2s_Ma" => "U–Pb Ref Accepted 2σ (Ma)",
        "UPb_Ref_Diff_Ma" => "U–Pb Ref Diff (Ma)",
        "UPb_Ref_Diff_pct" => "U–Pb Ref Diff (%)",
        "UPb_Ref_Within_2s" => "U–Pb Ref Within 2σ",
        "X_huttonite" => "X (Ht)",
        "X_cheralite" => "X (Cher)",
        "X_monazite" => "X (Mnz)",
        "X_LREE" => "X (LREE)",
        "X_HREE" => "X (HREE)",
        "ThUSi_af" => "Th+U+Si (apfu)",
        "REEYP_af" => "REE+Y+P (apfu)",
        "WeightedMean_Age_Ma" => "Weighted Mean Age (Ma)",
        "WeightedMean_2s_Ma" => "Weighted Mean 2σ (Ma)",
        "Mineral_Formula" => "Mineral Formula",
        _ => "",
    };
    if !fixed.is_empty() {
        return fixed.to_string();
    }
    let sig = |x: &str| x.replace('_', " ").split(' ').map(|w| if w == "2s" { "2σ" } else { w }).collect::<Vec<_>>().join(" ");
    if let Some((el, kind)) = crate::trace::trace_field(col) {
        let k = match kind {
            "2s_ppm" => " 2σ",
            "LOD_ppm" => " d.l.",
            _ => "",
        };
        return format!("{el}{k} (ppm)");
    }
    if let Some(b) = col.strip_suffix("_Ma_total") {
        return format!("{} total (Ma)", sig(b));
    }
    if let Some(b) = col.strip_suffix("_Ma") {
        return format!("{} (Ma)", sig(b));
    }
    if let Some(b) = col.strip_suffix("_pct_total") {
        return format!("{} total (%)", sig(b));
    }
    if let Some(b) = col.strip_suffix("_pct") {
        return format!("{} (%)", sig(b));
    }
    if let Some(b) = col.strip_suffix("_2s_wt%") {
        return format!("{b} 2σ (wt%)");
    }
    if let Some(b) = col.strip_suffix("_DL_wt%") {
        return format!("{b} d.l. (wt%)");
    }
    if let Some(b) = col.strip_suffix("_wt%") {
        return format!("{} (wt%)", b.replace('_', " "));
    }
    if let Some(b) = col.strip_suffix("_af") {
        return format!("{} (apfu)", b.replace('_', " "));
    }
    sig(col)
}

pub fn geochronology_block(col: &str) -> &'static str {
    if ["Label", "Mineral", "Is_Reference", "Flag", "Reference", "Comment", "EPMA_Session", "UPb_Session"].contains(&col) {
        "General"
    } else if upb_detail_columns().iter().any(|c| c == col) {
        "U–Pb ratios"
    } else if col.starts_with("UPb_") {
        "U–Pb ages"
    } else if col.starts_with("Ref_") || col.starts_with("Group_") {
        "Reference / group check"
    } else {
        "Chemical age"
    }
}

pub fn mineral_block(col: &str) -> &'static str {
    if ["Label", "Mineral", "Is_Reference", "Flag", "Reference", "Comment", "EPMA_Session", "UPb_Session"].contains(&col) {
        "General"
    } else if col == "Mineral_Formula" {
        "Formula"
    } else if col == "Below_DL" || col == "TE_Below_DL" {
        "b.d.l."
    } else if col.ends_with("_ppm") {
        "Trace elements (ppm)"
    } else if col.ends_with("_wt%") {
        "Raw data (wt%)"
    } else if col == "CSUTPS" || col == "ThUSi_af" || col == "REEYP_af" || col.starts_with("X_") {
        "Substitutions / end-members"
    } else {
        "apfu & site sums"
    }
}

pub fn decimals_for(col: &str, dp: &DecimalPlaces) -> Option<usize> {
    if col == "Calculated_PbO_wt%" || col == "Pb_difference" {
        return Some(dp.pb_calc);
    }
    if col == "CSUTPS" || col.starts_with("X_") {
        return Some(dp.csutps);
    }
    if col == "ThUSi_af" || col == "REEYP_af" {
        return Some(dp.apfu);
    }
    if col == "A_site_sum" || col == "B_site_sum" {
        return Some(dp.site_sum);
    }
    if col.ends_with("_wt%") {
        return Some(dp.wt_pct);
    }
    if col.ends_with("_af") {
        return Some(dp.apfu);
    }
    // shown with two decimals (stored at full precision like everything else)
    if col.contains("Disc7") || col.contains("RatioConsistency") || col.ends_with("_sigma") || col.contains("MSWD") {
        return Some(2);
    }
    if col.ends_with("_Ma") || col.ends_with("_pct") || col.ends_with("_Ma_total") || col.ends_with("_pct_total") {
        return Some(dp.age);
    }
    None
}

/// Fields offered by the X-Y / ternary plot pickers: (group, field, label).
pub fn plottable_fields(active: &[String]) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let mut ox: Vec<String> = PARENT_OXIDES.iter().map(|x| x.to_string()).collect();
    ox.extend(active.iter().cloned());
    for o in &ox {
        out.push(("Oxides (wt%)".into(), format!("{o}_wt%"), format!("{o} (wt%)")));
    }
    for o in &ox {
        if !QA_ONLY_OXIDES.contains(&o.as_str()) || o == "ZrO2" {
            let f = af_field(o);
            out.push(("apfu".into(), f.clone(), display_name(&f)));
        }
    }
    for f in ["Age_Ma", "Age_2s_Ma", "CSUTPS", "A_site_sum", "B_site_sum", "X_huttonite", "X_cheralite", "X_monazite", "X_LREE", "X_HREE", "ThUSi_af", "REEYP_af"] {
        out.push(("Derived".into(), f.into(), display_name(f)));
    }
    out
}
