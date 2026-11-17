// SPDX-License-Identifier: GPL-3.0-or-later
//! The session: immutable input channels plus everything a person decides
//! (groups, selections, settings). The decisions are saved as readable JSON and
//! raw files are referenced by path and hash, never copied or changed.

use crate::geochron::DecayConstants;
use crate::interp::InterpKind;
use crate::refmat::ReferenceMaterial;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Isotope {
    pub element: String,
    pub mass: u32,
}

/// Read "Pb206", "206Pb", "U238_CPS" or "Pb206(LR)" as an isotope.
pub fn parse_isotope(name: &str) -> Option<Isotope> {
    let head: String = name
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    let letters: String = head.chars().filter(|c| c.is_ascii_alphabetic()).collect();
    let digits: String = head.chars().filter(|c| c.is_ascii_digit()).collect();
    if letters.is_empty() || letters.len() > 2 || digits.is_empty() {
        return None;
    }
    // Digits must sit on one side of the letters only.
    let starts_digit = head.chars().next()?.is_ascii_digit();
    let tail_digit = head.chars().last()?.is_ascii_digit();
    if starts_digit == tail_digit {
        return None;
    }
    let mut element = String::new();
    for (i, c) in letters.chars().enumerate() {
        if i == 0 {
            element.push(c.to_ascii_uppercase());
        } else {
            element.push(c.to_ascii_lowercase());
        }
    }
    Some(Isotope {
        element,
        mass: digits.parse().ok()?,
    })
}

#[derive(Clone, Debug)]
pub struct InputChannel {
    pub name: String,
    pub isotope: Option<Isotope>,
    /// Counts per second, one per sweep. Missing sweeps are NaN.
    pub values: Vec<f64>,
}

impl InputChannel {
    /// True when the channel holds no signal at all, e.g. a mass set up but not measured.
    pub fn is_empty_signal(&self) -> bool {
        !self.values.iter().any(|v| v.is_finite() && *v > 0.0)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceFile {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
    pub label: String,
    pub format: String,
    /// Clock time of the file's first sweep as written in the file, when known.
    pub stamp: Option<String>,
    #[serde(default)]
    pub first_sweep: usize,
    #[serde(default)]
    pub n_sweeps: usize,
}

/// One laser firing from the log, in the laser's own clock (seconds since 1970).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LaserSpot {
    pub label: String,
    pub on: f64,
    pub off: f64,
    pub x: f64,
    pub y: f64,
    pub spot_size: f64,
    pub rep_rate: f64,
    pub fluence: f64,
}

/// Immutable sweeps of a session. Shared by reference: nothing can alter them.
#[derive(Clone, Debug, Default)]
pub struct Inputs {
    /// Seconds since 1970 of `time == 0`, when the files state a clock time.
    pub epoch_unix: f64,
    /// Index time: seconds since the first sweep, rising.
    pub time: Vec<f64>,
    pub channels: Vec<InputChannel>,
    pub laser: Vec<LaserSpot>,
    pub warnings: Vec<String>,
}

impl Inputs {
    pub fn channel_index(&self, name: &str) -> Option<usize> {
        self.channels.iter().position(|c| c.name == name)
    }
    pub fn find_isotope(&self, element: &str, mass: u32) -> Option<usize> {
        self.channels.iter().position(|c| {
            c.isotope
                .as_ref()
                .is_some_and(|i| i.element == element && i.mass == mass)
        })
    }
    /// Sweep range covering [start, end).
    pub fn range(&self, start: f64, end: f64) -> std::ops::Range<usize> {
        let a = self.time.partition_point(|t| *t < start);
        let b = self.time.partition_point(|t| *t < end);
        a..b.max(a)
    }
    /// Median time between sweeps.
    pub fn sweep_seconds(&self) -> f64 {
        if self.time.len() < 2 {
            return 0.0;
        }
        let diffs: Vec<f64> = self.time.windows(2).map(|w| w[1] - w[0]).collect();
        crate::stats::median(&diffs)
    }
    /// Runs of consecutive sweeps, split where acquisition paused.
    pub fn segments(&self) -> Vec<std::ops::Range<usize>> {
        if self.time.is_empty() {
            return Vec::new();
        }
        let gap = (self.sweep_seconds() * 4.0).max(1e-6);
        let mut out = Vec::new();
        let mut start = 0;
        for i in 1..self.time.len() {
            if self.time[i] - self.time[i - 1] > gap {
                out.push(start..i);
                start = i;
            }
        }
        out.push(start..self.time.len());
        out
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GroupKind {
    Baseline,
    ReferenceMaterial,
    Sample,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Group {
    pub id: u64,
    pub name: String,
    pub kind: GroupKind,
    /// Name in the reference-material list.
    pub rm: Option<String>,
    /// Interpolation through time when this group serves as baseline or primary reference.
    pub interp: InterpKind,
    /// Internal-standard content in ppm for this group's selections.
    pub is_ppm: Option<f64>,
    pub color: [u8; 3],
    /// 230Th correction: Th/U of the melt or fluid for this group.
    pub th_u_melt: Option<f64>,
    /// Mineral of this group's analyses (None: the reference material's
    /// matrix, otherwise monazite). Sent with the results.
    #[serde(default)]
    pub mineral: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SelectionSource {
    LaserLog,
    Signal,
    Manual,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Selection {
    pub id: u64,
    pub group: u64,
    pub label: String,
    pub start: f64,
    pub end: f64,
    pub comment: String,
    /// Left out of all calculations, still listed and exported as left out.
    pub excluded: bool,
    pub source: SelectionSource,
    /// Index time of the laser firing that belongs to this selection.
    pub laser_on: Option<f64>,
    /// Internal-standard content in ppm for this selection, overriding its group.
    pub is_ppm: Option<f64>,
    /// Parts of the signal left out for some elements only (inclusions, by hand).
    #[serde(default)]
    pub cutouts: Vec<Cutout>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub spot_size: Option<f64>,
}

/// A time range of one selection left out for the named channels only
/// (e.g. a Zr-rich inclusion in garnet: Zr and Hf cut, the rest kept).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cutout {
    pub start: f64,
    pub end: f64,
    /// Channel names (e.g. "Zr90").
    pub channels: Vec<String>,
}

/// How inclusions are taken out of element contents.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum InclusionMode {
    /// Only the cut-outs drawn by hand.
    #[default]
    ByHand,
    /// Only the automatic spike removal.
    Automatic,
    /// Both.
    Both,
}

impl InclusionMode {
    pub fn by_hand(&self) -> bool {
        matches!(self, InclusionMode::ByHand | InclusionMode::Both)
    }
    pub fn automatic(&self) -> bool {
        matches!(self, InclusionMode::Automatic | InclusionMode::Both)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LodMethod {
    Longerich,
    Howell,
    Pettke,
}

impl LodMethod {
    pub const ALL: [LodMethod; 3] = [LodMethod::Longerich, LodMethod::Howell, LodMethod::Pettke];
    pub fn label(&self) -> &'static str {
        match self {
            LodMethod::Longerich => "Longerich et al. (1996)",
            LodMethod::Howell => "Howell et al. (2013)",
            LodMethod::Pettke => "Pettke et al. (2012)",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DhfModel {
    None,
    Linear,
    Exponential,
    ExponentialLinear,
    Spline,
}

impl DhfModel {
    pub const ALL: [DhfModel; 5] = [
        DhfModel::None,
        DhfModel::Linear,
        DhfModel::Exponential,
        DhfModel::ExponentialLinear,
        DhfModel::Spline,
    ];
    pub fn label(&self) -> &'static str {
        match self {
            DhfModel::None => "No correction",
            DhfModel::Linear => "Linear",
            DhfModel::Exponential => "Exponential",
            DhfModel::ExponentialLinear => "Exponential + linear",
            DhfModel::Spline => "Smoothing spline",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BeamMethod {
    /// Beam seconds count from the laser firing in the log.
    LaserLog,
    /// Beam seconds count from the jump in the 5-point smoothed index channel.
    SignalJump,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommonPbMethod {
    None,
    Pb204,
    Pb207,
    Pb208,
}

impl CommonPbMethod {
    pub const ALL: [CommonPbMethod; 4] = [
        CommonPbMethod::None,
        CommonPbMethod::Pb204,
        CommonPbMethod::Pb207,
        CommonPbMethod::Pb208,
    ];
    pub fn label(&self) -> &'static str {
        match self {
            CommonPbMethod::None => "None",
            CommonPbMethod::Pb204 => "204Pb-based",
            CommonPbMethod::Pb207 => "207Pb-based",
            CommonPbMethod::Pb208 => "208Pb-based",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum CommonPbComposition {
    StaceyKramers,
    /// 206Pb/204Pb, 207Pb/204Pb, 208Pb/204Pb.
    User(f64, f64, f64),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TeSettings {
    pub enabled: bool,
    /// Internal-standard contents are typed as oxide wt% (else element ppm);
    /// they are kept as element ppm.
    #[serde(default = "yes")]
    pub is_as_oxide: bool,
    /// The oxide typed (empty: the usual oxide of the internal-standard element).
    #[serde(default)]
    pub is_oxide: String,
    /// Inclusions: cut-outs by hand, automatic spike removal, or both.
    #[serde(default)]
    pub inclusions: InclusionMode,
    /// Automatic removal: a sweep whose element / internal-standard ratio lies
    /// more than this many robust SD above the spot's median is left out for that element.
    #[serde(default = "inclusion_k_default")]
    pub inclusion_k: f64,
    /// Channels the automatic removal leaves alone.
    #[serde(default)]
    pub inclusion_skip: Vec<String>,
    pub primary_group: Option<u64>,
    /// Channel used as internal standard.
    pub is_channel: Option<String>,
    /// Off gives semi-quantitative results without an internal standard.
    pub use_internal_standard: bool,
    /// Per-channel calibration reference group, overriding the primary group.
    pub channel_group: BTreeMap<String, u64>,
    pub lod: LodMethod,
    /// Dwell time of one channel in seconds for the Pettke limit; estimated
    /// as sweep time / channel count when not stated.
    pub dwell_s: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpbSettings {
    pub enabled: bool,
    pub primary_group: Option<u64>,
    pub index_channel: Option<String>,
    pub beam: BeamMethod,
    pub dhf_r68: DhfModel,
    pub dhf_r75: DhfModel,
    pub dhf_r82: DhfModel,
    /// Beam seconds left out at the start of the down-hole fit.
    pub dhf_trim_s: f64,
    pub decay: DecayConstants,
    pub common_method: CommonPbMethod,
    pub common_composition: CommonPbComposition,
    /// Subtract 204Hg from mass 204 using the 202Hg channel.
    pub hg_correction: bool,
    /// 204Hg/202Hg used for that subtraction.
    pub hg_204_202: f64,
    pub th230_enabled: bool,
    /// How the session's excess uncertainty is set (from the lab settings).
    #[serde(default)]
    pub excess_rule: ExcessRule,
}

/// The excess uncertainty of the U–Pb ratios: from the session's reference
/// materials (primary and secondaries; a material that does not reproduce a
/// ratio is left out for it), or manual values. Set by the lab settings
/// (ChemChron: Settings > Uncertainty models > U–Pb).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExcessRule {
    /// Manual excess, 2σ % per ratio (206/238, 207/235, 207/206, 208/232); None: from the reference materials.
    pub manual_2s_pct: Option<[f64; 4]>,
    /// Reference materials (laser names) left out entirely, or one ratio of them: "name|r76".
    pub excluded: Vec<String>,
}

impl ExcessRule {
    pub fn excludes(&self, material: &str, ratio_key: &str) -> bool {
        self.excluded.iter().any(|x| x == material || *x == format!("{material}|{ratio_key}"))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QaSettings {
    /// Difference from the accepted value, in percent, that raises a warning.
    pub warn_pct: f64,
    /// Difference that raises an alarm.
    pub alarm_pct: f64,
    /// Discordance in percent above which an analysis is flagged.
    pub flag_discordance_pct: f64,
    /// Common 206Pb fraction in percent above which an analysis is flagged.
    pub flag_common_pct: f64,
    /// Share of outlier sweeps in percent above which an analysis is flagged.
    pub flag_outlier_pct: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    /// Outlier rule width in robust standard deviations; 0 turns rejection off.
    pub outlier_k: f64,
    pub te: TeSettings,
    pub upb: UpbSettings,
    pub qa: QaSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            outlier_k: 3.0,
            te: TeSettings {
                is_as_oxide: true,
                is_oxide: String::new(),
                inclusions: InclusionMode::ByHand,
                inclusion_k: inclusion_k_default(),
                inclusion_skip: Vec::new(),
                enabled: true,
                primary_group: None,
                is_channel: None,
                use_internal_standard: true,
                channel_group: BTreeMap::new(),
                lod: LodMethod::Pettke,
                dwell_s: None,
            },
            upb: UpbSettings {
                enabled: true,
                primary_group: None,
                index_channel: None,
                beam: BeamMethod::SignalJump,
                dhf_r68: DhfModel::Exponential,
                dhf_r75: DhfModel::Exponential,
                dhf_r82: DhfModel::Exponential,
                dhf_trim_s: 1.0,
                decay: DecayConstants::default(),
                common_method: CommonPbMethod::None,
                common_composition: CommonPbComposition::StaceyKramers,
                hg_correction: true,
                hg_204_202: 0.2301,
                th230_enabled: false,
                excess_rule: ExcessRule::default(),
            },
            qa: QaSettings {
                warn_pct: 1.0,
                alarm_pct: 3.0,
                flag_discordance_pct: 10.0,
                flag_common_pct: 1.0,
                flag_outlier_pct: 15.0,
            },
        }
    }
}

/// Facts about the analytical set-up for the publication table.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Metadata {
    pub laboratory: String,
    pub operator: String,
    pub instrument: String,
    pub laser: String,
    pub carrier_gas: String,
    pub notes: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SessionSource {
    /// The built-in demo: regenerated from this seed, with a known truth.
    Synthetic {
        seed: u64,
        #[serde(default)]
        part: DemoPart,
    },
    Files { paths: Vec<String>, laser_log: Option<String> },
}

/// Which simulated demo session: both methods in one, U–Pb only, or trace elements only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DemoPart {
    #[default]
    Combined,
    UPb,
    TraceElements,
}

/// Everything saved in a session file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub format: String,
    pub software_version: String,
    pub name: String,
    pub source: SessionSource,
    pub files: Vec<SourceFile>,
    pub laser_log_file: Option<SourceFile>,
    /// Seconds added to laser-log clock times to reach mass-spectrometer clock times.
    pub clock_offset_s: f64,
    /// How the offset was chosen, for the record.
    pub clock_note: String,
    pub groups: Vec<Group>,
    pub selections: Vec<Selection>,
    pub settings: Settings,
    pub rms: Vec<ReferenceMaterial>,
    pub metadata: Metadata,
    #[serde(skip)]
    pub inputs: Arc<Inputs>,
}

pub const SESSION_FORMAT: &str = "chemchron-laser-session-1";
/// An older format name of laser session files, still read.
pub const OLD_SESSION_FORMAT: &str = "tracechron-session-1";

/// Colour of new baseline groups: light enough to see on the dark and the light theme.
pub const BASELINE_COLOR: [u8; 3] = [178, 188, 204];
/// A former default baseline colour (too dark on the dark theme), replaced on loading.
pub const OLD_BASELINE_COLOR: [u8; 3] = [130, 135, 145];

pub const GROUP_COLORS: [[u8; 3]; 10] = [
    [86, 180, 233],
    [240, 150, 60],
    [100, 200, 130],
    [225, 100, 120],
    [170, 130, 225],
    [230, 200, 80],
    [90, 200, 200],
    [235, 140, 200],
    [160, 170, 90],
    [150, 160, 180],
];

impl Session {
    pub fn new(name: &str, source: SessionSource, inputs: Inputs, rms: Vec<ReferenceMaterial>) -> Session {
        Session {
            format: SESSION_FORMAT.to_string(),
            software_version: crate::VERSION.to_string(),
            name: name.to_string(),
            source,
            files: Vec::new(),
            laser_log_file: None,
            clock_offset_s: 0.0,
            clock_note: String::new(),
            groups: Vec::new(),
            selections: Vec::new(),
            settings: Settings::default(),
            rms,
            metadata: Metadata::default(),
            inputs: Arc::new(inputs),
        }
    }

    /// Mineral of a group: chosen, else its reference material's matrix when
    /// that is a mineral, else monazite.
    pub fn mineral_of(&self, group: u64) -> String {
        let Some(g) = self.group(group) else { return "Monazite".into() };
        if let Some(m) = g.mineral.as_ref().filter(|m| !m.is_empty()) {
            return m.clone();
        }
        match self.rm_of(group).map(|m| m.matrix.clone()) {
            Some(m) if !m.is_empty() && !m.eq_ignore_ascii_case("glass") => m,
            _ => "Monazite".into(),
        }
    }

    pub fn next_id(&self) -> u64 {
        let a = self.groups.iter().map(|g| g.id).max().unwrap_or(0);
        let b = self.selections.iter().map(|s| s.id).max().unwrap_or(0);
        a.max(b) + 1
    }

    pub fn group(&self, id: u64) -> Option<&Group> {
        self.groups.iter().find(|g| g.id == id)
    }

    pub fn group_of(&self, s: &Selection) -> Option<&Group> {
        self.group(s.group)
    }

    /// Find or create a group by name.
    pub fn ensure_group(&mut self, name: &str, kind: GroupKind) -> u64 {
        if let Some(g) = self.groups.iter().find(|g| g.name == name) {
            return g.id;
        }
        let id = self.next_id();
        let rm = if kind == GroupKind::Baseline {
            None
        } else {
            self.rms.iter().find(|m| m.matches_label(name)).map(|m| m.name.clone())
        };
        let kind = if rm.is_some() { GroupKind::ReferenceMaterial } else { kind };
        let color_index = self.groups.iter().filter(|g| g.kind != GroupKind::Baseline).count();
        let color = if kind == GroupKind::Baseline {
            BASELINE_COLOR
        } else {
            GROUP_COLORS[color_index % GROUP_COLORS.len()]
        };
        let interp = if kind == GroupKind::Baseline {
            InterpKind::StepForward
        } else {
            InterpKind::SmoothAuto
        };
        self.groups.push(Group {
            id,
            name: name.to_string(),
            kind,
            rm,
            interp,
            is_ppm: None,
            color,
            th_u_melt: None,
            mineral: None,
        });
        id
    }

    pub fn sort_selections(&mut self) {
        self.selections.sort_by(|a, b| a.start.total_cmp(&b.start));
    }

    /// Reference material of a group.
    pub fn rm_of(&self, group: u64) -> Option<&ReferenceMaterial> {
        let name = self.group(group)?.rm.as_ref()?;
        crate::refmat::find(&self.rms, name)
    }

    /// Internal-standard content of a selection: its own value, its group's,
    /// or the accepted value of its reference material.
    pub fn is_value(&self, s: &Selection, element: &str) -> Option<f64> {
        if let Some(v) = s.is_ppm {
            return Some(v);
        }
        let g = self.group_of(s)?;
        if let Some(v) = g.is_ppm {
            return Some(v);
        }
        self.rm_of(g.id)?.get(element).map(|v| v.value)
    }

    /// Of the candidate (element, channel) pairs, the one with the highest
    /// content in the unknowns, estimated from median count rates (background
    /// removed) against the calibration glass. None when that can't be told
    /// or no candidate reaches 0.1 wt%.
    pub fn most_abundant_in_unknowns(&self, candidates: &[(&str, usize)], glass: Option<u64>) -> Option<(f64, usize)> {
        let glass = glass?;
        let rm = self.rm_of(glass)?.clone();
        let inputs = &self.inputs;
        let level = |sel: &Selection, ch: usize| -> f64 {
            let r = inputs.range(sel.start, sel.end);
            let v = &inputs.channels[ch].values;
            let (a, b) = (r.start.min(v.len()), r.end.min(v.len()));
            if a >= b {
                return f64::NAN;
            }
            crate::stats::median(&v[a..b])
        };
        let kind_of = |s: &Selection| self.group_of(s).map(|g| g.kind);
        let mut best: Option<(f64, usize)> = None;
        for (e, ch) in candidates {
            let Some(content) = rm.get(e).map(|v| v.value) else { continue };
            let bl: Vec<f64> = self.selections.iter().filter(|s| kind_of(s) == Some(GroupKind::Baseline)).map(|s| level(s, *ch)).filter(|x| x.is_finite()).collect();
            let bg = if bl.is_empty() { 0.0 } else { crate::stats::median(&bl) };
            let gl: Vec<f64> = self.selections.iter().filter(|s| s.group == glass).map(|s| level(s, *ch) - bg).filter(|x| x.is_finite()).collect();
            let sl: Vec<f64> = self.selections.iter().filter(|s| kind_of(s) == Some(GroupKind::Sample)).map(|s| level(s, *ch) - bg).filter(|x| x.is_finite()).collect();
            if gl.is_empty() || sl.is_empty() {
                continue;
            }
            let g = crate::stats::median(&gl);
            if !(g > 0.0) {
                continue;
            }
            let estimate = crate::stats::median(&sl) / g * content;
            if estimate.is_finite() && best.is_none_or(|b| estimate > b.0) {
                best = Some((estimate, *ch));
            }
        }
        best.filter(|b| b.0 > 1000.0)
    }

    /// Fill in sensible defaults for anything not chosen yet.
    pub fn apply_defaults(&mut self) {
        let inputs = self.inputs.clone();
        let upb = &mut self.settings.upb;
        if upb.index_channel.is_none() {
            upb.index_channel = inputs
                .find_isotope("U", 238)
                .or_else(|| inputs.find_isotope("Pb", 206))
                .or(if inputs.channels.is_empty() { None } else { Some(0) })
                .map(|i| inputs.channels[i].name.clone());
        }
        let groups: Vec<(u64, Option<String>)> = self
            .groups
            .iter()
            .filter(|g| g.kind == GroupKind::ReferenceMaterial)
            .map(|g| (g.id, g.rm.clone()))
            .collect();
        let has_ratio = |rm: &Option<String>| {
            rm.as_ref()
                .and_then(|n| crate::refmat::find(&self.rms, n))
                .is_some_and(|m| m.get(crate::refmat::K_R68).is_some())
        };
        let has_element = |rm: &Option<String>| {
            rm.as_ref()
                .and_then(|n| crate::refmat::find(&self.rms, n))
                .is_some_and(|m| m.matrix == "Glass")
        };
        let count = |id: u64| self.selections.iter().filter(|s| s.group == id).count();
        if self.settings.upb.primary_group.is_none() {
            self.settings.upb.primary_group = groups
                .iter()
                .filter(|(_, rm)| has_ratio(rm))
                .max_by_key(|(id, _)| count(*id))
                .map(|(id, _)| *id);
        }
        if self.settings.te.primary_group.is_none() {
            self.settings.te.primary_group = groups
                .iter()
                .filter(|(_, rm)| has_element(rm))
                .max_by_key(|(id, _)| count(*id))
                .map(|(id, _)| *id);
        }
        if self.settings.te.is_channel.is_none() {
            // The usual internal standards, in this order when the unknowns
            // can't tell; otherwise the one most abundant in the unknowns,
            // estimated against the calibration glass (Ce in monazite, Si in
            // garnet or zircon, Ca in apatite, ...).
            let candidates: Vec<(&str, usize)> = [("Ce", 140), ("Si", 29), ("Ca", 43), ("P", 31)]
                .iter()
                .filter_map(|(e, m)| inputs.find_isotope(e, *m).map(|i| (*e, i)))
                .collect();
            let glass = self.settings.te.primary_group;
            if let Some((_, ch)) = self.most_abundant_in_unknowns(&candidates, glass) {
                self.settings.te.is_channel = Some(inputs.channels[ch].name.clone());
            } else if let Some((_, ch)) = candidates.first() {
                self.settings.te.is_channel = Some(inputs.channels[*ch].name.clone());
            }
            // No usual internal standard measured: contents can still be
            // estimated from count rates against the calibration glass.
            if self.settings.te.is_channel.is_none() {
                self.settings.te.use_internal_standard = false;
            }
        }
        if self.settings.te.primary_group.is_none() {
            self.settings.te.enabled = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isotope_names_parse_both_ways_round() {
        let want = Some(Isotope { element: "Pb".into(), mass: 206 });
        assert_eq!(parse_isotope("Pb206"), want);
        assert_eq!(parse_isotope("206Pb"), want);
        assert_eq!(parse_isotope("Pb206(LR)"), want);
        assert_eq!(parse_isotope("U238_CPS").unwrap().mass, 238);
        assert_eq!(parse_isotope("Time"), None);
        assert_eq!(parse_isotope("TotalBeam"), None);
    }
}

fn yes() -> bool {
    true
}

fn inclusion_k_default() -> f64 {
    4.0
}
