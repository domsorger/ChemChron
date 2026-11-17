// SPDX-License-Identifier: GPL-3.0-or-later
//! Builders of every EPMA and Combine plot as a [`Figure`].

use crate::figure::{alpha, Figure, Item};
use chemchron_core::concordia::{self, ConcordiaAge, Discordia};
use chemchron_core::constants::DecayConstants;
use chemchron_core::isochron::{error_ellipse_sigma, Isochron};
use chemchron_core::ree::{self, GroupPattern, NormSet, Source};
use chemchron_core::row::*;
use chemchron_core::upb::ellipse_scale_95;
use egui::Color32;
use std::collections::BTreeMap;

pub fn parse_hex(c: &str) -> Option<Color32> {
    let h = c.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some(Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

pub fn to_hex(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

/// Colour of a sample group: the user's choice, else a stable palette colour.
pub fn group_color(name: &str, colors: &BTreeMap<String, String>, index: usize) -> Color32 {
    colors.get(name).and_then(|c| parse_hex(c)).unwrap_or_else(|| chemchron_laser::theme::series_color(index))
}

/// Every sample group of the project with a fixed colour: the one chosen in
/// Settings, otherwise the automatic colour by alphabetical position (as in
/// Settings > Sample group colours). Tables, plots and Settings all use it, so
/// a group has the same colour everywhere.
pub fn project_colors(chosen: &BTreeMap<String, String>, rows: &[Row]) -> BTreeMap<String, String> {
    let mut flags: Vec<String> = rows.iter().map(|r| text(r, "Flag")).filter(|f| !f.is_empty()).collect();
    flags.sort();
    flags.dedup();
    let mut out = chosen.clone();
    for (i, f) in flags.iter().enumerate() {
        out.entry(f.clone()).or_insert_with(|| to_hex(group_color(f, chosen, i)));
    }
    out
}

pub struct Colors<'a> {
    pub flags: &'a BTreeMap<String, String>,
    pub order: Vec<String>,
}

impl Colors<'_> {
    /// Group name of a row (reference material or sample group).
    pub fn group_of(row: &Row) -> String {
        if truthy(row, "Is_Reference") { text(row, "Reference") } else { text(row, "Flag") }
    }
    pub fn of(&self, row: &Row) -> Color32 {
        let g = if truthy(row, "Is_Reference") { text(row, "Reference") } else { text(row, "Flag") };
        let i = self.order.iter().position(|x| *x == g).unwrap_or(0);
        group_color(&g, self.flags, i)
    }
    pub fn for_rows<'b>(flags: &'b BTreeMap<String, String>, rows: &[&Row]) -> Colors<'b> {
        let mut order: Vec<String> = Vec::new();
        for r in rows {
            let g = if truthy(r, "Is_Reference") { text(r, "Reference") } else { text(r, "Flag") };
            if !order.contains(&g) {
                order.push(g);
            }
        }
        Colors { flags, order }
    }
    pub fn legend(&self) -> Vec<(String, Color32)> {
        self.order.iter().enumerate().map(|(i, g)| (if g.is_empty() { "(no group)".into() } else { g.clone() }, group_color(g, self.flags, i))).collect()
    }
}

fn ellipse_poly(x: f64, hx: f64, y: f64, hy: f64) -> Vec<(f64, f64)> {
    (0..=48).map(|k| {
        let a = std::f64::consts::TAU * k as f64 / 48.0;
        (x + hx * a.cos(), y + hy * a.sin())
    }).collect()
}

/// Chemical isochron: PbO against ThO2* or UO2*, 95 % ellipses, York line.
pub fn isochron(rows: &[&Row], iso: &Isochron, title: &str, colors: &Colors, accent: Color32) -> Figure {
    let xl = if iso.th_form { "ThO₂* (wt%)" } else { "UO₂* (wt%)" };
    let mut f = Figure::new(title, xl, "PbO (wt%)");
    let k = error_ellipse_sigma();
    for (j, &i) in iso.used.iter().enumerate() {
        let (x, y, sx, sy) = (iso.x_star[j], iso.y_pb[j], iso.sx_star[j], iso.sy_pb[j]);
        let c = colors.of(rows[i]);
        f.items.push(Item::Poly { pts: ellipse_poly(x, k * sx, y, k * sy), fill: alpha(c, 0.35), stroke: c });
        f.items.push(Item::Marker { x, y, color: c, size: 2.0, hollow: false, id: Some(i), hover: vec![text(rows[i], "Label"), format!("{xl} {x:.4}, PbO {y:.4}")] });
    }
    for &i in &iso.rejected {
        if let Some((th, u, pb)) = chemchron_core::isochron::extract_th_u_pb(rows[i]) {
            let t = iso.age_ma.unwrap_or(500.0) * 1e6;
            let st = chemchron_core::isochron::th_u_star(th, u, t, &DecayConstants::default());
            let x = if iso.th_form { st.0 } else { st.1 };
            f.items.push(Item::Marker { x, y: pb, color: Color32::GRAY, size: 4.0, hollow: true, id: Some(i), hover: vec![text(rows[i], "Label"), "rejected as outlier".into()] });
        }
    }
    let xs: Vec<f64> = iso.x_star.iter().copied().chain([0.0]).collect();
    let (x0, x1) = (xs.iter().copied().fold(f64::INFINITY, f64::min), xs.iter().copied().fold(f64::NEG_INFINITY, f64::max));
    let pad = ((x1 - x0) * 0.15).max(0.01);
    let a = (x0 - pad).max(0.0);
    let b = x1 + pad;
    f.line(vec![(a, iso.slope * a + iso.intercept), (b, iso.slope * b + iso.intercept)], accent, 2.0);
    f.fit(0.06);
    f.x.0 = f.x.0.min(0.0);
    f.y.0 = f.y.0.min(0.0);
    let age = match (iso.age_ma, iso.age_2s_ma) {
        (Some(a), Some(s)) => format!("{a:.1} ± {s:.1} Ma"),
        _ => "n/a".into(),
    };
    f.notes.push(format!(
        "Chemical isochron age: {age} (n = {}{}){}",
        iso.n,
        iso.mswd.map(|m| format!(", MSWD = {m:.2}")).unwrap_or_default(),
        if iso.mswd_expanded { ", uncertainty × √MSWD" } else { "" }
    ));
    f.notes.push(format!(
        "PbO intercept {:.4} ± {:.4} wt% · analytical 2σ {:.1} Ma, systematic {:.1} Ma ({})",
        iso.intercept,
        iso.intercept_2s,
        iso.age_2s_analytical_ma.unwrap_or(f64::NAN),
        iso.long_term_2s_ma,
        iso.age_2s_method
    ));
    if !iso.rejected.is_empty() {
        f.notes.push(format!("{} of {} analyses rejected (|r| > {:.1}σ, leave-one-out)", iso.rejected.len(), iso.n_candidates, iso.rejection_sigma.unwrap_or(2.0)));
    }
    if !iso.bdl.is_empty() {
        f.notes.push(format!("{} analyses with PbO below detection limit not used", iso.bdl.len()));
    }
    f
}

pub struct Bar {
    pub id: usize,
    /// Sample group or reference material (for the legend).
    pub group: String,
    pub label: String,
    pub age: f64,
    pub s2: f64,
    pub used: bool,
    pub color: Color32,
}

/// Ranked weighted-mean plot (IsoplotR style): one
/// filled column per analysis spanning its 2σ age range, in its group's
/// colour (grey, see-through when not used), a red line for the weighted
/// mean and a light red band for its 2σ, the title "group / mean = … (n/N)"
/// and the MSWD readout in the corner. `mean` is (mean, 2σ, MSWD, P).
pub fn weighted_mean(bars: &mut [Bar], mean: Option<(f64, f64, f64, f64)>, title: &str, y_label: &str, rank: bool, _accent: Color32) -> Figure {
    const EDGE: Color32 = Color32::from_rgb(0x1f, 0x3a, 0x57);
    const NOT_USED: Color32 = Color32::from_rgb(0xa8, 0xa8, 0xa8);
    const NOT_USED_EDGE: Color32 = Color32::from_rgb(0x6e, 0x6e, 0x6e);
    const MEAN: Color32 = Color32::from_rgb(0xaa, 0x33, 0x33);
    const MSWD_OK: Color32 = Color32::from_rgb(0x2f, 0x6b, 0x3a);
    if rank {
        bars.sort_by(|a, b| a.age.partial_cmp(&b.age).unwrap_or(std::cmp::Ordering::Equal));
    }
    let n_used = bars.iter().filter(|b| b.used).count();
    let full_title = match mean {
        Some((m, s2, _, _)) => {
            // as many decimals as the 2σ needs
            let d = if s2 >= 10.0 { 0 } else if s2 >= 1.0 { 1 } else { 2 };
            format!("{title}\nmean = {m:.d$} ± {s2:.d$} Ma (2σ) ({n_used}/{})", bars.len())
        }
        None => title.to_string(),
    };
    let mut f = Figure::new(&full_title, if rank { "Analysis (ranked by age)" } else { "Analysis (table order)" }, y_label);
    f.legend_kind = crate::figure::LegendKind::Bar;
    if let Some((m, s2, _, _)) = mean {
        f.items.push(Item::Band { y0: m - s2, y1: m + s2, fill: alpha(MEAN, 0.15) });
    }
    let half = 0.3;
    for (i, b) in bars.iter().enumerate() {
        let x = i as f64 + 1.0;
        let (fill, edge) = if b.used { (alpha(b.color, 0.95), EDGE) } else { (alpha(NOT_USED, 0.55), NOT_USED_EDGE) };
        f.items.push(Item::Poly { pts: vec![(x - half, b.age - b.s2), (x + half, b.age - b.s2), (x + half, b.age + b.s2), (x - half, b.age + b.s2)], fill, stroke: edge });
        // hover and click target at the column's centre (not drawn)
        f.items.push(Item::Marker {
            x,
            y: b.age,
            color: edge,
            size: 0.0,
            hollow: true,
            id: Some(b.id),
            hover: vec![b.label.clone(), format!("{:.2} ± {:.2} Ma (2σ){}", b.age, b.s2, if b.used { "" } else { ", not used" })],
        });
    }
    f.fit(0.06);
    f.x = (0.3, bars.len() as f64 + 0.7);
    if let Some((m, _, mswd, p)) = mean {
        f.line(vec![(f.x.0, m), (f.x.1, m)], MEAN, 1.6);
        let (y0, y1) = f.y;
        let consistent = p >= 0.05;
        f.items.push(Item::Text {
            x: f.x.0 + 0.98 * (f.x.1 - f.x.0),
            y: y0 + 0.05 * (y1 - y0),
            text: format!("MSWD {mswd:.2}, P {p:.2} ({})", if consistent { "consistent" } else { "overdispersed" }),
            color: if consistent { MSWD_OK } else { MEAN },
            size: 12.0,
            bold: false,
            align: egui::Align::Max,
        });
    }
    let mut legend: Vec<(String, Color32)> = Vec::new();
    if bars.iter().any(|b| !b.used) {
        legend.push(("Rejected / not used".into(), NOT_USED));
    }
    for b in bars.iter().filter(|b| b.used) {
        let name = if b.group.is_empty() { "(no group)".to_string() } else { b.group.clone() };
        if !legend.iter().any(|(n, _)| *n == name) {
            legend.push((name, b.color));
        }
    }
    f.legend = legend;
    f
}

/// Concordia diagram (Wetherill or Tera-Wasserburg) with 95 % ellipses.
#[allow(clippy::too_many_arguments)]
pub fn concordia(
    pts: &[(usize, f64, f64, f64, f64, f64, Color32, bool, String)],
    wetherill: bool,
    c: &DecayConstants,
    ca: Option<&ConcordiaAge>,
    dc: Option<&Discordia>,
    title: &str,
    curve_color: Color32,
    accent: Color32,
) -> Figure {
    let (xl, yl) = if wetherill { ("²⁰⁷Pb/²³⁵U", "²⁰⁶Pb/²³⁸U") } else { ("²³⁸U/²⁰⁶Pb", "²⁰⁷Pb/²⁰⁶Pb") };
    let mut f = Figure::new(title, xl, yl);
    let k = ellipse_scale_95();
    for (id, x, sx, y, sy, rho, color, used, label) in pts {
        let e = concordia::ellipse(*x, *sx, *y, *sy, *rho, k, 60);
        let col = if *used { *color } else { Color32::GRAY };
        f.items.push(Item::Poly { pts: e, fill: alpha(col, if *used { 0.35 } else { 0.12 }), stroke: col });
        f.items.push(Item::Marker { x: *x, y: *y, color: col, size: 1.5, hollow: !*used, id: Some(*id), hover: vec![label.clone(), format!("{xl} {}", fmt5(*x)), format!("{yl} {}", fmt5(*y))] });
    }
    f.fit(0.08);
    // concordia curve over the visible ages
    let (l8, l5) = (c.lambda_238, c.lambda_235);
    let age_at = |x: f64| if wetherill { (x.max(1e-12)).ln_1p() / l5 } else { (1.0 / x.max(1e-12)).ln_1p() / l8 };
    let (ta, tb) = {
        let a = age_at(f.x.0.max(1e-9));
        let b = age_at(f.x.1);
        (a.min(b).max(1e5), a.max(b).min(4.6e9))
    };
    let curve: Vec<(f64, f64)> = (0..=300)
        .map(|i| {
            let t = ta + (tb - ta) * i as f64 / 300.0;
            if wetherill { concordia::wetherill_point(t, l8, l5) } else { concordia::tw_point(t, l8, l5) }
        })
        .collect();
    f.line(curve, curve_color, 1.4);
    // age ticks
    let span_ma = (tb - ta) / 1e6;
    let step = [10.0, 20.0, 50.0, 100.0, 200.0, 250.0, 500.0, 1000.0].into_iter().find(|s| span_ma / s <= 10.0).unwrap_or(1000.0);
    let mut t = (ta / 1e6 / step).ceil() * step;
    while t * 1e6 <= tb {
        let p = if wetherill { concordia::wetherill_point(t * 1e6, l8, l5) } else { concordia::tw_point(t * 1e6, l8, l5) };
        f.items.push(Item::Marker { x: p.0, y: p.1, color: curve_color, size: 2.5, hollow: false, id: None, hover: vec![format!("{t:.0} Ma")] });
        f.items.push(Item::Text { x: p.0, y: p.1, text: format!("  {t:.0}"), color: curve_color, size: 10.0, bold: false, align: egui::Align::Min });
        t += step;
    }
    if let Some(ca) = ca {
        let (mx, my) = ca.mean;
        let (cx, cy) = if wetherill { (mx, my) } else { (1.0 / my, mx / (chemchron_core::upb::U238_U235 * my)) };
        let p = concordia::wetherill_point(ca.age_ma * 1e6, l8, l5);
        let pp = if wetherill { p } else { concordia::tw_point(ca.age_ma * 1e6, l8, l5) };
        let _ = (cx, cy);
        f.items.push(Item::Marker { x: pp.0, y: pp.1, color: accent, size: 6.0, hollow: true, id: None, hover: vec![format!("Concordia age {:.2} ± {:.2} Ma", ca.age_ma, ca.age_2s_ma)] });
        f.notes.push(format!(
            "Concordia age {:.2} ± {:.2} Ma (2σ{}), n = {}, MSWD (conc. + equiv.) = {:.2}, P = {:.3}",
            ca.age_ma,
            ca.age_2s_ma,
            if ca.decay_errors { ", incl. decay constants" } else { "" },
            ca.n,
            ca.mswd,
            ca.p
        ));
    }
    if let Some(d) = dc {
        let line: Vec<(f64, f64)> = if wetherill {
            // the Tera-Wasserburg line mapped point by point to Wetherill space
            (0..=400)
                .filter_map(|i| {
                    let x = 10f64.powf(-1.5 + 5.0 * i as f64 / 400.0);
                    let y = d.fit.a + d.fit.b * x;
                    (y > 0.0).then(|| (y * chemchron_core::upb::U238_U235 / x, 1.0 / x))
                })
                .collect()
        } else {
            let (a, b) = (f.x.0.min(0.0), f.x.1);
            vec![(a, d.fit.a + d.fit.b * a), (b, d.fit.a + d.fit.b * b)]
        };
        f.dashed(line, accent, 1.5);
        let ints: Vec<String> = d
            .intercepts
            .iter()
            .map(|i| if i.anchored { format!("{:.1} Ma (anchored)", i.age_ma) } else { format!("{:.1} ± {:.1} Ma", i.age_ma, i.age_2s_ma) })
            .collect();
        f.notes.push(format!(
            "Discordia: intercepts {} · MSWD = {:.2}, n = {}{}",
            if ints.is_empty() { "none".into() } else { ints.join(", ") },
            d.fit.mswd,
            d.fit.n,
            if d.expanded { " (× √MSWD)" } else { "" }
        ));
    }
    f
}

fn fmt5(v: f64) -> String {
    chemchron_laser::theme::fmt_sig(v, 5)
}

const SQ3: f64 = 0.866_025_403_784_438_6;

fn tern(_a: f64, b: f64, c: f64) -> (f64, f64) {
    (b + 0.5 * c, SQ3 * c)
}

/// Ternary plot from three fractions per row (normalised): the triangle,
/// bold corner labels, markers with dark edges.
pub fn ternary(rows: &[&Row], fields: [&str; 3], labels: [&str; 3], title: &str, colors: &Colors, frame: Color32) -> (Figure, usize) {
    let mut f = Figure::new(title, "", "");
    f.bare = true;
    f.line(vec![(0.0, 0.0), (1.0, 0.0), (0.5, SQ3), (0.0, 0.0)], frame, 1.6);
    let label = |x: f64, y: f64, t: &str, align: egui::Align| Item::Text { x, y, text: t.into(), color: frame, size: 14.0, bold: true, align };
    f.items.push(label(-0.02, -0.08, labels[0], egui::Align::Max));
    f.items.push(label(1.02, -0.08, labels[1], egui::Align::Min));
    f.items.push(label(0.5, SQ3 + 0.07, labels[2], egui::Align::Center));
    let mut n = 0;
    for (i, r) in rows.iter().enumerate() {
        let v: Vec<Option<f64>> = fields.iter().map(|k| num(r, k)).collect();
        let (Some(a), Some(b), Some(c)) = (v[0], v[1], v[2]) else { continue };
        let s = a + b + c;
        if s <= 0.0 || a < 0.0 || b < 0.0 || c < 0.0 {
            continue;
        }
        let (x, y) = tern(a / s, b / s, c / s);
        f.items.push(Item::Marker {
            x,
            y,
            color: colors.of(r),
            size: 5.0,
            hollow: false,
            id: Some(i),
            hover: vec![text(r, "Label"), format!("{} {:.3} · {} {:.3} · {} {:.3}", labels[0], a / s, labels[1], b / s, labels[2], c / s)],
        });
        n += 1;
    }
    f.x = (-0.25, 1.25);
    f.y = (-0.16, SQ3 + 0.14);
    f.legend = colors.legend();
    (f, n)
}

pub fn exchange_vector(rows: &[&Row], title: &str, colors: &Colors) -> (Figure, usize) {
    let mut f = Figure::new(title, "Th + U + Si (apfu)", "REE + Y + P (apfu)");
    let mut n = 0;
    for (i, r) in rows.iter().enumerate() {
        if let (Some(x), Some(y)) = (num(r, "ThUSi_af"), num(r, "REEYP_af")) {
            f.marker(x, y, colors.of(r), Some(i), vec![text(r, "Label"), format!("{x:.3}, {y:.3} apfu")]);
            n += 1;
        }
    }
    f.fit(0.1);
    f.dashed(vec![(0.0, 2.0), (2.0, 0.0)], Color32::from_rgb(179, 88, 6), 1.3);
    f.dashed(vec![(0.0, 2.0), (0.5, 1.0)], Color32::from_rgb(27, 120, 55), 1.3);
    let mut leg = colors.legend();
    leg.push(("100 % Ht line".into(), Color32::from_rgb(179, 88, 6)));
    leg.push(("100 % Cher line".into(), Color32::from_rgb(27, 120, 55)));
    f.legend = leg;
    (f, n)
}

pub fn binary(rows: &[&Row], xf: &str, yf: &str, xl: &str, yl: &str, title: &str, colors: &Colors, log_y: bool) -> (Figure, usize) {
    let mut f = Figure::new(title, xl, yl);
    f.log_y = log_y;
    let mut n = 0;
    for (i, r) in rows.iter().enumerate() {
        if let (Some(x), Some(y)) = (num(r, xf), num(r, yf)) {
            if log_y && y <= 0.0 {
                continue;
            }
            f.marker(x, y, colors.of(r), Some(i), vec![text(r, "Label"), format!("{xl} {}", fmt5(x)), format!("{yl} {}", fmt5(y))]);
            n += 1;
        }
    }
    f.fit(0.06);
    f.legend = colors.legend();
    (f, n)
}

/// REE diagram: one pattern per analysis, or the
/// median per group with its interquartile range (or min to max) shaded (over runs of
/// determined elements). Filled symbols: EPMA values; open symbols: laser /
/// imported (ppm) values.
#[allow(clippy::too_many_arguments)]
pub fn ree_diagram(rows: &[&Row], norm: &NormSet, include_y: bool, src: Source, groups: bool, range: bool, title: &str, colors: &Colors, group_of: &dyn Fn(&Row) -> String) -> Figure {
    let els = ree::element_sequence(include_y);
    let mut f = Figure::new(&format!("{title}\nnormalized to {}", norm.name), "", "Sample / chondrite");
    f.log_y = true;
    f.legend_kind = crate::figure::LegendKind::Line;
    f.x_categories = els.iter().map(|e| e.to_string()).collect();
    let mut any_imported = false;
    let mut any_epma = false;
    let mut draw = |f: &mut Figure, vals: &[Option<f64>], imported: &[bool], c: Color32, w: f32, label: &str, id: Option<usize>, size: f32| {
        for (i, j, bridged) in ree::pattern_segments(vals) {
            let pts = vec![(i as f64, vals[i].unwrap()), (j as f64, vals[j].unwrap())];
            if bridged {
                f.dashed(pts, c, w);
            } else {
                f.line(pts, c, w);
            }
        }
        for (i, v) in vals.iter().enumerate() {
            if let Some(v) = v {
                let hollow = imported.get(i).copied().unwrap_or(false);
                if hollow {
                    any_imported = true;
                } else {
                    any_epma = true;
                }
                f.items.push(Item::Marker {
                    x: i as f64,
                    y: *v,
                    color: c,
                    size,
                    hollow,
                    id,
                    hover: vec![label.to_string(), format!("{} {} ({})", els[i], fmt5(*v), if hollow { "LA-ICP-MS" } else { "EPMA" })],
                });
            }
        }
    };
    if groups {
        let mut names: Vec<String> = Vec::new();
        for r in rows {
            let g = group_of(r);
            if !names.contains(&g) {
                names.push(g);
            }
        }
        for (gi, g) in names.iter().enumerate() {
            let members: Vec<&Row> = rows.iter().copied().filter(|r| group_of(r) == *g).collect();
            let gp: GroupPattern = ree::group_pattern(&members, &norm.values, &els, src);
            let c = group_color(g, colors.flags, gi);
            // shaded envelope (IQR, or min to max): one quadrilateral per pair of neighbouring determined elements
            let (lo, hi) = if range { (&gp.min, &gp.max) } else { (&gp.q1, &gp.q3) };
            for k in 0..els.len().saturating_sub(1) {
                if let (Some(a1), Some(a3), Some(b1), Some(b3)) = (lo[k], hi[k], lo[k + 1], hi[k + 1]) {
                    f.items.push(Item::Poly { pts: vec![(k as f64, a1), ((k + 1) as f64, b1), ((k + 1) as f64, b3), (k as f64, a3)], fill: alpha(c, 0.2), stroke: Color32::TRANSPARENT });
                }
            }
            draw(&mut f, &gp.median, &gp.imported, c, 2.0, &format!("{g} median"), None, 4.0);
            f.legend.push((format!("{} (n = {})", if g.is_empty() { "(no group)" } else { g }, members.len()), c));
        }
    } else {
        for (i, r) in rows.iter().enumerate() {
            let pat = ree::normalized_pattern(r, &norm.values, &els, src);
            let vals: Vec<Option<f64>> = pat.iter().map(|v| v.map(|x| x.0)).collect();
            let imported: Vec<bool> = pat.iter().map(|v| v.is_some_and(|x| x.1)).collect();
            draw(&mut f, &vals, &imported, colors.of(r), 1.2, &text(r, "Label"), Some(i), 3.0);
        }
        f.legend = colors.legend();
    }
    if any_imported {
        let grey = Color32::from_rgb(0x70, 0x70, 0x70);
        if any_epma {
            f.legend.push(("EPMA".into(), grey));
        }
        f.legend.push(("LA-ICP-MS (open)".into(), grey));
        f.legend_hollow.push("LA-ICP-MS (open)".into());
    }
    f.fit(0.05);
    f.x = (-0.5, els.len() as f64 - 0.5);
    f
}

/// Kernel density estimate with a histogram of ages (one curve per series).
pub fn age_distribution(series: &[(String, Vec<f64>, Color32)], bandwidth: f64, title: &str) -> Figure {
    let mut f = Figure::new(title, "Age (Ma)", "Density");
    let all: Vec<f64> = series.iter().flat_map(|s| s.1.iter().copied()).filter(|v| v.is_finite()).collect();
    if all.is_empty() {
        return f;
    }
    let (lo, hi) = (all.iter().copied().fold(f64::INFINITY, f64::min), all.iter().copied().fold(f64::NEG_INFINITY, f64::max));
    let span = (hi - lo).max(1.0);
    let (a, b) = (lo - 0.1 * span, hi + 0.1 * span);
    for (name, ages, c) in series {
        if ages.is_empty() {
            continue;
        }
        // Silverman's rule with the smaller of SD and IQR/1.34, so groups of
        // very different ages (two peaks) don't blur into one broad hump
        let sd = stdev(ages);
        let mut sorted: Vec<f64> = ages.iter().copied().filter(|v| v.is_finite()).collect();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let q = |p: f64| sorted.get(((sorted.len() - 1) as f64 * p).round() as usize).copied().unwrap_or(0.0);
        let iqr = if sorted.len() >= 4 { (q(0.75) - q(0.25)) / 1.34 } else { sd };
        let spread = if iqr > 0.0 { sd.min(iqr) } else { sd };
        let silverman = 0.9 * spread * (ages.len() as f64).powf(-0.2);
        // Ages of several groups (e.g. 600 and 1850 Ma) make SD and IQR large;
        // the typical distance to the k-th nearest age follows each cluster instead.
        let n = sorted.len();
        let k = ((n as f64).sqrt().ceil() as usize).clamp(1, n.saturating_sub(1).max(1));
        let mut knn: Vec<f64> = (0..n)
            .map(|i| {
                let mut d: Vec<f64> = sorted.iter().map(|v| (v - sorted[i]).abs()).collect();
                d.sort_by(|a, b| a.total_cmp(b));
                d.get(k).copied().unwrap_or(0.0)
            })
            .collect();
        knn.sort_by(|a, b| a.total_cmp(b));
        let local = knn.get(n / 2).copied().unwrap_or(0.0);
        let auto = if local > 0.0 { silverman.min(local) } else { silverman };
        let h = if bandwidth > 0.0 { bandwidth } else { auto.max(span * 0.004) };
        // fine enough steps for narrow peaks (at least four per bandwidth)
        let steps = (((b - a) / (h / 4.0)).ceil() as usize).clamp(300, 6000);
        let pts: Vec<(f64, f64)> = (0..=steps)
            .map(|i| {
                let x = a + (b - a) * i as f64 / steps as f64;
                let d: f64 = ages.iter().map(|v| (-0.5 * ((x - v) / h).powi(2)).exp()).sum::<f64>() / (ages.len() as f64 * h * (std::f64::consts::TAU).sqrt());
                (x, d)
            })
            .collect();
        let mut poly = pts.clone();
        poly.push((b, 0.0));
        poly.push((a, 0.0));
        f.line(pts, *c, 2.0);
        for v in ages {
            f.items.push(Item::Line { pts: vec![(*v, 0.0), (*v, 0.0)], color: *c, width: 1.0, dashed: false });
            f.items.push(Item::Marker { x: *v, y: 0.0, color: *c, size: 2.0, hollow: false, id: None, hover: vec![format!("{name}: {v:.1} Ma")] });
        }
        f.legend.push((format!("{name} (n = {})", ages.len()), *c));
    }
    f.fit(0.05);
    f.y.0 = 0.0;
    f
}

/// One logged reference analysis for the long-term plot.
pub struct LogPoint {
    pub reference: String,
    pub session: String,
    pub stamp: String,
    pub label: String,
    /// Deviation from the accepted age, % (and its 2σ, %).
    pub dev: f64,
    pub dev2s: f64,
}

/// Long-term log over time: every logged reference analysis as its deviation
/// from the accepted age (%), session after session (in time order), coloured
/// by material, with the 2σ as a bar; the zero line is the accepted age.
pub fn long_term_plot(points: &[LogPoint], title: &str, colors: &BTreeMap<String, String>) -> Figure {
    let mut f = Figure::new(title, "Session (in time order)", "Deviation from accepted age (%)");
    // sessions in time order (earliest entry first)
    let mut sessions: Vec<(String, String)> = Vec::new();
    for p in points {
        match sessions.iter_mut().find(|s| s.0 == p.session) {
            Some(s) => {
                if p.stamp < s.1 {
                    s.1 = p.stamp.clone();
                }
            }
            None => sessions.push((p.session.clone(), p.stamp.clone())),
        }
    }
    sessions.sort_by(|a, b| a.1.cmp(&b.1));
    let mut refs: Vec<String> = points.iter().map(|p| p.reference.clone()).collect();
    refs.sort();
    refs.dedup();
    for (k, p) in points.iter().enumerate() {
        let Some(si) = sessions.iter().position(|s| s.0 == p.session) else { continue };
        let ri = refs.iter().position(|r| *r == p.reference).unwrap_or(0);
        // spread the analyses of a session a little so they don't overlap
        let n_in = points.iter().filter(|q| q.session == p.session).count().max(1);
        let j = points.iter().take(k).filter(|q| q.session == p.session).count();
        let x = si as f64 + 1.0 + 0.6 * ((j as f64 + 0.5) / n_in as f64 - 0.5);
        let c = group_color(&p.reference, colors, ri);
        f.items.push(Item::Line { pts: vec![(x, p.dev - p.dev2s), (x, p.dev + p.dev2s)], color: alpha(c, 0.6), width: 1.2, dashed: false });
        f.items.push(Item::Marker { x, y: p.dev, color: c, size: 3.5, hollow: false, id: None, hover: vec![format!("{} ({})", p.label, p.reference), format!("session {}", p.session), format!("{:+.2} ± {:.2} %", p.dev, p.dev2s)] });
    }
    f.fit(0.08);
    f.x = (0.4, sessions.len() as f64 + 0.6);
    f.line(vec![(f.x.0, 0.0), (f.x.1, 0.0)], Color32::from_rgb(0x55, 0x55, 0x55), 1.0);
    f.legend = refs.iter().enumerate().map(|(i, r)| (r.clone(), group_color(r, colors, i))).collect();
    // session names under the axis, in time order
    f.x_categories = std::iter::once(String::new()).chain(sessions.iter().map(|s| s.0.clone())).collect();
    f
}

#[cfg(test)]
mod kde_tests {
    use super::*;

    #[test]
    fn two_age_groups_give_two_peaks() {
        let mut ages: Vec<f64> = (0..20).map(|i| 610.0 + (i as f64 - 10.0) * 1.5).collect();
        ages.extend((0..24).map(|i| 1850.0 + (i as f64 - 12.0) * 2.5));
        let f = age_distribution(&[("all".into(), ages, Color32::RED)], 0.0, "");
        let curve = f.items.iter().find_map(|it| match it {
            Item::Line { pts, .. } if pts.len() > 100 => Some(pts.clone()),
            _ => None,
        }).unwrap();
        let at = |x: f64| curve.iter().min_by(|a, b| (a.0 - x).abs().total_cmp(&(b.0 - x).abs())).unwrap().1;
        let peak = curve.iter().map(|p| p.1).fold(0.0, f64::max);
        assert!(at(1200.0) < 0.02 * peak, "the gap between the groups stays empty: {} vs {peak}", at(1200.0));
        assert!(at(610.0) > 0.3 * peak && at(1850.0) > 0.3 * peak);
    }
}
