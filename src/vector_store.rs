#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FacePoint {
    pub id: String,
    pub identity: String,
    pub filename: String,
    pub crop_path: String,
    pub vector: Vec<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct VectorFilePayload {
    collection: String,
    dim: usize,
    points: Vec<FacePoint>,
}

#[derive(Clone, Debug)]
pub struct SearchHit {
    pub point: FacePoint,
    pub score: f32,
}

pub struct VectorStore {
    file_path: PathBuf,
    points: RwLock<Vec<FacePoint>>,
}

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0_f64;
    let mut norm_a = 0.0_f64;
    let mut norm_b = 0.0_f64;
    for (&x, &y) in a.iter().zip(b.iter()) {
        let xf = f64::from(x);
        let yf = f64::from(y);
        dot += xf * yf;
        norm_a += xf * xf;
        norm_b += yf * yf;
    }
    let denom = (norm_a * norm_b).sqrt();
    if denom == 0.0 {
        0.0
    } else {
        (dot / denom) as f32
    }
}

impl VectorStore {
    pub fn load_or_create(file_path: &Path) -> Result<Self, String> {
        let points = if file_path.is_file() {
            let text = fs::read_to_string(file_path)
                .map_err(|e| format!("Failed reading {}: {e}", file_path.display()))?;
            let parsed: VectorFilePayload = serde_json::from_str(&text)
                .map_err(|e| format!("Failed parsing {}: {e}", file_path.display()))?;
            parsed.points
        } else {
            Vec::new()
        };
        Ok(Self {
            file_path: file_path.to_path_buf(),
            points: RwLock::new(points),
        })
    }

    fn persist_locked(&self, pts: &[FacePoint]) -> Result<(), String> {
        if let Some(parent) = self.file_path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Failed creating dir: {e}"))?;
        }
        let payload = VectorFilePayload {
            collection: "enterprise_faces".to_string(),
            dim: 768,
            points: pts.to_vec(),
        };
        let bytes = serde_json::to_vec(&payload).map_err(|e| format!("Serialize error: {e}"))?;
        let tmp = self.file_path.with_extension("json.tmp");
        fs::write(&tmp, bytes).map_err(|e| format!("Write tmp error: {e}"))?;
        fs::rename(&tmp, &self.file_path).map_err(|e| format!("Atomic rename error: {e}"))?;
        Ok(())
    }

    pub fn search(&self, query: &[f32], limit: usize) -> Vec<SearchHit> {
        let guard = self.points.read().expect("vector store lock");
        let mut hits: Vec<SearchHit> = guard
            .iter()
            .map(|pt| SearchHit {
                point: pt.clone(),
                score: cosine_similarity(query, &pt.vector),
            })
            .collect();
        hits.sort_by(|a, b| b.score.total_cmp(&a.score));
        hits.truncate(limit);
        hits
    }

    pub fn upsert(&self, point: FacePoint) -> Result<(), String> {
        let mut guard = self.points.write().expect("vector store lock");
        if let Some(existing) = guard.iter_mut().find(|p| p.id == point.id) {
            *existing = point;
        } else {
            guard.push(point);
        }
        self.persist_locked(&guard)
    }

    pub fn list_counts(&self) -> BTreeMap<String, usize> {
        let guard = self.points.read().expect("vector store lock");
        let mut counts = BTreeMap::new();
        for pt in guard.iter() {
            *counts.entry(pt.identity.clone()).or_insert(0) += 1;
        }
        counts
    }

    pub fn count_by_identity(&self, identity: &str) -> usize {
        let guard = self.points.read().expect("vector store lock");
        guard.iter().filter(|p| p.identity == identity).count()
    }

    pub fn points_by_identity(&self, identity: &str) -> Vec<FacePoint> {
        let guard = self.points.read().expect("vector store lock");
        guard
            .iter()
            .filter(|p| p.identity == identity)
            .cloned()
            .collect()
    }

    pub fn all_points(&self) -> Vec<FacePoint> {
        self.points.read().expect("vector store lock").clone()
    }

    pub fn rename_identity(&self, old_identity: &str, new_identity: &str) -> Result<usize, String> {
        let mut guard = self.points.write().expect("vector store lock");
        let mut updated = 0;
        for pt in guard.iter_mut() {
            if pt.identity == old_identity {
                pt.identity = new_identity.to_string();
                updated += 1;
            }
        }
        if updated > 0 {
            self.persist_locked(&guard)?;
        }
        Ok(updated)
    }

    pub fn delete_identity(&self, identity: &str) -> Result<usize, String> {
        let mut guard = self.points.write().expect("vector store lock");
        let before = guard.len();
        guard.retain(|p| p.identity != identity);
        let removed = before - guard.len();
        if removed > 0 {
            self.persist_locked(&guard)?;
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0_f32, 0.0, 0.0];
        let b = vec![1.0_f32, 0.0, 0.0];
        let c = vec![0.0_f32, 1.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-6);
        assert!(cosine_similarity(&a, &c).abs() < 1e-6);
    }
}
