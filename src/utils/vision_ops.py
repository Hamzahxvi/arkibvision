"""Pure algorithmic image hashing and adaptive OCR line-valley chunking helpers."""

from __future__ import annotations

import math


def compute_ahash_bits(pixels: list[int]) -> str:
    """Compute an average-hash bitstring ('0'/'1') from grayscale pixel intensities."""
    if not pixels:
        return ""
    average = sum(pixels) / len(pixels)
    return "".join("1" if pixel >= average else "0" for pixel in pixels)


def hash_distance(left: str, right: str) -> int:
    """Return the Hamming distance between two perceptual hash strings."""
    return sum(a != b for a, b in zip(left, right))


def compute_text_line_chunks(
    row_energies: list[float],
    height: int,
    max_chunks: int = 3,
) -> dict:
    """Plan horizontal document OCR chunks along natural whitespace valleys between text lines."""
    if height <= 0 or not row_energies:
        return {"detected_text_lines": 0, "chunk_count": 0, "chunks": []}

    clamped_max = max(1, min(max_chunks, 5))
    n = len(row_energies)
    energies = [float(v) if math.isfinite(v) else 0.0 for v in row_energies]

    win = max(3, height // 150)
    half_left = win // 2
    half_right = (win - 1) // 2
    inv_win = 1.0 / float(win)

    # Prefix sums for O(N) moving-average smoothing matching np.convolve(..., mode="same").
    prefix: list[float] = [0.0] * (n + 1)
    acc = 0.0
    for idx, val in enumerate(energies):
        acc += val
        prefix[idx + 1] = acc

    smooth: list[float] = []
    for i in range(n):
        lo = i - half_left
        if lo < 0:
            lo = 0
        hi = i + half_right + 1
        if hi > n:
            hi = n
        smooth.append((prefix[hi] - prefix[lo]) * inv_win)

    sorted_vals = sorted(smooth)
    pos = 0.30 * (len(sorted_vals) - 1)
    lo_i = int(pos)
    hi_i = min(lo_i + 1, len(sorted_vals) - 1)
    frac = pos - lo_i
    p30 = sorted_vals[lo_i] * (1.0 - frac) + sorted_vals[hi_i] * frac
    thresh = max(3.5, p30 * 1.35)

    min_line_h = max(4, height // 140)
    lines: list[tuple[int, int]] = []
    in_line = False
    start_y = 0
    for y, val in enumerate(smooth):
        active = val >= thresh
        if active and not in_line:
            in_line = True
            start_y = y
        elif not active and in_line:
            in_line = False
            if y - start_y >= min_line_h:
                lines.append((start_y, y))
    if in_line and n - start_y >= min_line_h:
        lines.append((start_y, n))

    if len(lines) <= 4 or height < 420 or clamped_max == 1:
        num_chunks = 1
    elif len(lines) <= 9 or height < 750:
        num_chunks = min(2, clamped_max)
    else:
        num_chunks = min(3, clamped_max)

    chunk_groups: list[list[tuple[int, int]]] = []
    if num_chunks == 1 or len(lines) < num_chunks * 2:
        chunk_groups = [lines]
    else:
        split_indices: list[int] = []
        for k in range(1, num_chunks):
            target = int(round(k * len(lines) / num_chunks))
            lo = max(1, target - max(1, len(lines) // 6))
            hi = min(len(lines) - 1, target + max(1, len(lines) // 6))
            best_idx = target
            best_gap = -1
            for idx in range(lo, hi + 1):
                if split_indices and idx <= split_indices[-1]:
                    continue
                gap = lines[idx][0] - lines[idx - 1][1]
                if gap > best_gap:
                    best_gap = gap
                    best_idx = idx
            split_indices.append(best_idx)
        prev = 0
        for s_idx in split_indices:
            chunk_groups.append(lines[prev:s_idx])
            prev = s_idx
        chunk_groups.append(lines[prev:])

    pad = max(12, int(height * 0.03))
    labels_by_count = {
        1: ["Full Document"],
        2: ["Top Section", "Bottom Section"],
        3: ["Top Section", "Middle Section", "Bottom Section"],
    }
    region_labels = labels_by_count.get(
        len(chunk_groups),
        [f"Section {i + 1}" for i in range(len(chunk_groups))],
    )

    chunks: list[dict] = []
    total_groups = len(chunk_groups)
    for idx, group in enumerate(chunk_groups, start=1):
        if group:
            y0 = max(0, group[0][0] - pad)
            y1 = min(height, group[-1][1] + pad)
        else:
            y0 = int((idx - 1) * height / total_groups)
            y1 = int(idx * height / total_groups)
        if idx == 1:
            y0 = 0
        if idx == total_groups:
            y1 = height
        pct_start = round(100 * y0 / max(1, height))
        pct_end = round(100 * y1 / max(1, height))
        chunks.append(
            {
                "chunk_index": idx,
                "region_label": region_labels[idx - 1],
                "y0": y0,
                "y1": max(y0 + 1, y1),
                "vertical_range_pct": f"{pct_start}%-{pct_end}%",
                "detected_text_lines": len(group) if group else 1,
            }
        )

    return {
        "detected_text_lines": len(lines),
        "chunk_count": len(chunks),
        "chunks": chunks,
    }
# >>> rust-anything: vision-ops-rs is on. `rust-anything disable ../../rust/vision-ops-rs` removes this block.
def _rust_anything_vision_ops_rs():
    import importlib, os, sys
    mode = os.environ.get("RUST_ANYTHING_MODE")
    if mode == "old" or os.environ.get("RUST_ANYTHING_UNIT_VISION_OPS") == "original":
        return
    at = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../rust/vision-ops-rs/python"))
    sys.path.insert(0, at)
    try:
        native = importlib.import_module("vision_ops_rs")
    except ImportError:
        if mode == "new":
            raise
        return
    finally:
        sys.path.remove(at)
    globals().update({name: getattr(native, name) for name in ["compute_ahash_bits", "hash_distance", "compute_text_line_chunks"]})


_rust_anything_vision_ops_rs()
del _rust_anything_vision_ops_rs
# <<< rust-anything: vision-ops-rs
