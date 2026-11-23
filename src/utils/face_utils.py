from io import BytesIO
import json
import os
from pathlib import Path
import re
import shutil
from PIL import Image, ImageOps
from qdrant_client.models import FieldCondition, Filter, MatchValue
from src.utils.paths import INTERIM_DIR, PROCESSED_DIR, PROJECT_ROOT

BASE_DIR = PROJECT_ROOT
LIBRECHAT_DIR = Path(os.getenv("LIBRECHAT_DIR", str(PROJECT_ROOT.parent / "LibreChat")))
LIBRECHAT_URL = os.getenv("LIBRECHAT_URL", "http://localhost:3080").rstrip("/")
IMAGE_EXTS = {".jpg", ".jpeg", ".png", ".webp", ".bmp"}
COLLECTION_NAME = "enterprise_faces"
STORE_FACES_DIR = INTERIM_DIR / "store-faces"
PROFILE_DIR = PROCESSED_DIR / "profiles"
PROFILE_WEB_DIR = LIBRECHAT_DIR / "images" / "profile-gallery"
PROFILE_DESCRIPTIONS = {
    "Mahathir Mohamad": (
        "Mahathir Mohamad (born July 10, 1925) is a Malaysian physician, author, and politician "
        "who served as the fourth and seventh Prime Minister of Malaysia, holding the position "
        "for a cumulative 24 years."
    ),
}


def profile_slug(identity: str) -> str:
    """Return a filesystem- and URL-safe directory name for an identity."""
    return re.sub(r"[^A-Za-z0-9._-]+", "_", identity).strip("._")


def primary_face_bytes(detector, image_path: Path) -> tuple[bytes, str]:
    """Crop the largest detected face so enrollment cannot label bystanders."""
    detections = detector.extract_features(str(image_path))
    if not detections:
        raise ValueError("No face was detected for enrollment.")
    primary = max(
        detections,
        key=lambda item: max(0, item["box"][2]) * max(0, item["box"][3]),
    )
    x, y, width, height = [int(value) for value in primary["box"]]
    with Image.open(image_path) as source:
        image = ImageOps.exif_transpose(source).convert("RGB")
        padding = int(max(width, height) * 0.35)
        crop = image.crop(
            (
                max(0, x - padding),
                max(0, y - padding),
                min(image.width, x + width + padding),
                min(image.height, y + height + padding),
            )
        )
        output = BytesIO()
        crop.save(output, format="JPEG", quality=95)
    return output.getvalue(), f"{image_path.stem}_primary.jpg"


def sync_profile_folder(qdrant, identity: str, source_image: Path | None = None) -> None:
    """Keep the filesystem profile record and web gallery synchronized."""
    slug = profile_slug(identity)
    profile_dir = PROFILE_DIR / slug
    images_dir = profile_dir / "images"
    web_dir = PROFILE_WEB_DIR / slug
    images_dir.mkdir(parents=True, exist_ok=True)
    web_dir.mkdir(parents=True, exist_ok=True)

    existing_meta: dict = {}
    metadata_path = profile_dir / "metadata.json"
    if metadata_path.is_file():
        try:
            existing_meta = json.loads(metadata_path.read_text(encoding="utf-8"))
        except (json.JSONDecodeError, OSError):
            existing_meta = {}

    existing_images = list(existing_meta.get("images", []))
    if source_image is not None and source_image.is_file():
        target_name = f"{len(existing_images) + 1:02d}_{source_image.name}"
        profile_target = images_dir / target_name
        web_target = web_dir / target_name
        if not profile_target.is_file():
            shutil.copy2(source_image, profile_target)
        if not web_target.is_file():
            shutil.copy2(source_image, web_target)
        rel_profile_path = str(profile_target.relative_to(PROFILE_DIR.parent))
        if not any(item.get("profile_path") == rel_profile_path for item in existing_images):
            existing_images.append(
                {
                    "filename": source_image.name,
                    "profile_path": rel_profile_path,
                    "url": f"{LIBRECHAT_URL}/images/profile-gallery/{slug}/{target_name}",
                    "source": "enrollment_original",
                    "description": existing_meta.get("description", "Unknown"),
                    "photograph_by": existing_meta.get("photograph_by", "Unknown"),
                    "location": existing_meta.get("location", "Unknown"),
                    "date": existing_meta.get("date", "Unknown"),
                    "time": existing_meta.get("time", "Unknown"),
                }
            )

    try:
        point_count = qdrant.count(
            collection_name=COLLECTION_NAME,
            count_filter=Filter(must=[FieldCondition(key="identity", match=MatchValue(value=identity))]),
            exact=True,
        ).count
    except Exception:
        point_count = len(existing_images)

    description = (
        existing_meta.get("description")
        if existing_meta.get("description") not in (None, "Unknown")
        else PROFILE_DESCRIPTIONS.get(identity, "Unknown")
    )
    raw_notes = str(existing_meta.get("notes") or "")
    if raw_notes.startswith("Only database-backed fields are listed."):
        raw_notes = ""
    profile = {
        "identity": identity,
        "description": description,
        "photograph_by": existing_meta.get("photograph_by", "Unknown"),
        "location": existing_meta.get("location", "Unknown"),
        "date": existing_meta.get("date", "Unknown"),
        "time": existing_meta.get("time", "Unknown"),
        "address": existing_meta.get("address", ""),
        "notes": raw_notes,
        "other_details": existing_meta.get("other_details", {}) if isinstance(existing_meta.get("other_details"), dict) else {},
        "database": f"Qdrant collection {COLLECTION_NAME}",
        "vector_count": point_count,
        "stored_original_image_count": len(existing_images),
        "images": existing_images,
    }
    metadata_path.write_text(json.dumps(profile, indent=2), encoding="utf-8")
    (profile_dir / "README.md").write_text(
        f"# {identity}\n\nLocal profile folder. Original reference images are in `images/`; structured metadata is in `metadata.json`.\n",
        encoding="utf-8",
    )
