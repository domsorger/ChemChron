// SPDX-License-Identifier: GPL-3.0-or-later
//! A log axis shorter than two decades gets labelled 2 and 5 lines.

use chemchron_laser::plot::{self, Frame2};

fn labels(y: (f64, f64)) -> Vec<String> {
    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(Default::default(), |ui| {
        let t = chemchron_laser::theme::Theme::get(true);
        let (_, painter, rect) = plot::allocate(ui, egui::vec2(500.0, 400.0));
        plot::axes(&painter, &t, &Frame2 { rect, x: (0.0, 1.0), y, log_y: true }, "x", "y");
    });
    out.textures_delta.clear();
    let mut v = Vec::new();
    for s in &out.shapes {
        if let egui::epaint::Shape::Text(tx) = &s.shape {
            v.push(tx.galley.text().to_string());
        }
    }
    v
}

#[test]
fn short_log_axis_has_more_labels() {
    let l = labels((1800.0, 14000.0));
    assert!(l.iter().any(|s| s == "2000") && l.iter().any(|s| s == "5000"), "{l:?}");
    let wide = labels((1.0, 1e5));
    assert!(!wide.iter().any(|s| s == "5000"), "{wide:?}");
}
