from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from RojakFace import LocalStorage, RojakFaceManager
from src.utils.face_utils import COLLECTION_NAME, IMAGE_EXTS, STORE_FACES_DIR, primary_face_bytes, sync_profile_folder
from src.utils.paths import PROCESSED_DIR, RAW_DIR
from src.utils.qdrant_helper import get_qdrant_client

ENROLL_DIR = RAW_DIR / "staff_enroll"
THRESHOLD = 1.1

storage = LocalStorage(base_path=str(STORE_FACES_DIR))
qdrant = get_qdrant_client(PROCESSED_DIR)

try:
    manager = RojakFaceManager(qdrant_client=qdrant, storage=storage, collection=COLLECTION_NAME)
    manager.init_storage()

    if not ENROLL_DIR.exists():
        print(f"Create {ENROLL_DIR} with one subfolder per employee first.")
        raise SystemExit(0)

    for person_dir in sorted(p for p in ENROLL_DIR.iterdir() if p.is_dir()):
        emp_id = person_dir.name
        images = [f for f in person_dir.iterdir() if f.is_file() and f.suffix.lower() in IMAGE_EXTS]
        print(f"\nProcessing {emp_id} ({len(images)} images)...")

        for img in images:
            try:
                image_bytes, _ = primary_face_bytes(manager.detector, img)
            except ValueError as exc:
                print(f"  ! Skipped {img.name}: {exc}")
                continue
            stats = manager.process(image_bytes, mode="learn", threshold=THRESHOLD, filename=f"{emp_id}_{img.stem}.jpg")
            for item in stats.get("details", []):
                temp_id = item.get("identity", "")
                if temp_id.startswith("Person_"):
                    manager.update_identity(temp_id, emp_id)
                    print(f"  + Enrolled new angle from {img.name} -> {emp_id}")
                else:
                    print(f"  = {img.name} already covered by {temp_id}")
            sync_profile_folder(qdrant, emp_id, img)

    print("\nBatch enrollment complete.")
finally:
    try:
        qdrant.close()
    except Exception:
        pass
