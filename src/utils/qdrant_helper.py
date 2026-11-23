import os
from pathlib import Path
from qdrant_client import QdrantClient
from src.utils.paths import PROCESSED_DIR


def get_qdrant_client(base_dir: Path | None = None) -> QdrantClient:
    """
    Connect to Qdrant either via QDRANT_URL (e.g. http://localhost:6333) or
    fallback to local embedded path (data/processed/rojakface_qdrant).
    Provides clear diagnostics if an embedded directory is locked by another process.
    """
    qdrant_url = os.getenv("QDRANT_URL")
    if qdrant_url:
        return QdrantClient(url=qdrant_url)

    storage_path = os.getenv("QDRANT_PATH")
    if not storage_path:
        if base_dir is not None and (base_dir / "rojakface_qdrant").exists():
            storage_path = str(base_dir / "rojakface_qdrant")
        else:
            storage_path = str(PROCESSED_DIR / "rojakface_qdrant")

    try:
        return QdrantClient(path=storage_path)
    except RuntimeError as exc:
        err_msg = str(exc)
        if "already accessed by another instance" in err_msg or "Storage folder" in err_msg:
            raise RuntimeError(
                f"\n[Qdrant Concurrency Lock]\n"
                f"Storage folder '{storage_path}' is already locked by another running instance (such as rojakface_mcp.py).\n"
                f"Solutions:\n"
                f"  1. Stop the running process holding the lock before running this command, OR\n"
                f"  2. Start Qdrant server container: 'docker compose -f config/docker-compose.qdrant.yml up -d'\n"
                f"     and set environment variable: QDRANT_URL=http://localhost:6333\n"
            ) from exc
        raise
