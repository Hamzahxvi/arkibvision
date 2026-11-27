#![forbid(unsafe_code)]

//! Maintainable Archival Categories registry (`data/processed/categories.json`).
//! Supports full CRUD (add, rename/update, delete with optional reassignment) and
//! atomic temp+rename persistence with cascade updates across existing documents.

use crate::config::AppConfig;
use crate::metadata::profile_slug;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CategoryItem {
    pub slug: String,
    pub name: String,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub created_at: String,
}

fn default_color() -> String {
    "#3b82f6".to_string()
}

pub fn category_slug(name: &str) -> String {
    let s = profile_slug(name).to_ascii_lowercase();
    if s.is_empty() {
        "lain_lain".to_string()
    } else {
        s
    }
}

fn default_categories() -> Vec<CategoryItem> {
    let now = Utc::now().to_rfc3339();
    let seeds = [
        (
            "surat_rasmi",
            "Surat Rasmi",
            "#3b82f6",
            "Official government, ministry, and royal correspondence",
        ),
        (
            "manuskrip",
            "Manuskrip",
            "#8b5cf6",
            "Historical Jawi, Kawi, Pallava, Rencong, and handwritten manuscripts",
        ),
        (
            "gambar",
            "Gambar",
            "#10b981",
            "Archival photographs, portraits, and visual heritage records",
        ),
        (
            "peta",
            "Peta",
            "#f59e0b",
            "Historical maps, land surveys, and architectural plans",
        ),
        (
            "warta",
            "Warta",
            "#ec4899",
            "Government gazettes, proclamations, and official notices",
        ),
        (
            "laporan",
            "Laporan",
            "#06b6d4",
            "Annual reports, royal commissions, and institutional studies",
        ),
        (
            "bahan_bercetak",
            "Bahan Bercetak",
            "#6366f1",
            "Newspapers, pamphlets, books, and printed publications",
        ),
        (
            "lain_lain",
            "Lain-lain",
            "#64748b",
            "Other archival records and miscellaneous materials",
        ),
    ];
    seeds
        .into_iter()
        .map(|(slug, name, color, description)| CategoryItem {
            slug: slug.to_string(),
            name: name.to_string(),
            color: color.to_string(),
            description: description.to_string(),
            created_at: now.clone(),
        })
        .collect()
}

fn atomic_write_json(path: &Path, items: &[CategoryItem]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed creating category dir: {e}"))?;
    }
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_string_pretty(items)
        .map_err(|e| format!("Failed serializing categories: {e}"))?;
    fs::write(&tmp, body).map_err(|e| format!("Failed writing temp categories: {e}"))?;
    fs::rename(&tmp, path).map_err(|e| format!("Failed replacing categories file: {e}"))?;
    Ok(())
}

pub fn ensure_default_categories(cfg: &AppConfig) {
    if cfg.categories_file.is_file() {
        return;
    }
    let _ = atomic_write_json(&cfg.categories_file, &default_categories());
}

pub fn load_categories(cfg: &AppConfig) -> Vec<CategoryItem> {
    ensure_default_categories(cfg);
    let Ok(raw) = fs::read_to_string(&cfg.categories_file) else {
        return default_categories();
    };
    serde_json::from_str::<Vec<CategoryItem>>(&raw).unwrap_or_else(|_| default_categories())
}

/// Resolve an input category string (name or slug, case-insensitive) to a canonical category item.
pub fn resolve_category(cfg: &AppConfig, input: &str) -> Option<CategoryItem> {
    let q = input.trim();
    if q.is_empty() {
        return None;
    }
    let q_lower = q.to_ascii_lowercase();
    let q_slug = category_slug(q);
    load_categories(cfg).into_iter().find(|c| {
        c.slug.eq_ignore_ascii_case(&q_lower)
            || c.slug == q_slug
            || c.name.eq_ignore_ascii_case(&q_lower)
    })
}

pub fn list_categories_with_counts(cfg: &AppConfig, documents: &[Value]) -> Vec<Value> {
    let cats = load_categories(cfg);
    let mut counts: HashMap<String, usize> = HashMap::new();
    for doc in documents {
        let raw = doc["category"].as_str().unwrap_or("").trim();
        if raw.is_empty() {
            continue;
        }
        let key = cats
            .iter()
            .find(|c| {
                c.slug.eq_ignore_ascii_case(raw)
                    || c.name.eq_ignore_ascii_case(raw)
                    || c.slug == category_slug(raw)
            })
            .map(|c| c.slug.clone())
            .unwrap_or_else(|| category_slug(raw));
        *counts.entry(key).or_insert(0) += 1;
    }

    cats.into_iter()
        .map(|c| {
            let count = *counts.get(&c.slug).unwrap_or(&0);
            json!({
                "slug": c.slug,
                "name": c.name,
                "color": c.color,
                "description": c.description,
                "created_at": c.created_at,
                "document_count": count,
            })
        })
        .collect()
}

pub fn create_category(
    cfg: &AppConfig,
    name: &str,
    color: Option<&str>,
    description: Option<&str>,
) -> Result<CategoryItem, String> {
    let clean_name = name.trim();
    if clean_name.is_empty() {
        return Err("Category name is required.".to_string());
    }
    let slug = category_slug(clean_name);
    let mut cats = load_categories(cfg);
    if cats
        .iter()
        .any(|c| c.slug == slug || c.name.eq_ignore_ascii_case(clean_name))
    {
        return Err(format!("Category '{clean_name}' already exists."));
    }
    let item = CategoryItem {
        slug,
        name: clean_name.to_string(),
        color: color
            .map(str::trim)
            .filter(|s| s.starts_with('#') && (s.len() == 4 || s.len() == 7))
            .unwrap_or("#3b82f6")
            .to_string(),
        description: description.unwrap_or("").trim().to_string(),
        created_at: Utc::now().to_rfc3339(),
    };
    cats.push(item.clone());
    atomic_write_json(&cfg.categories_file, &cats)?;
    Ok(item)
}

pub fn update_category(
    cfg: &AppConfig,
    slug: &str,
    new_name: Option<&str>,
    new_color: Option<&str>,
    new_description: Option<&str>,
) -> Result<(CategoryItem, usize), String> {
    let mut cats = load_categories(cfg);
    let Some(idx) = cats.iter().position(|c| c.slug == slug) else {
        return Err(format!("Category '{slug}' not found."));
    };
    let old_name = cats[idx].name.clone();
    let old_slug = cats[idx].slug.clone();

    if let Some(n) = new_name.map(str::trim).filter(|s| !s.is_empty()) {
        let candidate_slug = category_slug(n);
        if candidate_slug != old_slug
            && cats.iter().enumerate().any(|(i, c)| {
                i != idx && (c.slug == candidate_slug || c.name.eq_ignore_ascii_case(n))
            })
        {
            return Err(format!("Another category named '{n}' already exists."));
        }
        cats[idx].name = n.to_string();
        cats[idx].slug = candidate_slug;
    }
    if let Some(c) = new_color
        .map(str::trim)
        .filter(|s| s.starts_with('#') && (s.len() == 4 || s.len() == 7))
    {
        cats[idx].color = c.to_string();
    }
    if let Some(d) = new_description {
        cats[idx].description = d.trim().to_string();
    }

    let updated = cats[idx].clone();
    atomic_write_json(&cfg.categories_file, &cats)?;

    // Cascade category rename to documents that referenced old_name or old_slug.
    let mut updated_docs = 0usize;
    if updated.name != old_name || updated.slug != old_slug {
        updated_docs =
            rewrite_document_categories(&cfg.documents_dir, &old_slug, &old_name, &updated.name);
    }

    Ok((updated, updated_docs))
}

pub fn delete_category(
    cfg: &AppConfig,
    slug: &str,
    reassign_to: Option<&str>,
) -> Result<Value, String> {
    let mut cats = load_categories(cfg);
    let Some(idx) = cats.iter().position(|c| c.slug == slug) else {
        return Err(format!("Category '{slug}' not found."));
    };
    let removed = cats.remove(idx);
    atomic_write_json(&cfg.categories_file, &cats)?;

    let replacement_name = reassign_to
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|target| {
            cats.iter()
                .find(|c| {
                    c.slug.eq_ignore_ascii_case(target) || c.name.eq_ignore_ascii_case(target)
                })
                .map(|c| c.name.clone())
        })
        .unwrap_or_default();

    let updated_docs = rewrite_document_categories(
        &cfg.documents_dir,
        &removed.slug,
        &removed.name,
        &replacement_name,
    );

    Ok(json!({
        "status": "deleted",
        "deleted_slug": removed.slug,
        "deleted_name": removed.name,
        "reassigned_to": replacement_name,
        "updated_documents": updated_docs,
    }))
}

fn rewrite_document_categories(
    documents_dir: &Path,
    old_slug: &str,
    old_name: &str,
    new_name: &str,
) -> usize {
    let mut updated = 0usize;
    let Ok(entries) = fs::read_dir(documents_dir) else {
        return 0;
    };
    for entry in entries.flatten() {
        let meta_path = entry.path().join("metadata.json");
        if !meta_path.is_file() {
            continue;
        }
        let Ok(raw) = fs::read_to_string(&meta_path) else {
            continue;
        };
        let Ok(mut val) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        let Some(obj) = val.as_object_mut() else {
            continue;
        };
        let cur = obj
            .get("category")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if cur.eq_ignore_ascii_case(old_name)
            || cur.eq_ignore_ascii_case(old_slug)
            || category_slug(&cur) == old_slug
        {
            obj.insert("category".to_string(), Value::String(new_name.to_string()));
            if let Ok(serialized) = serde_json::to_string_pretty(&val) {
                let tmp = meta_path.with_extension("json.tmp");
                if fs::write(&tmp, serialized).is_ok() && fs::rename(&tmp, &meta_path).is_ok() {
                    updated += 1;
                }
            }
        }
    }
    updated
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn temp_cfg() -> AppConfig {
        let mut cfg = AppConfig::load();
        let root = std::env::temp_dir().join(format!("arkib_cat_test_{}", Uuid::new_v4().simple()));
        cfg.categories_file = root.join("categories.json");
        cfg.documents_dir = root.join("documents");
        let _ = fs::create_dir_all(&cfg.documents_dir);
        cfg
    }

    #[test]
    fn category_crud_and_document_cascade() {
        let cfg = temp_cfg();
        let created = create_category(
            &cfg,
            "Perjanjian Diraja",
            Some("#14b8a6"),
            Some("Royal treaties"),
        )
        .expect("create category");
        assert_eq!(created.slug, "perjanjian_diraja");

        // Create sample document referencing the new category
        let doc_dir = cfg.documents_dir.join("doc_a");
        fs::create_dir_all(&doc_dir).unwrap();
        fs::write(
            doc_dir.join("metadata.json"),
            r#"{"title":"Doc A","category":"Perjanjian Diraja"}"#,
        )
        .unwrap();

        let (renamed, count) = update_category(
            &cfg,
            "perjanjian_diraja",
            Some("Watikah Diraja"),
            None,
            None,
        )
        .expect("rename category");
        assert_eq!(renamed.slug, "watikah_diraja");
        assert_eq!(count, 1);

        let meta: Value =
            serde_json::from_str(&fs::read_to_string(doc_dir.join("metadata.json")).unwrap())
                .unwrap();
        assert_eq!(meta["category"], "Watikah Diraja");

        let del = delete_category(&cfg, "watikah_diraja", Some("manuskrip")).expect("delete");
        assert_eq!(del["updated_documents"], 1);
        let meta2: Value =
            serde_json::from_str(&fs::read_to_string(doc_dir.join("metadata.json")).unwrap())
                .unwrap();
        assert_eq!(meta2["category"], "Manuskrip");

        let _ = fs::remove_dir_all(cfg.categories_file.parent().unwrap());
    }
}
