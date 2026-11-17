// SPDX-License-Identifier: GPL-3.0-or-later
//! Exports of the analysis table and Summary: CSV (blocks), Excel and
//! OpenDocument workbooks (Geochronology, Mineral composition and Summary
//! sheets), and IsoplotR input.

use crate::app::ChemApp;
use chemchron_core::columns::{self, display_name, geochronology_block, mineral_block};
use chemchron_core::row::*;
use egui::{RichText, Ui};
use std::io::Write;
use std::path::Path;
use chemchron_laser::theme::section;

pub const GROUPS: [(&str, &str); 6] = [
    ("chem", "Chemical ages and uncertainties"),
    ("upb", "U–Pb ages, ratios and checks"),
    ("ref", "Reference / sample-group checks"),
    ("comp", "Mineral composition (wt%, apfu, end-members)"),
    ("trace", "Trace elements (ppm)"),
    ("summary", "Summary sheets"),
];

fn wanted(app: &ChemApp, g: &str) -> bool {
    *app.combine.export_choice.get(g).unwrap_or(&true)
}

fn keep_col(app: &ChemApp, c: &str) -> bool {
    let g = if ["Label", "Mineral", "Is_Reference", "Flag", "Reference", "Comment", "EPMA_Session", "UPb_Session", "Include_In_Average"].contains(&c) {
        return true;
    } else if c.starts_with("UPb_Ref_") || c.starts_with("Ref_") || c.starts_with("Group_") {
        "ref"
    } else if c.starts_with("UPb_") {
        "upb"
    } else if chemchron_core::trace::trace_field(c).is_some() || c == "TE_Below_DL" {
        "trace"
    } else {
        "chem"
    };
    wanted(app, g)
}

pub struct Sheet {
    pub name: String,
    pub columns: Vec<String>,
    pub rows: Vec<Row>,
    pub blocks: bool,
    pub mineral: bool,
}

pub fn sheets(app: &ChemApp) -> Vec<Sheet> {
    let upb = chemchron_core::overview::any_upb(&app.data.rows);
    let mut out = Vec::new();
    let samples: Vec<Row> = app.data.rows.iter().filter(|r| !truthy(r, "Is_Reference")).cloned().collect();
    let refs: Vec<Row> = app.data.rows.iter().filter(|r| truthy(r, "Is_Reference")).cloned().collect();
    for (name, rows, is_ref) in [("Geochronology Samples", &samples, false), ("Geochronology References", &refs, true)] {
        let mut cols: Vec<String> = columns::age_columns(is_ref, upb).into_iter().filter(|c| keep_col(app, c)).collect();
        if wanted(app, "trace") && !app.data.trace.is_empty() {
            cols.extend(chemchron_core::trace::trace_columns(&app.data.trace));
            cols.push("TE_Below_DL".into());
        }
        out.push(Sheet { name: name.into(), columns: cols, rows: rows.clone(), blocks: true, mineral: false });
    }
    if wanted(app, "comp") {
        for (name, rows, is_ref) in [("Mineral composition Samples", &samples, false), ("Mineral composition References", &refs, true)] {
            let cols: Vec<String> = columns::mineral_columns(&app.data.active, &app.data.trace, is_ref)
                .into_iter()
                .filter(|c| app.settings.show_uncertainty_columns || (!c.ends_with("_2s_wt%") && !c.ends_with("_DL_wt%")))
                .filter(|c| wanted(app, "trace") || chemchron_core::trace::trace_field(c).is_none())
                .collect();
            let rows: Vec<Row> = rows.iter().filter(|r| !truthy(r, "No_EPMA")).cloned().collect();
            out.push(Sheet { name: name.into(), columns: cols, rows, blocks: true, mineral: true });
        }
    }
    if wanted(app, "summary") {
        let (ro, fo, upb) = crate::combine::overviews(app, true);
        let extra = |base: Vec<String>, refs: bool| -> Vec<String> {
            let mut v = base;
            if upb {
                for t in chemchron_core::upbstats::AGE_TYPES {
                    for k in ["N", "N_disc", "Mean_Ma", "2s_Ma", "WM_Ma", "WM_2s_Ma", "MSWD"] {
                        v.push(format!("UPbSum_{t}_{k}"));
                    }
                    if refs {
                        for k in ["Accepted_Ma", "Accepted_2s_Ma", "Diff_Ma", "Diff_pct"] {
                            v.push(format!("UPbSum_{t}_{k}"));
                        }
                    }
                }
            }
            v
        };
        let rc = extra(crate::combine::summary_columns(true, false), true);
        let fc = extra(crate::combine::summary_columns(false, false), false);
        out.push(Sheet { name: "Reference Materials Overview".into(), columns: rc, rows: ro, blocks: false, mineral: false });
        out.push(Sheet { name: "Sample Group Overview".into(), columns: fc, rows: fo, blocks: false, mineral: false });
    }
    out
}

fn header_name(c: &str) -> String {
    if let Some(rest) = c.strip_prefix("UPbSum_") {
        let (t, k) = rest.split_at(3);
        return format!("{} {}", chemchron_core::upbstats::age_type_label(t), display_name(&format!("UPb{k}")));
    }
    display_name(c)
}

fn block_of(s: &Sheet, c: &str) -> &'static str {
    if s.mineral {
        mineral_block(c)
    } else {
        geochronology_block(c)
    }
}

fn cell(r: &Row, c: &str, dp: &chemchron_core::constants::DecimalPlaces) -> String {
    match c {
        "Is_Reference" => if truthy(r, c) { "Ref".into() } else { "Sample".into() },
        "Include_In_Average" => if flag(r, c, true) { "True".into() } else { "False".into() },
        "UPb_Include" => {
            if chemchron_core::upb::has_upb(r) {
                if chemchron_core::upb::included_in_upb(r) { "True".into() } else { "False".into() }
            } else {
                String::new()
            }
        }
        _ => match r.get(c) {
            // exports carry every decimal; rounding is for the window only
            Some(serde_json::Value::Number(n)) => n.to_string(),
            _ => crate::rowtable::cell_text(r, c, dp, true),
        },
    }
}

pub fn write_csv(app: &ChemApp, path: &Path) -> Result<(), String> {
    let dp = app.settings.decimals;
    let mut s = String::new();
    for sh in sheets(app) {
        s.push_str(&chemchron_core::table::csv_escape(&sh.name.to_uppercase()));
        s.push_str("\r\n");
        if sh.blocks {
            let mut last = "";
            let b: Vec<String> = sh.columns.iter().map(|c| {
                let bl = block_of(&sh, c);
                if bl != last {
                    last = bl;
                    bl.to_string()
                } else {
                    String::new()
                }
            }).collect();
            s.push_str(&b.iter().map(|x| chemchron_core::table::csv_escape(x)).collect::<Vec<_>>().join(","));
            s.push_str("\r\n");
        }
        s.push_str(&sh.columns.iter().map(|c| chemchron_core::table::csv_escape(&header_name(c))).collect::<Vec<_>>().join(","));
        s.push_str("\r\n");
        for r in &sh.rows {
            s.push_str(&sh.columns.iter().map(|c| chemchron_core::table::csv_escape(&cell(r, c, &dp))).collect::<Vec<_>>().join(","));
            s.push_str("\r\n");
        }
        s.push_str("\r\n");
    }
    std::fs::write(path, s).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn write_xlsx(app: &ChemApp, path: &Path) -> Result<(), String> {
    use rust_xlsxwriter::{Color, Format, FormatBorder, Workbook};
    let dp = app.settings.decimals;
    let mut wb = Workbook::new();
    let head = Format::new().set_bold().set_font_color(Color::White).set_background_color(Color::RGB(0x3A5F8A)).set_text_wrap().set_border(FormatBorder::Thin);
    let block = Format::new().set_bold().set_font_color(Color::White).set_background_color(Color::RGB(0x24405F));
    let within = Format::new().set_background_color(Color::RGB(0x2F6B3A)).set_font_color(Color::RGB(0xF0F0F0));
    let outside = Format::new().set_background_color(Color::RGB(0x7A3030)).set_font_color(Color::RGB(0xF0F0F0));
    for sh in sheets(app) {
        let ws = wb.add_worksheet();
        ws.set_name(&sh.name.chars().take(31).collect::<String>()).map_err(|e| e.to_string())?;
        let mut r0 = 0u32;
        if sh.blocks {
            let mut last = "";
            for (j, c) in sh.columns.iter().enumerate() {
                let bl = block_of(&sh, c);
                if bl != last {
                    ws.write_string_with_format(0, j as u16, bl, &block).map_err(|e| e.to_string())?;
                    last = bl;
                }
            }
            r0 = 1;
        }
        for (j, c) in sh.columns.iter().enumerate() {
            ws.write_string_with_format(r0, j as u16, header_name(c), &head).map_err(|e| e.to_string())?;
            ws.set_column_width(j as u16, if c == "Label" || c == "Mineral_Formula" { 22 } else { 13 }).map_err(|e| e.to_string())?;
        }
        for (i, r) in sh.rows.iter().enumerate() {
            let row = r0 + 1 + i as u32;
            let tint = match text(r, "Ref_Within_2s").as_str() {
                chemchron_core::reference::WITHIN_2S if truthy(r, "Is_Reference") => Some(&within),
                chemchron_core::reference::OUTSIDE_2S if truthy(r, "Is_Reference") => Some(&outside),
                _ => None,
            };
            for (j, c) in sh.columns.iter().enumerate() {
                let col = j as u16;
                let numeric = matches!(r.get(c), Some(serde_json::Value::Number(_))) && !c.ends_with("Within_2s");
                if numeric {
                    let v = num(r, c).unwrap_or(f64::NAN);
                    // the full value, without a rounding number format
                    let mut f = Format::new();
                    if let Some(t) = tint {
                        f = f.set_background_color(Color::RGB(if std::ptr::eq(t, &within) { 0x2F6B3A } else { 0x7A3030 })).set_font_color(Color::RGB(0xF0F0F0));
                    }
                    ws.write_number_with_format(row, col, v, &f).map_err(|e| e.to_string())?;
                } else {
                    let s = cell(r, c, &dp);
                    match tint {
                        Some(t) => ws.write_string_with_format(row, col, s, t).map(|_| ()),
                        None => ws.write_string(row, col, s).map(|_| ()),
                    }
                    .map_err(|e| e.to_string())?;
                }
            }
        }
        if !sh.rows.is_empty() && !sh.columns.is_empty() {
            let _ = ws.autofilter(r0, 0, r0 + sh.rows.len() as u32, sh.columns.len() as u16 - 1);
        }
        let _ = ws.set_freeze_panes(r0 + 1, 1);
    }
    wb.save(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A plain OpenDocument spreadsheet (one table per sheet, numbers kept as numbers).
pub fn write_ods(app: &ChemApp, path: &Path) -> Result<(), String> {
    let dp = app.settings.decimals;
    let mut content = String::from(r##"<?xml version="1.0" encoding="UTF-8"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" office:version="1.2"><office:automatic-styles><style:style style:name="h" style:family="table-cell"><style:table-cell-properties fo:background-color="#3a5f8a"/><style:text-properties fo:font-weight="bold" fo:color="#ffffff"/></style:style><style:style style:name="in" style:family="table-cell"><style:table-cell-properties fo:background-color="#2f6b3a"/><style:text-properties fo:color="#f0f0f0"/></style:style><style:style style:name="out" style:family="table-cell"><style:table-cell-properties fo:background-color="#7a3030"/><style:text-properties fo:color="#f0f0f0"/></style:style></office:automatic-styles><office:body><office:spreadsheet>"##);
    for sh in sheets(app) {
        content += &format!("<table:table table:name=\"{}\">", xml(&sh.name.chars().take(31).collect::<String>()));
        if sh.blocks {
            content += "<table:table-row>";
            let mut last = "";
            for c in &sh.columns {
                let bl = block_of(&sh, c);
                let t = if bl != last { bl } else { "" };
                last = bl;
                content += &format!("<table:table-cell table:style-name=\"h\" office:value-type=\"string\"><text:p>{}</text:p></table:table-cell>", xml(t));
            }
            content += "</table:table-row>";
        }
        content += "<table:table-row>";
        for c in &sh.columns {
            content += &format!("<table:table-cell table:style-name=\"h\" office:value-type=\"string\"><text:p>{}</text:p></table:table-cell>", xml(&header_name(c)));
        }
        content += "</table:table-row>";
        for r in &sh.rows {
            content += "<table:table-row>";
            let style = if truthy(r, "Is_Reference") {
                match text(r, "Ref_Within_2s").as_str() {
                    chemchron_core::reference::WITHIN_2S => " table:style-name=\"in\"",
                    chemchron_core::reference::OUTSIDE_2S => " table:style-name=\"out\"",
                    _ => "",
                }
            } else {
                ""
            };
            for c in &sh.columns {
                let s = cell(r, c, &dp);
                match (r.get(c), num(r, c)) {
                    (Some(serde_json::Value::Number(_)), Some(v)) => {
                        content += &format!("<table:table-cell{style} office:value-type=\"float\" office:value=\"{v}\"><text:p>{}</text:p></table:table-cell>", xml(&s));
                    }
                    _ => {
                        content += &format!("<table:table-cell{style} office:value-type=\"string\"><text:p>{}</text:p></table:table-cell>", xml(&s));
                    }
                }
            }
            content += "</table:table-row>";
        }
        content += "</table:table>";
    }
    content += "</office:spreadsheet></office:body></office:document-content>";
    let manifest = r##"<?xml version="1.0" encoding="UTF-8"?><manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.2"><manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.spreadsheet"/><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/></manifest:manifest>"##;
    let f = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut z = zip::ZipWriter::new(f);
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    z.start_file("mimetype", stored).map_err(|e| e.to_string())?;
    z.write_all(b"application/vnd.oasis.opendocument.spreadsheet").map_err(|e| e.to_string())?;
    z.start_file("content.xml", deflated).map_err(|e| e.to_string())?;
    z.write_all(content.as_bytes()).map_err(|e| e.to_string())?;
    z.start_file("META-INF/manifest.xml", deflated).map_err(|e| e.to_string())?;
    z.write_all(manifest.as_bytes()).map_err(|e| e.to_string())?;
    z.finish().map_err(|e| e.to_string())?;
    Ok(())
}

/// IsoplotR input (U–Pb format 2: 238U/206Pb, 2s, 207Pb/206Pb, 2s, rho), one laser spot once.
pub fn isoplotr_text(app: &ChemApp) -> String {
    let rows: Vec<&Row> = app.data.rows.iter().collect();
    let mut s = String::from("U238Pb206,errU238Pb206,Pb207Pb206,errPb207Pb206,rho,label\n");
    for (i, x, sx, y, sy, r) in chemchron_core::upbstats::concordia_data(&rows, false) {
        s += &format!("{x},{},{y},{},{r},{}\n", sx / 2.0, sy / 2.0, text(rows[i], "Label"));
    }
    s
}

pub fn page_export(app: &mut ChemApp, ui: &mut Ui) {
    let t = app.theme();
    ui.heading("Export");
    ui.label(RichText::new("The analysis table and the Summary: Geochronology and Mineral composition sheets for samples and references, plus the Summary sheets (U–Pb Summary for every age type). Reference rows are coloured within / outside 2σ in workbooks.").color(t.dim).size(12.0));
    section(ui, &t, "Columns", |ui| {
        for (k, label) in GROUPS {
            let mut on = *app.combine.export_choice.get(k).unwrap_or(&true);
            if ui.checkbox(&mut on, label).changed() {
                app.combine.export_choice.insert(k.to_string(), on);
            }
        }
    });
    let mut result: Option<Result<String, String>> = None;
    ui.horizontal_wrapped(|ui| {
        let name = chemchron_laser::app::sanitize(&app.project_name());
        for (label, ext) in [("Export to .xlsx…", "xlsx"), ("Export to .ods…", "ods"), ("Export to .csv…", "csv")] {
            if ui.button(label).clicked() {
                if let Some(p) = rfd::FileDialog::new().set_file_name(format!("{name}_results.{ext}")).add_filter(ext, &[ext]).save_file() {
                    let r = match ext {
                        "xlsx" => write_xlsx(app, &p),
                        "ods" => write_ods(app, &p),
                        _ => write_csv(app, &p),
                    };
                    result = Some(r.map(|_| format!("Exported {}", p.display())));
                }
            }
        }
        if ui.button("IsoplotR input (U–Pb)…").clicked() {
            if let Some(p) = rfd::FileDialog::new().set_file_name(format!("{name}_isoplotr.csv")).save_file() {
                result = Some(std::fs::write(&p, isoplotr_text(app)).map(|_| format!("Exported {}", p.display())).map_err(|e| e.to_string()));
            }
        }
        if ui.button("Export all figures…").on_hover_text("Every plot of the project into one folder: isochron, weighted means and concordia for each group, REE, composition, age distribution, chemical vs U–Pb and the long-term logs. Size, font and format are those of the Save figure window.").clicked() {
            if let Some(d) = rfd::FileDialog::new().set_title("Folder for the figures").pick_folder() {
                let d = d.join(format!("{name}_figures"));
                result = Some(crate::plot_settings::export_all(app, &d));
            }
        }
        if ui.button("Save the project…").clicked() {
            app.save_project(true);
        }
    });
    if let Some(r) = result {
        match r {
            Ok(m) => app.say(m, false),
            Err(e) => app.say(e, true),
        }
    }
    ui.add_space(8.0);
    for sh in sheets(app) {
        ui.label(RichText::new(format!("{}: {} rows × {} columns", sh.name, sh.rows.len(), sh.columns.len())).color(t.dim).size(12.0));
    }
    ui.add_space(6.0);
    ui.label(RichText::new("Laser results on their own (with every sweep and metadata) are exported from LA-ICP-MS > Export.").color(t.dim).size(12.0));
}
