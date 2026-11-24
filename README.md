# RojakFace Enterprise Vision, Biometric Identity & Multilingual OCR Platform (Pure Rust)

An **air-gapped, 100% on-premise multimodal AI platform** written in **100% safe Rust (`#![forbid(unsafe_code)]`)** that integrates **Biometric Face Recognition**, **Privacy-Aware Identity & Visual Photo Retrieval**, **ID Card / Document Data Ingestion**, and **Multilingual Document & Handwriting OCR (Jawi, Sanskrit, Tamil, Mandarin, Malay, English)** inside **LibreChat** (`http://localhost:3080`), powered by a single compiled **Rust Axum + Tokio MCP SSE Server** (`http://0.0.0.0:8005/sse`) and **Admin Portal** (`http://127.0.0.1:8005/admin`).

![RojakFace Pipeline Architecture Diagram](reports/pipeline_diagram.svg)

---

## Table of Contents
1. [Key Features](#1-key-features)
2. [Pure-Rust Architecture](#2-pure-rust-architecture)
3. [System Requirements & Prerequisites](#3-system-requirements--prerequisites)
4. [Build & Setup Guide](#4-build--setup-guide)
5. [Running & Stopping the Platform](#5-running--stopping-the-platform)
6. [Core Processing Pipelines & 14 MCP Tools](#6-core-processing-pipelines--14-mcp-tools)
7. [Admin Portal & Public/Private Data Governance](#7-admin-portal--publicprivate-data-governance)
8. [Chatbox Usage & Example Prompts](#8-chatbox-usage--example-prompts)
9. [Project Structure](#9-project-structure)
10. [Configuration & Environment Variables](#10-configuration--environment-variables)
11. [Testing & Verification](#11-testing--verification)

---

## 1. Key Features

- **100% Pure Rust Backend (`#![forbid(unsafe_code)]`, Zero Python Runtime)**: Compiles to a single lightweight native binary (`rojakface-server.exe`, ~9 MB) with instant startup, low memory footprint, true async multi-threading (`tokio` + `axum`), and compile-time memory safety.
- **100% Air-Gapped Local AI (Single Vision-Language Model)**: Zero cloud API dependencies. Runs entirely on local hardware using **Ollama (`qwen3-vl:4b-instruct-16k`)**, native **ONNX Runtime (`ort`)** for **YuNet** face detection and **CLIP ViT-B/32 (768-D)** embeddings, and an embedded **Pure-Rust Cosine Vector Store** (`data/processed/rojakface_vectors.json`).
- **Biometric Face Recognition & Auto-Gallery Capture**: Identifies enrolled individuals with an anti-false-positive cosine threshold (`>= 0.88`) and automatically captures new high-confidence (`>= 0.90`), non-duplicate face angles using 16×16 perceptual average hashing (`aHash`).
- **Direct Database & Visual Attribute Photo Search**: Case- and spacing-insensitive identity lookup supporting count limits (*"3 photos only"*), photos-only rendering, and visual attribute filtering (*"photo that wear songkok"*, *"wearing glasses"*) via per-profile `visual_index.json` + async `reqwest` Ollama vision fallback.
- **Public vs. Private Data Boundary**: Profile attributes are divided into **Public Data** (shown in chatbox Markdown tables) and **Private Data** (`license_no`, `dob`, `ic`, `nric`, `passport`, `exp`, `phone`, `email`, strictly hidden from chat and accessible only in the Admin Portal).
- **Multilingual Document & Handwriting OCR with Visual Chunking**:
  - **Adaptive Line-Valley Chunker** (`rojakface_ocr_document`): Projects horizontal edge energy in microseconds to count exact text lines and slices tall/dense manuscripts along natural whitespace valleys (`Top`, `Middle`, `Bottom`) with auto-contrast and `1.4×` sharpening.
  - **Jawi-to-Rumi Orthographic & Phonetic Decoder** (`rojakface_decode_jawi`): Restores 3-dot Jawi letters (`ڤ`=`p`, `ڠ`=`ng`, `ݢ`=`g`, `چ`=`c`, `ڽ`=`ny`), applies classical Jawi vowel rules, and matches phonetic skeletons against a Standard Bahasa Melayu Rumi lexicon.
  - Supports **Jawi**, **Sanskrit (Devanagari)**, **Tamil**, **Mandarin Chinese**, **Bahasa Melayu**, and **English**.
- **Role-Based Governance (Guest vs. Admin)**: Chatbox users operate as **Guests** (read-only search; enrollments and ID card metadata updates are staged as `REQ-*` requests). **Admins** review, approve, or reject requests and manage profiles in the authenticated Admin Portal.

---

## 2. Pure-Rust Architecture

| Component | Rust Module / Crate | Responsibility |
| :--- | :--- | :--- |
| **HTTP & Admin Portal** | `src/admin.rs` (`axum` + `tokio`) | Serves `/admin`, form login, 8-hour HttpOnly sessions, multipart uploads, profile CRUD, approval queue, and recognition test bench. |
| **MCP-over-SSE Server** | `src/mcp_sse.rs` (`axum` SSE + JSON-RPC 2.0) | Exposes all 14 MCP tools over `GET /sse` & `POST /messages/` for LibreChat (`host.docker.internal:8005/sse`). |
| **Biometric ONNX Engine** | `src/biometrics.rs` (`ort` dynamic runtime) | Runs YuNet (`yunet_face_detector.onnx`) + NMS + CLIP ViT-B/32 (`patch32_v1.onnx`) 768-D embeddings natively in Rust. |
| **Embedded Vector Store** | `src/vector_store.rs` | Thread-safe `RwLock` 768-D cosine similarity search with atomic JSON persistence (`rojakface_vectors.json`), zero file-lock conflicts. |
| **Vision & OCR Chunker** | `src/vision_ops.rs` (`image` + `kamadak-exif`) | EXIF auto-orient, 16×16 perceptual `aHash`, horizontal edge-energy line-valley chunking, autocontrast, and sharpening. |
| **Jawi Phonetic Decoder** | `src/jawi.rs` | 3-dot Jawi restoration, reduplication (`٢`), vowel inference, and Standard Bahasa Melayu Rumi skeleton matching. |
| **Metadata & Privacy** | `src/metadata.rs` | URL/filesystem slugging, identity capitalization, 12-hour AM/PM time formatting, public/private classification, and truncated JSON recovery. |
| **Orchestration Service** | `src/service.rs` (`reqwest`) | Implements all 14 MCP tools, LibreChat attachment resolution, perceptual ID cache, auto-gallery capture, staging queue, and Ollama visual filter. |

---

## 3. System Requirements & Prerequisites

| Requirement | Minimum / Recommended Specification |
| :--- | :--- |
| **Operating System** | Windows 10/11 (64-bit), Linux, or macOS |
| **Rust Toolchain** | Rust **1.97+** (`cargo`, `rustc`) — *no Python required at runtime* |
| **ONNX Runtime + Models** | `onnxruntime.dll` (or `.so`/`.dylib`), `yunet_face_detector.onnx`, and `patch32_v1.onnx` in `models/` (or cached under `~/.cache/`) |
| **Docker** | **Docker Desktop** (Compose v2) for LibreChat (`http://localhost:3080`) |
| **Ollama** | **Ollama** (`http://127.0.0.1:11434`) running `qwen3-vl:4b-instruct-16k` |

---

## 4. Build & Setup Guide

### Step 1 — Clone & Compile the Pure-Rust Binary
```powershell
git clone https://github.com/Hamzahxvi/rojakface-enterprise-vision.git
cd rojakface-enterprise-vision

cargo build --release
```
The compiled native binary is output to `target/release/rojakface-server.exe` (or `target/release/rojakface-server` on Linux/macOS).

### Step 2 — Pull & Build the Local Ollama Model
```powershell
ollama pull qwen3-vl:4b-instruct
ollama create qwen3-vl:4b-instruct-16k -f models/Modelfile
```

### Step 3 — Configure Sibling LibreChat (`../LibreChat`)
```powershell
if (-not (Test-Path ..\LibreChat)) {
    git clone https://github.com/danny-avila/LibreChat.git ..\LibreChat
    Copy-Item ..\LibreChat\.env.example ..\LibreChat\.env
}
New-Item -ItemType Directory -Force -Path ..\LibreChat\images | Out-Null

Copy-Item config\librechat\librechat.yaml ..\LibreChat\librechat.yaml -Force
Copy-Item config\librechat\docker-compose.override.yml ..\LibreChat\docker-compose.override.yml -Force
Copy-Item config\librechat\custom-index.html ..\LibreChat\custom-index.html -Force
Copy-Item config\librechat\royal-theme.css ..\LibreChat\images\royal-theme.css -Force
```

---

## 5. Running & Stopping the Platform

### One-Click Launch (Windows)
```powershell
cmd /c start.bat
```
`start.bat` builds `target\release\rojakface-server.exe` if needed, starts Ollama (`11434`), launches `rojakface-server.exe` (`8005`), starts LibreChat Docker (`3080`), and opens `http://localhost:3080`.

### One-Click Shutdown (Windows)
```powershell
cmd /c stop.bat
```

### Manual CLI Launch & Biometric Smoke-Test
```powershell
# Run the full MCP SSE + Admin Portal server
.\target\release\rojakface-server.exe

# Run an instant offline face identification check on any image
.\target\release\rojakface-server.exe --verify-face path\to\portrait.jpg
```

---

## 6. Core Processing Pipelines & 14 MCP Tools

Exposed over standard MCP SSE (`http://host.docker.internal:8005/sse` and `/messages/`):

| Pipeline | MCP Tool | Description |
| :--- | :--- | :--- |
| **Pipeline A: Biometric Face ID** | `rojakface_identify_faces` | YuNet face detection + 768-D CLIP embeddings + cosine match (`>= 0.88`), auto-gallery capture (`>= 0.90`), returns `MATCHED`, `NOT_IN_DATABASE`, or `NO_FACE_DETECTED` (with `ocr_document_chunks`). |
| | `rojakface_detect_faces` | Low-level YuNet bounding box `[x, y, w, h]` and confidence inspection. |
| **Pipeline B: Database & Photo Search** | `rojakface_search_identity` | Case/spacing-insensitive profile lookup supporting `limit`, `photos_only`, and visual `query` via `visual_index.json` + local Ollama vision fallback. Filters out Private Data and formats per-photo `Time` in 12-hour AM/PM. |
| | `rojakface_list_identities` | Lists all enrolled identities and reference vector counts. |
| **Pipeline C: Document Chunking & Jawi OCR** | `rojakface_ocr_document` | Adaptive horizontal edge-energy line-valley chunking with autocontrast & `1.4×` sharpening (`/images/ocr-chunks/`). |
| | `rojakface_decode_jawi` | Decodes Jawi script or rough transliterations into corrected 3-dot Jawi (`ڤ ڠ ݢ چ ڽ`) and Standard Bahasa Melayu (Rumi). |
| **Pipeline D: Role-Based Governance** | `rojakface_request_enrollment` | Stages a Guest enrollment request (`REQ-*`) for Admin review. |
| | `rojakface_request_update_metadata` | Stages a Guest metadata/ID-card update request (`REQ-*`), auto-classifying sensitive keys as `private`. |
| | `rojakface_list_pending_requests` | Lists pending, approved, or rejected staging requests. |
| | `rojakface_review_pending_request` | Admin-key guarded approval/rejection tool. |
| | `rojakface_enroll_named_identity` | Admin direct enrollment tool (auto-stages for Guests). |
| | `rojakface_enroll_or_learn` | Admin batch learning tool. |
| | `rojakface_rename_identity` | Admin rename across vector store, profiles, and gallery. |
| | `rojakface_delete_identity` | Admin delete across vector store and stored files. |

---

## 7. Admin Portal & Public/Private Data Governance

Open **`http://127.0.0.1:8005/admin`**:
- **Form Login & Session Auth**: Constant-time credential verification with 8-hour `HttpOnly` session cookies (`ROJAKFACE_ADMIN_USER` / `ROJAKFACE_ADMIN_PASS`).
- **Pending Approval Queue**: Inspect staged source photos, face crops, and extracted ID card fields, then **Approve** or **Reject** (logged to `data/interim/pending_requests/history.jsonl`).
- **Trusted Profile Editor**: Edit Name, Address, Notes, per-photo metadata (`Description`, `Photograph by`, `Location`, `Date`, `Time`), upload/delete photos, and toggle custom attributes between **Public Data** and **Private Data**.
- **Recognition Test Bench** (`http://127.0.0.1:8005/admin/test-recognition`): Upload test portraits and tune the cosine threshold (`0.88–0.98`) without modifying stored galleries.

---

## 8. Chatbox Usage & Example Prompts

- **Face Identification** *(attach portrait)*: `who is this? show all his details and photos`
- **Profile Lookup**: `show eula lawrence full details` / `show azhar idrus details`
- **Photo Limits & Visual Search**: `show lando zawawi photos. 2 photos only.` / `show lando zawawi photo that wear songkok`
- **Multilingual OCR** *(attach document/handwriting)*: `read this jawi text, convert to rumi, and explain the meaning` / `transcribe this sanskrit text chunk by chunk`
- **Guest Staging**: `enroll this person as Ahmad Faizal` / `extract all details from this ID card and save them to Carl Johnson's profile`

---

## 9. Project Structure

```text
├── Cargo.toml                      # Pure-Rust package manifest (rojakface-server v2.0.0)
├── Cargo.lock                      # Reproducible Rust dependency lockfile
├── src/                            # 100% safe Rust (#![forbid(unsafe_code)])
│   ├── main.rs                     # Tokio + Axum server entrypoint & CLI (--verify-face)
│   ├── config.rs                   # Environment config, paths & ONNX/DLL auto-discovery
│   ├── biometrics.rs               # Native YuNet ONNX face detector + CLIP ViT-B/32 embedder
│   ├── vector_store.rs             # Embedded thread-safe 768-D cosine vector store
│   ├── vision_ops.rs               # EXIF orientation, perceptual aHash, OCR line-valley chunker
│   ├── jawi.rs                     # Jawi-to-Rumi orthographic & phonetic skeleton decoder
│   ├── metadata.rs                 # Slugs, identity normalization, 12h time, privacy & JSON recovery
│   ├── service.rs                  # Core business logic backing all 14 MCP tools & Admin APIs
│   ├── mcp_sse.rs                  # MCP-over-SSE server (/sse & /messages/)
│   └── admin.rs                    # Axum Admin Portal routes & session auth middleware
├── templates/
│   └── admin_portal/               # login.html, dashboard.html, test_recognition.html
├── config/
│   ├── librechat/                  # librechat.yaml, docker-compose.override.yml, custom-index.html, royal-theme.css
│   ├── docker-compose.qdrant.yml
│   └── settings.json
├── models/
│   └── Modelfile                   # Ollama Modelfile for qwen3-vl:4b-instruct-16k
├── reports/
│   └── pipeline_diagram.svg
├── Dockerfile                      # Multi-stage Pure-Rust Docker image
├── start.bat                       # One-click launch (Ollama + rojakface-server.exe + LibreChat)
├── stop.bat                        # One-click shutdown for rojakface-server.exe
└── README.md
```

---

## 10. Configuration & Environment Variables

| Variable | Default | Purpose |
| :--- | :--- | :--- |
| `ROJAKFACE_HOST` | `0.0.0.0` | Bind host for MCP SSE & Admin Portal |
| `ROJAKFACE_PORT` | `8005` | Port for `/sse` and `/admin` |
| `ROJAKFACE_ADMIN_USER` | `admin` | Admin Portal login username |
| `ROJAKFACE_ADMIN_PASS` | `admin123` | Admin Portal login password (override in production) |
| `ROJAKFACE_ADMIN_KEY` | *(persisted in `.rojakface_admin_key`)* | Internal secret key for Admin-only MCP operations |
| `ROJAKFACE_OLLAMA_URL` | `http://127.0.0.1:11434/api/chat` | Local Ollama endpoint for visual attribute fallback |
| `ROJAKFACE_VISION_MODEL` | `qwen3-vl:4b-instruct-16k` | Local Ollama vision model |
| `ORT_DYLIB_PATH` | *(auto-discovered in `models/runtime/`)* | Path to `onnxruntime.dll` / `libonnxruntime.so` |
| `ROJAKFACE_YUNET_MODEL` | *(auto-discovered in `models/` or `~/.cache/`)* | Path to `yunet_face_detector.onnx` |
| `ROJAKFACE_CLIP_MODEL` | *(auto-discovered in `models/` or `~/.cache/`)* | Path to `patch32_v1.onnx` |
| `LIBRECHAT_DIR` | `../LibreChat` | Path to sibling LibreChat directory |
| `LIBRECHAT_URL` | `http://localhost:3080` | Base URL for profile gallery & OCR chunk links |

---

## 11. Testing & Verification

```powershell
# Lint with zero warnings
cargo clippy --all-targets -- -D warnings

# Run all Rust unit tests
cargo test

# Build optimized release binary
cargo build --release
```
