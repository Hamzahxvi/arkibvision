//! The Rust port of `src/utils/metadata_utils.py`.

#![forbid(unsafe_code)]

use regex::Regex;
use serde_json::{Map, Value};
use std::sync::OnceLock;

const PRIVATE_KEYS: [&str; 10] = [
    "license_no",
    "dob",
    "date_of_birth",
    "ic",
    "nric",
    "passport",
    "ssn",
    "phone",
    "email",
    "exp",
];

// Unicode Nd (Decimal_Number) zero code points for matching Python's `int()` on `\d`.
const ND_ZEROS: &[u32] = &[
    0x0660, 0x06F0, 0x07C0, 0x0966, 0x09E6, 0x0A66, 0x0AE6, 0x0B66, 0x0BE6, 0x0C66, 0x0CE6, 0x0D66,
    0x0DE6, 0x0E50, 0x0ED0, 0x0F20, 0x1040, 0x1090, 0x17E0, 0x1810, 0x1946, 0x19D0, 0x1A80, 0x1A90,
    0x1B50, 0x1BB0, 0x1C40, 0x1C50, 0xA620, 0xA8D0, 0xA900, 0xA9D0, 0xA9F0, 0xAA50, 0xABF0, 0xFF10,
    0x104A0, 0x10D30, 0x11066, 0x110F0, 0x11136, 0x111D0, 0x112F0, 0x11450, 0x114D0, 0x11650,
    0x116C0, 0x11730, 0x118E0, 0x11950, 0x11C50, 0x11D50, 0x11DA0, 0x11F50, 0x16A60, 0x16AC0,
    0x16B50, 0x1D7CE, 0x1D7D8, 0x1D7E2, 0x1D7EC, 0x1D7F6, 0x1E140, 0x1E2F0, 0x1E4F0, 0x1E950,
    0x1FBF0,
];

fn is_py_ws(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\u{001C}'..='\u{001F}')
}

fn py_all_cased(s: &str, pred: fn(char) -> bool) -> bool {
    let mut seen = false;
    for c in s.chars() {
        if c.is_lowercase() || c.is_uppercase() || c.to_lowercase().ne(c.to_uppercase()) {
            if !pred(c) {
                return false;
            }
            seen = true;
        }
    }
    seen
}

fn parse_unicode_uint(s: &str) -> Option<u32> {
    let mut val = 0u32;
    for c in s.chars() {
        let cp = c as u32;
        let d = if c.is_ascii_digit() {
            cp - ('0' as u32)
        } else {
            let z = ND_ZEROS.iter().copied().find(|&z| cp >= z && cp < z + 10)?;
            cp - z
        };
        val = val.checked_mul(10)?.checked_add(d)?;
    }
    (!s.is_empty()).then_some(val)
}

fn is_unknown_casefold(s: &str) -> bool {
    s.chars().count() == 7
        && s.chars().zip(b"unknown").all(|(c, &t)| match c {
            'A'..='Z' => (c as u8) + 32 == t,
            'a'..='z' => c as u8 == t,
            '\u{212A}' => t == b'k',
            _ => false,
        })
}

/// Port of `profile_slug` (`src/utils/metadata_utils.py:22`).
pub fn profile_slug(identity: String) -> Result<String, String> {
    let mut out = String::with_capacity(identity.len());
    let mut in_sep = false;
    for c in identity.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
            out.push(c);
            in_sep = false;
        } else if !in_sep {
            out.push('_');
            in_sep = true;
        }
    }
    Ok(out.trim_matches(|c| c == '.' || c == '_').to_string())
}

/// Port of `format_identity_name` (`src/utils/metadata_utils.py:27`).
pub fn format_identity_name(identity: String) -> Result<String, String> {
    let trimmed = identity.trim_matches(is_py_ws);
    let mut out = String::with_capacity(trimmed.len());
    for w in trimmed.split(is_py_ws).filter(|w| !w.is_empty()) {
        if !out.is_empty() {
            out.push(' ');
        }
        let mut cs = w.chars();
        if let Some(first) = cs.next() {
            out.extend(first.to_uppercase());
            let rest = cs.as_str();
            if py_all_cased(w, char::is_lowercase) || py_all_cased(w, char::is_uppercase) {
                out.push_str(&rest.to_lowercase());
            } else {
                out.push_str(rest);
            }
        }
    }
    Ok(out)
}

/// Port of `format_time_12h` (`src/utils/metadata_utils.py:39`).
pub fn format_time_12h(value: String) -> Result<String, String> {
    static AM_PM: OnceLock<Regex> = OnceLock::new();
    static T24: OnceLock<Regex> = OnceLock::new();
    let text = value.trim_matches(is_py_ws);
    if text.is_empty() || is_unknown_casefold(text) {
        return Ok("Unknown".to_string());
    }
    let am_pm = AM_PM.get_or_init(|| Regex::new(r"(?i)\b(am|pm)\b").expect("regex"));
    if am_pm.is_match(text) {
        return Ok(am_pm
            .replace_all(text, |c: &regex::Captures<'_>| c[1].to_ascii_uppercase())
            .into_owned());
    }
    let t24 = T24.get_or_init(|| Regex::new(r"\A(\d{1,2}):(\d{2})(?::(\d{2}))?\z").expect("regex"));
    if let Some(c) = t24.captures(text)
        && let (Some(h), Some(m)) = (parse_unicode_uint(&c[1]), parse_unicode_uint(&c[2]))
        && h <= 23
        && m <= 59
    {
        return Ok(format!(
            "{}:{m:02} {}",
            if h % 12 == 0 { 12 } else { h % 12 },
            if h < 12 { "AM" } else { "PM" }
        ));
    }
    Ok(text.to_string())
}

/// Port of `default_detail_visibility` (`src/utils/metadata_utils.py:58`).
pub fn default_detail_visibility(key: String) -> Result<String, String> {
    let norm: String = key
        .trim_matches(is_py_ws)
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    Ok(if PRIVATE_KEYS.contains(&norm.as_str()) {
        "private"
    } else {
        "public"
    }
    .to_string())
}

/// Port of `parse_proposed_metadata` (`src/utils/metadata_utils.py:64`).
pub fn parse_proposed_metadata(raw: String) -> Result<Value, String> {
    static KV: OnceLock<Regex> = OnceLock::new();
    let text = raw.trim_matches(is_py_ws);
    if text.is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    if text.starts_with('{') {
        if let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(text) {
            return Ok(Value::Object(obj));
        }
        let closed = format!("{}}}", text.trim_end_matches(','));
        if let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(&closed) {
            return Ok(Value::Object(obj));
        }
        let kv = KV.get_or_init(|| {
            Regex::new(r#""([^"\\]+)"[\s\x1c-\x1f]*:[\s\x1c-\x1f]*"([^"\\]*)""#).expect("regex")
        });
        let map: Map<String, Value> = kv
            .captures_iter(text)
            .map(|c| (c[1].to_string(), Value::String(c[2].to_string())))
            .collect();
        if !map.is_empty() {
            return Ok(Value::Object(map));
        }
    }
    let mut map = Map::with_capacity(1);
    map.insert("description".to_string(), Value::String(text.to_string()));
    Ok(Value::Object(map))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke_metadata_utils() {
        assert_eq!(
            profile_slug("Mahathir Mohamad".into()).unwrap(),
            "Mahathir_Mohamad"
        );
        assert_eq!(
            format_identity_name("  eula   lawrence ".into()).unwrap(),
            "Eula Lawrence"
        );
        assert_eq!(format_time_12h("13:45:30".into()).unwrap(), "1:45 PM");
        assert_eq!(
            default_detail_visibility("Date of Birth".into()).unwrap(),
            "private"
        );
        assert_eq!(
            parse_proposed_metadata(r#"{"name": "Eula", "broken": "#.into()).unwrap()["name"],
            "Eula"
        );
    }
}
