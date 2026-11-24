#![forbid(unsafe_code)]

use base64::Engine as _;
use rand::RngCore;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

pub const MIN_CLIP_IDENTIFY_THRESHOLD: f32 = 0.88;
pub const AUTO_GALLERY_MIN_SCORE: f32 = 0.90;
pub const AUTO_GALLERY_MAX_IMAGES: usize = 15;
pub const PERCEPTUAL_HASH_DISTANCE: i64 = 16;
pub const MAX_AUTO_IMAGE_AGE_SECS: u64 = 60;
pub const ADMIN_SESSION_COOKIE: &str = "rojakface_admin_session";
pub const ADMIN_SESSION_TTL_SECS: u64 = 8 * 60 * 60;
pub const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp", "bmp"];
pub const DOCUMENT_EXTS: &[&str] = &[
    "txt", "md", "json", "csv", "html", "htm", "xml", "pdf", "docx", "jpg", "jpeg", "png", "webp",
    "bmp",
];

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub project_root: PathBuf,
    pub store_faces_dir: PathBuf,
    pub profile_dir: PathBuf,
    pub locations_dir: PathBuf,
    pub documents_dir: PathBuf,
    pub pending_dir: PathBuf,
    pub pending_images_dir: PathBuf,
    pub pending_file: PathBuf,
    pub vectors_file: PathBuf,
    pub location_vectors_file: PathBuf,
    pub librechat_dir: PathBuf,
    pub profile_web_dir: PathBuf,
    pub location_web_dir: PathBuf,
    pub document_web_dir: PathBuf,
    pub librechat_url: String,
    pub image_dirs: Vec<PathBuf>,
    pub host: String,
    pub port: u16,
    pub admin_user: String,
    pub admin_pass: String,
    pub admin_key: String,
    pub ollama_url: String,
    pub vision_model: String,
    pub ort_dylib_path: PathBuf,
    pub yunet_model_path: PathBuf,
    pub clip_model_path: PathBuf,
    pub templates_dir: PathBuf,
}

pub fn is_image_ext(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

pub fn is_document_ext(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| DOCUMENT_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

fn find_project_root() -> PathBuf {
    if let Ok(explicit) = env::var("ROJAKFACE_ROOT") {
        let p = PathBuf::from(explicit);
        if p.is_dir() {
            return p;
        }
    }
    if let Ok(cwd) = env::current_dir() {
        for ancestor in cwd.ancestors() {
            if ancestor.join("config").join("settings.json").is_file() {
                return ancestor.to_path_buf();
            }
        }
    }
    if let Ok(exe) = env::current_exe() {
        for ancestor in exe.ancestors() {
            if ancestor.join("config").join("settings.json").is_file() {
                return ancestor.to_path_buf();
            }
        }
    }
    env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn user_home_dir() -> PathBuf {
    env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn find_ort_dylib(project_root: &Path) -> PathBuf {
    if let Ok(p) = env::var("ORT_DYLIB_PATH") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return pb;
        }
    }
    let dll_name = if cfg!(windows) {
        "onnxruntime.dll"
    } else if cfg!(target_os = "macos") {
        "libonnxruntime.dylib"
    } else {
        "libonnxruntime.so"
    };
    let mut candidates = vec![
        project_root.join("models").join("runtime").join(dll_name),
        project_root.join("models").join(dll_name),
        project_root
            .join(".venv")
            .join("Lib")
            .join("site-packages")
            .join("onnxruntime")
            .join("capi")
            .join(dll_name),
    ];
    if let Ok(exe) = env::current_exe()
        && let Some(dir) = exe.parent()
    {
        candidates.insert(0, dir.join(dll_name));
    }
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .unwrap_or_else(|| project_root.join("models").join("runtime").join(dll_name))
}

fn find_yunet_model(project_root: &Path) -> PathBuf {
    if let Ok(p) = env::var("ROJAKFACE_YUNET_MODEL") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return pb;
        }
    }
    let home = user_home_dir();
    let candidates = [
        project_root.join("models").join("yunet_face_detector.onnx"),
        home.join(".cache")
            .join("rojakface")
            .join("yunet_face_detector.onnx"),
    ];
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .unwrap_or_else(|| {
            home.join(".cache")
                .join("rojakface")
                .join("yunet_face_detector.onnx")
        })
}

fn find_clip_model(project_root: &Path) -> PathBuf {
    if let Ok(p) = env::var("ROJAKFACE_CLIP_MODEL") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return pb;
        }
    }
    let direct = project_root.join("models").join("patch32_v1.onnx");
    if direct.is_file() {
        return direct;
    }
    let hf_dir = user_home_dir()
        .join(".cache")
        .join("huggingface")
        .join("hub")
        .join("models--minimaxir--imgbeddings");
    if hf_dir.is_dir() {
        for entry in walkdir::WalkDir::new(&hf_dir)
            .follow_links(true)
            .into_iter()
            .flatten()
        {
            if entry.file_type().is_file()
                && entry
                    .file_name()
                    .to_str()
                    .is_some_and(|n| n.ends_with(".onnx"))
            {
                return entry.into_path();
            }
        }
    }
    direct
}

fn find_templates_dir(project_root: &Path) -> PathBuf {
    let candidates = [
        project_root.join("templates").join("admin_portal"),
        project_root.join("src").join("models").join("admin_portal"),
    ];
    candidates
        .into_iter()
        .find(|p| p.join("dashboard.html").is_file())
        .unwrap_or_else(|| project_root.join("templates").join("admin_portal"))
}

fn load_or_create_admin_key(project_root: &Path) -> String {
    if let Ok(k) = env::var("ROJAKFACE_ADMIN_KEY") {
        let trimmed = k.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    let key_file = project_root.join(".rojakface_admin_key");
    if let Ok(content) = fs::read_to_string(&key_file) {
        let trimmed = content.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    let mut bytes = [0u8; 24];
    rand::rng().fill_bytes(&mut bytes);
    let generated = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let _ = fs::write(&key_file, &generated);
    generated
}

impl AppConfig {
    pub fn load() -> Self {
        let project_root = find_project_root();
        let data_dir = project_root.join("data");
        let interim_dir = data_dir.join("interim");
        let processed_dir = data_dir.join("processed");
        let store_faces_dir = interim_dir.join("store-faces");
        let profile_dir = processed_dir.join("profiles");
        let locations_dir = processed_dir.join("locations");
        let documents_dir = processed_dir.join("documents");
        let pending_dir = interim_dir.join("pending_requests");
        let pending_images_dir = pending_dir.join("images");
        let pending_file = pending_dir.join("requests.json");
        let vectors_file = processed_dir.join("rojakface_vectors.json");
        let location_vectors_file = processed_dir.join("location_vectors.json");

        let default_lc = project_root
            .parent()
            .unwrap_or(&project_root)
            .join("LibreChat");
        let librechat_dir = env::var("LIBRECHAT_DIR")
            .map(PathBuf::from)
            .unwrap_or(default_lc);
        let profile_web_dir = librechat_dir.join("images").join("profile-gallery");
        let location_web_dir = librechat_dir.join("images").join("location-gallery");
        let document_web_dir = librechat_dir.join("images").join("document-gallery");
        let librechat_url = env::var("LIBRECHAT_URL")
            .unwrap_or_else(|_| "http://localhost:3080".to_string())
            .trim_end_matches('/')
            .to_string();

        let image_dirs = vec![
            librechat_dir.join("images"),
            librechat_dir.join("client").join("public").join("images"),
            librechat_dir.join("uploads"),
        ];

        let host = env::var("ROJAKFACE_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let port = env::var("ROJAKFACE_PORT")
            .ok()
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(8005);
        let admin_user = env::var("ROJAKFACE_ADMIN_USER").unwrap_or_else(|_| "admin".to_string());
        let admin_pass =
            env::var("ROJAKFACE_ADMIN_PASS").unwrap_or_else(|_| "admin123".to_string());
        let admin_key = load_or_create_admin_key(&project_root);
        let ollama_url = env::var("ROJAKFACE_OLLAMA_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:11434/api/chat".to_string());
        let vision_model = env::var("ROJAKFACE_VISION_MODEL")
            .unwrap_or_else(|_| "qwen3-vl:4b-instruct-16k".to_string());

        let ort_dylib_path = find_ort_dylib(&project_root);
        let yunet_model_path = find_yunet_model(&project_root);
        let clip_model_path = find_clip_model(&project_root);
        let templates_dir = find_templates_dir(&project_root);

        let _ = fs::create_dir_all(&pending_images_dir);
        let _ = fs::create_dir_all(&profile_dir);
        let _ = fs::create_dir_all(&locations_dir);
        let _ = fs::create_dir_all(&documents_dir);
        let _ = fs::create_dir_all(&location_web_dir);
        let _ = fs::create_dir_all(&document_web_dir);
        let _ = fs::create_dir_all(store_faces_dir.join("crops"));
        let _ = fs::create_dir_all(store_faces_dir.join("originals"));

        Self {
            project_root,
            store_faces_dir,
            profile_dir,
            locations_dir,
            documents_dir,
            pending_dir,
            pending_images_dir,
            pending_file,
            vectors_file,
            location_vectors_file,
            librechat_dir,
            profile_web_dir,
            location_web_dir,
            document_web_dir,
            librechat_url,
            image_dirs,
            host,
            port,
            admin_user,
            admin_pass,
            admin_key,
            ollama_url,
            vision_model,
            ort_dylib_path,
            yunet_model_path,
            clip_model_path,
            templates_dir,
        }
    }
}
