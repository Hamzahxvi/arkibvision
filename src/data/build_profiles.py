from collections import defaultdict
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from src.utils.face_utils import (
    COLLECTION_NAME,
    IMAGE_EXTS,
    PROFILE_DIR,
    sync_profile_folder,
)
from src.utils.paths import PROCESSED_DIR, RAW_DIR
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

    for identity in sorted(grouped):
        raw_person_dir = RAW_DIR / "staff_enroll" / identity
        raw_images = (
            sorted(p for p in raw_person_dir.iterdir() if p.is_file() and p.suffix.lower() in IMAGE_EXTS)
            if raw_person_dir.is_dir()
            else []
        )
        if raw_images:
            for raw_img in raw_images:
                sync_profile_folder(qdrant, identity, raw_img)
        else:
            sync_profile_folder(qdrant, identity)

    print(f"Built {len(grouped)} local profile folders in {PROFILE_DIR}")
finally:
    try:
        qdrant.close()
    except Exception:
        pass
