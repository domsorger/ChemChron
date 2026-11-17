// SPDX-License-Identifier: GPL-3.0-or-later
//! Reading and writing simple tables: CSV (delimiter sniffed), XLSX and ODS.

use std::path::Path;

/// Splits CSV text into rows (quotes and embedded newlines handled).
pub fn read_csv_rows(text: &str, delim: char) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    let mut any = false;
    while let Some(ch) = chars.next() {
        any = true;
        if quoted {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    cell.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            } else {
                cell.push(ch);
            }
            continue;
        }
        match ch {
            '"' if cell.is_empty() => quoted = true,
            c if c == delim => row.push(std::mem::take(&mut cell)),
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
                any = false;
            }
            '\n' => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
                any = false;
            }
            c => cell.push(c),
        }
    }
    if any || !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    // a blank line becomes an empty row
    rows.into_iter().map(|r| if r.len() == 1 && r[0].is_empty() { Vec::new() } else { r }).collect()
}

pub fn split_csv_line(line: &str, delim: char) -> Vec<String> {
    read_csv_rows(line, delim).into_iter().next().unwrap_or_default()
}

pub fn csv_escape(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// The most consistent of ",", ";" and tab over the first lines.
pub fn sniff_delimiter(sample: &str) -> char {
    let lines: Vec<&str> = sample.lines().filter(|l| !l.trim().is_empty()).take(20).collect();
    let mut best = (',', 0usize, 0usize);
    for d in [',', ';', '\t'] {
        let counts: Vec<usize> = lines.iter().map(|l| split_csv_line(l, d).len()).collect();
        if counts.is_empty() {
            continue;
        }
        let modal = *counts.iter().max_by_key(|c| counts.iter().filter(|x| x == c).count()).unwrap();
        let agree = counts.iter().filter(|c| **c == modal).count();
        if modal > 1 && (agree > best.2 || (agree == best.2 && modal > best.1)) {
            best = (d, modal, agree);
        }
    }
    best.0
}

fn cell_text(c: &calamine::Data) -> String {
    use calamine::Data;
    match c {
        Data::Empty => String::new(),
        Data::String(s) => s.clone(),
        Data::Float(f) => format!("{f}"),
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => if *b { "True".into() } else { "False".into() },
        Data::DateTime(d) => format!("{}", d.as_f64()),
        Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Error(_) => String::new(),
    }
}

pub fn list_sheets(path: &Path) -> Result<Vec<String>, String> {
    use calamine::Reader;
    let ext = ext_of(path);
    if matches!(ext.as_str(), "xlsx" | "xlsm" | "ods") {
        let wb = calamine::open_workbook_auto(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(wb.sheet_names().to_vec())
    } else {
        Ok(Vec::new())
    }
}

fn ext_of(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Every row of a CSV / XLSX / ODS sheet, as text cells.
pub fn read_table(path: &Path, sheet: Option<&str>) -> Result<Vec<Vec<String>>, String> {
    use calamine::Reader;
    let ext = ext_of(path);
    if matches!(ext.as_str(), "xlsx" | "xlsm" | "xls" | "xlsb" | "ods") {
        let mut wb = calamine::open_workbook_auto(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let names = wb.sheet_names().to_vec();
        let name = match sheet {
            Some(s) if names.iter().any(|n| n == s) => s.to_string(),
            _ => names.first().cloned().ok_or("The workbook has no sheets")?,
        };
        let range = wb.worksheet_range(&name).map_err(|e| format!("{name}: {e}"))?;
        let (r0, c0) = range.start().map(|(r, c)| (r as usize, c as usize)).unwrap_or((0, 0));
        let mut out: Vec<Vec<String>> = vec![Vec::new(); r0];
        for row in range.rows() {
            let mut cells: Vec<String> = vec![String::new(); c0];
            cells.extend(row.iter().map(cell_text));
            while cells.last().is_some_and(|c| c.is_empty()) {
                cells.pop();
            }
            out.push(cells);
        }
        return Ok(out);
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let text = String::from_utf8_lossy(&bytes).to_string();
    let text = text.trim_start_matches('\u{feff}');
    let sample: String = text.chars().take(20000).collect();
    Ok(read_csv_rows(text, sniff_delimiter(&sample)))
}

/// Writes rows as CSV (comma separated, quoted where needed).
pub fn write_csv(path: &Path, rows: &[Vec<String>]) -> Result<(), String> {
    let mut s = String::new();
    for r in rows {
        let cells: Vec<String> = r.iter().map(|c| csv_escape(c)).collect();
        s.push_str(&cells.join(","));
        s.push_str("\r\n");
    }
    std::fs::write(path, s).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn csv_basics() {
        let r = read_csv_rows("a,b\n\"x,1\",2\n\n3,4", ',');
        assert_eq!(r[1], vec!["x,1", "2"]);
        assert!(r[2].is_empty());
        assert_eq!(sniff_delimiter("a;b;c\n1;2;3\n"), ';');
        assert_eq!(sniff_delimiter("a\tb\n1\t2\n"), '\t');
    }
}
