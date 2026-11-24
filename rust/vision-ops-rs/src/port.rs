//! The Rust port of `src/utils/vision_ops.py`.

#![forbid(unsafe_code)]

use serde_json::{Value, json};

/// Port of `compute_ahash_bits` (`src/utils/vision_ops.py:8`).
pub fn compute_ahash_bits(pixels: Vec<i64>) -> Result<String, String> {
    if pixels.is_empty() {
        return Ok(String::new());
    }
    let sum: i128 = pixels.iter().map(|&p| i128::from(p)).sum();
    let average = (sum as f64) / (pixels.len() as f64);
    let min_pixel = average.ceil() as i128;
    let mut out = String::with_capacity(pixels.len());
    for pixel in pixels {
        out.push(if i128::from(pixel) >= min_pixel {
            '1'
        } else {
            '0'
        });
    }
    Ok(out)
}

/// Port of `hash_distance` (`src/utils/vision_ops.py:16`).
pub fn hash_distance(left: String, right: String) -> Result<i64, String> {
    let dist = if left.is_ascii() && right.is_ascii() {
        left.bytes()
            .zip(right.bytes())
            .filter(|(a, b)| a != b)
            .count()
    } else {
        left.chars()
            .zip(right.chars())
            .filter(|(a, b)| a != b)
            .count()
    };
    Ok(dist as i64)
}

/// Port of `compute_text_line_chunks` (`src/utils/vision_ops.py:21`).
pub fn compute_text_line_chunks(
    row_energies: Vec<f64>,
    height: i64,
    max_chunks: i64,
) -> Result<Value, String> {
    if height <= 0 || row_energies.is_empty() {
        return Ok(json!({ "detected_text_lines": 0, "chunk_count": 0, "chunks": [] }));
    }

    let clamped_max = max_chunks.clamp(1, 5) as usize;
    let n = row_energies.len();
    let win = (height / 150).max(3);
    let half_left = win / 2;
    let half_right = (win - 1) / 2;
    let inv_win = 1.0 / (win as f64);

    let mut prefix = Vec::with_capacity(n + 1);
    prefix.push(0.0_f64);
    let mut acc = 0.0_f64;
    for v in row_energies {
        acc += if v.is_finite() { v } else { 0.0 };
        prefix.push(acc);
    }

    let n_i64 = n as i64;
    let mut smooth = Vec::with_capacity(n);
    for i in 0..n {
        let idx = i as i64;
        let lo = idx.saturating_sub(half_left).max(0) as usize;
        let hi = idx.saturating_add(half_right).saturating_add(1).min(n_i64) as usize;
        smooth.push((prefix[hi] - prefix[lo]) * inv_win);
    }

    let mut sorted_vals = smooth.clone();
    sorted_vals.sort_by(f64::total_cmp);
    let pos = 0.30 * ((sorted_vals.len() - 1) as f64);
    let lo_i = pos as usize;
    let hi_i = (lo_i + 1).min(sorted_vals.len() - 1);
    let frac = pos - (lo_i as f64);
    let p30 = sorted_vals[lo_i] * (1.0 - frac) + sorted_vals[hi_i] * frac;
    let thresh = 3.5_f64.max(p30 * 1.35);

    let min_line_h = (height / 140).max(4);
    let mut lines: Vec<(i64, i64)> = Vec::new();
    let mut in_line = false;
    let mut start_y: i64 = 0;
    for (y, &val) in smooth.iter().enumerate() {
        let y_i64 = y as i64;
        let active = val >= thresh;
        if active && !in_line {
            in_line = true;
            start_y = y_i64;
        } else if !active && in_line {
            in_line = false;
            if y_i64 - start_y >= min_line_h {
                lines.push((start_y, y_i64));
            }
        }
    }
    if in_line && n_i64 - start_y >= min_line_h {
        lines.push((start_y, n_i64));
    }

    let num_chunks = if lines.len() <= 4 || height < 420 || clamped_max == 1 {
        1
    } else if lines.len() <= 9 || height < 750 {
        2.min(clamped_max)
    } else {
        3.min(clamped_max)
    };

    let mut chunk_groups: Vec<&[(i64, i64)]> = Vec::new();
    if num_chunks == 1 || lines.len() < num_chunks * 2 {
        chunk_groups.push(&lines);
    } else {
        let mut split_indices: Vec<usize> = Vec::with_capacity(num_chunks - 1);
        let radius = (lines.len() / 6).max(1);
        for k in 1..num_chunks {
            let target =
                ((k * lines.len()) as f64 / (num_chunks as f64)).round_ties_even() as usize;
            let lo = target.saturating_sub(radius).max(1);
            let hi = (target + radius).min(lines.len() - 1);
            let mut best_idx = target;
            let mut best_gap: i64 = -1;
            for idx in lo..=hi {
                if split_indices.last().is_some_and(|&prev| idx <= prev) {
                    continue;
                }
                let gap = lines[idx].0 - lines[idx - 1].1;
                if gap > best_gap {
                    best_gap = gap;
                    best_idx = idx;
                }
            }
            split_indices.push(best_idx);
        }
        let mut prev = 0;
        for s_idx in split_indices {
            chunk_groups.push(&lines[prev..s_idx]);
            prev = s_idx;
        }
        chunk_groups.push(&lines[prev..]);
    }

    let pad = 12_i64.max(((height as f64) * 0.03) as i64);
    let total_groups = chunk_groups.len();
    let mut chunks = Vec::with_capacity(total_groups);
    for (i, group) in chunk_groups.iter().enumerate() {
        let idx = i + 1;
        let mut y0 = if let Some(&(first_y, _)) = group.first() {
            (first_y - pad).max(0)
        } else {
            (((idx - 1) as f64 * (height as f64)) / (total_groups as f64)) as i64
        };
        let mut y1 = if let Some(&(_, last_y)) = group.last() {
            last_y.saturating_add(pad).min(height)
        } else {
            ((idx as f64 * (height as f64)) / (total_groups as f64)) as i64
        };
        if idx == 1 {
            y0 = 0;
        }
        if idx == total_groups {
            y1 = height;
        }
        let label = match (total_groups, i) {
            (1, 0) => "Full Document".to_string(),
            (2, 0) => "Top Section".to_string(),
            (2, 1) => "Bottom Section".to_string(),
            (3, 0) => "Top Section".to_string(),
            (3, 1) => "Middle Section".to_string(),
            (3, 2) => "Bottom Section".to_string(),
            _ => format!("Section {idx}"),
        };
        let denom = height.max(1) as f64;
        let pct_start = ((100_i128 * i128::from(y0)) as f64 / denom).round_ties_even() as i64;
        let pct_end = ((100_i128 * i128::from(y1)) as f64 / denom).round_ties_even() as i64;
        chunks.push(json!({
            "chunk_index": idx,
            "region_label": label,
            "y0": y0,
            "y1": y1.max(y0.saturating_add(1)),
            "vertical_range_pct": format!("{pct_start}%-{pct_end}%"),
            "detected_text_lines": if group.is_empty() { 1 } else { group.len() },
        }));
    }

    Ok(json!({
        "detected_text_lines": lines.len(),
        "chunk_count": chunks.len(),
        "chunks": chunks,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke_vision_ops() {
        assert_eq!(
            compute_ahash_bits(vec![0, 10, 20, 200, 210, 220]).unwrap(),
            "000111"
        );
        assert_eq!(hash_distance("000111".into(), "010101".into()).unwrap(), 2);
        let mut energies = vec![1.0; 900];
        for l in 0..12 {
            let s = 40 + l * 70;
            for v in &mut energies[s..s + 24] {
                *v = 28.0;
            }
        }
        let plan = compute_text_line_chunks(energies, 900, 3).unwrap();
        assert_eq!(plan["detected_text_lines"], 12);
        assert_eq!(plan["chunk_count"], 3);
    }
}
