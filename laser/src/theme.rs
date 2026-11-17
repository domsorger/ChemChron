// SPDX-License-Identifier: GPL-3.0-or-later
//! Colours and small shared widgets.

use egui::{Color32, CornerRadius, FontId, Frame, Margin, RichText, Stroke, Ui, Vec2};

#[derive(Clone, Copy)]
pub struct Theme {
    pub dark: bool,
    pub bg: Color32,
    pub panel: Color32,
    pub raised: Color32,
    pub border: Color32,
    pub text: Color32,
    pub dim: Color32,
    pub accent: Color32,
    pub on_accent: Color32,
    pub ok: Color32,
    pub warn: Color32,
    pub alarm: Color32,
    pub grid: Color32,
    pub curve: Color32,
}

impl Theme {
    pub fn get(dark: bool) -> Theme {
        if dark {
            Theme {
                dark,
                bg: Color32::from_rgb(19, 22, 27),
                panel: Color32::from_rgb(27, 31, 38),
                raised: Color32::from_rgb(37, 42, 51),
                border: Color32::from_rgb(52, 58, 69),
                text: Color32::from_rgb(222, 226, 232),
                dim: Color32::from_rgb(150, 158, 171),
                accent: Color32::from_rgb(79, 195, 176),
                on_accent: Color32::from_rgb(10, 30, 27),
                ok: Color32::from_rgb(108, 195, 138),
                warn: Color32::from_rgb(227, 169, 72),
                alarm: Color32::from_rgb(235, 104, 110),
                grid: Color32::from_rgb(38, 43, 52),
                curve: Color32::from_rgb(240, 120, 110),
            }
        } else {
            Theme {
                dark,
                bg: Color32::from_rgb(244, 244, 240),
                panel: Color32::from_rgb(255, 255, 253),
                raised: Color32::from_rgb(234, 234, 228),
                border: Color32::from_rgb(208, 208, 200),
                text: Color32::from_rgb(30, 34, 41),
                dim: Color32::from_rgb(92, 100, 114),
                accent: Color32::from_rgb(10, 122, 108),
                on_accent: Color32::WHITE,
                ok: Color32::from_rgb(38, 128, 78),
                warn: Color32::from_rgb(166, 106, 8),
                alarm: Color32::from_rgb(190, 50, 58),
                grid: Color32::from_rgb(230, 230, 224),
                curve: Color32::from_rgb(190, 50, 58),
            }
        }
    }

    pub fn apply(&self, ctx: &egui::Context) {
        let mut v = if self.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        v.panel_fill = self.bg;
        v.window_fill = self.panel;
        v.extreme_bg_color = if self.dark { Color32::from_rgb(15, 17, 21) } else { Color32::WHITE };
        v.faint_bg_color = self.raised;
        v.override_text_color = Some(self.text);
        v.hyperlink_color = self.accent;
        v.selection.bg_fill = self.accent.linear_multiply(if self.dark { 0.35 } else { 0.25 });
        v.selection.stroke = Stroke::new(1.0, self.accent);
        v.window_stroke = Stroke::new(1.0, self.border);
        let radius = CornerRadius::same(4);
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = radius;
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.border);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, self.text);
        v.widgets.inactive.bg_fill = self.raised;
        v.widgets.inactive.weak_bg_fill = self.raised;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, self.border);
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, self.text);
        v.widgets.hovered.bg_fill = self.raised;
        v.widgets.hovered.weak_bg_fill = self.raised;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, self.accent);
        v.widgets.hovered.fg_stroke = Stroke::new(1.0, self.text);
        v.widgets.active.fg_stroke = Stroke::new(1.0, self.text);
        v.widgets.open.fg_stroke = Stroke::new(1.0, self.text);
        ctx.set_visuals(v);
        ctx.global_style_mut(|s| {
            s.spacing.item_spacing = Vec2::new(8.0, 6.0);
            s.spacing.button_padding = Vec2::new(9.0, 4.0);
            s.spacing.interact_size.y = 24.0;
            s.text_styles.insert(egui::TextStyle::Body, FontId::proportional(13.5));
            s.text_styles.insert(egui::TextStyle::Button, FontId::proportional(13.5));
            s.text_styles.insert(egui::TextStyle::Small, FontId::proportional(11.5));
            s.text_styles.insert(egui::TextStyle::Heading, FontId::proportional(19.0));
            s.text_styles.insert(egui::TextStyle::Monospace, FontId::monospace(12.5));
        });
    }

    pub fn card(&self) -> Frame {
        Frame::NONE
            .fill(self.panel)
            .stroke(Stroke::new(1.0, self.border))
            .corner_radius(6)
            .inner_margin(Margin::same(12))
    }
}

/// Colours for channels on the signal plot.
pub const SERIES: [[u8; 3]; 14] = [
    [86, 180, 233],
    [240, 150, 60],
    [100, 200, 130],
    [225, 100, 120],
    [170, 130, 225],
    [230, 200, 80],
    [90, 200, 200],
    [235, 140, 200],
    [160, 180, 90],
    [130, 150, 240],
    [210, 120, 80],
    [120, 220, 180],
    [200, 170, 140],
    [170, 190, 210],
];

pub fn series_color(index: usize) -> Color32 {
    let c = SERIES[index % SERIES.len()];
    Color32::from_rgb(c[0], c[1], c[2])
}

pub fn group_color(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// A titled block on a page; click its title to fold it away (remembered
/// until Settings > Reset window layout).
pub fn section(ui: &mut Ui, t: &Theme, title: &str, add: impl FnOnce(&mut Ui)) {
    t.card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::CollapsingHeader::new(RichText::new(title).strong().size(13.5)).id_salt(("section", title, LAYOUT_GEN.load(std::sync::atomic::Ordering::Relaxed))).default_open(true).show(ui, |ui| {
            ui.add_space(2.0);
            add(ui);
        });
    });
    ui.add_space(8.0);
}

/// One row of a settings form: label on the left, control on the right.
pub fn form_row(ui: &mut Ui, t: &Theme, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [118.0, 22.0],
            egui::Label::new(RichText::new(label).color(t.dim)).wrap_mode(egui::TextWrapMode::Truncate),
        );
        add(ui);
    });
}

/// A row of mutually exclusive choices.
pub fn segmented<V: PartialEq + Copy>(ui: &mut Ui, t: &Theme, value: &mut V, options: &[(V, &str)]) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (v, label) in options {
            let on = *value == *v;
            let text = if on {
                RichText::new(*label).color(t.on_accent).strong()
            } else {
                RichText::new(*label)
            };
            let button = egui::Button::new(text).fill(if on { t.accent } else { t.raised });
            if ui.add(button).clicked() && !on {
                *value = *v;
                changed = true;
            }
        }
    });
    changed
}

/// A small coloured label.
pub fn tag(ui: &mut Ui, text: &str, color: Color32) {
    Frame::NONE
        .fill(color.linear_multiply(0.16))
        .stroke(Stroke::new(1.0, color.linear_multiply(0.6)))
        .corner_radius(3)
        .inner_margin(Margin::symmetric(5, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(text).color(color).size(11.5));
        });
}

/// A headline number with a caption.
pub fn stat(ui: &mut Ui, t: &Theme, caption: &str, value: &str, color: Color32, width: f32) {
    stat_r(ui, t, caption, value, color, width);
}

/// The same, returning the card (for hover text).
pub fn stat_r(ui: &mut Ui, t: &Theme, caption: &str, value: &str, color: Color32, width: f32) -> egui::Response {
    t.card()
        .show(ui, |ui| {
            ui.set_width(width);
            ui.label(RichText::new(caption).color(t.dim).size(11.5));
            ui.label(RichText::new(value).color(color).size(18.0).strong());
        })
        .response
}

pub fn fmt_sig(v: f64, digits: usize) -> String {
    if !v.is_finite() {
        return "–".to_string();
    }
    if v == 0.0 {
        return "0".to_string();
    }
    let mag = v.abs().log10().floor() as i32;
    let decimals = (digits as i32 - 1 - mag).max(0) as usize;
    if mag >= 6 || mag <= -5 {
        return format!("{:.*e}", digits.saturating_sub(1), v);
    }
    format!("{v:.decimals$}")
}

/// A value with its uncertainty, both shown to the digits the uncertainty supports.
pub fn fmt_pm(v: f64, u: f64) -> String {
    if !v.is_finite() {
        return "–".to_string();
    }
    if !u.is_finite() || u <= 0.0 {
        return format!("{} ± –", fmt_sig(v, 5));
    }
    let decimals = ((1.0 - u.log10().floor()) as i32).clamp(0, 7) as usize;
    format!("{v:.decimals$} ± {u:.decimals$}")
}

/// Counts layout resets: part of the ids of resizable columns and folding
/// sections, so "Reset window layout" always starts them afresh.
pub static LAYOUT_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Id of a resizable column for this layout generation.
pub fn layout_id(name: &str) -> String {
    format!("{name}#{}", LAYOUT_GEN.load(std::sync::atomic::Ordering::Relaxed))
}

/// Starts every column width and folded section afresh.
pub fn reset_layout(ctx: &egui::Context) {
    LAYOUT_GEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    ctx.memory_mut(|m| {
        m.data.clear();
        m.reset_areas();
    });
}
