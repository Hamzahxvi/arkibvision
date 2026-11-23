import ast
from collections import Counter
from io import BytesIO
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import subprocess
import sys
from threading import Lock
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

import uvicorn
import numpy as np
from PIL import Image, ImageEnhance, ImageOps
from mcp.server.mcpserver import MCPServer
from mcp.server.transport_security import TransportSecuritySettings
from qdrant_client.models import FieldCondition, Filter, MatchValue
from RojakFace import LocalStorage, RojakFaceManager
from src.utils.face_utils import (
    BASE_DIR,
    COLLECTION_NAME,
    IMAGE_EXTS,
    LIBRECHAT_DIR,
    LIBRECHAT_URL,
    PROFILE_DESCRIPTIONS,
    PROFILE_DIR,
    PROFILE_WEB_DIR,
    STORE_FACES_DIR,
    primary_face_bytes,
    profile_slug as _profile_slug,
    sync_profile_folder,
)
from src.utils.jawi_decoder import decode_jawi_text
from src.utils.paths import INTERIM_DIR, PROCESSED_DIR
from src.utils.qdrant_helper import get_qdrant_client

IMAGE_DIRS = [
    LIBRECHAT_DIR / "images",
    LIBRECHAT_DIR / "client" / "public" / "images",
    LIBRECHAT_DIR / "uploads",
]

# Role & Security Configuration
ADMIN_KEY_FILE = BASE_DIR / ".rojakface_admin_key"
ADMIN_KEY = (os.getenv("ROJAKFACE_ADMIN_KEY") or "").strip()
if not ADMIN_KEY:
    if ADMIN_KEY_FILE.is_file():
        ADMIN_KEY = ADMIN_KEY_FILE.read_text(encoding="utf-8").strip()
    if not ADMIN_KEY:
        ADMIN_KEY = secrets.token_urlsafe(24)
        ADMIN_KEY_FILE.write_text(ADMIN_KEY, encoding="utf-8")
        print(f"Generated a new admin key and saved it to {ADMIN_KEY_FILE}.")
PENDING_DIR = INTERIM_DIR / "pending_requests"
PENDING_IMAGES_DIR = PENDING_DIR / "images"
PENDING_FILE = PENDING_DIR / "requests.json"
_pending_lock = Lock()
_admin_sessions: dict[str, float] = {}
ADMIN_SESSION_COOKIE = "rojakface_admin_session"
ADMIN_SESSION_TTL_SECONDS = 60 * 60 * 8

PENDING_IMAGES_DIR.mkdir(parents=True, exist_ok=True)
if not PENDING_FILE.exists():
    PENDING_FILE.write_text("{}", encoding="utf-8")

# Hard floor for CLIP embeddings (prevents false positives like 0.725 stranger matches)
MIN_CLIP_IDENTIFY_THRESHOLD = 0.88
PERCEPTUAL_HASH_DISTANCE = 6
MAX_AUTO_IMAGE_AGE_SECONDS = 60
_image_cache_lock = Lock()
_identification_cache: list[dict] = []
_latest_image_from_chat_record = False


def _verify_admin(admin_key: str = "") -> bool:
    """Return True only when a non-empty key matches the configured admin key."""
    if not ADMIN_KEY:
        return False
    candidate = (admin_key or "").strip()
    if not candidate:
        return False
    return secrets.compare_digest(candidate, ADMIN_KEY)


def _resolve_request(all_reqs: dict, identifier: str) -> tuple[str | None, dict | None]:
    """Flexibly resolve request by number (1, 2), name ('Farid Azizan'), 'latest', or request_id."""
    clean = (identifier or "").strip().lstrip("#").strip()
    if not clean:
        return None, None

    if clean in all_reqs:
        return clean, all_reqs[clean]

    pending_chronological = [
        (k, v) for k, v in all_reqs.items()
        if v.get("status") == "pending"
    ]
    pending_chronological.sort(key=lambda x: x[1].get("submitted_at") or "")

    if clean.lower() == "latest":
        if pending_chronological:
            return pending_chronological[-1][0], pending_chronological[-1][1]
        return None, None

    if clean.isdigit():
        idx = int(clean) - 1
        if 0 <= idx < len(pending_chronological):
            return pending_chronological[idx][0], pending_chronological[idx][1]
        return None, None

    for req_id, req in pending_chronological:
        target_name = (req.get("identity") or "").casefold()
        if clean.casefold() == target_name or clean.casefold() in target_name:
            return req_id, req

    for req_id, req in all_reqs.items():
        if clean.upper() in req_id.upper():
            return req_id, req

    return None, None


def _load_pending_requests() -> dict:
    with _pending_lock:
        if not PENDING_FILE.exists():
            return {}
        try:
            return json.loads(PENDING_FILE.read_text(encoding="utf-8"))
        except Exception:
            return {}


def _save_pending_requests(data: dict) -> None:
    with _pending_lock:
        PENDING_FILE.write_text(json.dumps(data, indent=2), encoding="utf-8")


def _finalize_pending_request(
    all_reqs: dict,
    req_id: str,
    req: dict,
    status: str,
    comment: str,
) -> None:
    """Remove a reviewed request from pending_requests, clean up staged images, and log history."""
    archived = dict(req)
    archived["status"] = status
    archived["reviewed_at"] = time.strftime("%Y-%m-%d %H:%M:%S")
    archived["admin_comment"] = comment

    for staged in PENDING_IMAGES_DIR.glob(f"{req_id}_*"):
        staged.unlink(missing_ok=True)
    for key in ("primary_crop", "source_image"):
        rel = req.get(key)
        if rel:
            (PENDING_IMAGES_DIR / Path(rel).name).unlink(missing_ok=True)

    all_reqs.pop(req_id, None)
    with _pending_lock:
        PENDING_FILE.write_text(json.dumps(all_reqs, indent=2), encoding="utf-8")
        with (PENDING_DIR / "history.jsonl").open("a", encoding="utf-8") as fh:
            fh.write(json.dumps(archived) + "\n")


def _find_docker_binary() -> Path | None:
    which_docker = shutil.which("docker")
    if which_docker:
        return Path(which_docker)
    candidates = [
        Path(r"C:\Program Files\Docker\Docker\resources\bin\docker.exe"),
        Path(r"C:\Program Files\Docker\Docker\resources\docker.exe"),
        Path.home() / "AppData" / "Local" / "Programs" / "Docker" / "resources" / "bin" / "docker.exe",
    ]
    for candidate in candidates:
        if candidate.is_file():
            return candidate
    return None


def _perceptual_hash(image_path: Path) -> str:
    """Return a small average hash that survives JPEG/PNG re-encoding and resizing."""
    with Image.open(image_path) as image:
        image = ImageOps.exif_transpose(image).convert("L").resize((16, 16))
        pixels = list(image.getdata())
    average = sum(pixels) / len(pixels)
    return "".join("1" if pixel >= average else "0" for pixel in pixels)


def _hash_distance(left: str, right: str) -> int:
    return sum(a != b for a, b in zip(left, right))


def _cached_identification(image_path: Path, threshold: float) -> dict | None:
    image_hash = _perceptual_hash(image_path)
    with _image_cache_lock:
        for entry in _identification_cache:
            if (
                entry["threshold"] == threshold
                and _hash_distance(image_hash, entry["hash"]) <= PERCEPTUAL_HASH_DISTANCE
            ):
                return {**entry, "file": image_path.name, "deduplicated": True}
    return None


def _cache_identification(image_path: Path, threshold: float, result: dict) -> None:
    entry = {
        "hash": _perceptual_hash(image_path),
        "threshold": threshold,
        "matches_count": result["matches_count"],
        "matches": result["matches"],
        "cached_from": image_path.name,
    }
    with _image_cache_lock:
        _identification_cache.append(entry)
        if len(_identification_cache) > 128:
            del _identification_cache[:-128]


def _clear_identification_cache() -> None:
    """Drop cached identification results after any identity mutation."""
    with _image_cache_lock:
        _identification_cache.clear()


AUTO_GALLERY_MIN_SCORE = 0.90
AUTO_GALLERY_MAX_IMAGES = 15


def _maybe_save_recognized_image(identity: str, image_path: Path, score: float) -> bool:
    """Save a high-confidence, non-duplicate recognized photo to the profile gallery."""
    if not identity or identity == "Unknown" or float(score or 0.0) < AUTO_GALLERY_MIN_SCORE:
        return False
    images_dir = PROFILE_DIR / _profile_slug(identity) / "images"
    existing_files = [
        p for p in images_dir.glob("*")
        if p.is_file() and p.suffix.lower() in IMAGE_EXTS
    ] if images_dir.is_dir() else []
    if len(existing_files) >= AUTO_GALLERY_MAX_IMAGES:
        return False
    try:
        new_hash = _perceptual_hash(image_path)
    except Exception:
        return False
    for existing in existing_files:
        try:
            if _hash_distance(new_hash, _perceptual_hash(existing)) <= PERCEPTUAL_HASH_DISTANCE:
                return False
        except Exception:
            continue
    _sync_profile_folder(identity, image_path)
    return True


def _primary_face_image(image_path: Path) -> tuple[bytes, str]:
    """Crop the largest detected face so enrollment cannot label bystanders."""
    return primary_face_bytes(manager.detector, image_path)


def _format_identity_name(identity: str) -> str:
    """Normalize whitespace and capitalize the first letter of each word."""
    words = (identity or "").strip().split()
    formatted = []
    for word in words:
        if word.islower() or word.isupper():
            formatted.append(word[:1].upper() + word[1:].lower())
        else:
            formatted.append(word[:1].upper() + word[1:])
    return " ".join(formatted)


def _canonical_identity(identity: str) -> str:
    """Reuse the existing enrolled spelling when a name differs by case or spacing."""
    clean_identity = _format_identity_name(identity)
    if not clean_identity or not PROFILE_DIR.exists():
        return clean_identity
    norm_target = re.sub(r"[^a-z0-9]", "", clean_identity.casefold())
    matches = []
    fuzzy_matches = []
    for metadata_path in PROFILE_DIR.glob("*/metadata.json"):
        try:
            stored_identity = str(json.loads(metadata_path.read_text(encoding="utf-8")).get("identity", "")).strip()
        except (json.JSONDecodeError, OSError):
            continue
        if not stored_identity:
            continue
        if stored_identity.casefold() == clean_identity.casefold():
            matches.append(stored_identity)
        elif norm_target and re.sub(r"[^a-z0-9]", "", stored_identity.casefold()) == norm_target:
            fuzzy_matches.append(stored_identity)
    pool = matches or fuzzy_matches
    if not pool:
        return clean_identity
    exact = next((name for name in pool if name == clean_identity), None)
    return exact or next((name for name in pool if name[:1].isupper()), pool[0])


def _sync_profile_folder(identity: str, source_image: Path | None = None) -> None:
    """Keep the filesystem profile record synchronized after enrollment."""
    sync_profile_folder(qdrant, identity, source_image)


def _collect_referenced_face_files(identity: str | None = None) -> tuple[set[str], set[str]]:
    """Return (crop relative paths, original filenames) referenced by Qdrant payloads.

    When ``identity`` is provided only that identity's vectors are considered.
    """
    referenced_crops: set[str] = set()
    referenced_originals: set[str] = set()
    offset = None
    scroll_kwargs: dict = {}
    if identity is not None:
        scroll_kwargs["scroll_filter"] = Filter(
            must=[FieldCondition(key="identity", match=MatchValue(value=identity))]
        )
    while True:
        points, offset = qdrant.scroll(
            collection_name=COLLECTION_NAME,
            limit=256,
            offset=offset,
            with_payload=True,
            with_vectors=False,
            **scroll_kwargs,
        )
        for point in points:
            payload = point.payload or {}
            crop_rel = str(payload.get("crop_path") or "").replace("\\", "/").lstrip("/")
            if crop_rel:
                referenced_crops.add(crop_rel)
            filename = str(payload.get("filename") or "")
            if filename:
                referenced_originals.add(Path(filename).name)
        if offset is None:
            break
    return referenced_crops, referenced_originals


def _purge_identity_files(identity: str) -> dict:
    """Remove stored crops/originals referenced by a specific identity's vectors."""
    referenced_crops, referenced_originals = _collect_referenced_face_files(identity=identity)
    store_root = STORE_FACES_DIR.resolve()
    removed_crops: list[str] = []
    removed_originals: list[str] = []

    for rel in referenced_crops:
        target = (store_root / rel).resolve()
        try:
            target.relative_to(store_root)
        except ValueError:
            continue
        if target.is_file():
            target.unlink(missing_ok=True)
            removed_crops.append(rel)

    for name in referenced_originals:
        target = store_root / "originals" / name
        if target.is_file():
            target.unlink(missing_ok=True)
            removed_originals.append(name)

    return {"removed_crops": removed_crops, "removed_originals": removed_originals}


def _purge_unreferenced_face_files() -> dict:
    """Delete stored crops/originals that no Qdrant vector references."""
    referenced_crops, referenced_originals = _collect_referenced_face_files()
    if not referenced_crops and not referenced_originals:
        return {
            "removed_crops": [],
            "removed_originals": [],
            "skipped": "no referenced files found; database appears empty",
        }

    store_root = STORE_FACES_DIR.resolve()
    removed_crops: list[str] = []
    removed_originals: list[str] = []

    for path in (store_root / "crops").glob("*"):
        if not path.is_file():
            continue
        rel = path.relative_to(store_root).as_posix()
        if rel not in referenced_crops:
            path.unlink(missing_ok=True)
            removed_crops.append(rel)

    for path in (store_root / "originals").glob("*"):
        if not path.is_file():
            continue
        if path.name not in referenced_originals:
            path.unlink(missing_ok=True)
            removed_originals.append(path.name)

    return {"removed_crops": removed_crops, "removed_originals": removed_originals}


def _create_enrollment_request(
    identity: str,
    image_path: Path,
    proposed_metadata: dict | None = None,
    notes: str = "",
    role: str = "guest",
) -> dict:
    """Stage an enrollment request into the pending approval queue."""
    clean_identity = _canonical_identity(identity)
    if not re.fullmatch(r"[A-Za-z][A-Za-z0-9 .'-]{0,79}", clean_identity):
        return {
            "status": "rejected",
            "reason": "Identity must be a plain name (letters, numbers, spaces, dots, dashes, apostrophes).",
            "identity": clean_identity,
        }

    # Anti-false-positive duplicate check: does this face already match a known identity?
    safe_threshold = MIN_CLIP_IDENTIFY_THRESHOLD
    existing = manager.process(str(image_path), mode="identify", threshold=safe_threshold)
    known = [
        {k: v for k, v in match.items() if k != "face_image"}
        for match in existing
        if match.get("identity") not in {None, "Unknown"}
    ]
    if known:
        matched_identity = known[0]["identity"]
        if proposed_metadata:
            return _create_metadata_update_request(
                identity=matched_identity,
                proposed_metadata=proposed_metadata,
                notes=notes,
                role=role,
            )
        return {
            "status": "rejected",
            "reason": f"Face already matches enrolled identity '{matched_identity}' (confidence: {known[0].get('similarity')}). Cannot submit duplicate enrollment.",
            "existing_matches": known,
        }

    try:
        primary_bytes, _ = _primary_face_image(image_path)
    except ValueError as exc:
        return {"status": "rejected", "reason": str(exc)}

    timestamp_str = time.strftime("%Y%m%d_%H%M%S")
    req_id = f"REQ-{timestamp_str}-{os.urandom(3).hex().upper()}"

    source_dest = PENDING_IMAGES_DIR / f"{req_id}_source{image_path.suffix.lower()}"
    crop_dest = PENDING_IMAGES_DIR / f"{req_id}_primary.jpg"
    shutil.copy2(image_path, source_dest)
    crop_dest.write_bytes(primary_bytes)

    req_data = {
        "request_id": req_id,
        "type": "enrollment",
        "identity": clean_identity,
        "status": "pending",
        "role": role,
        "submitted_at": time.strftime("%Y-%m-%d %H:%M:%S"),
        "source_image": str(source_dest.relative_to(BASE_DIR)),
        "primary_crop": str(crop_dest.relative_to(BASE_DIR)),
        "proposed_metadata": proposed_metadata or {},
        "notes": notes,
        "reviewed_at": None,
        "admin_comment": None,
    }

    all_reqs = _load_pending_requests()
    all_reqs[req_id] = req_data
    _save_pending_requests(all_reqs)

    return {
        "status": "staged_for_admin_review",
        "role": role,
        "request_id": req_id,
        "identity": clean_identity,
        "message": (
            f"Enrollment request for '{clean_identity}' submitted for Admin verification (Request ID: {req_id}). "
            "To prevent false enrollments, Guest users cannot write directly to the database. "
            "An Admin must review and approve this request."
        ),
        "details": {
            "request_id": req_id,
            "identity": clean_identity,
            "submitted_at": req_data["submitted_at"],
            "proposed_metadata": req_data["proposed_metadata"],
        },
    }


def _create_metadata_update_request(
    identity: str,
    proposed_metadata: dict,
    notes: str = "",
    role: str = "guest",
) -> dict:
    """Stage a metadata update request into the pending approval queue."""
    clean_identity = _canonical_identity(identity)
    if not clean_identity:
        return {"status": "rejected", "reason": "Identity name cannot be blank."}

    # Verify identity exists in Qdrant
    identities = json.loads(rojakface_list_identities()).get("identities", {})
    canonical = next((name for name in identities if name.casefold() == clean_identity.casefold()), None)
    if not canonical:
        return {
            "status": "rejected",
            "reason": f"Identity '{clean_identity}' is not found in the enrolled database.",
            "available_identities": sorted(identities.keys()),
        }

    timestamp_str = time.strftime("%Y%m%d_%H%M%S")
    req_id = f"REQ-{timestamp_str}-{os.urandom(3).hex().upper()}"

    req_data = {
        "request_id": req_id,
        "type": "metadata_update",
        "identity": canonical,
        "status": "pending",
        "role": role,
        "submitted_at": time.strftime("%Y-%m-%d %H:%M:%S"),
        "proposed_metadata": proposed_metadata,
        "notes": notes,
        "reviewed_at": None,
        "admin_comment": None,
    }

    all_reqs = _load_pending_requests()
    all_reqs[req_id] = req_data
    _save_pending_requests(all_reqs)

    return {
        "status": "staged_for_admin_review",
        "role": role,
        "request_id": req_id,
        "identity": canonical,
        "message": (
            f"Metadata update for '{canonical}' submitted for Admin review (Request ID: {req_id}). "
            "Guest users cannot modify metadata directly."
        ),
        "details": req_data,
    }


storage = LocalStorage(base_path=str(STORE_FACES_DIR))
qdrant = get_qdrant_client(PROCESSED_DIR)
manager = RojakFaceManager(qdrant_client=qdrant, storage=storage, collection=COLLECTION_NAME)
manager.init_storage()

try:
    _purge_result = _purge_unreferenced_face_files()
    _purged_crops = len(_purge_result.get("removed_crops", []))
    _purged_originals = len(_purge_result.get("removed_originals", []))
    if _purged_crops or _purged_originals:
        print(f"Purged unreferenced face files: {_purged_crops} crops, {_purged_originals} originals.")
except Exception as exc:
    print(f"Warning: could not purge unreferenced face files: {exc}")

mcp = MCPServer("RojakFace-Service")


def _latest_uploaded_image() -> Path | None:
    global _latest_image_from_chat_record
    _latest_image_from_chat_record = False

    docker_bin = _find_docker_binary()
    if docker_bin is not None:
        mongo_container = os.getenv("LIBRECHAT_MONGO_CONTAINER", "chat-mongodb")
        mongo_db = os.getenv("LIBRECHAT_MONGO_DB", "LibreChat")
        query = (
            'db.messages.find({"files.0.type":/image/},{files:1,createdAt:1})'
            '.sort({createdAt:-1}).limit(1)'
            '.forEach(x=>print(JSON.stringify(x)))'
        )
        try:
            completed = subprocess.run(
                [str(docker_bin), "exec", mongo_container, "mongosh", mongo_db, "--quiet", "--eval", query],
                capture_output=True,
                text=True,
                timeout=5,
                check=False,
            )
            for line in reversed(completed.stdout.splitlines()):
                try:
                    record = json.loads(line)
                    file_record = (record.get("files") or [None])[0] or {}
                    stored_path = str(file_record.get("filepath") or "")
                    if stored_path.startswith("/images/"):
                        candidate = (LIBRECHAT_DIR / "images" / stored_path.removeprefix("/images/")).resolve()
                    elif stored_path.startswith("/uploads/"):
                        candidate = (LIBRECHAT_DIR / "uploads" / stored_path.removeprefix("/uploads/")).resolve()
                    else:
                        candidate = Path(stored_path)
                    if candidate.suffix.lower() in IMAGE_EXTS and candidate.is_file():
                        _latest_image_from_chat_record = True
                        return candidate
                except (json.JSONDecodeError, OSError, IndexError, TypeError):
                    continue
        except (OSError, subprocess.SubprocessError):
            pass

    candidates: list[Path] = []
    for folder in IMAGE_DIRS:
        if folder.exists():
            for p in folder.rglob("*"):
                if p.is_file() and p.suffix.lower() in IMAGE_EXTS:
                    candidates.append(p)
    if not candidates:
        return None

    return max(candidates, key=lambda p: max(p.stat().st_atime, p.stat().st_mtime)).resolve()


def _resolve_image_path(image_path: str = "") -> Path:
    cleaned = (image_path or "").strip().strip('"').strip("'")
    if cleaned and cleaned.lower() not in {"latest", "uploaded", "image", "chat_image"}:
        candidate = Path(cleaned)
        if candidate.is_file():
            return candidate.resolve()
        local_candidate = BASE_DIR / cleaned
        if local_candidate.is_file():
            return local_candidate.resolve()
        for folder in IMAGE_DIRS:
            if folder.exists():
                matches = list(folder.rglob(candidate.name))
                if matches:
                    return matches[0].resolve()
        raise FileNotFoundError(f"Specified image '{image_path}' not found on disk.")

    latest = _latest_uploaded_image()
    if latest is not None:
        age_seconds = max(0, time.time() - max(latest.stat().st_atime, latest.stat().st_mtime))
        if not _latest_image_from_chat_record and age_seconds > MAX_AUTO_IMAGE_AGE_SECONDS:
            raise FileNotFoundError(
                "The current chat attachment could not be resolved safely. "
                "The newest stored image is older than 60 seconds and may belong "
                "to another message. Re-upload the image in the current message."
            )
        return latest

    raise FileNotFoundError(
        "No image attachment found in LibreChat. Please attach an image in your message."
    )


@mcp.tool()
def rojakface_detect_faces(image_path: str = "") -> str:
    """Detect faces in an image using YuNet. Leave image_path empty for chat-attached images."""
    resolved = _resolve_image_path(image_path)
    detections = manager.detector.extract_features(str(resolved))
    summary = [
        {
            "face_index": idx,
            "box_xywh": item["box"],
            "confidence": round(item["confidence"], 4),
            "embeddable": bool(item["embedding"]),
        }
        for idx, item in enumerate(detections)
    ]
    return json.dumps(
        {"file": resolved.name, "faces_detected": len(summary), "detections": summary},
        indent=2,
    )


def _build_ocr_document_chunks(image_path: Path, max_chunks: int = 3) -> dict:
    """Slice a document/handwriting image along natural horizontal line valleys into contrast-enhanced chunks."""
    max_chunks = max(1, min(int(max_chunks or 3), 5))
    with Image.open(image_path) as raw_img:
        img = ImageOps.exif_transpose(raw_img).convert("RGB")
        width, height = img.size
        gray = np.asarray(img.convert("L"), dtype=np.float32)

    if width > 2:
        row_energy = np.abs(np.diff(gray, axis=1)).mean(axis=1)
    else:
        row_energy = np.zeros(height, dtype=np.float32)

    win = max(3, height // 150)
    kernel = np.ones(win, dtype=np.float32) / float(win)
    smooth = np.convolve(row_energy, kernel, mode="same")
    thresh = max(3.5, float(np.percentile(smooth, 30)) * 1.35)
    active = smooth >= thresh

    min_line_h = max(4, height // 140)
    lines: list[tuple[int, int]] = []
    in_line = False
    start_y = 0
    for y, val in enumerate(active):
        if val and not in_line:
            in_line = True
            start_y = y
        elif not val and in_line:
            in_line = False
            if y - start_y >= min_line_h:
                lines.append((start_y, y))
    if in_line and height - start_y >= min_line_h:
        lines.append((start_y, height))

    if len(lines) <= 4 or height < 420 or max_chunks == 1:
        num_chunks = 1
    elif len(lines) <= 9 or height < 750:
        num_chunks = min(2, max_chunks)
    else:
        num_chunks = min(3, max_chunks)

    chunk_groups: list[list[tuple[int, int]]] = []
    if num_chunks == 1 or len(lines) < num_chunks * 2:
        chunk_groups = [lines]
        num_chunks = 1
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

    out_dir = LIBRECHAT_DIR / "images" / "ocr-chunks"
    out_dir.mkdir(parents=True, exist_ok=True)
    stem = re.sub(r"[^A-Za-z0-9._-]+", "_", image_path.stem)[:48]
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
    with Image.open(image_path) as raw_img:
        base_img = ImageOps.exif_transpose(raw_img).convert("RGB")
        for idx, group in enumerate(chunk_groups, start=1):
            if group:
                y0 = max(0, group[0][0] - pad)
                y1 = min(height, group[-1][1] + pad)
            else:
                y0 = int((idx - 1) * height / len(chunk_groups))
                y1 = int(idx * height / len(chunk_groups))
            if idx == 1:
                y0 = 0
            if idx == len(chunk_groups):
                y1 = height
            crop = base_img.crop((0, y0, width, max(y0 + 1, y1)))
            enhanced = ImageOps.autocontrast(crop, cutoff=1)
            enhanced = ImageEnhance.Sharpness(enhanced).enhance(1.4)
            chunk_name = f"{stem}_chunk_{idx}.jpg"
            enhanced.save(out_dir / chunk_name, format="JPEG", quality=92)
            chunks.append(
                {
                    "chunk_index": idx,
                    "region_label": region_labels[idx - 1],
                    "vertical_range_pct": f"{round(100 * y0 / max(1, height))}%-{round(100 * y1 / max(1, height))}%",
                    "box_xywh": [0, y0, width, max(1, y1 - y0)],
                    "detected_text_lines": len(group) if group else 1,
                    "enhanced_chunk_url": f"{LIBRECHAT_URL}/images/ocr-chunks/{chunk_name}",
                }
            )

    return {
        "file": image_path.name,
        "image_dimensions": {"width": width, "height": height},
        "total_chunks": len(chunks),
        "total_detected_text_lines": len(lines),
        "chunks": chunks,
        "anti_hallucination_guide": (
            "Read the document/handwriting strictly chunk-by-chunk from Top to Bottom, matching the "
            "detected_text_lines count in each chunk. Transcribe ONLY characters visibly present in the image. "
            "Never invent extra lines, never guess missing words from memory, and mark any illegible characters with [?]."
        ),
    }


@mcp.tool()
def rojakface_ocr_document(image_path: str = "", max_chunks: int = 3) -> str:
    """Chunk and enhance an attached document or handwriting image (Jawi, Sanskrit, Tamil, Mandarin, Malay, English) along natural line valleys to prevent OCR hallucination."""
    try:
        resolved = _resolve_image_path(image_path)
    except FileNotFoundError as exc:
        return json.dumps({"status": "unresolved", "reason": str(exc)}, indent=2)
    chunks_info = _build_ocr_document_chunks(resolved, max_chunks=max_chunks)
    chunks_info["jawi_orthography_guide"] = (
        "If the image contains Jawi script (Arabic-script Bahasa Melayu): "
        "1) First read all lines from the image and call rojakface_decode_jawi_mcp_rojakface-tools with your raw Jawi transcription (or rough phonetic reading) to convert 1-dot/3-dot Jawi letters (ڤ=p, ڠ=ng, ݢ=g, چ=c, ڽ=ny, final ى=-a, ؤ=o, ٢=word reduplication) into exact Standard Bahasa Melayu Rumi. "
        "2) Never output gibberish Arabic phonetics like 'Fimboakan' or 'Deygan' — always output the exact Standard Bahasa Melayu Rumi returned by rojakface_decode_jawi_mcp_rojakface-tools and translate the Malay meaning clearly."
    )
    return json.dumps({"status": "ready", **chunks_info}, indent=2)


@mcp.tool()
def rojakface_decode_jawi(text: str) -> str:
    """Decode raw Jawi script or rough phonetic transliteration into corrected 3-dot Jawi (ڤ ڠ ݢ چ ڽ) and natural Standard Bahasa Melayu (Rumi)."""
    decoded = decode_jawi_text(text)
    return json.dumps(
        {
            "status": "decoded",
            **decoded,
            "display_instruction": (
                "Use corrected_jawi_lines for the Original Jawi Script section, "
                "use standard_malay_rumi_lines for the Rumi (Standard Bahasa Melayu) section, "
                "and provide the true Bahasa Melayu / English meaning for the Translation / Meaning section. "
                "Never output raw phonetic gibberish like 'Fimboakan'."
            ),
        },
        indent=2,
    )


@mcp.tool()
def rojakface_identify_faces(image_path: str = "", threshold: float = 0.88, save_new_image: bool = True) -> str:
    """Identify faces against enrolled identities with strict anti-false-positive threshold (All Roles)."""
    try:
        resolved = _resolve_image_path(image_path)
    except FileNotFoundError as exc:
        return json.dumps({"status": "unresolved", "reason": str(exc)}, indent=2)
    safe_threshold = max(float(threshold), MIN_CLIP_IDENTIFY_THRESHOLD)
    cached = _cached_identification(resolved, safe_threshold)
    if cached is not None:
        cached_matches = cached["matches"]
        cached_names = [m.get("identity") for m in cached_matches if m.get("identity") not in {None, "Unknown"}]
        matched_profiles = [
            profile
            for name in dict.fromkeys(cached_names)
            if (profile := _public_profile_display(name)) is not None
        ]
        cached_status = (
            "MATCHED"
            if cached_names
            else ("NO_FACE_DETECTED" if cached["matches_count"] == 0 else "NOT_IN_DATABASE")
        )
        ocr_chunks = _build_ocr_document_chunks(resolved) if cached_status == "NO_FACE_DETECTED" else None
        return json.dumps(
            {
                "file": cached["file"],
                "database_status": cached_status,
                "faces_detected": cached["matches_count"],
                "matched_identities": cached_names,
                "matched_profiles": matched_profiles,
                "ocr_document_chunks": ocr_chunks,
                "applied_threshold": safe_threshold,
                "matches_count": cached["matches_count"],
                "matches": cached_matches,
                "deduplicated": True,
                "cached_from": cached["cached_from"],
            },
            indent=2,
        )
    matches = manager.process(str(resolved), mode="identify", threshold=safe_threshold)
    clean_matches = [
        {k: v for k, v in m.items() if k != "face_image"} for m in matches
    ]
    matched_names = [m.get("identity") for m in clean_matches if m.get("identity") not in {None, "Unknown"}]
    gallery_saved = False
    if save_new_image and len(clean_matches) == 1 and len(matched_names) == 1:
        score = clean_matches[0].get("similarity", clean_matches[0].get("confidence", 0.0))
        gallery_saved = _maybe_save_recognized_image(matched_names[0], resolved, float(score or 0.0))
    matched_profiles = [
        profile
        for name in dict.fromkeys(matched_names)
        if (profile := _public_profile_display(name)) is not None
    ]
    db_status = (
        "MATCHED"
        if matched_names
        else ("NO_FACE_DETECTED" if len(clean_matches) == 0 else "NOT_IN_DATABASE")
    )
    ocr_chunks = _build_ocr_document_chunks(resolved) if db_status == "NO_FACE_DETECTED" else None
    result = {
        "file": resolved.name,
        "database_status": db_status,
        "faces_detected": len(clean_matches),
        "matched_identities": matched_names,
        "matched_profiles": matched_profiles,
        "ocr_document_chunks": ocr_chunks,
        "gallery_image_saved": gallery_saved,
        "applied_threshold": safe_threshold,
        "matches_count": len(clean_matches),
        "matches": clean_matches,
        "deduplicated": False,
    }
    _cache_identification(resolved, safe_threshold, result)
    return json.dumps(result, indent=2)


@mcp.tool()
def rojakface_request_enrollment(
    identity: str,
    image_path: str = "",
    proposed_metadata: str = "",
    notes: str = "",
) -> str:
    """Guest / User tool: Submit an enrollment request for an unknown face. Staged for Admin verification."""
    try:
        resolved = _resolve_image_path(image_path)
    except FileNotFoundError as exc:
        return json.dumps({"status": "unresolved", "reason": str(exc)}, indent=2)

    meta_dict = _parse_proposed_metadata(proposed_metadata)

    result = _create_enrollment_request(
        identity=identity,
        image_path=resolved,
        proposed_metadata=meta_dict,
        notes=notes,
        role="guest",
    )
    return json.dumps(result, indent=2)


STANDARD_META_FIELDS = {"description", "photograph_by", "location", "date", "time", "address", "notes"}
PROTECTED_META_FIELDS = {"identity", "name", "images", "database", "vector_count", "stored_original_image_count"}
PRIVATE_DETAIL_KEYS = {"license_no", "dob", "date_of_birth", "ic", "nric", "passport", "ssn", "phone", "email", "exp"}


def _default_detail_visibility(key: str) -> str:
    norm = re.sub(r"[^a-z0-9_]", "_", key.strip().lower())
    return "private" if norm in PRIVATE_DETAIL_KEYS else "public"


def _normalize_other_details(raw: dict | None) -> dict[str, dict[str, str]]:
    """Normalize other_details entries to {key: {'value': str, 'visibility': 'public'|'private'}}."""
    out: dict[str, dict[str, str]] = {}
    if not isinstance(raw, dict):
        return out
    for k, v in raw.items():
        key = str(k).strip()
        if not key:
            continue
        if isinstance(v, dict):
            val = str(v.get("value", "")).strip()
            vis = str(v.get("visibility", "")).strip().lower()
            if vis not in {"public", "private"}:
                vis = _default_detail_visibility(key)
        else:
            val = "" if v is None else str(v).strip()
            vis = _default_detail_visibility(key)
        if val:
            out[key] = {"value": val, "visibility": vis}
    return out


def _public_other_details(raw: dict | None) -> dict[str, str]:
    """Return only public other_details key-value pairs for chat responses."""
    normalized = _normalize_other_details(raw)
    return {k: item["value"] for k, item in normalized.items() if item["visibility"] == "public"}


def _parse_proposed_metadata(raw: str | dict) -> dict:
    """Parse JSON/dict metadata safely, recovering key-value pairs from truncated JSON."""
    if isinstance(raw, dict):
        return raw
    text = (raw or "").strip()
    if not text:
        return {}
    if text.startswith("{"):
        for candidate in (text, text.rstrip(",") + "}"):
            try:
                obj = json.loads(candidate)
                if isinstance(obj, dict):
                    return obj
            except Exception:
                pass
            try:
                obj = ast.literal_eval(candidate)
                if isinstance(obj, dict):
                    return obj
            except Exception:
                pass
        pairs = re.findall(r'"([^"\\]+)"\s*:\s*"([^"\\]*)"', text)
        if pairs:
            return dict(pairs)
    return {"description": text}


def _merge_profile_metadata(meta: dict, proposed_meta: dict) -> dict:
    """Merge standard fields and custom public/private key-value fields into metadata.json."""
    normalized = dict(proposed_meta or {})
    replace_other = bool(normalized.pop("_replace_other", False))
    other = {} if replace_other else _normalize_other_details(meta.get("other_details"))
    if isinstance(normalized.get("description"), str) and normalized["description"].strip().startswith("{"):
        nested = _parse_proposed_metadata(normalized.pop("description"))
        normalized = {**nested, **normalized}

    for k, v in normalized.items():
        if k in PROTECTED_META_FIELDS:
            continue
        if k == "other_details" and isinstance(v, dict):
            other.update(_normalize_other_details(v))
        elif k in STANDARD_META_FIELDS:
            if v is not None:
                val = str(v).strip()
                if val or replace_other:
                    meta[k] = val
        else:
            key = str(k).strip()
            if isinstance(v, dict):
                other.update(_normalize_other_details({key: v}))
            else:
                val = "" if v is None else str(v).strip()
                if key and val:
                    vis = other[key]["visibility"] if key in other else _default_detail_visibility(key)
                    other[key] = {"value": val, "visibility": vis}
    meta["other_details"] = other
    return meta


@mcp.tool()
def rojakface_request_update_metadata(
    identity: str,
    proposed_metadata: str,
    notes: str = "",
    admin_key: str = "",
) -> str:
    """Request a metadata update. Guest requests are queued for Admin Portal approval; do not ask the user for an admin key."""
    parsed_meta = _parse_proposed_metadata(proposed_metadata)

    if _verify_admin(admin_key):
        clean_identity = (identity or "").strip()
        profile_dir = PROFILE_DIR / _profile_slug(clean_identity)
        meta_file = profile_dir / "metadata.json"
        if not meta_file.is_file():
            return json.dumps({"status": "not_found", "reason": f"Identity '{clean_identity}' not found."}, indent=2)
        meta = _merge_profile_metadata(json.loads(meta_file.read_text(encoding="utf-8")), parsed_meta)
        meta_file.write_text(json.dumps(meta, indent=2), encoding="utf-8")
        return json.dumps({"status": "updated_by_admin", "identity": clean_identity, "metadata": meta}, indent=2)

    result = _create_metadata_update_request(identity, parsed_meta, notes=notes, role="guest")
    return json.dumps(result, indent=2)


@mcp.tool()
def rojakface_enroll_named_identity(
    identity: str,
    image_path: str = "",
    admin_key: str = "",
    proposed_metadata: str = "",
) -> str:
    """Enroll a face for an administrator. Guest requests are queued for Admin Portal approval; never ask the user for an admin key."""
    clean_identity = _canonical_identity(identity)
    if not re.fullmatch(r"[A-Za-z][A-Za-z0-9 .'-]{0,79}", clean_identity):
        return json.dumps(
            {"status": "rejected", "reason": "Identity must be a plain name.", "identity": clean_identity},
            indent=2,
        )

    try:
        resolved = _resolve_image_path(image_path)
    except FileNotFoundError as exc:
        return json.dumps({"status": "unresolved", "reason": str(exc)}, indent=2)

    # GUEST ENFORCEMENT: Guests cannot directly enroll
    if not _verify_admin(admin_key):
        meta_dict = {}
        if proposed_metadata:
            try:
                meta_dict = json.loads(proposed_metadata) if proposed_metadata.strip().startswith("{") else {"description": proposed_metadata}
            except Exception:
                meta_dict = {"description": proposed_metadata}

        staged = _create_enrollment_request(
            identity=clean_identity,
            image_path=resolved,
            proposed_metadata=meta_dict,
            role="guest",
        )
        return json.dumps(staged, indent=2)

    # ADMIN DIRECT ENROLLMENT PATH
    safe_threshold = MIN_CLIP_IDENTIFY_THRESHOLD
    existing = manager.process(str(resolved), mode="identify", threshold=safe_threshold)
    known = [
        {k: v for k, v in match.items() if k != "face_image"}
        for match in existing
        if match.get("identity") not in {None, "Unknown"}
    ]
    if known:
        return json.dumps(
            {
                "status": "rejected",
                "reason": "The image already matches an enrolled identity; no rename was performed.",
                "existing_matches": known,
            },
            indent=2,
        )

    primary_bytes, primary_filename = _primary_face_image(resolved)
    stats = manager.process(
        primary_bytes, mode="learn", threshold=safe_threshold, filename=primary_filename
    )
    created = [
        detail.get("identity")
        for detail in stats.get("details", [])
        if detail.get("status") == "new" and detail.get("identity")
    ]
    for generated_identity in created:
        manager.update_identity(generated_identity, clean_identity)
    if created:
        _sync_profile_folder(clean_identity, resolved)
        _clear_identification_cache()
    return json.dumps(
        {
            "status": "enrolled_by_admin" if created else "unchanged",
            "identity": clean_identity,
            "role": "admin",
            "generated_identities": created,
            "details": stats.get("details", []),
        },
        indent=2,
    )


@mcp.tool()
def rojakface_enroll_or_learn(
    image_path: str = "",
    threshold: float = 0.88,
    admin_key: str = "",
) -> str:
    """Direct unsupervised learning. Restricted to admins to avoid false enrollments."""
    if not _verify_admin(admin_key):
        return json.dumps(
            {
                "status": "rejected",
                "reason": "Direct unsupervised learning is restricted to Admins. Guest users must use rojakface_request_enrollment.",
            },
            indent=2,
        )

    try:
        resolved = _resolve_image_path(image_path)
    except FileNotFoundError as exc:
        return json.dumps({"status": "unresolved", "reason": str(exc)}, indent=2)
    safe_threshold = max(float(threshold), MIN_CLIP_IDENTIFY_THRESHOLD)
    primary_bytes, primary_filename = _primary_face_image(resolved)
    stats = manager.process(
        primary_bytes, mode="learn", threshold=safe_threshold, filename=primary_filename
    )
    for detail in stats.get("details", []):
        generated_identity = detail.get("identity")
        if generated_identity:
            _sync_profile_folder(generated_identity, resolved)
    _clear_identification_cache()
    return json.dumps(stats, indent=2)


@mcp.tool()
def rojakface_list_pending_requests(
    status: str = "pending",
) -> str:
    """List enrollment and metadata requests with simple #1, #2 numbering for one-word approval (All Roles)."""
    clean_status = (status or "pending").strip().lower()
    all_reqs = _load_pending_requests()

    if clean_status == "all":
        filtered = list(all_reqs.values())
    else:
        filtered = [r for r in all_reqs.values() if r.get("status") == clean_status]

    filtered.sort(key=lambda x: x.get("submitted_at") or "")

    formatted_items = []
    for idx, r in enumerate(filtered, start=1):
        formatted_items.append({
            "number": idx,
            "request_id": r["request_id"],
            "type": r.get("type", "enrollment"),
            "identity": r.get("identity"),
            "status": r.get("status"),
            "submitted_at": r.get("submitted_at"),
            "quick_approve": f"approve {idx} (or 'approve {r.get('identity')}')",
            "quick_reject": f"reject {idx} (or 'reject {r.get('identity')}')",
            "proposed_metadata": r.get("proposed_metadata", {}),
            "notes": r.get("notes", ""),
        })

    return json.dumps(
        {
            "total_requests": len(formatted_items),
            "filter_status": clean_status,
            "instructions": "To approve or reject, simply say 'approve 1', 'approve [Name]', 'approve latest', or 'reject 1'.",
            "requests": formatted_items,
        },
        indent=2,
    )


@mcp.tool()
def rojakface_review_pending_request(
    request_id: str,
    action: str,
    admin_key: str = "",
    comment: str = "",
) -> str:
    """Approve or reject a pending request by number ('1'), person name ('Farid Azizan'), 'latest', or full ID (Admin only)."""
    if not _verify_admin(admin_key):
        return json.dumps(
            {
                "status": "unauthorized",
                "reason": "Invalid or missing admin_key.",
            },
            indent=2,
        )

    clean_action = (action or "").strip().lower()
    if clean_action not in {"approve", "reject"}:
        return json.dumps(
            {"status": "rejected", "reason": "Action must be either 'approve' or 'reject'."},
            indent=2,
        )

    all_reqs = _load_pending_requests()
    resolved_id, req = _resolve_request(all_reqs, request_id)
    if not resolved_id or not req:
        return json.dumps(
            {
                "status": "not_found",
                "reason": f"No pending request found matching '{request_id}'. Say 'list pending' to see active requests.",
            },
            indent=2,
        )

    if req.get("status") != "pending":
        return json.dumps(
            {
                "status": "already_reviewed",
                "reason": f"Request for '{req.get('identity')}' has already been {req.get('status')} on {req.get('reviewed_at')}.",
                "request": req,
            },
            indent=2,
        )

    clean_id = resolved_id

    if clean_action == "approve":
        if req.get("type") == "enrollment":
            identity = _canonical_identity(req["identity"])
            crop_path = PENDING_IMAGES_DIR / Path(req["primary_crop"]).name
            if not crop_path.is_file():
                crop_path = BASE_DIR / req["primary_crop"]
            source_path = PENDING_IMAGES_DIR / Path(req["source_image"]).name
            if not source_path.is_file():
                source_path = BASE_DIR / req["source_image"]
            if not crop_path.is_file():
                return json.dumps(
                    {"status": "error", "reason": f"Cropped face file '{crop_path}' missing."},
                    indent=2,
                )

            primary_bytes = crop_path.read_bytes()
            primary_filename = crop_path.name

            stats = manager.process(
                primary_bytes,
                mode="learn",
                threshold=MIN_CLIP_IDENTIFY_THRESHOLD,
                filename=primary_filename,
            )
            details = stats.get("details", [])
            created = [
                detail.get("identity")
                for detail in details
                if detail.get("status") == "new" and detail.get("identity")
            ]
            matched_same = any(
                (detail.get("identity") or "").casefold() == identity.casefold()
                for detail in details
            )
            if not created and not matched_same:
                return json.dumps(
                    {
                        "status": "error",
                        "request_id": clean_id,
                        "identity": identity,
                        "reason": "No face could be enrolled from the staged crop (no new vector was created). The request remains pending; reject it or ask the user to resubmit a clearer image.",
                        "details": details,
                    },
                    indent=2,
                )

            for generated_identity in created:
                manager.update_identity(generated_identity, identity)

            _sync_profile_folder(identity, source_path)
            proposed_meta = req.get("proposed_metadata") or {}
            if isinstance(proposed_meta, dict) and proposed_meta:
                profile_dir = PROFILE_DIR / _profile_slug(identity)
                meta_file = profile_dir / "metadata.json"
                if meta_file.is_file():
                    meta = _merge_profile_metadata(json.loads(meta_file.read_text(encoding="utf-8")), proposed_meta)
                    meta_file.write_text(json.dumps(meta, indent=2), encoding="utf-8")

            _clear_identification_cache()
            _finalize_pending_request(all_reqs, clean_id, req, "approved", comment or "Approved by admin.")

            return json.dumps(
                {
                    "status": "approved",
                    "request_id": clean_id,
                    "identity": identity,
                    "message": f"Enrollment for '{identity}' approved and successfully added to Qdrant and local profiles.",
                    "details": stats.get("details", []),
                },
                indent=2,
            )

        elif req.get("type") == "metadata_update":
            identity = req["identity"]
            proposed_meta = req.get("proposed_metadata") or {}
            profile_dir = PROFILE_DIR / _profile_slug(identity)
            meta_file = profile_dir / "metadata.json"
            if not meta_file.is_file():
                return json.dumps(
                    {"status": "error", "reason": f"Profile metadata file for '{identity}' not found."},
                    indent=2,
                )

            meta = _merge_profile_metadata(json.loads(meta_file.read_text(encoding="utf-8")), proposed_meta)
            meta_file.write_text(json.dumps(meta, indent=2), encoding="utf-8")

            _finalize_pending_request(all_reqs, clean_id, req, "approved", comment or "Approved by admin.")

            return json.dumps(
                {
                    "status": "approved",
                    "request_id": clean_id,
                    "identity": identity,
                    "message": f"Metadata update for '{identity}' approved and saved.",
                    "updated_fields": proposed_meta,
                },
                indent=2,
            )

    else:  # reject
        _finalize_pending_request(all_reqs, clean_id, req, "rejected", comment or "Rejected by admin.")

        return json.dumps(
            {
                "status": "rejected",
                "request_id": clean_id,
                "identity": req.get("identity"),
                "message": f"Request '{clean_id}' has been rejected by Admin.",
                "reason": comment or "No comment provided.",
            },
            indent=2,
        )


@mcp.tool()
def rojakface_rename_identity(old_identity: str, new_identity: str, admin_key: str = "") -> str:
    """Rename an enrolled identity in Qdrant and filesystem (Admin only)."""
    if not _verify_admin(admin_key):
        return json.dumps(
            {
                "status": "unauthorized",
                "reason": "Only Administrators can rename identities. Please provide valid admin_key.",
            },
            indent=2,
        )

    old_clean = (old_identity or "").strip()
    new_clean = (new_identity or "").strip()
    manager.update_identity(old_clean, new_clean)

    old_slug = _profile_slug(old_clean)
    new_slug = _profile_slug(new_clean)
    old_profile_dir = PROFILE_DIR / old_slug
    new_profile_dir = PROFILE_DIR / new_slug
    if old_profile_dir.exists() and old_slug != new_slug:
        same_location = str(old_profile_dir.resolve()).casefold() == str(new_profile_dir.resolve()).casefold()
        if not same_location:
            if new_profile_dir.exists():
                return json.dumps(
                    {"status": "conflict", "reason": f"A profile folder already exists for '{new_clean}'."},
                    indent=2,
                )
            old_profile_dir.rename(new_profile_dir)

    old_web_dir = PROFILE_WEB_DIR / old_slug
    new_web_dir = PROFILE_WEB_DIR / new_slug
    if old_web_dir.exists() and old_slug != new_slug:
        same_location = str(old_web_dir.resolve()).casefold() == str(new_web_dir.resolve()).casefold()
        if not same_location:
            if new_web_dir.exists():
                return json.dumps(
                    {"status": "conflict", "reason": f"A web profile folder already exists for '{new_clean}'."},
                    indent=2,
                )
            old_web_dir.rename(new_web_dir)

    _sync_profile_folder(new_clean)
    _clear_identification_cache()
    return json.dumps(
        {"status": "updated", "old_identity": old_clean, "new_identity": new_clean}
    )


@mcp.tool()
def rojakface_list_identities() -> str:
    """List all enrolled identities currently stored in Qdrant and their reference vector counts (All Roles)."""
    counts: Counter[str] = Counter()
    offset = None
    while True:
        points, offset = qdrant.scroll(
            collection_name=COLLECTION_NAME,
            limit=256,
            offset=offset,
            with_payload=True,
            with_vectors=False,
        )
        for pt in points:
            identity = (pt.payload or {}).get("identity", "Unknown")
            counts[identity] += 1
        if offset is None:
            break
    return json.dumps(
        {"total_vectors": sum(counts.values()), "identities": dict(counts)},
        indent=2,
    )


def _format_time_12h(value: str | None) -> str:
    """Format a 24-hour time string (HH:MM or HH:MM:SS) as 12-hour AM/PM (e.g. '12:15 PM')."""
    text = str(value or "").strip()
    if not text or text.casefold() == "unknown":
        return "Unknown"
    if re.search(r"\b(am|pm)\b", text, flags=re.IGNORECASE):
        return re.sub(r"\b(am|pm)\b", lambda m: m.group(1).upper(), text, flags=re.IGNORECASE)
    match = re.fullmatch(r"(\d{1,2}):(\d{2})(?::(\d{2}))?", text)
    if not match:
        return text
    hour = int(match.group(1))
    minute = int(match.group(2))
    if not (0 <= hour <= 23 and 0 <= minute <= 59):
        return text
    period = "AM" if hour < 12 else "PM"
    hour_12 = hour % 12 or 12
    return f"{hour_12}:{minute:02d} {period}"


def _public_profile_display(canonical: str) -> dict | None:
    """Return the public-only profile metadata and per-image details for chat display."""
    profile_dir = PROFILE_DIR / _profile_slug(canonical)
    profile_metadata = profile_dir / "metadata.json"
    if not profile_metadata.is_file():
        return None
    try:
        profile = json.loads(profile_metadata.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None
    return {
        "name": profile.get("identity", canonical),
        "public_other_details": _public_other_details(profile.get("other_details")),
        "images": [
            {
                "filename": item.get("filename"),
                "url": item.get("url"),
                "description": item.get("description") or profile.get("description", "Unknown"),
                "photograph_by": item.get("photograph_by") or profile.get("photograph_by", "Unknown"),
                "location": item.get("location") or profile.get("location", "Unknown"),
                "date": item.get("date") or profile.get("date", "Unknown"),
                "time": _format_time_12h(item.get("time") or profile.get("time", "Unknown")),
            }
            for item in profile.get("images", [])
            if item.get("url")
        ],
    }


def _encode_image_b64(image_path: Path, max_dim: int = 320) -> str:
    with Image.open(image_path) as img:
        img = ImageOps.exif_transpose(img).convert("RGB")
        img.thumbnail((max_dim, max_dim))
        buf = BytesIO()
        img.save(buf, format="JPEG", quality=80)
        return __import__("base64").b64encode(buf.getvalue()).decode("ascii")


def _match_images_via_ollama(img_entries: list[tuple[dict, Path]], query: str) -> list[dict]:
    """Use one local Ollama vision call across candidate photos to pick matching image numbers."""
    import urllib.request

    if not img_entries:
        return []
    try:
        images_b64 = [_encode_image_b64(p) for _, p in img_entries]
        n = len(images_b64)
        payload = json.dumps(
            {
                "model": os.getenv("ROJAKFACE_VISION_MODEL", "qwen3-vl:4b-instruct-16k"),
                "stream": False,
                "think": False,
                "keep_alive": "30m",
                "options": {"temperature": 0, "num_predict": 32},
                "messages": [
                    {
                        "role": "user",
                        "content": (
                            f"You are given {n} numbered photo(s) in order from 1 to {n}. "
                            f"Which photo number(s) match the request: '{query}'? "
                            "Reply ONLY with the matching number(s) separated by commas (e.g. 1 or 1, 3), or NONE."
                        ),
                        "images": images_b64,
                    }
                ],
            }
        ).encode("utf-8")
        req = urllib.request.Request(
            os.getenv("ROJAKFACE_OLLAMA_URL", "http://127.0.0.1:11434/api/chat"),
            data=payload,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        with urllib.request.urlopen(req, timeout=90) as resp:
            data = json.loads(resp.read().decode("utf-8"))
        answer = str((data.get("message") or {}).get("content") or "").strip().upper()
        if "NONE" in answer:
            return []
        nums = [int(tok) for tok in re.findall(r"\b(\d+)\b", answer)]
        selected: list[dict] = []
        seen: set[int] = set()
        for num in nums:
            if 1 <= num <= n and num not in seen:
                seen.add(num)
                selected.append(img_entries[num - 1][0])
        return selected
    except Exception:
        return []


def _filter_profile_images(canonical: str, images: list[dict], query: str) -> list[dict]:
    """Filter profile images by text metadata / cached visual index first, then by local Ollama vision."""
    clean_query = (query or "").strip()
    if not clean_query or not images:
        return images

    stopwords = {
        "the", "a", "an", "that", "this", "with", "in", "on", "of", "is", "are",
        "photo", "photos", "image", "images", "picture", "pictures", "show", "only",
        "his", "her", "their", "wearing", "wear", "man", "woman", "person",
    }
    tokens = [
        tok for tok in re.findall(r"[a-z0-9]+", clean_query.casefold())
        if tok not in stopwords
    ]
    slug = _profile_slug(canonical)
    visual_index_path = PROFILE_DIR / slug / "visual_index.json"
    visual_index: dict[str, str] = {}
    if visual_index_path.is_file():
        try:
            loaded = json.loads(visual_index_path.read_text(encoding="utf-8"))
            if isinstance(loaded, dict):
                visual_index = {str(k): str(v) for k, v in loaded.items()}
        except (OSError, json.JSONDecodeError):
            visual_index = {}

    if tokens:
        text_matches = []
        for item in images:
            url_name = Path(str(item.get("url") or item.get("filename") or "")).name
            haystack = " ".join(
                [
                    *(str(item.get(k) or "") for k in ("description", "photograph_by", "location", "date", "time", "filename")),
                    visual_index.get(url_name, ""),
                ]
            ).casefold()
            if all(tok in haystack for tok in tokens):
                text_matches.append(item)
        if text_matches:
            return text_matches

    candidates_with_paths: list[tuple[dict, Path]] = []
    for item in images:
        url_name = Path(str(item.get("url") or item.get("filename") or "")).name
        for candidate in (PROFILE_DIR / slug / "images" / url_name, PROFILE_WEB_DIR / slug / url_name):
            if candidate.is_file():
                candidates_with_paths.append((item, candidate))
                break
    return _match_images_via_ollama(candidates_with_paths, clean_query)


@mcp.tool()
def rojakface_search_identity(
    identity: str,
    query: str = "",
    limit: int = 0,
    photos_only: bool = False,
) -> str:
    """Search the local profile database for an enrolled person (case-insensitive).

    Args:
        identity: Person's name (any case/spacing).
        query: Optional filter for a specific photo (e.g. 'wear songkok', 'green batik', 'Ngawi').
        limit: Optional max number of photos to return (e.g. 3 for '3 photos only'; 0 = all).
        photos_only: Set True when the user asks for photos/images only without metadata tables.
    """
    requested = (identity or "").strip()
    if not requested:
        return json.dumps({"status": "rejected", "reason": "Provide an identity name."}, indent=2)

    identities = rojakface_list_identities()
    known_names = json.loads(identities).get("identities", {})
    norm_requested = re.sub(r"[^a-z0-9]", "", requested.casefold())
    canonical = next((name for name in known_names if name.casefold() == requested.casefold()), None)
    if canonical is None and norm_requested:
        canonical = next(
            (name for name in known_names if re.sub(r"[^a-z0-9]", "", name.casefold()) == norm_requested),
            None,
        )
    if canonical is None:
        partial_matches = [
            name
            for name in known_names
            if requested.casefold() in name.casefold()
            or (norm_requested and norm_requested in re.sub(r"[^a-z0-9]", "", name.casefold()))
        ]
        if len(partial_matches) == 1:
            canonical = partial_matches[0]
        elif len(partial_matches) > 1:
            return json.dumps(
                {"status": "ambiguous", "identity": requested, "matching_identities": sorted(partial_matches)},
                indent=2,
            )
    if canonical is None:
        return json.dumps(
            {"status": "not_found", "identity": requested, "available_identities": sorted(known_names)},
            indent=2,
        )

    display_profile = _public_profile_display(canonical)
    if display_profile is not None:
        clean_query = (query or "").strip()
        if clean_query:
            display_profile["images"] = _filter_profile_images(canonical, display_profile["images"], clean_query)
        try:
            max_photos = int(limit or 0)
        except (TypeError, ValueError):
            max_photos = 0
        if max_photos > 0:
            display_profile["images"] = display_profile["images"][:max_photos]

        if clean_query and not display_profile["images"]:
            instruction = (
                f"No stored photo for {canonical} matched '{clean_query}'. "
                "Tell the user clearly that no matching photo was found in the database."
            )
        elif photos_only:
            instruction = (
                "Render ONLY each image in profile_metadata.images using ![Original image](url), one per line. "
                "Do NOT include metadata tables, extra fields, or private data."
            )
        else:
            instruction = (
                "If the user specifically asked for photos/images only, render ONLY each image in profile_metadata.images as ![Original image](url) without extra metadata tables. "
                "Otherwise, format the response using clean Markdown tables with centered columns: "
                "1) First show a table with columns | Field | Value | (separator | :---: | :---: |) containing Name and every key-value pair in public_other_details (if non-empty). "
                "2) Then show a table with columns | Image | Description | Photograph by | Location | Date | Time | (separator | :---: | :---: | :---: | :---: | :---: | :---: |) with one row per image in profile_metadata.images, "
                "putting ![Original image](url) in the Image column and that image's own Description, Photograph by, Location, Date, and 12-hour AM/PM Time in the other columns. "
                "Never show private data, profile folder, or face crops."
            )
        return json.dumps(
            {
                "status": "found",
                "identity": canonical,
                "database_only": True,
                "query_applied": clean_query or None,
                "limit_applied": max_photos if max_photos > 0 else None,
                "photos_only": bool(photos_only),
                "matched_image_count": len(display_profile["images"]),
                "profile_metadata": display_profile,
                "display_instruction": instruction,
            },
            indent=2,
        )

    gallery_root = LIBRECHAT_DIR / "images" / "identity-gallery"
    gallery_dir = gallery_root / _profile_slug(canonical)
    gallery_dir.mkdir(parents=True, exist_ok=True)
    records = []
    offset = None
    while True:
        points, offset = qdrant.scroll(
            collection_name=COLLECTION_NAME,
            limit=256,
            offset=offset,
            with_payload=True,
            with_vectors=False,
            scroll_filter=Filter(
                must=[FieldCondition(key="identity", match=MatchValue(value=canonical))]
            ),
        )
        for point in points:
            payload = dict(point.payload or {})
            filename = str(payload.get("filename") or "")
            crop_rel = str(payload.get("crop_path") or "")
            crop_source = STORE_FACES_DIR / crop_rel
            original_candidate = STORE_FACES_DIR / "originals" / Path(filename).name
            original_matches = list((STORE_FACES_DIR / "originals").glob(f"*_{filename}"))
            original_source = original_candidate if original_candidate.is_file() else (original_matches[0] if original_matches else None)
            images = []
            stem = re.sub(r"[^A-Za-z0-9._-]+", "_", str(point.id))
            if crop_source.is_file():
                crop_name = f"{stem}_crop{crop_source.suffix.lower()}"
                shutil.copy2(crop_source, gallery_dir / crop_name)
                images.append({"kind": "face_crop", "url": f"/images/identity-gallery/{gallery_dir.name}/{crop_name}"})
            if original_source is not None and original_source.is_file():
                original_name = f"{stem}_original{original_source.suffix.lower()}"
                shutil.copy2(original_source, gallery_dir / original_name)
                images.append({"kind": "source_image", "url": f"/images/identity-gallery/{gallery_dir.name}/{original_name}"})
            records.append(
                {
                    "point_id": str(point.id),
                    "database_payload": payload,
                    "images": images,
                }
            )
        if offset is None:
            break

    return json.dumps(
        {
            "status": "found",
            "identity": canonical,
            "database_only": True,
            "vector_count": len(records),
            "records": records,
            "display_instruction": "Render each returned image URL as Markdown when displaying the database images.",
        },
        indent=2,
    )


@mcp.tool()
def rojakface_delete_identity(
    identity: str,
    purge_files: bool = True,
    admin_key: str = "",
) -> str:
    """Remove an enrolled identity and all of its vectors from Qdrant (Admin only)."""
    if not _verify_admin(admin_key):
        return json.dumps(
            {
                "status": "unauthorized",
                "reason": "Only Administrators can delete identities. Please provide valid admin_key.",
            },
            indent=2,
        )

    clean_identity = _canonical_identity(identity)
    purged = {"removed_crops": [], "removed_originals": []}
    if purge_files:
        # Collect and remove files before the vectors are deleted.
        purged = _purge_identity_files(clean_identity)

    qdrant.delete(
        collection_name=COLLECTION_NAME,
        points_selector=Filter(
            must=[FieldCondition(key="identity", match=MatchValue(value=clean_identity))]
        ),
    )

    if purge_files:
        slug = _profile_slug(clean_identity)
        profile_dir = PROFILE_DIR / slug
        if profile_dir.exists():
            shutil.rmtree(profile_dir, ignore_errors=True)
        web_dir = PROFILE_WEB_DIR / slug
        if web_dir.exists():
            shutil.rmtree(web_dir, ignore_errors=True)

    _clear_identification_cache()
    return json.dumps(
        {
            "status": "deleted",
            "identity": clean_identity,
            "files_purged": purge_files,
            "purged_files": purged,
        }
    )


def _admin_profile_identity(profile_slug: str) -> str | None:
    """Resolve a URL-safe profile slug to the canonical enrolled identity."""
    clean_slug = (profile_slug or "").strip()
    if not re.fullmatch(r"[A-Za-z0-9._-]{1,100}", clean_slug):
        return None
    for identity in json.loads(rojakface_list_identities()).get("identities", {}):
        if _profile_slug(identity) == clean_slug:
            return identity
    return None


def _admin_profile_record(identity: str) -> dict:
    """Return editable profile data for the click-based Admin Portal."""
    profile_dir = PROFILE_DIR / _profile_slug(identity)
    metadata_file = profile_dir / "metadata.json"
    metadata = {}
    if metadata_file.is_file():
        try:
            metadata = json.loads(metadata_file.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            metadata = {}
    meta_images_by_name: dict[str, dict] = {}
    for item in metadata.get("images", []) if isinstance(metadata.get("images"), list) else []:
        if isinstance(item, dict):
            pname = Path(str(item.get("profile_path") or item.get("filename") or "")).name
            if pname:
                meta_images_by_name[pname] = item

    images = []
    for image_path in sorted((profile_dir / "images").glob("*")) if (profile_dir / "images").is_dir() else []:
        if image_path.is_file() and image_path.suffix.lower() in IMAGE_EXTS:
            # Admin gallery is for source/reference photos only. Face crops
            # produced by enrollment are typically small and/or marked as
            # primary/crop; keep them out of the management UI.
            if any(marker in image_path.stem.casefold() for marker in ("_primary", "crop", "face_crop")):
                continue
            try:
                with Image.open(image_path) as image:
                    if max(image.size) <= 160:
                        continue
            except (OSError, Image.UnidentifiedImageError):
                continue
            img_meta = meta_images_by_name.get(image_path.name, {})
            images.append({
                "filename": image_path.name,
                "url": f"/admin/profile-images/{_profile_slug(identity)}/{image_path.name}",
                "description": img_meta.get("description", metadata.get("description", "")),
                "photograph_by": img_meta.get("photograph_by", metadata.get("photograph_by", "")),
                "location": img_meta.get("location", metadata.get("location", "")),
                "date": img_meta.get("date", metadata.get("date", "")),
                "time": img_meta.get("time", metadata.get("time", "")),
            })
    vector_count = metadata.get("vector_count", 0)
    try:
        vector_count = qdrant.count(
            collection_name=COLLECTION_NAME,
            count_filter=Filter(must=[FieldCondition(key="identity", match=MatchValue(value=identity))]),
            exact=True,
        ).count
    except Exception:
        pass
    raw_notes = str(metadata.get("notes") or "")
    if raw_notes.startswith("Only database-backed fields are listed."):
        raw_notes = ""
    return {
        "slug": _profile_slug(identity),
        "identity": identity,
        "description": metadata.get("description", PROFILE_DESCRIPTIONS.get(identity, "")),
        "photograph_by": metadata.get("photograph_by", ""),
        "location": metadata.get("location", ""),
        "date": metadata.get("date", ""),
        "time": metadata.get("time", ""),
        "address": metadata.get("address", ""),
        "notes": raw_notes,
        "other_details": _normalize_other_details(metadata.get("other_details")),
        "vector_count": vector_count,
        "images": images,
    }


def _admin_add_profile_image(identity: str, image_path: Path) -> dict:
    """Add another face reference to an existing identity as an Admin."""
    clean_identity = _canonical_identity(identity)
    if not clean_identity or not image_path.is_file():
        return {"status": "rejected", "reason": "Identity and image are required."}
    try:
        existing = manager.process(str(image_path), mode="identify", threshold=MIN_CLIP_IDENTIFY_THRESHOLD)
        known = {match.get("identity") for match in existing if match.get("identity") not in {None, "Unknown"}}
        if known and clean_identity not in known:
            return {"status": "rejected", "reason": "The image matches a different enrolled identity.", "matches": sorted(known)}
        primary_bytes, primary_filename = _primary_face_image(image_path)
        stats = manager.process(primary_bytes, mode="learn", threshold=MIN_CLIP_IDENTIFY_THRESHOLD, filename=primary_filename)
        for detail in stats.get("details", []):
            generated_identity = detail.get("identity")
            if generated_identity:
                manager.update_identity(generated_identity, clean_identity)
        _sync_profile_folder(clean_identity, image_path)
        _clear_identification_cache()
        return {"status": "image_added", "identity": clean_identity, "details": stats.get("details", [])}
    except Exception as exc:
        return {"status": "error", "reason": str(exc)}


if __name__ == "__main__":
    from src.models.admin_routes import mount_admin_portal

    host = os.getenv("ROJAKFACE_HOST", "0.0.0.0")
    port = int(os.getenv("ROJAKFACE_PORT", "8005"))

    app = mcp.sse_app(
        transport_security=TransportSecuritySettings(
            enable_dns_rebinding_protection=True,
            allowed_hosts=["localhost:*", "127.0.0.1:*", "host.docker.internal:*"],
        )
    )

    app, admin_password = mount_admin_portal(
        app,
        admin_key=ADMIN_KEY,
        admin_sessions=_admin_sessions,
        session_cookie=ADMIN_SESSION_COOKIE,
        session_ttl_seconds=ADMIN_SESSION_TTL_SECONDS,
        pending_dir=PENDING_DIR,
        pending_images_dir=PENDING_IMAGES_DIR,
        pending_file=PENDING_FILE,
        pending_lock=_pending_lock,
        profile_dir=PROFILE_DIR,
        profile_web_dir=PROFILE_WEB_DIR,
        image_exts=IMAGE_EXTS,
        min_clip_threshold=MIN_CLIP_IDENTIFY_THRESHOLD,
        identify_fn=rojakface_identify_faces,
        list_identities_fn=rojakface_list_identities,
        enroll_named_fn=rojakface_enroll_named_identity,
        update_metadata_fn=rojakface_request_update_metadata,
        rename_identity_fn=rojakface_rename_identity,
        delete_identity_fn=rojakface_delete_identity,
        review_request_fn=rojakface_review_pending_request,
        profile_identity_fn=_admin_profile_identity,
        profile_record_fn=_admin_profile_record,
        add_profile_image_fn=_admin_add_profile_image,
    )

    print(f"RojakFace MCP Server starting on http://{host}:{port}/sse (Local AI)")
    print(f"Admin Portal: http://127.0.0.1:{port}/admin (form login required)")
    print("Roles Active: Guest (default, staging review) | Admin (valid admin_key required)")
    if host == "0.0.0.0":
        print("Warning: bound to 0.0.0.0. Set ROJAKFACE_HOST=127.0.0.1 if LibreChat runs on this host.")
        if admin_password == "admin123":
            print("Security warning: using default Admin Portal password while bound to 0.0.0.0. Set ROJAKFACE_ADMIN_PASS.")

    try:
        uvicorn.run(app, host=host, port=port)
    finally:
        try:
            qdrant.close()
        except Exception:
            pass
