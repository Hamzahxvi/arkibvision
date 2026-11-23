from collections import defaultdict
import json
from pathlib import Path
import shutil
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from src.utils.face_utils import (
    COLLECTION_NAME,
    LIBRECHAT_URL,
    PROFILE_DESCRIPTIONS,
    PROFILE_DIR,
    PROFILE_WEB_DIR,
    STORE_FACES_DIR,
    profile_slug,
)
from src.utils.paths import PROCESSED_DIR
from src.utils.qdrant_helper import get_qdrant_client

qdrant = get_qdrant_client(PROCESSED_DIR)
try:
    grouped = defaultdict(list)
    offset = None
    while True:
        points, offset = qdrant.scroll(
            collection_name=COLLECTION_NAME,
            limit=256,
            offset=offset,
            with_payload=True,
            with_vectors=False,
        )
        for point in points:
            grouped[(point.payload or {}).get("identity", "Unknown")].append(point)
        if offset is None:
            break

    for identity, points in sorted(grouped.items()):
        slug = profile_slug(identity)
        profile_dir = PROFILE_DIR / slug
        images_dir = profile_dir / "images"
        web_dir = PROFILE_WEB_DIR / slug
        images_dir.mkdir(parents=True, exist_ok=True)
        web_dir.mkdir(parents=True, exist_ok=True)

        existing_meta = {}
        meta_file = profile_dir / "metadata.json"
        if meta_file.is_file():
            try:
                existing_meta = json.loads(meta_file.read_text(encoding="utf-8"))
            except Exception:
                existing_meta = {}

        description = (
            existing_meta.get("description")
            if existing_meta.get("description") not in (None, "Unknown")
            else PROFILE_DESCRIPTIONS.get(identity, "Unknown")
        )

        image_records = []
        seen = set()
        for index, point in enumerate(points, start=1):
            payload = dict(point.payload or {})
            filename = Path(str(payload.get("filename") or "")).name
            source = STORE_FACES_DIR / "originals" / filename
            if not source.is_file():
                matches = list((STORE_FACES_DIR / "originals").glob(f"*_{filename}"))
                source = matches[0] if matches else None
            if source is None or not source.is_file() or source.name in seen:
                continue
            seen.add(source.name)
            target_name = f"{index:02d}_{source.name}"
            profile_target = images_dir / target_name
            web_target = web_dir / target_name
            shutil.copy2(source, profile_target)
            shutil.copy2(source, web_target)
            image_records.append(
                {
                    "filename": source.name,
                    "profile_path": str(profile_target.relative_to(PROFILE_DIR.parent)),
                    "url": f"{LIBRECHAT_URL}/images/profile-gallery/{slug}/{target_name}",
                    "qdrant_point_id": str(point.id),
                }
            )
        metadata = {
            "identity": identity,
            "description": description,
            "photograph_by": existing_meta.get("photograph_by", "Unknown"),
            "location": existing_meta.get("location", "Unknown"),
            "date": existing_meta.get("date", "Unknown"),
            "time": existing_meta.get("time", "Unknown"),
            "database": f"Qdrant collection {COLLECTION_NAME}",
            "vector_count": len(points),
            "stored_original_image_count": len(image_records),
            "images": image_records,
            "notes": "Only database-backed fields are listed. No web biography or inferred metadata.",
        }
        meta_file.write_text(json.dumps(metadata, indent=2), encoding="utf-8")
        (profile_dir / "README.md").write_text(
            f"# {identity}\n\nLocal profile folder. Original reference images are in `images/`; structured metadata is in `metadata.json`.\n",
            encoding="utf-8",
        )

    print(f"Built {len(grouped)} local profile folders in {PROFILE_DIR}")
finally:
    try:
        qdrant.close()
    except Exception:
        pass
