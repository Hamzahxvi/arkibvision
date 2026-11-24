#![forbid(unsafe_code)]

use regex::Regex;
use serde_json::{Map, Value, json};
use std::sync::OnceLock;

pub const PRIVATE_KEYS: [&str; 10] = [
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

pub const STANDARD_META_FIELDS: [&str; 7] = [
    "description",
    "photograph_by",
    "location",
    "date",
    "time",
    "address",
    "notes",
];

pub const PROTECTED_META_FIELDS: [&str; 6] = [
    "identity",
    "name",
    "images",
    "database",
    "vector_count",
    "stored_original_image_count",
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

pub fn profile_slug(identity: &str) -> String {
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
    out.trim_matches(|c| c == '.' || c == '_').to_string()
}

pub fn format_identity_name(identity: &str) -> String {
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
    out
}

pub fn format_time_12h(value: &str) -> String {
    static AM_PM: OnceLock<Regex> = OnceLock::new();
    static T24: OnceLock<Regex> = OnceLock::new();
    let text = value.trim_matches(is_py_ws);
    if text.is_empty() || text.eq_ignore_ascii_case("unknown") {
        return "Unknown".to_string();
    }
    let am_pm = AM_PM.get_or_init(|| Regex::new(r"(?i)\b(am|pm)\b").expect("regex"));
    if am_pm.is_match(text) {
        return am_pm
            .replace_all(text, |c: &regex::Captures<'_>| c[1].to_ascii_uppercase())
            .into_owned();
    }
    let t24 = T24.get_or_init(|| Regex::new(r"\A(\d{1,2}):(\d{2})(?::(\d{2}))?\z").expect("regex"));
    if let Some(c) = t24.captures(text)
        && let (Ok(h), Ok(m)) = (c[1].parse::<u32>(), c[2].parse::<u32>())
        && h <= 23
        && m <= 59
    {
        return format!(
            "{}:{m:02} {}",
            if h % 12 == 0 { 12 } else { h % 12 },
            if h < 12 { "AM" } else { "PM" }
        );
    }
    text.to_string()
}

pub fn default_detail_visibility(key: &str) -> &'static str {
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
    if PRIVATE_KEYS.contains(&norm.as_str()) {
        "private"
    } else {
        "public"
    }
}

pub fn parse_proposed_metadata(raw: &str) -> Map<String, Value> {
    static KV: OnceLock<Regex> = OnceLock::new();
    let text = raw.trim_matches(is_py_ws);
    if text.is_empty() {
        return Map::new();
    }
    if text.starts_with('{') {
        if let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(text) {
            return obj;
        }
        let closed = format!("{}}}", text.trim_end_matches(','));
        if let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(&closed) {
            return obj;
        }
        let kv = KV.get_or_init(|| {
            Regex::new(r#""([^"\\]+)"[\s\x1c-\x1f]*:[\s\x1c-\x1f]*"([^"\\]*)""#).expect("regex")
        });
        let map: Map<String, Value> = kv
            .captures_iter(text)
            .map(|c| (c[1].to_string(), Value::String(c[2].to_string())))
            .collect();
        if !map.is_empty() {
            return map;
        }
    }
    let mut map = Map::with_capacity(1);
    map.insert("description".to_string(), Value::String(text.to_string()));
    map
}

pub fn normalize_alnum_lower(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

pub fn value_to_trimmed_string(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.trim().to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

pub fn normalize_other_details(raw: Option<&Value>) -> Map<String, Value> {
    let mut out = Map::new();
    let Some(Value::Object(obj)) = raw else {
        return out;
    };
    for (k, v) in obj {
        let key = k.trim();
        if key.is_empty() {
            continue;
        }
        let (val, vis) = if let Value::Object(inner) = v {
            let val = inner
                .get("value")
                .map(value_to_trimmed_string)
                .unwrap_or_default();
            let raw_vis = inner
                .get("visibility")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            let vis = if raw_vis == "public" || raw_vis == "private" {
                raw_vis
            } else {
                default_detail_visibility(key).to_string()
            };
            (val, vis)
        } else {
            (
                value_to_trimmed_string(v),
                default_detail_visibility(key).to_string(),
            )
        };
        if !val.is_empty() {
            out.insert(key.to_string(), json!({ "value": val, "visibility": vis }));
        }
    }
    out
}

pub fn public_other_details(raw: Option<&Value>) -> Map<String, Value> {
    let normalized = normalize_other_details(raw);
    let mut out = Map::new();
    for (k, item) in normalized {
        if item.get("visibility").and_then(Value::as_str) == Some("public")
            && let Some(v) = item.get("value").and_then(Value::as_str)
        {
            out.insert(k, Value::String(v.to_string()));
        }
    }
    out
}

pub fn merge_profile_metadata(meta: &mut Map<String, Value>, proposed_meta: &Map<String, Value>) {
    let mut normalized = proposed_meta.clone();
    let replace_other = normalized
        .remove("_replace_other")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let mut other = if replace_other {
        Map::new()
    } else {
        normalize_other_details(meta.get("other_details"))
    };

    if let Some(Value::String(desc)) = normalized.get("description").cloned()
        && desc.trim().starts_with('{')
    {
        normalized.remove("description");
        let nested = parse_proposed_metadata(&desc);
        let mut combined = nested;
        for (k, v) in normalized {
            combined.insert(k, v);
        }
        normalized = combined;
    }

    for (k, v) in normalized {
        if PROTECTED_META_FIELDS.contains(&k.as_str()) {
            continue;
        }
        if k == "other_details" && v.is_object() {
            for (ok, ov) in normalize_other_details(Some(&v)) {
                other.insert(ok, ov);
            }
        } else if STANDARD_META_FIELDS.contains(&k.as_str()) {
            if !v.is_null() {
                let val = value_to_trimmed_string(&v);
                if !val.is_empty() || replace_other {
                    meta.insert(k, Value::String(val));
                }
            }
        } else {
            let key = k.trim().to_string();
            if v.is_object() {
                let mut single = Map::new();
                single.insert(key, v);
                for (ok, ov) in normalize_other_details(Some(&Value::Object(single))) {
                    other.insert(ok, ov);
                }
            } else {
                let val = value_to_trimmed_string(&v);
                if !key.is_empty() && !val.is_empty() {
                    let vis = other
                        .get(&key)
                        .and_then(|existing| existing.get("visibility"))
                        .and_then(Value::as_str)
                        .unwrap_or_else(|| default_detail_visibility(&key))
                        .to_string();
                    other.insert(key, json!({ "value": val, "visibility": vis }));
                }
            }
        }
    }
    meta.insert("other_details".to_string(), Value::Object(other));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata_helpers() {
        assert_eq!(profile_slug("Mahathir Mohamad"), "Mahathir_Mohamad");
        assert_eq!(
            profile_slug("  Azalina Othman Said! "),
            "Azalina_Othman_Said"
        );
        assert_eq!(format_identity_name("  eula   lawrence "), "Eula Lawrence");
        assert_eq!(format_identity_name("AZHAR IDRUS"), "Azhar Idrus");
        assert_eq!(format_identity_name("McDonald's"), "McDonald's");
        assert_eq!(format_time_12h("00:05"), "12:05 AM");
        assert_eq!(format_time_12h("13:45:30"), "1:45 PM");
        assert_eq!(format_time_12h("11:48 am"), "11:48 AM");
        assert_eq!(format_time_12h("unknown"), "Unknown");
        assert_eq!(default_detail_visibility("Date of Birth"), "private");
        assert_eq!(default_detail_visibility("Organization"), "public");
        assert_eq!(
            parse_proposed_metadata(r#"{"name": "Eula", "broken": "#)
                .get("name")
                .and_then(Value::as_str),
            Some("Eula")
        );
    }
}
