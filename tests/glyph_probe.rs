// SPDX-License-Identifier: GPL-3.0-or-later
//! Every character the window shows must be in the built-in fonts (egui
//! panics on a missing glyph in tests and shows a box on screen).

#[test]
fn all_text_characters_are_in_the_fonts() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut chars = std::collections::BTreeSet::new();
    for dir in ["src", "core/src"] {
        for e in std::fs::read_dir(root.join(dir)).unwrap().flatten() {
            if e.path().extension().is_some_and(|x| x == "rs") {
                for ch in std::fs::read_to_string(e.path()).unwrap().chars() {
                    if !ch.is_ascii() {
                        chars.insert(ch);
                    }
                }
            }
        }
    }
    // drawn by the shared plot code (log-axis decades 10⁰ … 10⁹)
    chars.extend("\u{2070}\u{b9}\u{b2}\u{b3}\u{2074}\u{2075}\u{2076}\u{2077}\u{2078}\u{2079}".chars());
    let ctx = egui::Context::default();
    let mut missing = String::new();
    let mut out = ctx.run_ui(Default::default(), |ui| {
        for ch in &chars {
            if !ui.fonts_mut(|f| f.has_glyph(&egui::FontId::proportional(13.0), *ch)) {
                missing.push(*ch);
            }
        }
    });
    out.textures_delta.clear();
    assert!(missing.is_empty(), "characters without a glyph: {missing}");
}
