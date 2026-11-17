// SPDX-License-Identifier: GPL-3.0-or-later
//! Publication figures as SVG: concordia and weighted-mean plots.

use crate::drs::Reduction;
use crate::geochron::{ellipse, ConcordiaPoint};
use crate::model::*;
use std::fmt::Write as _;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConcordiaKind {
    Wetherill,
    TeraWasserburg,
}

/// Rounded tick positions covering a range.
pub fn nice_ticks(lo: f64, hi: f64, count: usize) -> Vec<f64> {
    if !(hi > lo) || !lo.is_finite() || !hi.is_finite() {
        return Vec::new();
    }
    let raw = (hi - lo) / count.max(1) as f64;
    let mag = 10f64.powf(raw.log10().floor());
    let norm = raw / mag;
    let step = mag * if norm < 1.5 { 1.0 } else if norm < 3.5 { 2.0 } else if norm < 7.5 { 5.0 } else { 10.0 };
    let mut t = (lo / step).ceil() * step;
    let mut out = Vec::new();
    while t <= hi + step * 1e-6 {
        out.push(if t.abs() < step * 1e-9 { 0.0 } else { t });
        t += step;
    }
    out
}

/// Age labels for concordia, spaced to suit the range shown.
pub fn age_ticks(lo: f64, hi: f64) -> Vec<f64> {
    let span = (hi - lo).max(1.0);
    let step = [1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0]
        .iter()
        .copied()
        .find(|s| span / s <= 9.0)
        .unwrap_or(1000.0);
    let mut out = Vec::new();
    let mut t = (lo / step).ceil() * step;
    while t <= hi {
        if t > 0.0 {
            out.push(t);
        }
        t += step;
    }
    out
}

/// One analysis as a concordia point, or None if a value is missing.
pub fn point_for(kind: ConcordiaKind, u: &crate::drs::upb::UpbValue) -> Option<ConcordiaPoint> {
    let p = match kind {
        ConcordiaKind::Wetherill => ConcordiaPoint {
            x: u.r75.mean,
            y: u.r68.mean,
            sx: u.r75.prop2s,
            sy: u.r68.prop2s,
            rho: u.rho_68_75,
        },
        ConcordiaKind::TeraWasserburg => ConcordiaPoint {
            x: u.r38,
            y: u.r76.mean,
            sx: u.r38_prop2s,
            sy: u.r76.prop2s,
            rho: u.rho_76_38,
        },
    };
    (p.x.is_finite() && p.y.is_finite() && p.sx.is_finite() && p.sy.is_finite()).then_some(p)
}

/// Curve value at age t.
pub fn curve_point(kind: ConcordiaKind, d: &crate::geochron::DecayConstants, t: f64) -> (f64, f64) {
    match kind {
        ConcordiaKind::Wetherill => (d.r75(t), d.r68(t)),
        ConcordiaKind::TeraWasserburg => (1.0 / d.r68(t), d.r76(t)),
    }
}

/// Age range of the curve inside a data window.
pub fn visible_ages(kind: ConcordiaKind, d: &crate::geochron::DecayConstants, x: (f64, f64), y: (f64, f64)) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = 0.0f64;
    let mut t = 1.0;
    while t < 4600.0 {
        let (px, py) = curve_point(kind, d, t);
        if px >= x.0 && px <= x.1 && py >= y.0 && py <= y.1 {
            lo = lo.min(t);
            hi = hi.max(t);
        }
        t *= 1.01;
    }
    if lo.is_finite() {
        (lo, hi)
    } else {
        (1.0, 4600.0)
    }
}

/// Data window around the analyses, with a margin.
pub fn window(points: &[ConcordiaPoint]) -> Option<((f64, f64), (f64, f64))> {
    if points.is_empty() {
        return None;
    }
    let mut x = (f64::INFINITY, f64::NEG_INFINITY);
    let mut y = (f64::INFINITY, f64::NEG_INFINITY);
    for p in points {
        x.0 = x.0.min(p.x - p.sx);
        x.1 = x.1.max(p.x + p.sx);
        y.0 = y.0.min(p.y - p.sy);
        y.1 = y.1.max(p.y + p.sy);
    }
    let (mx, my) = (((x.1 - x.0) * 0.08).max(1e-9), ((y.1 - y.0) * 0.08).max(1e-9));
    Some(((x.0 - mx, x.1 + mx), (y.0 - my, y.1 + my)))
}

fn header(w: f64, h: f64) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\" font-family=\"Helvetica, Arial, sans-serif\" font-size=\"12\">\n<rect width=\"{w}\" height=\"{h}\" fill=\"white\"/>\n"
    )
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn fmt_tick(v: f64, span: f64) -> String {
    let decimals = (2.0 - span.log10().floor()).clamp(0.0, 6.0) as usize;
    format!("{v:.decimals$}")
}

struct Frame {
    x0: f64,
    y0: f64,
    w: f64,
    h: f64,
    xr: (f64, f64),
    yr: (f64, f64),
}

impl Frame {
    fn px(&self, x: f64) -> f64 {
        self.x0 + (x - self.xr.0) / (self.xr.1 - self.xr.0) * self.w
    }
    fn py(&self, y: f64) -> f64 {
        self.y0 + self.h - (y - self.yr.0) / (self.yr.1 - self.yr.0) * self.h
    }
    fn axes(&self, out: &mut String, x_label: &str, y_label: &str) {
        let _ = writeln!(
            out,
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"#222\"/>",
            self.x0, self.y0, self.w, self.h
        );
        let (span_x, span_y) = (self.xr.1 - self.xr.0, self.yr.1 - self.yr.0);
        for t in nice_ticks(self.xr.0, self.xr.1, 6) {
            let x = self.px(t);
            let bottom = self.y0 + self.h;
            let _ = writeln!(out, "<line x1=\"{x:.2}\" y1=\"{bottom}\" x2=\"{x:.2}\" y2=\"{:.2}\" stroke=\"#222\"/>", bottom + 5.0);
            let _ = writeln!(out, "<text x=\"{x:.2}\" y=\"{:.2}\" text-anchor=\"middle\">{}</text>", bottom + 18.0, fmt_tick(t, span_x));
        }
        for t in nice_ticks(self.yr.0, self.yr.1, 6) {
            let y = self.py(t);
            let _ = writeln!(out, "<line x1=\"{:.2}\" y1=\"{y:.2}\" x2=\"{}\" y2=\"{y:.2}\" stroke=\"#222\"/>", self.x0 - 5.0, self.x0);
            let _ = writeln!(out, "<text x=\"{:.2}\" y=\"{:.2}\" text-anchor=\"end\">{}</text>", self.x0 - 8.0, y + 4.0, fmt_tick(t, span_y));
        }
        let _ = writeln!(out, "<text x=\"{:.2}\" y=\"{:.2}\" text-anchor=\"middle\">{}</text>", self.x0 + self.w / 2.0, self.y0 + self.h + 38.0, escape(x_label));
        let _ = writeln!(
            out,
            "<text transform=\"translate({:.2},{:.2}) rotate(-90)\" text-anchor=\"middle\">{}</text>",
            self.x0 - 62.0,
            self.y0 + self.h / 2.0,
            escape(y_label)
        );
    }
}

fn rgb(c: [u8; 3]) -> String {
    format!("rgb({},{},{})", c[0], c[1], c[2])
}

/// Concordia diagram of chosen groups, ellipses at 2 sigma with propagated uncertainties.
pub fn concordia_svg(session: &Session, red: &Reduction, kind: ConcordiaKind, groups: &[u64]) -> String {
    let d = &session.settings.upb.decay;
    let mut pts = Vec::new();
    for (s, r) in session.selections.iter().zip(&red.selections) {
        if s.excluded || !groups.contains(&s.group) {
            continue;
        }
        if let Some(p) = r.upb.as_ref().and_then(|u| point_for(kind, u)) {
            pts.push((p, session.group(s.group).map_or([80, 80, 80], |g| g.color)));
        }
    }
    let (w, h) = (640.0, 520.0);
    let mut out = header(w, h);
    let only: Vec<ConcordiaPoint> = pts.iter().map(|p| p.0).collect();
    let Some((xr, yr)) = window(&only) else {
        out.push_str("<text x=\"20\" y=\"30\">No analyses to plot</text></svg>");
        return out;
    };
    let frame = Frame { x0: 90.0, y0: 20.0, w: 520.0, h: 440.0, xr, yr };
    let (x_label, y_label) = match kind {
        ConcordiaKind::Wetherill => ("207Pb/235U", "206Pb/238U"),
        ConcordiaKind::TeraWasserburg => ("238U/206Pb", "207Pb/206Pb"),
    };
    let _ = writeln!(
        out,
        "<clipPath id=\"plot\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/></clipPath>",
        frame.x0, frame.y0, frame.w, frame.h
    );
    out.push_str("<g clip-path=\"url(#plot)\">\n");
    let (lo, hi) = visible_ages(kind, d, xr, yr);
    let mut path = String::new();
    let span = (hi - lo).max(1.0);
    let mut t = (lo - span * 0.1).max(0.5);
    let end = hi + span * 0.1;
    let step = ((end - t) / 300.0).max(1e-3);
    while t <= end {
        let (x, y) = curve_point(kind, d, t);
        let _ = write!(path, "{}{:.2},{:.2} ", if path.is_empty() { "M" } else { "L" }, frame.px(x), frame.py(y));
        t += step;
    }
    let _ = writeln!(out, "<path d=\"{path}\" fill=\"none\" stroke=\"#a03030\" stroke-width=\"1.4\"/>");
    for (p, c) in &pts {
        let mut e = String::new();
        for (i, (x, y)) in ellipse(p, 48).iter().enumerate() {
            let _ = write!(e, "{}{:.2},{:.2} ", if i == 0 { "M" } else { "L" }, frame.px(*x), frame.py(*y));
        }
        let color = rgb(*c);
        let _ = writeln!(out, "<path d=\"{e}Z\" fill=\"{color}\" fill-opacity=\"0.35\" stroke=\"{color}\"/>");
    }
    for age in age_ticks(lo, hi) {
        let (x, y) = curve_point(kind, d, age);
        let (px, py) = (frame.px(x), frame.py(y));
        let _ = writeln!(out, "<circle cx=\"{px:.2}\" cy=\"{py:.2}\" r=\"3\" fill=\"#a03030\"/>");
        let _ = writeln!(out, "<text x=\"{:.2}\" y=\"{:.2}\" fill=\"#a03030\" font-size=\"11\">{age:.0} Ma</text>", px + 6.0, py - 4.0);
    }
    out.push_str("</g>\n");
    frame.axes(&mut out, x_label, y_label);
    out.push_str("</svg>\n");
    out
}

/// Weighted-mean plot of one group: 206Pb/238U ages with 2 sigma bars.
pub fn weighted_mean_svg(session: &Session, red: &Reduction, group: u64) -> String {
    let mut bars: Vec<(String, f64, f64)> = Vec::new();
    for (s, r) in session.selections.iter().zip(&red.selections) {
        if s.excluded || s.group != group {
            continue;
        }
        if let Some(u) = &r.upb {
            bars.push((s.label.clone(), u.a68.age, u.a68.prop2s));
        }
    }
    let (w, h) = (640.0, 420.0);
    let mut out = header(w, h);
    if bars.is_empty() {
        out.push_str("<text x=\"20\" y=\"30\">No analyses to plot</text></svg>");
        return out;
    }
    let lo = bars.iter().map(|b| b.1 - b.2).fold(f64::INFINITY, f64::min);
    let hi = bars.iter().map(|b| b.1 + b.2).fold(f64::NEG_INFINITY, f64::max);
    let pad = ((hi - lo) * 0.1).max(1e-3);
    let frame = Frame {
        x0: 90.0,
        y0: 50.0,
        w: 520.0,
        h: 320.0,
        xr: (0.0, bars.len() as f64 + 1.0),
        yr: (lo - pad, hi + pad),
    };
    let values: Vec<(f64, f64)> = bars.iter().map(|b| (b.1, b.2)).collect();
    let color = session.group(group).map_or([80, 80, 80], |g| g.color);
    if let Some(m) = crate::stats::weighted_mean(&values) {
        let (top, bottom) = (frame.py(m.mean + m.unc2s_mswd), frame.py(m.mean - m.unc2s_mswd));
        let _ = writeln!(
            out,
            "<rect x=\"{}\" y=\"{top:.2}\" width=\"{}\" height=\"{:.2}\" fill=\"{}\" fill-opacity=\"0.18\"/>",
            frame.x0,
            frame.w,
            bottom - top,
            rgb(color)
        );
        let y = frame.py(m.mean);
        let _ = writeln!(out, "<line x1=\"{}\" y1=\"{y:.2}\" x2=\"{}\" y2=\"{y:.2}\" stroke=\"#222\"/>", frame.x0, frame.x0 + frame.w);
        let name = session.group(group).map_or("", |g| g.name.as_str());
        let _ = writeln!(
            out,
            "<text x=\"{}\" y=\"30\" font-size=\"13\">{}: mean 206Pb/238U age {:.1} ± {:.1} Ma (2s), MSWD {:.2}, n = {}</text>",
            frame.x0,
            escape(name),
            m.mean,
            m.unc2s_mswd,
            m.mswd,
            m.n
        );
    }
    for (i, (_, age, s)) in bars.iter().enumerate() {
        let x = frame.px(i as f64 + 1.0);
        let _ = writeln!(
            out,
            "<line x1=\"{x:.2}\" y1=\"{:.2}\" x2=\"{x:.2}\" y2=\"{:.2}\" stroke=\"{}\" stroke-width=\"3\"/>",
            frame.py(age + s),
            frame.py(age - s),
            rgb(color)
        );
    }
    frame.axes(&mut out, "Analysis number", "206Pb/238U age (Ma)");
    out.push_str("</svg>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_are_round_and_cover_the_range() {
        let t = nice_ticks(0.043, 0.081, 5);
        assert!(t.len() >= 3);
        assert!(t.iter().all(|v| *v >= 0.043 && *v <= 0.081));
        assert!((t[1] - t[0] - 0.01).abs() < 1e-9 || (t[1] - t[0] - 0.005).abs() < 1e-9);
    }

    #[test]
    fn an_empty_range_gives_no_ticks() {
        assert!(nice_ticks(1.0, 1.0, 5).is_empty());
    }
}
