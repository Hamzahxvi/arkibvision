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
    #[serde(skip)]
    unit_vector: Vec<f32>,
}

impl FacePoint {
    pub fn new(
        id: String,
        identity: String,
        filename: String,
        crop_path: String,
        vector: Vec<f32>,
    ) -> Self {
        let unit_vector = normalize_l2(&vector);
        Self {
            id,
            identity,
            filename,
            crop_path,
            vector,
            unit_vector,
        }
    }

    fn ensure_normalized(&mut self) {
        if self.unit_vector.len() != self.vector.len() {
            self.unit_vector = normalize_l2(&self.vector);
        }
    }
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

/// L2-normalize a vector once so cosine similarity becomes a pure SIMD-friendly dot product.
pub fn normalize_l2(v: &[f32]) -> Vec<f32> {
    let norm_sq: f64 = v.iter().map(|&x| f64::from(x) * f64::from(x)).sum();
    let norm = norm_sq.sqrt();
    if norm == 0.0 {
        return vec![0.0_f32; v.len()];
    }
    let inv = (1.0 / norm) as f32;
    v.iter().map(|&x| x * inv).collect()
}

#[inline]
fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    a.iter().zip(b.iter()).map(|(&x, &y)| x * y).sum()
}

#[cfg(test)]
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let ua = normalize_l2(a);
    let ub = normalize_l2(b);
    dot_product(&ua, &ub)
}

impl VectorStore {
    pub fn load_or_create(file_path: &Path) -> Result<Self, String> {
        let mut points = if file_path.is_file() {
            let text = fs::read_to_string(file_path)
                .map_err(|e| format!("Failed reading {}: {e}", file_path.display()))?;
            let parsed: VectorFilePayload = serde_json::from_str(&text)
                .map_err(|e| format!("Failed parsing {}: {e}", file_path.display()))?;
            parsed.points
        } else {
            Vec::new()
        };
        for pt in &mut points {
            pt.ensure_normalized();
        }
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
        if query.is_empty() || limit == 0 {
            return Vec::new();
        }
        let q_unit = normalize_l2(query);
        let guard = self.points.read().expect("vector store lock");
        let mut hits: Vec<SearchHit> = guard
            .iter()
            .map(|pt| SearchHit {
                point: pt.clone(),
                score: dot_product(&q_unit, &pt.unit_vector),
            })
            .collect();
        hits.sort_by(|a, b| b.score.total_cmp(&a.score));
        hits.truncate(limit);
        hits
    }

    pub fn upsert(&self, mut point: FacePoint) -> Result<(), String> {
        point.ensure_normalized();
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

    pub fn total_vectors(&self) -> usize {
        self.points.read().expect("vector store lock").len()
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
    fn test_cosine_similarity_and_prenorm_search() {
        let a = vec![3.0_f32, 4.0, 0.0];
        let b = vec![6.0_f32, 8.0, 0.0];
        let c = vec![0.0_f32, 0.0, 5.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-6);
        assert!(cosine_similarity(&a, &c).abs() < 1e-6);

        let pt1 = FacePoint::new(
            "1".into(),
            "Alice".into(),
            "a.jpg".into(),
            "".into(),
            a.clone(),
        );
        let pt2 = FacePoint::new("2".into(), "Bob".into(), "b.jpg".into(), "".into(), c);
        assert!((dot_product(&normalize_l2(&b), &pt1.unit_vector) - 1.0).abs() < 1e-6);
        assert!(dot_product(&normalize_l2(&b), &pt2.unit_vector).abs() < 1e-6);
    }
}
