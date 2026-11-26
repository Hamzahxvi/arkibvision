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

fn default_embedding_model() -> String {
    "clip_vit_b32_768".to_string()
}

fn default_embedding_dim() -> usize {
    768
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct VectorFilePayload {
    collection: String,
    #[serde(default = "default_embedding_model")]
    embedding_model: String,
    #[serde(default)]
    embedding_dim: usize,
    #[serde(default = "default_embedding_dim")]
    dim: usize,
    points: Vec<FacePoint>,
}

#[derive(Clone, Debug)]
pub struct SearchHit {
    pub point: FacePoint,
    pub score: f32,
}

#[derive(Clone, Debug)]
struct StoreMeta {
    embedding_model: String,
    embedding_dim: usize,
    mismatch_warning: Option<String>,
}

pub struct VectorStore {
    file_path: PathBuf,
    points: RwLock<Vec<FacePoint>>,
    meta: RwLock<StoreMeta>,
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
    pub fn load_or_create_with_model(
        file_path: &Path,
        expected_model: &str,
        expected_dim: usize,
    ) -> Result<Self, String> {
        let (mut points, stored_model, stored_dim) = if file_path.is_file() {
            let text = fs::read_to_string(file_path)
                .map_err(|e| format!("Failed reading {}: {e}", file_path.display()))?;
            let parsed: VectorFilePayload = serde_json::from_str(&text)
                .map_err(|e| format!("Failed parsing {}: {e}", file_path.display()))?;
            let inferred_dim = parsed
                .points
                .first()
                .map(|p| p.vector.len())
                .filter(|&d| d > 0)
                .unwrap_or(parsed.embedding_dim.max(parsed.dim));
            let inferred_model =
                if inferred_dim == 512 && parsed.embedding_model == "clip_vit_b32_768" {
                    "mobilefacenet_512".to_string()
                } else {
                    parsed.embedding_model
                };
            (parsed.points, inferred_model, inferred_dim)
        } else {
            (Vec::new(), expected_model.to_string(), expected_dim)
        };

        for pt in &mut points {
            pt.ensure_normalized();
        }

        let mismatch_warning = if !points.is_empty()
            && (stored_dim != expected_dim || stored_model != expected_model)
        {
            Some(format!(
                "Vector store {} contains {} {}-D ({}) vectors, but active encoder expects {}-D ({}). Re-index from data/interim/store-faces/originals/.",
                file_path.display(),
                points.len(),
                stored_dim,
                stored_model,
                expected_dim,
                expected_model
            ))
        } else {
            None
        };

        Ok(Self {
            file_path: file_path.to_path_buf(),
            points: RwLock::new(points),
            meta: RwLock::new(StoreMeta {
                embedding_model: stored_model,
                embedding_dim: stored_dim,
                mismatch_warning,
            }),
        })
    }

    #[cfg(test)]
    pub fn has_dimension_mismatch(&self) -> bool {
        self.meta
            .read()
            .expect("meta lock")
            .mismatch_warning
            .is_some()
    }

    pub fn mismatch_warning(&self) -> Option<String> {
        self.meta
            .read()
            .expect("meta lock")
            .mismatch_warning
            .clone()
    }

    /// Clears stored points and switches the store header to a new embedding model/dimension
    /// prior to automatic re-indexing from `data/interim/store-faces/originals/`.
    pub fn reset_for_model(&self, model: &str, dim: usize) -> Result<(), String> {
        let mut pts = self.points.write().expect("vector store lock");
        let mut meta = self.meta.write().expect("meta lock");
        pts.clear();
        meta.embedding_model = model.to_string();
        meta.embedding_dim = dim;
        meta.mismatch_warning = None;
        self.persist_with_meta(&pts, &meta)
    }

    fn persist_with_meta(&self, pts: &[FacePoint], meta: &StoreMeta) -> Result<(), String> {
        if let Some(parent) = self.file_path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Failed creating dir: {e}"))?;
        }
        let payload = VectorFilePayload {
            collection: "enterprise_faces".to_string(),
            embedding_model: meta.embedding_model.clone(),
            embedding_dim: meta.embedding_dim,
            dim: meta.embedding_dim,
            points: pts.to_vec(),
        };
        let bytes = serde_json::to_vec(&payload).map_err(|e| format!("Serialize error: {e}"))?;
        let tmp = self.file_path.with_extension("json.tmp");
        fs::write(&tmp, bytes).map_err(|e| format!("Write tmp error: {e}"))?;
        fs::rename(&tmp, &self.file_path).map_err(|e| format!("Atomic rename error: {e}"))?;
        Ok(())
    }

    fn persist_locked(&self, pts: &[FacePoint]) -> Result<(), String> {
        let meta = self.meta.read().expect("meta lock").clone();
        self.persist_with_meta(pts, &meta)
    }

    pub fn search(&self, query: &[f32], limit: usize) -> Vec<SearchHit> {
        if query.is_empty() || limit == 0 {
            return Vec::new();
        }
        let q_unit = normalize_l2(query);
        let q_dim = q_unit.len();
        let guard = self.points.read().expect("vector store lock");
        // Strictly guard dimension equality so legacy 768-D CLIP vectors are NEVER compared against 512-D MobileFaceNet vectors.
        let mut scored: Vec<(usize, f32)> = guard
            .iter()
            .enumerate()
            .filter(|(_, pt)| pt.unit_vector.len() == q_dim)
            .map(|(i, pt)| (i, dot_product(&q_unit, &pt.unit_vector)))
            .collect();
        if scored.len() > limit {
            scored.select_nth_unstable_by(limit, |a, b| b.1.total_cmp(&a.1));
            scored.truncate(limit);
        }
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        scored
            .into_iter()
            .map(|(i, score)| SearchHit {
                point: guard[i].clone(),
                score,
            })
            .collect()
    }

    pub fn upsert(&self, mut point: FacePoint) -> Result<(), String> {
        point.ensure_normalized();
        let mut guard = self.points.write().expect("vector store lock");
        {
            let mut meta = self.meta.write().expect("meta lock");
            if guard.is_empty() && !point.vector.is_empty() {
                meta.embedding_dim = point.vector.len();
                meta.embedding_model = if point.vector.len() == 512 {
                    "mobilefacenet_512".to_string()
                } else {
                    "clip_vit_b32_768".to_string()
                };
                meta.mismatch_warning = None;
            } else if !point.vector.is_empty() && point.vector.len() != meta.embedding_dim {
                return Err(format!(
                    "Vector dimension mismatch: attempted to store {}-D vector into {}-D ({}) store.",
                    point.vector.len(),
                    meta.embedding_dim,
                    meta.embedding_model
                ));
            }
        }
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
        assert!((dot_product(&normalize_l2(&b), &pt2.unit_vector)).abs() < 1e-6);
    }

    #[test]
    fn test_vector_dimension_mismatch_guard() {
        let tmp_path = std::env::temp_dir().join(format!(
            "arkib_vec_dim_guard_{}.json",
            uuid::Uuid::new_v4().simple()
        ));
        let store_768 =
            VectorStore::load_or_create_with_model(&tmp_path, "clip_vit_b32_768", 768).unwrap();
        store_768
            .upsert(FacePoint::new(
                "p1".into(),
                "Alice".into(),
                "a.jpg".into(),
                "".into(),
                vec![0.1_f32; 768],
            ))
            .unwrap();
        // Reloading the 768-D store with 512-D MobileFaceNet expectation must flag mismatch and block cross-dim search
        let store_512 =
            VectorStore::load_or_create_with_model(&tmp_path, "mobilefacenet_512", 512).unwrap();
        assert!(store_512.has_dimension_mismatch());
        assert!(store_512.search(&vec![0.1_f32; 512], 5).is_empty());
        assert!(
            store_512
                .upsert(FacePoint::new(
                    "p2".into(),
                    "Bob".into(),
                    "b.jpg".into(),
                    "".into(),
                    vec![0.2_f32; 512],
                ))
                .is_err()
        );
        let _ = fs::remove_file(&tmp_path);
    }
}
