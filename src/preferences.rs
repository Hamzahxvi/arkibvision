#![forbid(unsafe_code)]

//! User Click & Interaction Preference Learning Engine (100% Pure Safe Rust).
//!
//! Learns what categories, sources, group folders, and topics each user interacts with
//! most frequently (with exponential time-decay weighting), lets users pin/add custom
//! preference items, and ranks related archival records accordingly.

use crate::config::AppConfig;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InteractionEvent {
    pub timestamp: String,
    pub epoch_secs: i64,
    pub event_type: String, // "click_document" | "filter_category" | "filter_source" | "filter_folder" | "search" | "export"
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub group_folder: String,
    #[serde(default)]
    pub year: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub item_slug: String,
    #[serde(default)]
    pub item_title: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UserPrefProfile {
    #[serde(default)]
    pub events: Vec<InteractionEvent>,
    #[serde(default)]
    pub pinned_categories: Vec<String>,
    #[serde(default)]
    pub pinned_sources: Vec<String>,
    #[serde(default)]
    pub pinned_folders: Vec<String>,
    #[serde(default)]
    pub custom_interests: Vec<String>,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PreferencesStore {
    #[serde(default)]
    pub users: HashMap<String, UserPrefProfile>,
}

fn load_store(path: &Path) -> PreferencesStore {
    let Ok(raw) = fs::read_to_string(path) else {
        return PreferencesStore::default();
    };
    serde_json::from_str::<PreferencesStore>(&raw).unwrap_or_default()
}

fn save_store(path: &Path, store: &PreferencesStore) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed creating prefs dir: {e}"))?;
    }
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_string_pretty(store)
        .map_err(|e| format!("Failed serializing preferences: {e}"))?;
    fs::write(&tmp, body).map_err(|e| format!("Failed writing preferences temp file: {e}"))?;
    fs::rename(&tmp, path).map_err(|e| format!("Failed replacing preferences file: {e}"))?;
    Ok(())
}

fn event_base_weight(event_type: &str) -> f64 {
    match event_type {
        "export" | "save_document" => 3.0,
        "click_document" | "edit_document" | "click_person" | "click_location" => 2.0,
        "filter_category" | "filter_source" | "filter_folder" => 1.5,
        _ => 1.0,
    }
}

fn decay_factor(now_epoch: i64, event_epoch: i64) -> f64 {
    let age_days = ((now_epoch - event_epoch).max(0) as f64) / 86_400.0;
    0.95_f64.powf(age_days)
}

pub fn track_interaction(
    cfg: &AppConfig,
    user: &str,
    mut event: InteractionEvent,
) -> Result<Value, String> {
    let now = Utc::now();
    if event.timestamp.is_empty() {
        event.timestamp = now.to_rfc3339();
    }
    if event.epoch_secs == 0 {
        event.epoch_secs = now.timestamp();
    }
    event.category = event.category.trim().to_string();
    event.source = event.source.trim().to_string();
    event.group_folder = event.group_folder.trim().to_string();
    event.year = event.year.trim().to_string();

    let mut store = load_store(&cfg.preferences_file);
    let profile = store.users.entry(user.to_string()).or_default();
    profile.events.push(event);
    // Keep last 400 interaction events per user
    if profile.events.len() > 400 {
        let drop_n = profile.events.len() - 400;
        profile.events.drain(0..drop_n);
    }
    profile.updated_at = now.to_rfc3339();
    let summary = summarize_profile(profile, now.timestamp());
    save_store(&cfg.preferences_file, &store)?;

    Ok(summary)
}

pub fn update_pinned_preferences(
    cfg: &AppConfig,
    user: &str,
    pinned_categories: Option<Vec<String>>,
    pinned_sources: Option<Vec<String>>,
    pinned_folders: Option<Vec<String>>,
    custom_interests: Option<Vec<String>>,
) -> Result<Value, String> {
    let mut store = load_store(&cfg.preferences_file);
    let profile = store.users.entry(user.to_string()).or_default();

    let clean_list = |items: Vec<String>| -> Vec<String> {
        let mut out = Vec::new();
        for item in items {
            let trimmed = item.trim().to_string();
            if !trimmed.is_empty()
                && !out
                    .iter()
                    .any(|x: &String| x.eq_ignore_ascii_case(&trimmed))
            {
                out.push(trimmed);
            }
        }
        out
    };

    if let Some(cats) = pinned_categories {
        profile.pinned_categories = clean_list(cats);
    }
    if let Some(srcs) = pinned_sources {
        profile.pinned_sources = clean_list(srcs);
    }
    if let Some(flds) = pinned_folders {
        profile.pinned_folders = clean_list(flds);
    }
    if let Some(tags) = custom_interests {
        profile.custom_interests = clean_list(tags);
    }
    let now = Utc::now();
    profile.updated_at = now.to_rfc3339();
    let summary = summarize_profile(profile, now.timestamp());
    save_store(&cfg.preferences_file, &store)?;
    Ok(summary)
}

pub fn reset_preferences(cfg: &AppConfig, user: &str) -> Result<Value, String> {
    let mut store = load_store(&cfg.preferences_file);
    store
        .users
        .insert(user.to_string(), UserPrefProfile::default());
    save_store(&cfg.preferences_file, &store)?;
    Ok(json!({ "status": "reset", "user": user }))
}

fn ranked_entries(scores: HashMap<String, (String, f64, usize)>, limit: usize) -> Vec<Value> {
    let mut list: Vec<(String, f64, usize)> = scores.into_values().collect();
    list.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| b.2.cmp(&a.2)));
    list.truncate(limit);
    list.into_iter()
        .map(|(label, score, clicks)| {
            json!({
                "value": label,
                "score": (score * 100.0).round() / 100.0,
                "clicks": clicks,
            })
        })
        .collect()
}

pub fn summarize_profile(profile: &UserPrefProfile, now_epoch: i64) -> Value {
    let mut cat_scores: HashMap<String, (String, f64, usize)> = HashMap::new();
    let mut src_scores: HashMap<String, (String, f64, usize)> = HashMap::new();
    let mut fld_scores: HashMap<String, (String, f64, usize)> = HashMap::new();
    let mut tag_scores: HashMap<String, (String, f64, usize)> = HashMap::new();

    let add_score =
        |map: &mut HashMap<String, (String, f64, usize)>, raw: &str, w: f64, click: usize| {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return;
            }
            let key = trimmed.to_ascii_lowercase();
            let entry = map
                .entry(key)
                .or_insert_with(|| (trimmed.to_string(), 0.0, 0));
            entry.1 += w;
            entry.2 += click;
        };

    for ev in &profile.events {
        let w = event_base_weight(&ev.event_type) * decay_factor(now_epoch, ev.epoch_secs);
        add_score(&mut cat_scores, &ev.category, w, 1);
        add_score(&mut src_scores, &ev.source, w, 1);
        add_score(&mut fld_scores, &ev.group_folder, w, 1);
        for t in &ev.tags {
            add_score(&mut tag_scores, t, w * 0.75, 1);
        }
    }

    // Pinned / custom preferences receive a strong boost so user-chosen items always rank at top!
    for c in &profile.pinned_categories {
        add_score(&mut cat_scores, c, 12.0, 0);
    }
    for s in &profile.pinned_sources {
        add_score(&mut src_scores, s, 12.0, 0);
    }
    for f in &profile.pinned_folders {
        add_score(&mut fld_scores, f, 12.0, 0);
    }
    for t in &profile.custom_interests {
        add_score(&mut tag_scores, t, 12.0, 0);
    }

    json!({
        "total_interactions": profile.events.len(),
        "updated_at": profile.updated_at,
        "top_categories": ranked_entries(cat_scores, 8),
        "top_sources": ranked_entries(src_scores, 8),
        "top_folders": ranked_entries(fld_scores, 8),
        "top_tags": ranked_entries(tag_scores, 10),
        "pinned_categories": profile.pinned_categories,
        "pinned_sources": profile.pinned_sources,
        "pinned_folders": profile.pinned_folders,
        "custom_interests": profile.custom_interests,
    })
}

/// Return full preferences summary + personalized related document recommendations.
pub fn get_preferences_and_recommendations(
    cfg: &AppConfig,
    user: &str,
    documents: &[Value],
) -> Value {
    let store = load_store(&cfg.preferences_file);
    let profile = store.users.get(user).cloned().unwrap_or_default();
    let now_epoch = Utc::now().timestamp();
    let summary = summarize_profile(&profile, now_epoch);

    let extract_weights = |arr: Option<&Vec<Value>>| -> HashMap<String, f64> {
        let mut m = HashMap::new();
        if let Some(items) = arr {
            for item in items {
                if let (Some(v), Some(s)) = (item["value"].as_str(), item["score"].as_f64()) {
                    m.insert(v.to_ascii_lowercase(), s);
                }
            }
        }
        m
    };

    let cat_w = extract_weights(summary["top_categories"].as_array());
    let src_w = extract_weights(summary["top_sources"].as_array());
    let fld_w = extract_weights(summary["top_folders"].as_array());
    let tag_w = extract_weights(summary["top_tags"].as_array());

    let mut scored_docs: Vec<(f64, Value)> = Vec::new();
    for doc in documents {
        let mut score = 0.0f64;
        let mut reasons: Vec<String> = Vec::new();

        let cat = doc["category"].as_str().unwrap_or("").trim();
        if let Some(&w) = cat_w.get(&cat.to_ascii_lowercase()) {
            score += w * 1.4;
            reasons.push(format!("Category: {cat}"));
        }

        let src = doc["author_or_source"].as_str().unwrap_or("").trim();
        if let Some(&w) = src_w.get(&src.to_ascii_lowercase()) {
            score += w * 1.3;
            reasons.push(format!("Source: {src}"));
        }

        let grp = doc["group"].as_str().unwrap_or("").trim();
        if let Some(&w) = fld_w.get(&grp.to_ascii_lowercase()) {
            score += w * 1.0;
            reasons.push(format!("Folder: {grp}"));
        }

        let haystack = format!(
            "{} {} {} {}",
            doc["title"].as_str().unwrap_or(""),
            doc["summary"].as_str().unwrap_or(""),
            doc["notes"].as_str().unwrap_or(""),
            doc["key_topics"]
                .as_array()
                .map(|a| a
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" "))
                .unwrap_or_default()
        )
        .to_ascii_lowercase();

        for (tag_lc, &w) in &tag_w {
            if !tag_lc.is_empty() && haystack.contains(tag_lc) {
                score += w * 0.9;
                reasons.push(format!("Interest: {tag_lc}"));
            }
        }

        if score > 0.0 {
            scored_docs.push((
                score,
                json!({
                    "slug": doc["slug"],
                    "title": doc["title"],
                    "category": doc["category"],
                    "year": doc["year"],
                    "author_or_source": doc["author_or_source"],
                    "group": doc["group"],
                    "relevance_score": (score * 10.0).round() / 10.0,
                    "match_reasons": reasons,
                }),
            ));
        }
    }

    scored_docs.sort_by(|a, b| b.0.total_cmp(&a.0));
    let recommendations: Vec<Value> = scored_docs.into_iter().take(12).map(|(_, v)| v).collect();

    // Also build a quick slug -> score lookup map for "Sort by: For You (Preferences)"
    let mut score_map = serde_json::Map::new();
    for rec in &recommendations {
        if let (Some(slug), Some(sc)) = (rec["slug"].as_str(), rec["relevance_score"].as_f64()) {
            score_map.insert(slug.to_string(), json!(sc));
        }
    }

    json!({
        "user": user,
        "preferences": summary,
        "recommended_documents": recommendations,
        "document_scores": score_map,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn temp_cfg() -> AppConfig {
        let mut cfg = AppConfig::load();
        let root =
            std::env::temp_dir().join(format!("arkib_pref_test_{}", Uuid::new_v4().simple()));
        cfg.preferences_file = root.join("user_preferences.json");
        cfg
    }

    #[test]
    fn preference_learning_ranks_related_documents() {
        let cfg = temp_cfg();
        for _ in 0..3 {
            track_interaction(
                &cfg,
                "admin",
                InteractionEvent {
                    timestamp: String::new(),
                    epoch_secs: 0,
                    event_type: "click_document".to_string(),
                    category: "Manuskrip".to_string(),
                    source: "Arkib Negara Malaysia".to_string(),
                    group_folder: "Historical Manuscripts".to_string(),
                    year: "1906".to_string(),
                    tags: vec!["jawi".to_string()],
                    item_slug: "surat_sultan".to_string(),
                    item_title: "Surat Sultan".to_string(),
                },
            )
            .expect("track");
        }
        update_pinned_preferences(
            &cfg,
            "admin",
            None,
            None,
            None,
            Some(vec!["merdeka".into()]),
        )
        .expect("pin");

        let docs = vec![
            json!({
                "slug": "doc_unrelated",
                "title": "Invoice 2024",
                "category": "Laporan",
                "author_or_source": "Vendor XYZ",
                "group": "Finance",
                "year": "2024",
                "summary": "Monthly hardware invoice"
            }),
            json!({
                "slug": "doc_match",
                "title": "Watikah Jawi Merdeka",
                "category": "Manuskrip",
                "author_or_source": "Arkib Negara Malaysia",
                "group": "Historical Manuscripts",
                "year": "1957",
                "summary": "Naskhah jawi berkaitan kemerdekaan"
            }),
        ];

        let res = get_preferences_and_recommendations(&cfg, "admin", &docs);
        let recs = res["recommended_documents"].as_array().unwrap();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0]["slug"], "doc_match");

        let _ = fs::remove_dir_all(cfg.preferences_file.parent().unwrap());
    }
}
