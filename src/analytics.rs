#![forbid(unsafe_code)]

//! User Behavior, Search History, Chatbot Query & Hot-Topics Analytics Engine
//! (100% Pure Safe Rust, zero external services, air-gapped local).

use crate::config::AppConfig;
use chrono::{Duration, Local, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BehaviorEvent {
    pub id: String,
    pub timestamp: String,
    pub epoch_secs: i64,
    pub source: String, // "chatbot_prompt" | "chatbot_tool" | "admin_search" | "item_check"
    pub actor: String,
    #[serde(default)]
    pub query_text: String,
    #[serde(default)]
    pub tool_name: String,
    #[serde(default)]
    pub feature_group: String,
    #[serde(default)]
    pub entity_type: String, // "document" | "profile" | "location" | ""
    #[serde(default)]
    pub item_slug: String,
    #[serde(default)]
    pub item_title: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

pub fn append_behavior_event(cfg: &AppConfig, ev: &BehaviorEvent) {
    if let Some(parent) = cfg.analytics_log_file.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(line) = serde_json::to_string(ev)
        && let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&cfg.analytics_log_file)
    {
        let _ = writeln!(file, "{line}");
    }
}

pub fn classify_tool_feature(tool_name: &str) -> &'static str {
    let t = tool_name.to_ascii_lowercase();
    if t.contains("face") || t.contains("identity") || t.contains("enroll") || t.contains("person")
    {
        "Biometric Identity"
    } else if t.contains("jawi") || t.contains("ancient") || t.contains("script") {
        "Jawi & Historical Scripts"
    } else if t.contains("ocr") || t.contains("document") || t.contains("summarize") {
        "Documents & OCR"
    } else if t.contains("authenticity") || t.contains("forensic") || t.contains("metadata") {
        "Authenticity Forensics"
    } else if t.contains("conservation") || t.contains("enhance") || t.contains("crop") {
        "Conservation Lab"
    } else if t.contains("location") {
        "Archival Locations"
    } else if t.is_empty() {
        "General Chat"
    } else {
        "Archival Tools"
    }
}

/// Automatically record any MCP tool invocation triggered by a user's chatbot message.
pub fn record_mcp_tool_usage(cfg: &AppConfig, tool_name: &str, args: &Value) {
    let now = Utc::now();
    let str_arg = |keys: &[&str]| -> String {
        for k in keys {
            if let Some(s) = args.get(*k).and_then(Value::as_str) {
                let t = s.trim();
                if !t.is_empty() {
                    return t.chars().take(240).collect();
                }
            }
        }
        String::new()
    };

    let query = str_arg(&[
        "query",
        "identity",
        "text",
        "jawi_text",
        "document_title",
        "location_name",
        "script",
    ]);
    let category = str_arg(&["category", "script_filter", "script_type"]);
    let item_title = str_arg(&["identity", "document_title", "location_name", "slug"]);
    let feature = classify_tool_feature(tool_name);

    let mut tags = Vec::new();
    if !category.is_empty() {
        tags.push(category.clone());
    }
    if tool_name.contains("jawi") {
        tags.push("Jawi".to_string());
    } else if tool_name.contains("face") || tool_name.contains("identity") {
        tags.push("Identity".to_string());
    } else if tool_name.contains("authenticity") {
        tags.push("Authenticity".to_string());
    } else if tool_name.contains("conservation") {
        tags.push("Conservation".to_string());
    }

    let ev = BehaviorEvent {
        id: format!("bhv_{}", &Uuid::new_v4().simple().to_string()[..10]),
        timestamp: now.to_rfc3339(),
        epoch_secs: now.timestamp(),
        source: "chatbot_tool".to_string(),
        actor: "chatbot_user".to_string(),
        query_text: query,
        tool_name: tool_name.to_string(),
        feature_group: feature.to_string(),
        entity_type: if tool_name.contains("document") {
            "document".to_string()
        } else if tool_name.contains("location") {
            "location".to_string()
        } else if tool_name.contains("identity") || tool_name.contains("face") {
            "profile".to_string()
        } else {
            String::new()
        },
        item_slug: str_arg(&["slug", "document_slug"]),
        item_title,
        category,
        tags,
    };
    append_behavior_event(cfg, &ev);
}

/// Record a natural-language chatbot prompt sent by a user in LibreChat.
pub fn record_chat_prompt(cfg: &AppConfig, prompt: &str, actor: &str) {
    let clean: String = prompt.trim().chars().take(280).collect();
    if clean.len() < 2 {
        return;
    }
    let now = Utc::now();
    let lower = clean.to_ascii_lowercase();
    let feature = if lower.contains("jawi") || lower.contains("rumi") || lower.contains("manuskrip")
    {
        "Jawi & Historical Scripts"
    } else if lower.contains("face")
        || lower.contains("siapa")
        || lower.contains("who is")
        || lower.contains("identify")
        || lower.contains("wajah")
        || lower.contains("enroll")
    {
        "Biometric Identity"
    } else if lower.contains("fake")
        || lower.contains("authentic")
        || lower.contains("tul")
        || lower.contains("tamper")
        || lower.contains("forensic")
    {
        "Authenticity Forensics"
    } else if lower.contains("mold")
        || lower.contains("tear")
        || lower.contains("restore")
        || lower.contains("enhance")
        || lower.contains("pulihara")
    {
        "Conservation Lab"
    } else if lower.contains("location")
        || lower.contains("dewan")
        || lower.contains("building")
        || lower.contains("lokasi")
    {
        "Archival Locations"
    } else if lower.contains("doc")
        || lower.contains("surat")
        || lower.contains("pdf")
        || lower.contains("ocr")
        || lower.contains("ringkas")
        || lower.contains("summary")
    {
        "Documents & OCR"
    } else {
        "General Chat"
    };

    let ev = BehaviorEvent {
        id: format!("bhv_{}", &Uuid::new_v4().simple().to_string()[..10]),
        timestamp: now.to_rfc3339(),
        epoch_secs: now.timestamp(),
        source: "chatbot_prompt".to_string(),
        actor: if actor.trim().is_empty() {
            "chatbot_user".to_string()
        } else {
            actor.trim().to_string()
        },
        query_text: clean,
        tool_name: String::new(),
        feature_group: feature.to_string(),
        entity_type: String::new(),
        item_slug: String::new(),
        item_title: String::new(),
        category: String::new(),
        tags: Vec::new(),
    };
    append_behavior_event(cfg, &ev);
}

#[allow(clippy::too_many_arguments)]
pub fn record_admin_activity(
    cfg: &AppConfig,
    source: &str, // "admin_search" | "item_check"
    actor: &str,
    query_text: &str,
    entity_type: &str,
    item_slug: &str,
    item_title: &str,
    category: &str,
    tags: Vec<String>,
) {
    let now = Utc::now();
    let ev = BehaviorEvent {
        id: format!("bhv_{}", &Uuid::new_v4().simple().to_string()[..10]),
        timestamp: now.to_rfc3339(),
        epoch_secs: now.timestamp(),
        source: source.to_string(),
        actor: actor.to_string(),
        query_text: query_text.trim().chars().take(200).collect(),
        tool_name: String::new(),
        feature_group: match entity_type {
            "document" => "Documents & OCR",
            "profile" => "Biometric Identity",
            "location" => "Archival Locations",
            _ => "Admin Search",
        }
        .to_string(),
        entity_type: entity_type.to_string(),
        item_slug: item_slug.to_string(),
        item_title: item_title.to_string(),
        category: category.to_string(),
        tags,
    };
    append_behavior_event(cfg, &ev);
}

pub fn load_behavior_events(cfg: &AppConfig, max_events: usize) -> Vec<BehaviorEvent> {
    let Ok(raw) = fs::read_to_string(&cfg.analytics_log_file) else {
        return Vec::new();
    };
    let mut events: Vec<BehaviorEvent> = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<BehaviorEvent>(l).ok())
        .collect();
    if events.len() > max_events {
        let drop_n = events.len() - max_events;
        events.drain(0..drop_n);
    }
    events
}

fn is_stopword(w: &str) -> bool {
    const STOPS: &[&str] = &[
        // English
        "the", "and", "for", "with", "that", "this", "from", "are", "was", "were", "what", "who",
        "where", "when", "why", "how", "can", "you", "please", "tell", "show", "find", "search",
        "about", "into", "have", "has", "had", "not", "but", "all", "any", "get", "check", "read",
        "image", "file", "photo", "picture", "text", "translate", "convert", "open", "list",
        "give", "help", "make", "need", "want", "more", "some", "there", "their", "they", "them",
        "will", "would", "could", "should", "does", "did", "been", "being", "than", "then", "also",
        // Bahasa Malaysia
        "yang", "dan", "ini", "itu", "untuk", "dengan", "pada", "dari", "daripada", "dalam", "ada",
        "tidak", "atau", "juga", "akan", "telah", "boleh", "sila", "tolong", "baca", "cari",
        "tunjuk", "siapa", "apa", "bila", "mana", "bagaimana", "kenapa", "mengapa", "saya", "kami",
        "kita", "anda", "mereka", "gambar", "dokumen", "fail", "teks", "tukar", "semak", "lihat",
        "lagi", "satu", "dua", "semua", "bagi", "oleh", "kerana", "sebagai", "kepada", "tentang",
    ];
    STOPS.contains(&w)
}

fn extract_topic_tokens(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() || ('\u{0600}'..='\u{06FF}').contains(&ch) {
            for lc in ch.to_lowercase() {
                cur.push(lc);
            }
        } else if !cur.is_empty() {
            if cur.chars().count() >= 3 && !cur.chars().all(|c| c.is_ascii_digit()) && !is_stopword(&cur) {
                words.push(cur.clone());
            }
            cur.clear();
        }
    }
    if !cur.is_empty()
        && cur.chars().count() >= 3
        && !cur.chars().all(|c| c.is_ascii_digit())
        && !is_stopword(&cur)
    {
        words.push(cur);
    }

    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for w in &words {
        if seen.insert(w.clone()) {
            out.push(w.clone());
        }
    }
    // Add meaningful 2-word phrases (bigrams) for crisp archival topics like "surat sultan"
    for win in words.windows(2) {
        let phrase = format!("{} {}", win[0], win[1]);
        if seen.insert(phrase.clone()) {
            out.push(phrase);
        }
    }
    out
}

fn recency_weight(now_epoch: i64, ev_epoch: i64) -> f64 {
    let days = ((now_epoch - ev_epoch).max(0) as f64) / 86_400.0;
    0.92_f64.powf(days)
}

fn pretty_topic(raw: &str) -> String {
    raw.split_whitespace()
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(c) => format!("{}{}", c.to_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Build full behavioral analytics payload (hot topics, chatbot questions, search history,
/// checked items, feature breakdown, 7-day trend graph data, and admin insights).
pub fn build_behavior_analytics(cfg: &AppConfig) -> Value {
    let events = load_behavior_events(cfg, 2500);
    let now = Utc::now();
    let now_epoch = now.timestamp();

    let mut chatbot_count = 0usize;
    let mut search_count = 0usize;
    let mut item_check_count = 0usize;

    // topic -> (display, count, recency_score, recent_24h_count)
    let mut topic_map: HashMap<String, (String, usize, f64, usize)> = HashMap::new();
    // normalized question -> (display, count, recency_score, last_seen, feature)
    let mut chat_q_map: HashMap<String, (String, usize, f64, String, String)> = HashMap::new();
    // normalized search -> (display, count, recency_score, last_seen, source)
    let mut search_map: HashMap<String, (String, usize, f64, String, String)> = HashMap::new();
    struct ItemStat {
        entity_type: String,
        slug: String,
        title: String,
        category: String,
        count: usize,
        score: f64,
        last_seen: String,
    }
    let mut item_map: HashMap<String, ItemStat> = HashMap::new();
    let mut feature_counts: HashMap<String, usize> = HashMap::new();
    let mut category_counts: HashMap<String, usize> = HashMap::new();

    // 7-day local date buckets
    let today_local = Local::now().date_naive();
    let mut day_buckets: Vec<(String, String, usize, usize, usize)> = (0..7)
        .rev()
        .map(|offset| {
            let d = today_local - Duration::days(offset);
            (
                d.format("%Y-%m-%d").to_string(),
                d.format("%d %b").to_string(),
                0,
                0,
                0,
            )
        })
        .collect();

    for ev in &events {
        let w = recency_weight(now_epoch, ev.epoch_secs);
        let is_24h = (now_epoch - ev.epoch_secs) <= 86_400;

        let local_day = Local
            .timestamp_opt(ev.epoch_secs, 0)
            .single()
            .map(|dt| dt.format("%Y-%m-%d").to_string())
            .unwrap_or_default();

        match ev.source.as_str() {
            "chatbot_prompt" | "chatbot_tool" => {
                chatbot_count += 1;
                if let Some(b) = day_buckets.iter_mut().find(|b| b.0 == local_day) {
                    b.2 += 1;
                }
            }
            "admin_search" => {
                search_count += 1;
                if let Some(b) = day_buckets.iter_mut().find(|b| b.0 == local_day) {
                    b.3 += 1;
                }
            }
            _ => {
                item_check_count += 1;
                if let Some(b) = day_buckets.iter_mut().find(|b| b.0 == local_day) {
                    b.4 += 1;
                }
            }
        }

        if !ev.feature_group.trim().is_empty() {
            *feature_counts.entry(ev.feature_group.clone()).or_insert(0) += 1;
        }
        if !ev.category.trim().is_empty() {
            *category_counts.entry(ev.category.clone()).or_insert(0) += 1;
        }

        // Extract hot topics from query_text, item_title, category, and tags
        let mut combined_topics = Vec::new();
        if !ev.query_text.trim().is_empty() {
            combined_topics.extend(extract_topic_tokens(&ev.query_text));
        }
        if !ev.item_title.trim().is_empty() {
            combined_topics.extend(extract_topic_tokens(&ev.item_title));
        }
        if !ev.category.trim().is_empty() {
            combined_topics.push(ev.category.trim().to_ascii_lowercase());
        }
        for t in &ev.tags {
            let tc = t.trim().to_ascii_lowercase();
            if tc.len() >= 2 {
                combined_topics.push(tc);
            }
        }
        let mut dedup_ev_topics = HashSet::new();
        for tok in combined_topics {
            if !dedup_ev_topics.insert(tok.clone()) {
                continue;
            }
            let entry = topic_map
                .entry(tok.clone())
                .or_insert_with(|| (pretty_topic(&tok), 0, 0.0, 0));
            entry.1 += 1;
            entry.2 += w;
            if is_24h {
                entry.3 += 1;
            }
        }

        // Track chatbot questions
        if matches!(ev.source.as_str(), "chatbot_prompt" | "chatbot_tool")
            && !ev.query_text.trim().is_empty()
        {
            let norm = ev.query_text.trim().to_ascii_lowercase();
            let entry = chat_q_map.entry(norm).or_insert_with(|| {
                (
                    ev.query_text.trim().to_string(),
                    0,
                    0.0,
                    ev.timestamp.clone(),
                    ev.feature_group.clone(),
                )
            });
            entry.1 += 1;
            entry.2 += w;
            entry.3 = ev.timestamp.clone();
        }

        // Track search queries
        if (ev.source == "admin_search" || ev.tool_name.contains("search"))
            && !ev.query_text.trim().is_empty()
        {
            let norm = ev.query_text.trim().to_ascii_lowercase();
            let entry = search_map.entry(norm).or_insert_with(|| {
                (
                    ev.query_text.trim().to_string(),
                    0,
                    0.0,
                    ev.timestamp.clone(),
                    if ev.source == "admin_search" {
                        "Admin Portal".to_string()
                    } else {
                        "Chatbot".to_string()
                    },
                )
            });
            entry.1 += 1;
            entry.2 += w;
            entry.3 = ev.timestamp.clone();
        }

        // Track most frequently checked items
        if !ev.item_slug.trim().is_empty() || !ev.item_title.trim().is_empty() {
            let key = format!(
                "{}:{}",
                ev.entity_type,
                if !ev.item_slug.is_empty() {
                    ev.item_slug.to_ascii_lowercase()
                } else {
                    ev.item_title.to_ascii_lowercase()
                }
            );
            let title = if !ev.item_title.is_empty() {
                ev.item_title.clone()
            } else {
                ev.item_slug.clone()
            };
            let entry = item_map.entry(key).or_insert_with(|| ItemStat {
                entity_type: if ev.entity_type.is_empty() {
                    "document".to_string()
                } else {
                    ev.entity_type.clone()
                },
                slug: ev.item_slug.clone(),
                title,
                category: ev.category.clone(),
                count: 0,
                score: 0.0,
                last_seen: ev.timestamp.clone(),
            });
            entry.count += 1;
            entry.score += w;
            entry.last_seen = ev.timestamp.clone();
        }
    }

    let mut hot_topics: Vec<Value> = topic_map
        .into_values()
        .map(|(topic, count, score, recent_24h)| {
            json!({
                "topic": topic,
                "count": count,
                "score": (score * 100.0).round() / 100.0,
                "recent_24h": recent_24h,
            })
        })
        .collect();
    hot_topics.sort_by(|a, b| {
        b["score"]
            .as_f64()
            .unwrap_or(0.0)
            .total_cmp(&a["score"].as_f64().unwrap_or(0.0))
            .then_with(|| {
                b["count"]
                    .as_u64()
                    .unwrap_or(0)
                    .cmp(&a["count"].as_u64().unwrap_or(0))
            })
    });
    hot_topics.truncate(12);

    let mut top_chatbot_questions: Vec<Value> = chat_q_map
        .into_values()
        .map(|(q, count, score, last_seen, feature)| {
            json!({
                "question": q,
                "count": count,
                "score": (score * 100.0).round() / 100.0,
                "last_seen": last_seen,
                "feature": feature,
            })
        })
        .collect();
    top_chatbot_questions.sort_by(|a, b| {
        b["score"]
            .as_f64()
            .unwrap_or(0.0)
            .total_cmp(&a["score"].as_f64().unwrap_or(0.0))
    });
    top_chatbot_questions.truncate(12);

    let mut top_searches: Vec<Value> = search_map
        .into_values()
        .map(|(q, count, score, last_seen, channel)| {
            json!({
                "query": q,
                "count": count,
                "score": (score * 100.0).round() / 100.0,
                "last_seen": last_seen,
                "channel": channel,
            })
        })
        .collect();
    top_searches.sort_by(|a, b| {
        b["score"]
            .as_f64()
            .unwrap_or(0.0)
            .total_cmp(&a["score"].as_f64().unwrap_or(0.0))
    });
    top_searches.truncate(12);

    let mut top_checked_items: Vec<Value> = item_map
        .into_values()
        .map(|it| {
            json!({
                "entity_type": it.entity_type,
                "slug": it.slug,
                "title": it.title,
                "category": it.category,
                "count": it.count,
                "score": (it.score * 100.0).round() / 100.0,
                "last_seen": it.last_seen,
            })
        })
        .collect();
    top_checked_items.sort_by(|a, b| {
        b["score"]
            .as_f64()
            .unwrap_or(0.0)
            .total_cmp(&a["score"].as_f64().unwrap_or(0.0))
    });
    top_checked_items.truncate(10);

    let mut tool_usage: Vec<Value> = feature_counts
        .into_iter()
        .map(|(name, count)| json!({ "name": name, "count": count }))
        .collect();
    tool_usage.sort_by(|a, b| {
        b["count"]
            .as_u64()
            .unwrap_or(0)
            .cmp(&a["count"].as_u64().unwrap_or(0))
    });

    let mut category_interest: Vec<Value> = category_counts
        .into_iter()
        .map(|(name, count)| json!({ "name": name, "count": count }))
        .collect();
    category_interest.sort_by(|a, b| {
        b["count"]
            .as_u64()
            .unwrap_or(0)
            .cmp(&a["count"].as_u64().unwrap_or(0))
    });

    let daily_trend: Vec<Value> = day_buckets
        .into_iter()
        .map(|(date, label, chatbot, searches, views)| {
            json!({
                "date": date,
                "label": label,
                "chatbot": chatbot,
                "searches": searches,
                "item_views": views,
                "total": chatbot + searches + views,
            })
        })
        .collect();

    let mut recent_history = events.clone();
    recent_history.reverse();
    recent_history.truncate(40);

    // Build clear, plain-language actionable insights for admins
    let mut admin_insights: Vec<String> = Vec::new();
    if let Some(top_t) = hot_topics.first() {
        admin_insights.push(format!(
            "Hot Topic: \"{}\" is currently the #1 trending topic ({} mentions, {} in the last 24h). Consider featuring or tagging related records.",
            top_t["topic"].as_str().unwrap_or(""),
            top_t["count"].as_u64().unwrap_or(0),
            top_t["recent_24h"].as_u64().unwrap_or(0)
        ));
    }
    if let Some(top_f) = tool_usage.first() {
        admin_insights.push(format!(
            "Most Used Capability: \"{}\" leads user demand ({} interactions).",
            top_f["name"].as_str().unwrap_or(""),
            top_f["count"].as_u64().unwrap_or(0)
        ));
    }
    if let Some(top_item) = top_checked_items.first() {
        admin_insights.push(format!(
            "Most Checked Record: \"{}\" ({}) has been opened {} time(s) recently.",
            top_item["title"].as_str().unwrap_or(""),
            top_item["entity_type"].as_str().unwrap_or("record"),
            top_item["count"].as_u64().unwrap_or(0)
        ));
    }
    if admin_insights.is_empty() {
        admin_insights.push(
            "Ask questions in the chatbot or search/open records in the Admin Portal — hot topics, search history, and graphs update automatically in real time.".to_string(),
        );
    }

    json!({
        "generated_at": now.to_rfc3339(),
        "summary": {
            "total_events": events.len(),
            "chatbot_questions": chatbot_count,
            "search_queries": search_count,
            "item_checks": item_check_count,
            "hot_topic_count": hot_topics.len(),
        },
        "hot_topics": hot_topics,
        "top_chatbot_questions": top_chatbot_questions,
        "top_searches": top_searches,
        "top_checked_items": top_checked_items,
        "tool_usage": tool_usage,
        "category_interest": category_interest,
        "daily_trend": daily_trend,
        "recent_history": recent_history,
        "admin_insights": admin_insights,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_cfg() -> AppConfig {
        let mut cfg = AppConfig::load();
        let root =
            std::env::temp_dir().join(format!("arkib_ana_test_{}", Uuid::new_v4().simple()));
        cfg.analytics_log_file = root.join("behavior_analytics.jsonl");
        cfg
    }

    #[test]
    fn behavior_analytics_ranks_hot_topics_and_chatbot_questions() {
        let cfg = temp_cfg();
        record_chat_prompt(&cfg, "Tolong baca manuskrip Jawi Surat Sultan 1324H", "guest");
        record_chat_prompt(&cfg, "Apa isi Surat Sultan 1324H dalam Jawi?", "guest");
        record_mcp_tool_usage(
            &cfg,
            "arkib_search_documents",
            &json!({ "query": "Surat Sultan", "script_filter": "jawi" }),
        );
        record_admin_activity(
            &cfg,
            "item_check",
            "admin",
            "",
            "document",
            "surat_sultan",
            "Surat Sultan 1324H",
            "Manuskrip",
            vec!["jawi".into()],
        );

        let res = build_behavior_analytics(&cfg);
        assert_eq!(res["summary"]["total_events"], 4);
        assert_eq!(res["summary"]["chatbot_questions"], 3);
        let topics = res["hot_topics"].as_array().unwrap();
        assert!(!topics.is_empty());
        assert!(topics
            .iter()
            .any(|t| t["topic"].as_str().unwrap_or("").to_ascii_lowercase().contains("jawi")
                || t["topic"].as_str().unwrap_or("").to_ascii_lowercase().contains("surat")));
        assert_eq!(res["daily_trend"].as_array().unwrap().len(), 7);

        let _ = fs::remove_dir_all(cfg.analytics_log_file.parent().unwrap());
    }
}
