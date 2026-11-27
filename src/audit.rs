#![forbid(unsafe_code)]

//! Audit Management, Deterministic Rules Engine, Statistical Outlier Detection,
//! SQL/Query Inspection & AI Anomaly Explanation (100% Pure Safe Rust).

use crate::config::{AppConfig, OLLAMA_UNIFORM_NUM_CTX};
use chrono::{Local, TimeZone, Timelike, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::OnceLock;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: String,
    pub timestamp: String,
    pub epoch_secs: i64,
    pub actor: String,
    pub role: String,
    pub ip: String,
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub query: String,
    pub action: String, // "READ" | "CREATE" | "UPDATE" | "DELETE" | "EXPORT" | "LOGIN_OK" | "LOGIN_FAIL" | "SQL_QUERY"
    pub resource_type: String,
    pub resource_id: String,
    pub status_code: u16,
    #[serde(default)]
    pub payload_preview: String,
    #[serde(default)]
    pub duration_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuleAlert {
    pub id: String,
    pub rule_id: String,
    pub severity: String, // "CRITICAL" | "HIGH" | "MEDIUM" | "LOW"
    pub category: String, // "CRUD" | "AUTH" | "SQL" | "ACCESS" | "INJECTION"
    pub title: String,
    pub description: String,
    pub actor: String,
    pub event_count: usize,
    pub timestamp: String,
    pub evidence: Vec<String>,
    pub recommended_action: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutlierFinding {
    pub id: String,
    pub metric: String,
    pub severity: String,
    pub actor: String,
    pub observed_value: f64,
    pub baseline_mean: f64,
    pub baseline_std: f64,
    pub z_score: f64,
    pub explanation: String,
    pub timestamp: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuditReportSnapshot {
    pub generated_at: String,
    pub total_events_analyzed: usize,
    pub risk_level: String, // "LOW" | "MEDIUM" | "HIGH" | "CRITICAL"
    pub risk_score: u32,    // 0..100
    pub severity_counts: HashMap<String, usize>,
    pub rule_alerts: Vec<RuleAlert>,
    pub statistical_outliers: Vec<OutlierFinding>,
    pub recent_events: Vec<AuditEvent>,
    pub top_actors: Vec<Value>,
}

fn sqli_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)(\bunion\s+(all\s+)?select\b|'\s*or\s+'?1'?\s*=\s*'?1|\bdrop\s+(table|database|schema)\b|\btruncate\s+table\b|\binformation_schema\b|\bsleep\s*\(|\bbenchmark\s*\(|;\s*--|/\*.*?\*/|\bxp_cmdshell\b)")
            .expect("valid sqli regex")
    })
}

fn nosql_or_traversal_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)(\$where\b|\$ne\b|\$gt\b|\$regex\b|\.\./|\.\.\\|%2e%2e|%252e%252e)")
            .expect("valid nosql/traversal regex")
    })
}

fn destructive_sql_no_where_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^\s*(delete\s+from\s+\S+|update\s+\S+\s+set\s+.+?)(;|\s*$)")
            .expect("valid destructive sql regex")
    })
}

fn ddl_or_priv_sql_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\b(drop\s+table|drop\s+database|truncate\s+table|alter\s+table|grant\s+all|create\s+user|revoke\s+)\b")
            .expect("valid ddl sql regex")
    })
}

fn bulk_select_star_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\bselect\s+\*\s+from\s+([a-zA-Z0-9_.\-]+)")
            .expect("valid select star regex")
    })
}

pub fn append_audit_event(cfg: &AppConfig, event: &AuditEvent) {
    if let Some(parent) = cfg.audit_log_file.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(line) = serde_json::to_string(event)
        && let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&cfg.audit_log_file)
    {
        let _ = writeln!(file, "{line}");
    }
}

#[allow(clippy::too_many_arguments)]
pub fn log_event(
    cfg: &AppConfig,
    actor: &str,
    role: &str,
    ip: &str,
    method: &str,
    path: &str,
    query: &str,
    action: &str,
    resource_type: &str,
    resource_id: &str,
    status_code: u16,
    payload_preview: &str,
    duration_ms: u64,
) {
    let now = Utc::now();
    let preview: String = payload_preview.chars().take(240).collect();
    let ev = AuditEvent {
        id: format!("aud_{}", &Uuid::new_v4().simple().to_string()[..10]),
        timestamp: now.to_rfc3339(),
        epoch_secs: now.timestamp(),
        actor: actor.to_string(),
        role: role.to_string(),
        ip: ip.to_string(),
        method: method.to_string(),
        path: path.to_string(),
        query: query.chars().take(300).collect(),
        action: action.to_string(),
        resource_type: resource_type.to_string(),
        resource_id: resource_id.to_string(),
        status_code,
        payload_preview: preview,
        duration_ms,
    };
    append_audit_event(cfg, &ev);
}

pub fn load_audit_events(path: &Path, max_events: usize) -> Vec<AuditEvent> {
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut events: Vec<AuditEvent> = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<AuditEvent>(l).ok())
        .collect();
    if events.len() > max_events {
        let drop_n = events.len() - max_events;
        events.drain(0..drop_n);
    }
    events
}

/// Layer 1: Deterministic Rules Engine across live ArkibVision CRUD/HTTP telemetry & SQL logs.
pub fn evaluate_rules(events: &[AuditEvent]) -> Vec<RuleAlert> {
    let mut alerts = Vec::new();

    // Group events by actor for burst detection
    let mut deletes_by_actor: HashMap<&str, Vec<&AuditEvent>> = HashMap::new();
    let mut exports_by_actor: HashMap<&str, Vec<&AuditEvent>> = HashMap::new();
    let mut writes_by_actor: HashMap<&str, Vec<&AuditEvent>> = HashMap::new();
    let mut login_fails_by_key: HashMap<String, Vec<&AuditEvent>> = HashMap::new();
    let mut off_hours_writes: Vec<&AuditEvent> = Vec::new();

    for ev in events {
        let combined_input = format!("{} {} {}", ev.path, ev.query, ev.payload_preview);

        // R005: SQL Injection pattern in HTTP query / payload / SQL log
        if sqli_regex().is_match(&combined_input) {
            alerts.push(RuleAlert {
                id: format!("alt_{}", &Uuid::new_v4().simple().to_string()[..8]),
                rule_id: "R005_SQL_INJECTION_PATTERN".to_string(),
                severity: "CRITICAL".to_string(),
                category: "SQL".to_string(),
                title: "Suspected SQL Injection / Destructive SQL Pattern".to_string(),
                description: format!(
                    "Detected SQL injection or destructive SQL signature from '{}' on '{}'.",
                    ev.actor, ev.path
                ),
                actor: ev.actor.clone(),
                event_count: 1,
                timestamp: ev.timestamp.clone(),
                evidence: vec![combined_input.chars().take(180).collect()],
                recommended_action: "Verify actor identity, block source IP if unauthorized, and inspect affected archival tables/files.".to_string(),
            });
        }

        // R006: NoSQL injection or path traversal probe
        if nosql_or_traversal_regex().is_match(&combined_input) {
            alerts.push(RuleAlert {
                id: format!("alt_{}", &Uuid::new_v4().simple().to_string()[..8]),
                rule_id: "R006_NOSQL_OR_PATH_TRAVERSAL".to_string(),
                severity: "HIGH".to_string(),
                category: "INJECTION".to_string(),
                title: "NoSQL Operator or Directory Traversal Probe".to_string(),
                description: format!(
                    "Detected suspicious traversal/NoSQL operator payload from '{}' targeting '{}'.",
                    ev.actor, ev.path
                ),
                actor: ev.actor.clone(),
                event_count: 1,
                timestamp: ev.timestamp.clone(),
                evidence: vec![combined_input.chars().take(180).collect()],
                recommended_action: "Audit origin session and confirm sandbox/path-guard blocked file escape.".to_string(),
            });
        }

        // R007: External SQL log inspection (missing WHERE, DDL, bulk SELECT *)
        if ev.action == "SQL_QUERY" {
            let sql = ev.payload_preview.trim();
            let lower = sql.to_ascii_lowercase();
            if destructive_sql_no_where_regex().is_match(sql) && !lower.contains(" where ") {
                alerts.push(RuleAlert {
                    id: format!("alt_{}", &Uuid::new_v4().simple().to_string()[..8]),
                    rule_id: "R007_ABNORMAL_SQL_QUERY".to_string(),
                    severity: "CRITICAL".to_string(),
                    category: "SQL".to_string(),
                    title: "Unbounded Destructive SQL (Missing WHERE Clause)".to_string(),
                    description: format!(
                        "SQL statement by '{}' attempts DELETE/UPDATE without a WHERE filter.",
                        ev.actor
                    ),
                    actor: ev.actor.clone(),
                    event_count: 1,
                    timestamp: ev.timestamp.clone(),
                    evidence: vec![sql.chars().take(180).collect()],
                    recommended_action: "Halt query execution immediately and restore from verified archival backup if committed.".to_string(),
                });
            } else if ddl_or_priv_sql_regex().is_match(sql) {
                alerts.push(RuleAlert {
                    id: format!("alt_{}", &Uuid::new_v4().simple().to_string()[..8]),
                    rule_id: "R007_ABNORMAL_SQL_QUERY".to_string(),
                    severity: "HIGH".to_string(),
                    category: "SQL".to_string(),
                    title: "Privileged DDL / Schema-Altering SQL Query".to_string(),
                    description: format!(
                        "Detected schema or privilege modification SQL statement by '{}'.",
                        ev.actor
                    ),
                    actor: ev.actor.clone(),
                    event_count: 1,
                    timestamp: ev.timestamp.clone(),
                    evidence: vec![sql.chars().take(180).collect()],
                    recommended_action: "Confirm whether an approved maintenance change ticket exists for this DDL change.".to_string(),
                });
            } else if bulk_select_star_regex().is_match(sql) && !lower.contains(" limit ") {
                alerts.push(RuleAlert {
                    id: format!("alt_{}", &Uuid::new_v4().simple().to_string()[..8]),
                    rule_id: "R007_ABNORMAL_SQL_QUERY".to_string(),
                    severity: "MEDIUM".to_string(),
                    category: "SQL".to_string(),
                    title: "Unbounded Full-Table SELECT * Extraction".to_string(),
                    description: format!(
                        "Actor '{}' ran SELECT * without LIMIT — potential bulk data exfiltration.",
                        ev.actor
                    ),
                    actor: ev.actor.clone(),
                    event_count: 1,
                    timestamp: ev.timestamp.clone(),
                    evidence: vec![sql.chars().take(180).collect()],
                    recommended_action: "Review query justification and enforce row limits / column masking on private fields.".to_string(),
                });
            }
        }

        if ev.action == "DELETE" {
            deletes_by_actor.entry(&ev.actor).or_default().push(ev);
        }
        if ev.action == "EXPORT" {
            exports_by_actor.entry(&ev.actor).or_default().push(ev);
        }
        if matches!(ev.action.as_str(), "CREATE" | "UPDATE" | "DELETE") {
            writes_by_actor.entry(&ev.actor).or_default().push(ev);
            if let Some(local_dt) = Local.timestamp_opt(ev.epoch_secs, 0).single()
                && local_dt.hour() < 5
            {
                off_hours_writes.push(ev);
            }
        }
        if ev.action == "LOGIN_FAIL" || ev.status_code == 401 {
            let key = format!("{}@{}", ev.actor, ev.ip);
            login_fails_by_key.entry(key).or_default().push(ev);
        }
    }

    fn find_burst<'a>(
        list: &[&'a AuditEvent],
        window_secs: i64,
        threshold: usize,
    ) -> Option<Vec<&'a AuditEvent>> {
        if list.len() < threshold {
            return None;
        }
        let mut sorted = list.to_vec();
        sorted.sort_by_key(|e| e.epoch_secs);
        let mut best: Vec<&'a AuditEvent> = Vec::new();
        let mut left = 0usize;
        for right in 0..sorted.len() {
            while sorted[right].epoch_secs - sorted[left].epoch_secs > window_secs {
                left += 1;
            }
            if right - left + 1 > best.len() {
                best = sorted[left..=right].to_vec();
            }
        }
        if best.len() >= threshold {
            Some(best)
        } else {
            None
        }
    }

    // R001: Suspicious DELETE burst (>= 3 deletes in 5 minutes)
    for (actor, list) in deletes_by_actor {
        if let Some(burst) = find_burst(&list, 300, 3) {
            alerts.push(RuleAlert {
                id: format!("alt_{}", &Uuid::new_v4().simple().to_string()[..8]),
                rule_id: "R001_SUSPICIOUS_DELETE_BURST".to_string(),
                severity: "HIGH".to_string(),
                category: "CRUD".to_string(),
                title: format!("Suspicious Bulk Deletion Burst ({} deletes)", burst.len()),
                description: format!(
                    "Actor '{actor}' performed {} DELETE operations within 5 minutes.",
                    burst.len()
                ),
                actor: actor.to_string(),
                event_count: burst.len(),
                timestamp: burst.last().map(|e| e.timestamp.clone()).unwrap_or_default(),
                evidence: burst
                    .iter()
                    .take(5)
                    .map(|e| format!("{} {} ({})", e.method, e.path, e.timestamp))
                    .collect(),
                recommended_action: "Verify deletion authorization immediately and check Change History / backups for recovery.".to_string(),
            });
        }
    }

    // R002: Bulk export/download burst (>= 5 exports in 5 minutes)
    for (actor, list) in exports_by_actor {
        if let Some(burst) = find_burst(&list, 300, 5) {
            alerts.push(RuleAlert {
                id: format!("alt_{}", &Uuid::new_v4().simple().to_string()[..8]),
                rule_id: "R002_BULK_EXPORT_BURST".to_string(),
                severity: "MEDIUM".to_string(),
                category: "ACCESS".to_string(),
                title: format!("Unusual Bulk Export Burst ({} exports)", burst.len()),
                description: format!(
                    "Actor '{actor}' exported {} archival records within 5 minutes.",
                    burst.len()
                ),
                actor: actor.to_string(),
                event_count: burst.len(),
                timestamp: burst.last().map(|e| e.timestamp.clone()).unwrap_or_default(),
                evidence: burst
                    .iter()
                    .take(5)
                    .map(|e| format!("{} ({})", e.path, e.timestamp))
                    .collect(),
                recommended_action: "Confirm researcher/admin export clearance and review exported document sensitivity.".to_string(),
            });
        }
    }

    // R003: Auth brute force (>= 3 failed logins / 401s in 10 minutes)
    for (actor_ip, list) in login_fails_by_key {
        if let Some(burst) = find_burst(&list, 600, 3) {
            alerts.push(RuleAlert {
                id: format!("alt_{}", &Uuid::new_v4().simple().to_string()[..8]),
                rule_id: "R003_AUTH_BRUTE_FORCE".to_string(),
                severity: "CRITICAL".to_string(),
                category: "AUTH".to_string(),
                title: format!("Repeated Authentication Failures ({} attempts)", burst.len()),
                description: format!(
                    "Detected {} failed login or unauthorized access attempts from '{actor_ip}' within 10 minutes.",
                    burst.len()
                ),
                actor: actor_ip,
                event_count: burst.len(),
                timestamp: burst.last().map(|e| e.timestamp.clone()).unwrap_or_default(),
                evidence: burst
                    .iter()
                    .take(5)
                    .map(|e| format!("HTTP {} on {} ({})", e.status_code, e.path, e.timestamp))
                    .collect(),
                recommended_action: "Rotate admin credentials if targeted and verify network/localhost access controls.".to_string(),
            });
        }
    }

    // R008: Rapid CRUD mutation burst (>= 10 writes in 3 minutes)
    for (actor, list) in writes_by_actor {
        if let Some(burst) = find_burst(&list, 180, 10) {
            alerts.push(RuleAlert {
                id: format!("alt_{}", &Uuid::new_v4().simple().to_string()[..8]),
                rule_id: "R008_RAPID_CRUD_MUTATION".to_string(),
                severity: "HIGH".to_string(),
                category: "CRUD".to_string(),
                title: format!("Rapid CRUD Write Spike ({} mutations in 3m)", burst.len()),
                description: format!(
                    "Actor '{actor}' executed {} create/update/delete operations in under 3 minutes.",
                    burst.len()
                ),
                actor: actor.to_string(),
                event_count: burst.len(),
                timestamp: burst.last().map(|e| e.timestamp.clone()).unwrap_or_default(),
                evidence: burst
                    .iter()
                    .take(5)
                    .map(|e| format!("{} {} ({})", e.action, e.path, e.timestamp))
                    .collect(),
                recommended_action: "Inspect recent revisions in Before/After Diff viewer to confirm no automated overwrite occurred.".to_string(),
            });
        }
    }

    // R004: Off-hours mutations (00:00–05:00 local time)
    if !off_hours_writes.is_empty() {
        let sample_actor = off_hours_writes[0].actor.clone();
        alerts.push(RuleAlert {
            id: format!("alt_{}", &Uuid::new_v4().simple().to_string()[..8]),
            rule_id: "R004_OFF_HOURS_MUTATION".to_string(),
            severity: "MEDIUM".to_string(),
            category: "ACCESS".to_string(),
            title: format!("Off-Hours Archival Data Modification ({} events)", off_hours_writes.len()),
            description: format!(
                "Detected {} data modification(s) between 00:00 and 05:00 local time.",
                off_hours_writes.len()
            ),
            actor: sample_actor,
            event_count: off_hours_writes.len(),
            timestamp: off_hours_writes
                .last()
                .map(|e| e.timestamp.clone())
                .unwrap_or_default(),
            evidence: off_hours_writes
                .iter()
                .take(5)
                .map(|e| format!("{} {} by {} ({})", e.action, e.path, e.actor, e.timestamp))
                .collect(),
            recommended_action: "Verify whether after-hours archival cataloging was scheduled.".to_string(),
        });
    }

    alerts.sort_by(|a, b| {
        severity_rank(&b.severity)
            .cmp(&severity_rank(&a.severity))
            .then_with(|| b.timestamp.cmp(&a.timestamp))
    });
    alerts
}

fn severity_rank(sev: &str) -> u8 {
    match sev {
        "CRITICAL" => 4,
        "HIGH" => 3,
        "MEDIUM" => 2,
        _ => 1,
    }
}

/// Layer 2: Pure-Rust Statistical Outlier Detection (Z-score + IQR fences).
pub fn detect_outliers(events: &[AuditEvent]) -> Vec<OutlierFinding> {
    let mut findings = Vec::new();
    if events.len() < 4 {
        return findings;
    }

    // 1. Hourly activity volume outlier detection across time buckets
    let mut hour_counts: HashMap<(String, i64), (usize, String)> = HashMap::new();
    for ev in events {
        let bucket = ev.epoch_secs / 3600;
        let entry = hour_counts
            .entry((ev.actor.clone(), bucket))
            .or_insert_with(|| (0, ev.timestamp.clone()));
        entry.0 += 1;
        entry.1 = ev.timestamp.clone();
    }

    if hour_counts.len() >= 3 {
        let vals: Vec<f64> = hour_counts.values().map(|(c, _)| *c as f64).collect();
        let mean = vals.iter().sum::<f64>() / vals.len() as f64;
        let variance = vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / vals.len() as f64;
        let std = variance.sqrt();

        let mut sorted_vals = vals.clone();
        sorted_vals.sort_by(|a, b| a.total_cmp(b));
        let q1 = sorted_vals[sorted_vals.len() / 4];
        let q3 = sorted_vals[(sorted_vals.len() * 3) / 4];
        let iqr_upper = q3 + 1.5 * (q3 - q1).max(1.0);

        for ((actor, _bucket), (count, ts)) in &hour_counts {
            let v = *count as f64;
            let z = if std > 1e-6 { (v - mean) / std } else { 0.0 };
            if (*count >= 8) && (z >= 2.0 || v > iqr_upper.max(mean * 2.5)) {
                let severity = if z >= 3.0 { "HIGH" } else { "MEDIUM" };
                findings.push(OutlierFinding {
                    id: format!("out_{}", &Uuid::new_v4().simple().to_string()[..8]),
                    metric: "hourly_request_volume".to_string(),
                    severity: severity.to_string(),
                    actor: actor.clone(),
                    observed_value: v,
                    baseline_mean: (mean * 100.0).round() / 100.0,
                    baseline_std: (std * 100.0).round() / 100.0,
                    z_score: (z * 100.0).round() / 100.0,
                    explanation: format!(
                        "Actor '{actor}' triggered {count} actions in a single hour vs baseline mean {:.1} (Z={:.2}).",
                        mean, z
                    ),
                    timestamp: ts.clone(),
                });
            }
        }
    }

    // 2. Request duration / heavy-query latency outliers
    let durations: Vec<f64> = events
        .iter()
        .filter(|e| e.duration_ms > 0)
        .map(|e| e.duration_ms as f64)
        .collect();
    if durations.len() >= 5 {
        let mean = durations.iter().sum::<f64>() / durations.len() as f64;
        let variance =
            durations.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / durations.len() as f64;
        let std = variance.sqrt();
        if std > 1e-6 {
            for ev in events {
                let v = ev.duration_ms as f64;
                let z = (v - mean) / std;
                if ev.duration_ms >= 3000 && z >= 2.5 {
                    findings.push(OutlierFinding {
                        id: format!("out_{}", &Uuid::new_v4().simple().to_string()[..8]),
                        metric: "query_latency_ms".to_string(),
                        severity: "MEDIUM".to_string(),
                        actor: ev.actor.clone(),
                        observed_value: v,
                        baseline_mean: (mean * 10.0).round() / 10.0,
                        baseline_std: (std * 10.0).round() / 10.0,
                        z_score: (z * 100.0).round() / 100.0,
                        explanation: format!(
                            "Unusually heavy request on '{}' took {} ms vs baseline {:.0} ms (Z={:.2}).",
                            ev.path, ev.duration_ms, mean, z
                        ),
                        timestamp: ev.timestamp.clone(),
                    });
                }
            }
        }
    }

    // 3. Per-actor destructive write ratio outlier
    let mut actor_totals: HashMap<&str, (usize, usize, String)> = HashMap::new();
    for ev in events {
        let entry = actor_totals
            .entry(&ev.actor)
            .or_insert_with(|| (0, 0, ev.timestamp.clone()));
        entry.0 += 1;
        if ev.action == "DELETE" {
            entry.1 += 1;
        }
        entry.2 = ev.timestamp.clone();
    }
    for (actor, (total, deletes, ts)) in actor_totals {
        if deletes >= 2 && total >= 3 {
            let ratio = deletes as f64 / total as f64;
            if ratio >= 0.45 {
                findings.push(OutlierFinding {
                    id: format!("out_{}", &Uuid::new_v4().simple().to_string()[..8]),
                    metric: "delete_action_ratio".to_string(),
                    severity: "HIGH".to_string(),
                    actor: actor.to_string(),
                    observed_value: (ratio * 100.0).round(),
                    baseline_mean: 5.0,
                    baseline_std: 5.0,
                    z_score: (((ratio * 100.0) - 5.0) / 5.0 * 10.0).round() / 10.0,
                    explanation: format!(
                        "Actor '{actor}' has an abnormally high deletion ratio: {deletes}/{total} ({:.0}%).",
                        ratio * 100.0
                    ),
                    timestamp: ts,
                });
            }
        }
    }

    findings.sort_by(|a, b| b.z_score.total_cmp(&a.z_score));
    findings.truncate(20);
    findings
}

/// Inspect pasted or uploaded external SQL / database audit logs for abnormal queries & patterns.
pub fn inspect_external_sql_logs(raw_log: &str, actor_default: &str) -> Value {
    let now = Utc::now();
    let mut parsed_events = Vec::new();
    for (idx, line) in raw_log.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let (actor, sql_text) = if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
            let a = v["user"]
                .as_str()
                .or_else(|| v["actor"].as_str())
                .unwrap_or(actor_default)
                .to_string();
            let q = v["query"]
                .as_str()
                .or_else(|| v["sql"].as_str())
                .unwrap_or(trimmed)
                .to_string();
            (a, q)
        } else {
            (actor_default.to_string(), trimmed.to_string())
        };
        parsed_events.push(AuditEvent {
            id: format!("sql_{idx}"),
            timestamp: now.to_rfc3339(),
            epoch_secs: now.timestamp() + idx as i64,
            actor,
            role: "external_sql".to_string(),
            ip: "imported".to_string(),
            method: "SQL".to_string(),
            path: "/external/sql-audit".to_string(),
            query: String::new(),
            action: "SQL_QUERY".to_string(),
            resource_type: "sql_database".to_string(),
            resource_id: format!("line_{}", idx + 1),
            status_code: 200,
            payload_preview: sql_text,
            duration_ms: 0,
        });
    }

    let alerts = evaluate_rules(&parsed_events);
    let risk_level = compute_risk_level(&alerts, &[]);
    json!({
        "status": "ok",
        "lines_analyzed": parsed_events.len(),
        "anomaly_count": alerts.len(),
        "risk_level": risk_level.0,
        "risk_score": risk_level.1,
        "alerts": alerts,
    })
}

fn compute_risk_level(alerts: &[RuleAlert], outliers: &[OutlierFinding]) -> (String, u32) {
    let mut score = 0u32;
    for a in alerts {
        score = score.saturating_add(match a.severity.as_str() {
            "CRITICAL" => 35,
            "HIGH" => 20,
            "MEDIUM" => 10,
            _ => 4,
        });
    }
    for o in outliers {
        score = score.saturating_add(match o.severity.as_str() {
            "HIGH" => 15,
            "MEDIUM" => 8,
            _ => 3,
        });
    }
    score = score.min(100);
    let level = if score >= 70 || alerts.iter().any(|a| a.severity == "CRITICAL") {
        "CRITICAL"
    } else if score >= 40 {
        "HIGH"
    } else if score >= 15 {
        "MEDIUM"
    } else {
        "LOW"
    };
    (level.to_string(), score)
}

pub fn build_audit_snapshot(cfg: &AppConfig) -> AuditReportSnapshot {
    let events = load_audit_events(&cfg.audit_log_file, 1500);
    let rule_alerts = evaluate_rules(&events);
    let statistical_outliers = detect_outliers(&events);
    let (risk_level, risk_score) = compute_risk_level(&rule_alerts, &statistical_outliers);

    let mut severity_counts: HashMap<String, usize> = HashMap::from([
        ("CRITICAL".to_string(), 0),
        ("HIGH".to_string(), 0),
        ("MEDIUM".to_string(), 0),
        ("LOW".to_string(), 0),
    ]);
    for a in &rule_alerts {
        *severity_counts.entry(a.severity.clone()).or_insert(0) += 1;
    }
    for o in &statistical_outliers {
        *severity_counts.entry(o.severity.clone()).or_insert(0) += 1;
    }

    let mut actor_map: HashMap<String, (usize, usize, usize)> = HashMap::new();
    for ev in &events {
        let entry = actor_map.entry(ev.actor.clone()).or_insert((0, 0, 0));
        entry.0 += 1;
        if matches!(ev.action.as_str(), "CREATE" | "UPDATE" | "DELETE") {
            entry.1 += 1;
        }
        if ev.action == "DELETE" {
            entry.2 += 1;
        }
    }
    let mut top_actors: Vec<Value> = actor_map
        .into_iter()
        .map(|(actor, (total, writes, deletes))| {
            json!({
                "actor": actor,
                "total_events": total,
                "write_events": writes,
                "delete_events": deletes,
            })
        })
        .collect();
    top_actors.sort_by(|a, b| {
        b["total_events"]
            .as_u64()
            .unwrap_or(0)
            .cmp(&a["total_events"].as_u64().unwrap_or(0))
    });
    top_actors.truncate(10);

    let mut recent_events = events.clone();
    recent_events.reverse();
    recent_events.truncate(60);

    AuditReportSnapshot {
        generated_at: Utc::now().to_rfc3339(),
        total_events_analyzed: events.len(),
        risk_level,
        risk_score,
        severity_counts,
        rule_alerts,
        statistical_outliers,
        recent_events,
        top_actors,
    }
}

/// Build a clean deterministic narrative fallback (always instant, 0 hallucinations).
pub fn deterministic_anomaly_explanation(snapshot: &AuditReportSnapshot) -> String {
    if snapshot.rule_alerts.is_empty() && snapshot.statistical_outliers.is_empty() {
        return format!(
            "Audit Inspection Summary ({}): Analyzed {} recent events. No suspicious CRUD bursts, abnormal SQL/injection patterns, authentication brute-force attempts, or statistical outliers were detected. Current system risk posture is LOW (score 0/100).",
            snapshot.generated_at, snapshot.total_events_analyzed
        );
    }

    let mut lines = vec![format!(
        "Audit Risk Assessment: {} (Risk Score {}/100 across {} events).",
        snapshot.risk_level, snapshot.risk_score, snapshot.total_events_analyzed
    )];
    if !snapshot.rule_alerts.is_empty() {
        lines.push(format!(
            "Rules Engine flagged {} alert(s):",
            snapshot.rule_alerts.len()
        ));
        for a in snapshot.rule_alerts.iter().take(6) {
            lines.push(format!(
                "- [{}] {} ({}) — {} Recommended: {}",
                a.severity, a.title, a.rule_id, a.description, a.recommended_action
            ));
        }
    }
    if !snapshot.statistical_outliers.is_empty() {
        lines.push(format!(
            "Statistical Outlier Detector flagged {} behavioral anomaly(ies):",
            snapshot.statistical_outliers.len()
        ));
        for o in snapshot.statistical_outliers.iter().take(4) {
            lines.push(format!("- [{}] {}", o.severity, o.explanation));
        }
    }
    lines.join("\n")
}

/// Layer 3: Use local Ollama (`qwen3-vl:4b-instruct-16k`) to generate a plain-language
/// executive explanation of detected anomalies, with deterministic Rust fallback.
pub async fn explain_anomalies_with_ai(cfg: &AppConfig, snapshot: &AuditReportSnapshot) -> Value {
    let deterministic = deterministic_anomaly_explanation(snapshot);
    if snapshot.rule_alerts.is_empty() && snapshot.statistical_outliers.is_empty() {
        return json!({
            "status": "ok",
            "source": "deterministic",
            "explanation": deterministic,
            "risk_level": snapshot.risk_level,
            "risk_score": snapshot.risk_score,
        });
    }

    let compact_summary = json!({
        "risk_level": snapshot.risk_level,
        "risk_score": snapshot.risk_score,
        "total_events": snapshot.total_events_analyzed,
        "rule_alerts": snapshot.rule_alerts.iter().take(8).collect::<Vec<_>>(),
        "statistical_outliers": snapshot.statistical_outliers.iter().take(5).collect::<Vec<_>>(),
    });

    let prompt = format!(
        "You are an archival security auditor for Arkib Negara Malaysia (ArkibVision). \
Explain the following audit anomalies clearly in plain English (with concise Bahasa Malaysia terms where helpful) in under 220 words. \
Structure your response into: 1) Executive Risk Summary, 2) Key Anomalies Explained, 3) Recommended Admin Actions. \
Do not invent events not present in the JSON:\n\n{}",
        serde_json::to_string_pretty(&compact_summary).unwrap_or_default()
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(45))
        .build();
    let Ok(client) = client else {
        return json!({
            "status": "ok",
            "source": "deterministic_fallback",
            "explanation": deterministic,
            "risk_level": snapshot.risk_level,
            "risk_score": snapshot.risk_score,
        });
    };

    let body = json!({
        "model": cfg.vision_model,
        "stream": false,
        "options": { "num_ctx": OLLAMA_UNIFORM_NUM_CTX, "temperature": 0.2 },
        "messages": [
            { "role": "user", "content": prompt }
        ]
    });

    if let Ok(resp) = client.post(&cfg.ollama_url).json(&body).send().await
        && resp.status().is_success()
        && let Ok(val) = resp.json::<Value>().await
        && let Some(content) = val["message"]["content"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    {
        return json!({
            "status": "ok",
            "source": "ollama_ai",
            "model": cfg.vision_model,
            "explanation": content,
            "deterministic_summary": deterministic,
            "risk_level": snapshot.risk_level,
            "risk_score": snapshot.risk_score,
        });
    }

    json!({
        "status": "ok",
        "source": "deterministic_fallback",
        "explanation": deterministic,
        "risk_level": snapshot.risk_level,
        "risk_score": snapshot.risk_score,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_event(actor: &str, action: &str, path: &str, query: &str, payload: &str, epoch: i64) -> AuditEvent {
        AuditEvent {
            id: format!("e_{epoch}"),
            timestamp: Utc.timestamp_opt(epoch, 0).single().unwrap().to_rfc3339(),
            epoch_secs: epoch,
            actor: actor.to_string(),
            role: "admin".to_string(),
            ip: "127.0.0.1".to_string(),
            method: if action == "DELETE" { "DELETE" } else { "GET" }.to_string(),
            path: path.to_string(),
            query: query.to_string(),
            action: action.to_string(),
            resource_type: "document".to_string(),
            resource_id: "doc_1".to_string(),
            status_code: if action == "LOGIN_FAIL" { 401 } else { 200 },
            payload_preview: payload.to_string(),
            duration_ms: 15,
        }
    }

    #[test]
    fn rules_engine_catches_delete_burst_sqli_and_abnormal_sql() {
        let base = 1_750_000_000i64;
        let events = vec![
            mk_event("admin", "DELETE", "/admin/api/documents/a", "", "", base),
            mk_event("admin", "DELETE", "/admin/api/documents/b", "", "", base + 10),
            mk_event("admin", "DELETE", "/admin/api/documents/c", "", "", base + 20),
            mk_event("guest", "READ", "/admin/api/documents", "q=' OR 1=1 UNION SELECT * FROM users--", "", base + 30),
        ];
        let alerts = evaluate_rules(&events);
        assert!(alerts.iter().any(|a| a.rule_id == "R001_SUSPICIOUS_DELETE_BURST"));
        assert!(alerts.iter().any(|a| a.rule_id == "R005_SQL_INJECTION_PATTERN"));

        let sql_res = inspect_external_sql_logs(
            "DELETE FROM archival_records;\nSELECT * FROM confidential_profiles;\n",
            "db_user",
        );
        assert!(sql_res["anomaly_count"].as_u64().unwrap_or(0) >= 2);
    }

    #[test]
    fn statistical_outlier_detector_catches_hourly_spike() {
        let base = 1_750_000_000i64;
        let mut events = Vec::new();
        // Normal baseline: 1 event per hour for 6 hours
        for h in 0..6 {
            events.push(mk_event("alice", "READ", "/admin/api/documents", "", "", base + h * 3600));
        }
        // Sudden spike: 18 events in hour 7
        for i in 0..18 {
            events.push(mk_event("alice", "READ", "/admin/api/documents", "", "", base + 7 * 3600 + i * 10));
        }
        let outliers = detect_outliers(&events);
        assert!(outliers.iter().any(|o| o.metric == "hourly_request_volume" && o.actor == "alice"));
    }
}
