import json
from pathlib import Path
from threading import Lock
import tempfile
import unittest

from starlette.applications import Starlette

from src.models.admin_routes import mount_admin_portal
from src.utils.face_utils import profile_slug
from src.utils.jawi_decoder import decode_jawi_text
from src.utils.paths import CONFIG_DIR, DATA_DIR, INTERIM_DIR, MODELS_DIR, PROCESSED_DIR, PROJECT_ROOT, RAW_DIR


class TestProjectStructure(unittest.TestCase):
    def test_paths_layout(self):
        self.assertTrue(PROJECT_ROOT.is_dir())
        self.assertEqual(CONFIG_DIR, PROJECT_ROOT / "config")
        self.assertEqual(RAW_DIR, DATA_DIR / "raw")
        self.assertEqual(INTERIM_DIR, DATA_DIR / "interim")
        self.assertEqual(PROCESSED_DIR, DATA_DIR / "processed")
        self.assertEqual(MODELS_DIR, PROJECT_ROOT / "models")

    def test_profile_slug(self):
        self.assertEqual(profile_slug("Mahathir Mohamad"), "Mahathir_Mohamad")
        self.assertEqual(profile_slug("  Azalina Othman Said! "), "Azalina_Othman_Said")
        self.assertEqual(profile_slug("Lando_Zawawi"), "Lando_Zawawi")

    def test_settings_and_librechat_config(self):
        settings = json.loads((CONFIG_DIR / "settings.json").read_text(encoding="utf-8"))
        self.assertEqual(settings["qdrant_collection"], "enterprise_faces")
        self.assertGreaterEqual(settings["min_clip_identify_threshold"], 0.88)
        self.assertEqual(settings["mcp_port"], 8005)

        librechat_dir = CONFIG_DIR / "librechat"
        for name in ("librechat.yaml", "docker-compose.override.yml", "custom-index.html", "royal-theme.css"):
            self.assertTrue((librechat_dir / name).is_file(), f"Missing {name} in config/librechat/")

    def test_admin_portal_templates_and_routes(self):
        portal_dir = PROJECT_ROOT / "src" / "models" / "admin_portal"
        for name in ("login.html", "dashboard.html", "test_recognition.html"):
            self.assertTrue((portal_dir / name).is_file(), f"Missing {name}")

        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            pending_file = tmp_path / "requests.json"
            pending_file.write_text("{}", encoding="utf-8")
            app, _ = mount_admin_portal(
                Starlette(),
                admin_key="test-key",
                admin_sessions={},
                session_cookie="rojakface_admin_session",
                session_ttl_seconds=3600,
                pending_dir=tmp_path,
                pending_images_dir=tmp_path,
                pending_file=pending_file,
                pending_lock=Lock(),
                profile_dir=tmp_path,
                profile_web_dir=tmp_path,
                image_exts={".jpg", ".png"},
                min_clip_threshold=0.88,
                identify_fn=lambda *a, **kw: "{}",
                list_identities_fn=lambda: '{"identities": {}}',
                enroll_named_fn=lambda **kw: "{}",
                update_metadata_fn=lambda **kw: "{}",
                rename_identity_fn=lambda *a, **kw: "{}",
                delete_identity_fn=lambda **kw: "{}",
                review_request_fn=lambda **kw: "{}",
                profile_identity_fn=lambda slug: None,
                profile_record_fn=lambda ident: {},
                add_profile_image_fn=lambda ident, p: {},
            )
            self.assertTrue(callable(app))

    def test_jawi_decoder(self):
        sample = (
            "Fimboakan\n"
            "Deygan Naci Alaa Yag Mahi Fimorah Lai Maha\n"
            "Mikhassiyahani (1) Sibkali Foji Terintento Bayi\n"
            "Tuarag 2 Yag Sisat (7)"
        )
        decoded = decode_jawi_text(sample)["standard_malay_rumi_lines"]
        self.assertEqual(decoded[0], "Pembukaan")
        self.assertEqual(decoded[1], "dengan nama Allah yang Maha Pemurah lagi Maha")
        self.assertEqual(decoded[2], "Mengasihani (1) Segala puji tertentu bagi")
        self.assertEqual(decoded[3], "orang-orang yang sesat (7)")


if __name__ == "__main__":
    unittest.main()
