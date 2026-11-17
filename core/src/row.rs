// SPDX-License-Identifier: GPL-3.0-or-later
//! An analysis row: a JSON object with one field per quantity, as stored in
//! project files. Blank values are `""`.

use serde_json::{Map, Value};

pub type Row = Map<String, Value>;

/// A number stored in the row (not a string, not a bool).
pub fn num(row: &Row, key: &str) -> Option<f64> {
    match row.get(key) {
        Some(Value::Number(n)) => n.as_f64().filter(|v| v.is_finite()),
        _ => None,
    }
}

/// A number, also accepting numeric text (decimal comma allowed).
pub fn num_loose(row: &Row, key: &str) -> Option<f64> {
    match row.get(key) {
        Some(Value::Number(n)) => n.as_f64().filter(|v| v.is_finite()),
        Some(Value::String(s)) => parse_locale(s).ok().flatten(),
        Some(Value::Bool(b)) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

/// The value as text: numbers formatted, blanks empty.
pub fn text(row: &Row, key: &str) -> String {
    value_text(row.get(key))
}

pub fn value_text(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => if *b { "True".into() } else { "False".into() },
        Some(Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else {
                let f = n.as_f64().unwrap_or(f64::NAN);
                fmt_g(f)
            }
        }
        Some(other) => other.to_string(),
    }
}

/// Shortest text that reads back as the same number.
pub fn fmt_g(f: f64) -> String {
    if !f.is_finite() {
        return String::new();
    }
    if f == f.trunc() && f.abs() < 1e15 {
        return format!("{f:.1}");
    }
    format!("{f}")
}

/// Whether a field is set: true, a non-empty text, a non-zero number or a non-empty list.
pub fn truthy(row: &Row, key: &str) -> bool {
    match row.get(key) {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Number(n)) => n.as_f64().is_some_and(|v| v != 0.0),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// A field read as a bool, with `default` when it is missing.
pub fn flag(row: &Row, key: &str, default: bool) -> bool {
    match row.get(key) {
        None => default,
        Some(_) => truthy(row, key),
    }
}

pub fn parse_bool(v: Option<&Value>, default: bool) -> bool {
    match v {
        None | Some(Value::Null) => default,
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) if s.is_empty() => default,
        Some(Value::String(s)) => matches!(s.trim().to_lowercase().as_str(), "true" | "1" | "yes" | "y"),
        Some(Value::Number(n)) => n.as_f64().is_some_and(|v| v != 0.0),
        Some(_) => default,
    }
}

/// Parses a number written with a decimal point or a single decimal comma.
/// Empty text is `Ok(None)`.
pub fn parse_locale(text: &str) -> Result<Option<f64>, String> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(None);
    }
    let candidate = if t.matches(',').count() == 1 && !t.contains('.') { t.replace(',', ".") } else { t.to_string() };
    match candidate.parse::<f64>() {
        Ok(v) => Ok(Some(v)),
        Err(_) => {
            let lower = candidate.to_lowercase();
            match lower.as_str() {
                "nan" => Ok(Some(f64::NAN)),
                "inf" | "infinity" => Ok(Some(f64::INFINITY)),
                "-inf" | "-infinity" => Ok(Some(f64::NEG_INFINITY)),
                _ => Err(format!("could not parse numeric value: '{t}'")),
            }
        }
    }
}

/// `parse_locale_float(row.get(key) or 0.0)`: a value, 0 when blank.
pub fn locale_value(v: Option<&Value>, field: &str) -> Result<f64, String> {
    match v {
        None | Some(Value::Null) => Ok(0.0),
        Some(Value::Number(n)) => Ok(n.as_f64().unwrap_or(0.0)),
        Some(Value::Bool(b)) => Ok(if *b { 1.0 } else { 0.0 }),
        Some(Value::String(s)) => parse_locale(s)
            .map(|o| o.unwrap_or(0.0))
            .map_err(|_| format!("could not parse numeric value for {field}: '{}'", s.trim())),
        Some(_) => Err(format!("could not parse numeric value for {field}")),
    }
}

/// Rounds to `n` decimals (ties to even on the exact binary value).
pub fn round_to(x: f64, digits: usize) -> f64 {
    if !x.is_finite() {
        return x;
    }
    format!("{x:.digits$}").parse().unwrap_or(x)
}

pub fn jnum(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::String(String::new()))
}

/// A number at full precision (values are rounded only when shown), or `""`
/// when not finite. `_digits` is the display precision, kept for reference.
pub fn jround(x: f64, _digits: usize) -> Value {
    if x.is_finite() {
        jnum(x)
    } else {
        Value::String(String::new())
    }
}

pub fn jopt(x: Option<f64>) -> Value {
    match x {
        Some(v) if v.is_finite() => jnum(v),
        _ => blank(),
    }
}

pub fn blank() -> Value {
    Value::String(String::new())
}

pub fn jstr(s: impl Into<String>) -> Value {
    Value::String(s.into())
}

pub fn set(row: &mut Row, key: &str, v: Value) {
    row.insert(key.to_string(), v);
}

/// Mean and population/sample standard deviations.
pub fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.iter().sum::<f64>() / v.len() as f64
}

pub fn pstdev(v: &[f64]) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    let m = mean(v);
    (v.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / v.len() as f64).sqrt()
}

pub fn stdev(v: &[f64]) -> f64 {
    if v.len() < 2 {
        return f64::NAN;
    }
    let m = mean(v);
    (v.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (v.len() - 1) as f64).sqrt()
}

pub fn median(v: &[f64]) -> f64 {
    let mut s: Vec<f64> = v.iter().copied().filter(|x| x.is_finite()).collect();
    if s.is_empty() {
        return f64::NAN;
    }
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = s.len();
    if n % 2 == 1 {
        s[n / 2]
    } else {
        0.5 * (s[n / 2 - 1] + s[n / 2])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rounding_ties_to_even_on_the_binary_value() {
        assert_eq!(round_to(0.25, 1), 0.2);
        assert_eq!(round_to(2.675, 2), 2.67);
        assert_eq!(round_to(2.5, 0), 2.0);
        assert_eq!(round_to(1234.5678, 2), 1234.57);
    }
    #[test]
    fn decimal_comma() {
        assert_eq!(parse_locale("12,34").unwrap(), Some(12.34));
        assert!(parse_locale("1,234.5").is_err() || parse_locale("1,234.5").unwrap() != Some(1.2345));
    }
}
