// SPDX-License-Identifier: GPL-3.0-or-later
//! A simulated session with Poisson counting noise and known truth: drift,
//! down-hole fractionation, concentrations and ages. It is the built-in demo
//! and the independent test that the reduction gets the right answer.

use crate::autosel::{from_log, AutoOptions};
use crate::geochron::{stacey_kramers, DecayConstants};
use crate::model::*;
use crate::refmat;

pub const EPOCH: f64 = 1_735_000_000.0;
pub const SWEEP_S: f64 = 0.25;
/// Dwell per channel: quantises low signals into whole counts, like real data.
pub const DWELL_S: f64 = 0.01;
/// Sensitivity for mass 100, cps per ppm.
const SENSITIVITY: f64 = 900.0;
/// True 206Pb/238U down-hole fractionation at beam time tau.
pub fn true_dhf(tau: f64) -> f64 {
    1.0 + 0.30 * (-0.14 * tau).exp()
}

/// Simple deterministic random numbers (xorshift64*).
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1)
    }
    pub fn unit(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let v = self.0.wrapping_mul(0x2545F4914F6CDD1D);
        (v >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn normal(&mut self) -> f64 {
        let u = self.unit().max(1e-12);
        let v = self.unit();
        (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
    }
    pub fn poisson(&mut self, mean: f64) -> f64 {
        if mean <= 0.0 {
            return 0.0;
        }
        if mean > 40.0 {
            return (mean + mean.sqrt() * self.normal()).round().max(0.0);
        }
        let limit = (-mean).exp();
        let mut k = 0.0;
        let mut p = 1.0;
        loop {
            p *= self.unit();
            if p <= limit {
                return k;
            }
            k += 1.0;
        }
    }
}

/// What one material truly contains.
#[derive(Clone, Debug)]
pub struct Truth {
    pub name: &'static str,
    pub age_ma: f64,
    pub u: f64,
    pub th: f64,
    /// Share of 206Pb that is common Pb.
    pub f206: f64,
    /// Radiogenic Pb lost, as a fraction: produces discordance.
    pub pb_loss: f64,
    pub ree_factor: f64,
    pub ce: f64,
}

pub const NIST610_GLASS: &str = "NIST610";
pub const NIST612_GLASS: &str = "NIST612";

pub fn truths() -> Vec<Truth> {
    vec![
        Truth { name: "44069", age_ma: 424.9, u: 3200.0, th: 42000.0, f206: 0.0, pb_loss: 0.0, ree_factor: 1.0, ce: 230000.0 },
        Truth { name: "Trebilcock", age_ma: 272.0, u: 1800.0, th: 60000.0, f206: 0.0, pb_loss: 0.0, ree_factor: 0.9, ce: 210000.0 },
        // verified secondaries of the U–Pb demo (Gonçalves et al. 2016, ID-TIMS)
        Truth { name: "MBA", age_ma: 507.7, u: 2500.0, th: 52000.0, f206: 0.0, pb_loss: 0.0, ree_factor: 1.0, ce: 220000.0 },
        Truth { name: "MDI", age_ma: 506.44, u: 2607.0, th: 88564.0, f206: 0.0, pb_loss: 0.0, ree_factor: 0.95, ce: 225000.0 },
        Truth { name: "Sample A", age_ma: 1850.0, u: 2600.0, th: 38000.0, f206: 0.0, pb_loss: 0.0, ree_factor: 1.1, ce: 250000.0 },
        Truth { name: "Sample B", age_ma: 615.0, u: 1500.0, th: 52000.0, f206: 0.0, pb_loss: 0.0, ree_factor: 0.8, ce: 240000.0 },
    ]
}

fn sensitivity(mass: u32) -> f64 {
    SENSITIVITY * (0.45 + 0.55 * (mass as f64 / 100.0).min(1.6))
}

/// Chondrite-shaped monazite REE content, ppm.
fn monazite_content(element: &str, t: &Truth) -> f64 {
    let v = match element {
        "P" => 130000.0,
        "Ca" => 6000.0,
        "Y" => 9000.0,
        "La" => 120000.0,
        "Ce" => t.ce,
        "Pr" => 28000.0,
        "Nd" => 110000.0,
        "Sm" => 18000.0,
        "Eu" => 700.0,
        "Gd" => 10000.0,
        "Tb" => 900.0,
        "Dy" => 3500.0,
        "Ho" => 400.0,
        "Er" => 700.0,
        "Tm" => 70.0,
        "Yb" => 300.0,
        "Lu" => 25.0,
        _ => 0.0,
    };
    if element == "Ce" {
        v
    } else {
        v * t.ree_factor
    }
}

const CHANNELS: [(&str, u32); 24] = [
    ("P", 31), ("Ca", 43), ("Y", 89), ("La", 139), ("Ce", 140), ("Pr", 141), ("Nd", 146), ("Sm", 147),
    ("Eu", 153), ("Gd", 157), ("Tb", 159), ("Dy", 163), ("Ho", 165), ("Er", 166), ("Tm", 169), ("Yb", 172),
    ("Lu", 175), ("Hg", 202), ("Pb", 204), ("Pb", 206), ("Pb", 207), ("Pb", 208), ("Th", 232), ("U", 238),
];

struct Spot {
    label: String,
    /// Index in truths, or None for a glass.
    truth: Option<usize>,
    /// The glass (reference-material name) when `truth` is None.
    glass: &'static str,
    f206: f64,
    pb_loss: f64,
    /// A xenotime inclusion hit for a few seconds: Y and the HREE jump.
    inclusion: bool,
}

/// Elements a xenotime inclusion raises in the demo.
pub const INCLUSION_ELEMENTS: [&str; 9] = ["Y", "Gd", "Tb", "Dy", "Ho", "Er", "Tm", "Yb", "Lu"];
/// Seconds after laser-on during which the demo inclusion is ablated.
pub const INCLUSION_WINDOW: (f64, f64) = (9.0, 12.0);

fn plan_spots(part: DemoPart) -> Vec<Spot> {
    let truth = truths();
    let idx = |name: &str| truth.iter().position(|t| t.name == name).expect("demo material");
    let mut spots = Vec::new();
    let mut counts: std::collections::BTreeMap<&'static str, usize> = Default::default();
    let mut next = |name: &'static str| {
        let c = counts.entry(name).or_insert(0);
        *c += 1;
        format!("{name}_{c}")
    };
    let glass = |label: String, name: &'static str| Spot { label, truth: None, glass: name, f206: 0.0, pb_loss: 0.0, inclusion: false };
    let rm = |label: String, t: usize| Spot { label, truth: Some(t), glass: "", f206: 0.0, pb_loss: 0.0, inclusion: false };
    let (primary, treb, mba, mdi, a, b) = (idx("44069"), idx("Trebilcock"), idx("MBA"), idx("MDI"), idx("Sample A"), idx("Sample B"));
    for block in 0..6 {
        match part {
            DemoPart::Combined => {
                for _ in 0..2 {
                    spots.push(glass(next(NIST610_GLASS), NIST610_GLASS));
                }
                for _ in 0..4 {
                    spots.push(rm(next("44069"), primary));
                }
                for _ in 0..2 {
                    spots.push(rm(next("Trebilcock"), treb));
                }
            }
            DemoPart::UPb => {
                for _ in 0..4 {
                    spots.push(rm(next("44069"), primary));
                }
                for _ in 0..2 {
                    spots.push(rm(next("MBA"), mba));
                }
                for _ in 0..2 {
                    spots.push(rm(next("MDI"), mdi));
                }
            }
            DemoPart::TraceElements => {
                for _ in 0..2 {
                    spots.push(glass(next(NIST610_GLASS), NIST610_GLASS));
                }
                spots.push(glass(next(NIST612_GLASS), NIST612_GLASS));
                spots.push(rm(next("44069"), primary));
            }
        }
        for k in 0..4 {
            let mut s = rm(next("Sample A"), a);
            // Two spots with common Pb, one with Pb loss.
            if block == 2 && k == 1 {
                s.f206 = 0.06;
            }
            if block == 4 && k == 2 {
                s.pb_loss = 0.25;
            }
            spots.push(s);
        }
        for k in 0..3 {
            let mut s = rm(next("Sample B"), b);
            if block == 3 && k == 0 {
                s.f206 = 0.1;
            }
            // one spot hits a xenotime inclusion (the inclusion-correction example)
            if block == 1 && k == 1 && part != DemoPart::UPb {
                s.inclusion = true;
            }
            spots.push(s);
        }
    }
    match part {
        DemoPart::TraceElements => {
            spots.push(glass(next(NIST610_GLASS), NIST610_GLASS));
            spots.push(glass(next(NIST612_GLASS), NIST612_GLASS));
        }
        _ => {
            for _ in 0..3 {
                spots.push(rm(next("44069"), primary));
            }
            if part == DemoPart::Combined {
                spots.push(glass(next(NIST610_GLASS), NIST610_GLASS));
            }
        }
    }
    spots
}

/// Generate the demo session (U–Pb and trace elements in one): groups and
/// selections made by the same code that handles real laser logs.
pub fn demo_session(seed: u64) -> Session {
    demo_session_part(seed, DemoPart::Combined)
}

/// One of the demo sessions: both methods in one, U–Pb only (primary 44069,
/// secondaries MBA and MDI), or trace elements only (primary NIST610,
/// secondary NIST612), with the same sample spots.
pub fn demo_session_part(seed: u64, part: DemoPart) -> Session {
    let d = DecayConstants::default();
    let mut rms = refmat::builtin(&d);
    // the demo's reference values are the known truth of the simulation
    let trusted: &[&str] = match part {
        DemoPart::UPb => &["44069", "MBA", "MDI"],
        DemoPart::TraceElements => &[NIST610_GLASS, NIST612_GLASS],
        DemoPart::Combined => &[],
    };
    for m in rms.iter_mut() {
        if trusted.contains(&m.name.as_str()) {
            m.confirmed = true;
        }
    }
    let inputs = synth_inputs(seed, &d, &rms, part);
    let name = match part {
        DemoPart::Combined => "Demo session (simulated data)",
        DemoPart::UPb => "Demo U–Pb session (simulated data)",
        DemoPart::TraceElements => "Demo trace-element session (simulated data)",
    };
    let mut session = Session::new(name, SessionSource::Synthetic { seed, part }, inputs, rms);
    session.clock_offset_s = 0.0;
    session.clock_note = "Simulated data: both clocks agree.".to_string();
    session.metadata = Metadata {
        laboratory: "Simulated data, not from a laboratory".to_string(),
        operator: String::new(),
        instrument: "Simulated single-collector ICP-MS".to_string(),
        laser: "Simulated 193 nm excimer".to_string(),
        carrier_gas: String::new(),
        notes: String::new(),
    };
    from_log(&mut session, &AutoOptions::default());
    for g in session.groups.iter_mut() {
        if g.kind == GroupKind::Baseline {
            continue;
        }
        if let Some(t) = truths().iter().find(|t| t.name == g.name) {
            g.is_ppm = Some(t.ce);
        }
    }
    let id_of = |n: &str, s: &Session| s.groups.iter().find(|g| g.name == n).map(|g| g.id);
    session.settings.upb.primary_group = id_of("44069", &session);
    session.settings.te.primary_group = id_of(NIST610_GLASS, &session);
    session.settings.te.is_channel = Some("Ce140".to_string());
    // the demo shows the inclusion correction at work (Sample B, one spot)
    session.settings.te.inclusions = InclusionMode::Both;
    session.settings.upb.beam = BeamMethod::LaserLog;
    session.apply_defaults();
    match part {
        DemoPart::UPb => session.settings.te.enabled = false,
        DemoPart::TraceElements => session.settings.upb.enabled = false,
        DemoPart::Combined => {}
    }
    session
}

fn synth_inputs(seed: u64, d: &DecayConstants, rms: &[crate::refmat::ReferenceMaterial], part: DemoPart) -> Inputs {
    let mut rng = Rng::new(seed);
    let truth = truths();
    let spots = plan_spots(part);
    // a U–Pb session records the U–Th–Pb masses only, a trace-element session no Hg and minor Pb isotopes
    let channels: Vec<usize> = CHANNELS
        .iter()
        .enumerate()
        .filter(|(_, (e, m))| match part {
            DemoPart::Combined => true,
            DemoPart::UPb => matches!(*e, "Hg" | "Pb" | "Th" | "U"),
            DemoPart::TraceElements => !matches!((*e, *m), ("Hg", _) | ("Pb", 204) | ("Pb", 206) | ("Pb", 207)),
        })
        .map(|(j, _)| j)
        .collect();
    let per_channel_time = DWELL_S;
    let n_sweeps_per_spot = 170usize;
    let mut time = Vec::with_capacity(spots.len() * n_sweeps_per_spot);
    let mut columns: Vec<Vec<f64>> = vec![Vec::with_capacity(spots.len() * n_sweeps_per_spot); channels.len()];
    let mut laser = Vec::new();
    let pre_on = 18.0;
    let burn = 22.0;
    let mut clock = 0.0;
    let session_len = spots.len() as f64 * (n_sweeps_per_spot as f64 * SWEEP_S + 8.0);
    for spot in &spots {
        let spot_start = clock;
        let on = spot_start + pre_on;
        let off = on + burn;
        laser.push(LaserSpot {
            label: spot.label.clone(),
            on: EPOCH + on,
            off: EPOCH + off,
            x: 1000.0 + rng.unit() * 8000.0,
            y: 1000.0 + rng.unit() * 8000.0,
            spot_size: 15.0,
            rep_rate: 5.0,
            fluence: 2.5,
        });
        let beam_scale = 0.75 + 0.5 * rng.unit();
        // Each analysis scatters a little more than counting statistics allow.
        let excess_68 = 1.0 + 0.003 * rng.normal();
        for k in 0..n_sweeps_per_spot {
            let t = spot_start + k as f64 * SWEEP_S;
            let tau = t - on;
            let phase = t / session_len;
            let sens = 1.0 + 0.18 * (phase * 4.0).sin() - 0.12 * phase;
            let frac_68 = 0.82 * (1.0 + 0.05 * (phase * 3.0 + 0.5).sin());
            let frac_76 = 0.96 * (1.0 + 0.004 * (phase * 2.0).cos());
            let frac_82 = 1.08 * (1.0 + 0.03 * (phase * 2.5).sin());
            let on_beam = t >= on && t < off;
            let beam = if on_beam {
                beam_scale * (-tau / 45.0).exp() * (1.0 - (-tau / 0.6).exp())
            } else if t >= off {
                beam_scale * 0.5 * (-(t - off) / 1.2).exp()
            } else {
                0.0
            };
            let pure = |cps: f64, rng: &mut Rng| -> f64 {
                let counts = rng.poisson(cps * per_channel_time);
                counts / per_channel_time
            };
            // Radiogenic, common and interfering signals in cps.
            let (u_cps, th_cps, mut c206, mut c207, mut c208, mut c204);
            match spot.truth {
                None => {
                    let glass = refmat::find(rms, spot.glass).expect("built-in glass");
                    let g = |e: &str| glass.get(e).map_or(0.0, |v| v.value);
                    u_cps = g("U") * sensitivity(238) * sens * beam;
                    th_cps = g("Th") * sensitivity(232) * sens * beam;
                    // The glass carries ordinary lead, not radiogenic lead.
                    let pb = g("Pb") * sensitivity(206) * sens * beam;
                    c204 = pb * 0.014;
                    c206 = pb * 0.241;
                    c207 = pb * 0.221;
                    c208 = pb * 0.524;
                }
                Some(i) => {
                    let m = &truth[i];
                    let age = m.age_ma;
                    u_cps = m.u * sensitivity(238) * sens * beam;
                    th_cps = m.th * sensitivity(232) * sens * beam;
                    let dhf = true_dhf(tau.max(0.0));
                    let dhf_th = 1.0 + 0.5 * (true_dhf(tau.max(0.0)) - 1.0);
                    let keep = 1.0 - spot.pb_loss;
                    // a reference material gives its accepted ratios (also when they are
                    // not concordant), a sample those of its age
                    let acc = |k: &str| refmat::find(rms, m.name).and_then(|r| r.get(k)).map(|v| v.value);
                    let r68 = acc(crate::refmat::K_R68).unwrap_or(d.r68(age)) * keep * excess_68;
                    let r76 = acc(crate::refmat::K_R76).unwrap_or(d.r76(age));
                    let star206 = u_cps * r68 * frac_68 * dhf;
                    let star207 = star206 * r76 * frac_76;
                    let star208 = th_cps * acc(crate::refmat::K_R82).unwrap_or(d.r82(age)) * keep * frac_82 * dhf_th;
                    let f = spot.f206 + m.f206;
                    let common206 = star206 * f / (1.0 - f).max(0.01);
                    let (a, b, c) = stacey_kramers(age, d);
                    let pb204 = common206 / a;
                    c204 = pb204;
                    c206 = star206 + common206;
                    c207 = star207 + pb204 * b;
                    c208 = star208 + pb204 * c;
                }
            }
            let hg = 4000.0 * (1.0 + 0.04 * (phase * 7.0).sin());
            c204 += hg * 0.2301;
            c206 += 12.0;
            c207 += 10.0;
            c208 += 25.0;
            for (col, &j) in channels.iter().enumerate() {
                let (element, mass) = &CHANNELS[j];
                let cps = match (*element, *mass) {
                    ("Hg", _) => hg,
                    ("Pb", 204) => c204,
                    ("Pb", 206) => c206,
                    ("Pb", 207) => c207,
                    ("Pb", 208) => c208,
                    ("Th", _) => th_cps + 8.0,
                    ("U", _) => u_cps + 4.0,
                    (e, m) => {
                        let content = match spot.truth {
                            None => refmat::find(rms, spot.glass).and_then(|g| g.get(e)).map_or(0.0, |v| v.value),
                            Some(i) => monazite_content(e, &truth[i]),
                        };
                        let abundance = abundance_scale(e);
                        let inclusion = if spot.inclusion && tau >= INCLUSION_WINDOW.0 && tau < INCLUSION_WINDOW.1 && INCLUSION_ELEMENTS.contains(&e) { 6.0 } else { 1.0 };
                        content * inclusion * abundance * sensitivity(m) * sens * beam + background(e)
                    }
                };
                columns[col].push(pure(cps, &mut rng));
            }
            time.push(t);
        }
        clock = spot_start + n_sweeps_per_spot as f64 * SWEEP_S + 8.0;
    }
    let channels = channels
        .iter()
        .map(|&j| CHANNELS[j])
        .zip(columns)
        .map(|((e, m), values)| {
            let name = format!("{e}{m}");
            InputChannel {
                isotope: parse_isotope(&name),
                name,
                values,
            }
        })
        .collect();
    Inputs {
        epoch_unix: EPOCH,
        time,
        channels,
        laser,
        warnings: Vec::new(),
    }
}

fn background(element: &str) -> f64 {
    match element {
        "P" => 900.0,
        "Ca" => 2500.0,
        _ => 6.0,
    }
}

/// Keeps very abundant elements in a realistic count range.
fn abundance_scale(element: &str) -> f64 {
    match element {
        "P" => 0.02,
        "Ca" => 0.01,
        "Ce" | "La" | "Nd" => 0.05,
        "Pr" | "Sm" => 0.1,
        _ => 0.3,
    }
}

/// True content in ppm of an element in a named material, for tests.
pub fn true_ppm(material: &str, element: &str) -> Option<f64> {
    let t = truths();
    let m = t.iter().find(|m| m.name == material)?;
    match element {
        "U" => Some(m.u),
        "Th" => Some(m.th),
        e => {
            let v = monazite_content(e, m);
            (v > 0.0).then_some(v)
        }
    }
}

/// Elements whose sensitivity is scaled the same in glass and monazite, and
/// so can be compared with truth directly.
pub fn test_elements() -> [&'static str; 8] {
    ["La", "Nd", "Sm", "Gd", "Dy", "Yb", "Lu", "Y"]
}
