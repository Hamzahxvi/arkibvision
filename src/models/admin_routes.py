import asyncio
import json
import mimetypes
import os
from pathlib import Path
import re
import secrets
import time
from starlette.requests import Request as StarletteRequest
from starlette.responses import HTMLResponse, JSONResponse, RedirectResponse, Response
from starlette.routing import Route


def mount_admin_portal(
    app,
    *,
    admin_key: str,
    admin_sessions: dict[str, float],
    session_cookie: str,
    session_ttl_seconds: int,
    pending_dir: Path,
    pending_images_dir: Path,
    pending_file: Path,
    pending_lock,
    profile_dir: Path,
    profile_web_dir: Path,
    image_exts: set[str],
    min_clip_threshold: float,
    identify_fn,
    list_identities_fn,
    enroll_named_fn,
    update_metadata_fn,
    rename_identity_fn,
    delete_identity_fn,
    review_request_fn,
    profile_identity_fn,
    profile_record_fn,
    add_profile_image_fn,
):
    """Register Admin Portal routes and wrap the Starlette app with session auth."""
    admin_username = os.getenv("ROJAKFACE_ADMIN_USER", "admin")
    admin_password = os.getenv("ROJAKFACE_ADMIN_PASS", "admin123")

    portal_dir = Path(__file__).resolve().parent / "admin_portal"
    login_html = (portal_dir / "login.html").read_text(encoding="utf-8")
    dashboard_html = (portal_dir / "dashboard.html").read_text(encoding="utf-8")
    test_recognition_html = (portal_dir / "test_recognition.html").read_text(encoding="utf-8")

    def _admin_cookie(scope) -> str | None:
        now = time.time()
        for raw_name, raw_value in scope.get("headers", []):
            if raw_name.lower() == b"cookie":
                for item in raw_value.decode("latin-1").split(";"):
                    name, _, value = item.strip().partition("=")
                    if name == session_cookie:
                        expires_at = admin_sessions.get(value)
                        if expires_at is not None:
                            if expires_at > now:
                                return value
                            admin_sessions.pop(value, None)
        return None

    def _login_page(error: str = "") -> HTMLResponse:
        error_html = f'<div class="error">{error}</div>' if error else ""
        return HTMLResponse(login_html.replace("{{ERROR}}", error_html))

    async def admin_login(request: StarletteRequest):
        if request.method == "GET":
            return _login_page()
        form = await request.form()
        username = str(form.get("username", "")).strip()
        password = str(form.get("password", ""))
        if not (secrets.compare_digest(username, admin_username) and secrets.compare_digest(password, admin_password)):
            return _login_page("Invalid username or password.")
        now = time.time()
        for tok, exp in list(admin_sessions.items()):
            if exp <= now:
                admin_sessions.pop(tok, None)
        session = secrets.token_urlsafe(32)
        admin_sessions[session] = now + session_ttl_seconds
        response = RedirectResponse("/admin", status_code=303)
        response.set_cookie(session_cookie, session, max_age=session_ttl_seconds, httponly=True, samesite="lax", path="/admin")
        return response

    async def admin_dashboard(request: StarletteRequest):
        return HTMLResponse(dashboard_html)

    async def admin_test_recognition_page(request: StarletteRequest):
        return HTMLResponse(test_recognition_html)

    async def admin_test_recognition(request: StarletteRequest):
        form = await request.form()
        upload = form.get("image")
        if not upload or not getattr(upload, "filename", ""):
            return JSONResponse({"status": "error", "reason": "Choose an image to test."}, status_code=400)
        suffix = Path(upload.filename).suffix.lower()
        if suffix not in image_exts:
            return JSONResponse({"status": "error", "reason": "Use JPG, PNG, WEBP, or BMP images."}, status_code=400)
        try:
            threshold = max(float(form.get("threshold", min_clip_threshold)), 0.50)
        except (TypeError, ValueError):
            threshold = min_clip_threshold
        temp_path = pending_images_dir / f"ADMIN-TEST-{secrets.token_hex(8)}{suffix}"
        try:
            temp_path.write_bytes(await upload.read())
            result = json.loads(await asyncio.to_thread(identify_fn, str(temp_path), threshold=threshold, save_new_image=False))
            if result.get("status") == "unresolved":
                return JSONResponse(result, status_code=400)
            return JSONResponse(result)
        except Exception as exc:
            return JSONResponse({"status": "error", "reason": str(exc)}, status_code=400)
        finally:
            temp_path.unlink(missing_ok=True)

    async def admin_api_requests(request: StarletteRequest):
        with pending_lock:
            reqs = json.loads(pending_file.read_text(encoding="utf-8")) if pending_file.exists() else {}
        result = []
        for req_id, req in reqs.items():
            result.append({
                "id": req_id,
                "name": req.get("identity", ""),
                "status": req.get("status", "pending"),
                "request_type": req.get("type", ""),
                "submitted_at": req.get("submitted_at", ""),
                "metadata": req.get("proposed_metadata", {}),
                "note": req.get("notes", ""),
            })
        result.sort(key=lambda x: x["submitted_at"], reverse=True)
        return JSONResponse(result)

    async def admin_api_profiles(request: StarletteRequest):
        def _load_profiles():
            identities = json.loads(list_identities_fn()).get("identities", {})
            return [profile_record_fn(identity) for identity in sorted(identities, key=str.casefold)]

        return JSONResponse(await asyncio.to_thread(_load_profiles))

    async def admin_profile_image(request: StarletteRequest):
        profile_slug = request.path_params.get("profile_slug", "")
        filename = request.path_params.get("filename", "")
        identity = profile_identity_fn(profile_slug)
        if not identity or Path(filename).name != filename:
            return Response(status_code=404)
        image_path = (profile_dir / profile_slug / "images" / filename).resolve()
        image_root = (profile_dir / profile_slug / "images").resolve()
        if image_root not in image_path.parents or not image_path.is_file():
            return Response(status_code=404)
        mime = mimetypes.guess_type(str(image_path))[0] or "image/jpeg"
        return Response(content=image_path.read_bytes(), media_type=mime)

    async def admin_create_profile(request: StarletteRequest):
        form = await request.form()
        identity = str(form.get("identity", "")).strip()
        upload = form.get("image")
        if not identity or upload is None or not hasattr(upload, "read"):
            return JSONResponse({"status": "rejected", "reason": "Name and a face image are required."}, status_code=400)
        suffix = Path(getattr(upload, "filename", "") or "upload.jpg").suffix.lower()
        if suffix not in image_exts:
            return JSONResponse({"status": "rejected", "reason": "Use JPG, PNG, WEBP, or BMP images."}, status_code=400)
        temp_path = pending_images_dir / f"ADMIN-{secrets.token_hex(8)}{suffix}"
        try:
            temp_path.write_bytes(await upload.read())
            proposed = {
                key: str(form.get(key, "")).strip()
                for key in ("address", "notes", "description", "photograph_by", "location", "date", "time")
                if str(form.get(key, "")).strip()
            }
            raw_other = str(form.get("other_details", "")).strip()
            if raw_other:
                try:
                    parsed_other = json.loads(raw_other)
                    if isinstance(parsed_other, dict):
                        proposed["other_details"] = parsed_other
                except json.JSONDecodeError:
                    pass
            result = json.loads(
                await asyncio.to_thread(
                    enroll_named_fn,
                    identity=identity,
                    image_path=str(temp_path),
                    admin_key=admin_key,
                    proposed_metadata=json.dumps(proposed),
                )
            )
            if result.get("status") not in {"enrolled_by_admin", "unchanged"}:
                return JSONResponse(result, status_code=400)
            if proposed:
                metadata_result = json.loads(
                    await asyncio.to_thread(
                        update_metadata_fn,
                        identity=identity,
                        proposed_metadata=json.dumps(proposed),
                        admin_key=admin_key,
                    )
                )
                if metadata_result.get("status") != "updated_by_admin":
                    return JSONResponse(metadata_result, status_code=400)
            return JSONResponse({**result, "profile": await asyncio.to_thread(profile_record_fn, identity)})
        except Exception as exc:
            return JSONResponse({"status": "error", "reason": str(exc)}, status_code=400)
        finally:
            temp_path.unlink(missing_ok=True)

    async def admin_update_profile(request: StarletteRequest):
        profile_slug = request.path_params.get("profile_slug", "")
        identity = profile_identity_fn(profile_slug)
        if not identity:
            return JSONResponse({"status": "not_found", "reason": "Enrolled person not found."}, status_code=404)
        payload = await request.json()
        new_identity = str(payload.pop("identity", identity)).strip()
        allowed = {"address", "notes", "description", "photograph_by", "location", "date", "time"}
        metadata = {key: str(value).strip() for key, value in payload.items() if key in allowed and value is not None}
        if isinstance(payload.get("other_details"), dict):
            metadata["other_details"] = payload["other_details"]
            metadata["_replace_other"] = True
        if new_identity != identity:
            result = json.loads(await asyncio.to_thread(rename_identity_fn, identity, new_identity, admin_key=admin_key))
            if result.get("status") != "updated":
                return JSONResponse(result, status_code=400)
            identity = new_identity
        if metadata:
            result = json.loads(
                await asyncio.to_thread(
                    update_metadata_fn,
                    identity=identity,
                    proposed_metadata=json.dumps(metadata),
                    admin_key=admin_key,
                )
            )
            if result.get("status") != "updated_by_admin":
                return JSONResponse(result, status_code=400)
        return JSONResponse({"status": "updated", "profile": await asyncio.to_thread(profile_record_fn, identity)})

    async def admin_delete_profile(request: StarletteRequest):
        profile_slug = request.path_params.get("profile_slug", "")
        identity = profile_identity_fn(profile_slug)
        if not identity:
            return JSONResponse({"status": "not_found", "reason": "Enrolled person not found."}, status_code=404)
        result = json.loads(await asyncio.to_thread(delete_identity_fn, identity=identity, purge_files=True, admin_key=admin_key))
        if result.get("status") != "deleted":
            return JSONResponse(result, status_code=400)
        return JSONResponse(result)

    async def admin_add_profile_image(request: StarletteRequest):
        profile_slug = request.path_params.get("profile_slug", "")
        identity = profile_identity_fn(profile_slug)
        form = await request.form()
        upload = form.get("image")
        if not identity or upload is None or not hasattr(upload, "read"):
            return JSONResponse({"status": "rejected", "reason": "An enrolled person and image are required."}, status_code=400)
        suffix = Path(getattr(upload, "filename", "") or "upload.jpg").suffix.lower()
        if suffix not in image_exts:
            return JSONResponse({"status": "rejected", "reason": "Use JPG, PNG, WEBP, or BMP images."}, status_code=400)
        temp_path = pending_images_dir / f"ADMIN-{secrets.token_hex(8)}{suffix}"
        try:
            temp_path.write_bytes(await upload.read())
            result = await asyncio.to_thread(add_profile_image_fn, identity, temp_path)
            if result.get("status") != "image_added":
                return JSONResponse(result, status_code=400)
            return JSONResponse({**result, "profile": await asyncio.to_thread(profile_record_fn, identity)})
        finally:
            temp_path.unlink(missing_ok=True)

    async def admin_update_profile_image(request: StarletteRequest):
        profile_slug = request.path_params.get("profile_slug", "")
        filename = request.path_params.get("filename", "")
        identity = profile_identity_fn(profile_slug)
        if not identity or Path(filename).name != filename:
            return JSONResponse({"status": "not_found", "reason": "Profile image not found."}, status_code=404)
        payload = await request.json()
        pdir = profile_dir / profile_slug
        metadata_file = pdir / "metadata.json"
        if not metadata_file.is_file():
            return JSONResponse({"status": "not_found", "reason": "Profile metadata not found."}, status_code=404)
        try:
            metadata = json.loads(metadata_file.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            return JSONResponse({"status": "error", "reason": str(exc)}, status_code=400)
        allowed = {"description", "photograph_by", "location", "date", "time"}
        updated = False
        images_list = metadata.get("images") if isinstance(metadata.get("images"), list) else []
        for item in images_list:
            if isinstance(item, dict) and Path(str(item.get("profile_path") or item.get("filename") or "")).name == filename:
                for k in allowed:
                    if k in payload and payload[k] is not None:
                        item[k] = str(payload[k]).strip() or "Unknown"
                updated = True
                break
        if not updated:
            new_entry = {
                "filename": filename,
                "profile_path": f"profiles/{profile_slug}/images/{filename}",
                "url": f"/admin/profile-images/{profile_slug}/{filename}",
            }
            for k in allowed:
                if k in payload and payload[k] is not None:
                    new_entry[k] = str(payload[k]).strip() or "Unknown"
            images_list.append(new_entry)
            metadata["images"] = images_list
        metadata_file.write_text(json.dumps(metadata, indent=2), encoding="utf-8")
        return JSONResponse({"status": "image_updated", "identity": identity, "filename": filename, "profile": profile_record_fn(identity)})

    async def admin_delete_profile_image(request: StarletteRequest):
        profile_slug = request.path_params.get("profile_slug", "")
        filename = request.path_params.get("filename", "")
        identity = profile_identity_fn(profile_slug)
        if not identity or Path(filename).name != filename:
            return JSONResponse({"status": "not_found", "reason": "Profile image not found."}, status_code=404)
        pdir = profile_dir / profile_slug
        image_path = (pdir / "images" / filename).resolve()
        image_root = (pdir / "images").resolve()
        if image_root not in image_path.parents or not image_path.is_file():
            return JSONResponse({"status": "not_found", "reason": "Profile image not found."}, status_code=404)
        image_path.unlink()
        web_path = (profile_web_dir / profile_slug / filename).resolve()
        web_root = (profile_web_dir / profile_slug).resolve()
        if web_root in web_path.parents and web_path.is_file():
            web_path.unlink()
        metadata_file = pdir / "metadata.json"
        if metadata_file.is_file():
            try:
                metadata = json.loads(metadata_file.read_text(encoding="utf-8"))
                metadata["images"] = [item for item in metadata.get("images", []) if Path(str(item.get("profile_path", ""))).name != filename]
                metadata["stored_original_image_count"] = len(metadata["images"])
                metadata_file.write_text(json.dumps(metadata, indent=2), encoding="utf-8")
            except (OSError, json.JSONDecodeError):
                pass
        return JSONResponse({"status": "image_deleted", "identity": identity, "filename": filename, "profile": profile_record_fn(identity)})

    async def admin_logout(request: StarletteRequest):
        token = _admin_cookie(request.scope)
        if token:
            admin_sessions.pop(token, None)
        response = _login_page("You have been signed out.")
        response.delete_cookie(session_cookie, path="/admin")
        return response

    async def admin_serve_image(request: StarletteRequest):
        req_id = request.path_params.get("request_id", "")
        if not re.fullmatch(r"[A-Za-z0-9_-]+", req_id):
            return Response(status_code=404)
        img_type = request.path_params.get("img_type", "source")
        if img_type not in {"source", "crop"}:
            return Response(status_code=404)
        pattern = f"{req_id}_primary.*" if img_type == "crop" else f"{req_id}_source.*"
        candidates = list(pending_images_dir.glob(pattern))
        if not candidates:
            return Response(status_code=404)
        img_path = candidates[0].resolve()
        if pending_images_dir.resolve() not in img_path.parents or not img_path.is_file():
            return Response(status_code=404)
        mime = mimetypes.guess_type(str(img_path))[0] or "image/jpeg"
        return Response(content=img_path.read_bytes(), media_type=mime)

    async def admin_approve(request: StarletteRequest):
        req_id = request.path_params["request_id"]
        result = json.loads(await asyncio.to_thread(review_request_fn, request_id=req_id, action="approve", admin_key=admin_key))
        if result.get("status") == "approved":
            return JSONResponse(result)
        return JSONResponse(result, status_code=400)

    async def admin_reject(request: StarletteRequest):
        req_id = request.path_params["request_id"]
        result = json.loads(await asyncio.to_thread(review_request_fn, request_id=req_id, action="reject", admin_key=admin_key))
        if result.get("status") == "rejected":
            return JSONResponse(result)
        return JSONResponse(result, status_code=400)

    class AdminAuthMiddleware:
        """Require the Admin Portal's form-login session for protected routes."""

        def __init__(self, inner_app):
            self.app = inner_app

        async def __call__(self, scope, receive, send):
            if scope.get("type") == "http":
                path = scope.get("path", "")
                if path == "/admin" or path.startswith("/admin/"):
                    if path in {"/admin/login", "/admin/logout"}:
                        await self.app(scope, receive, send)
                        return
                    if _admin_cookie(scope) is None:
                        if path.startswith("/admin/api/") or path.startswith("/admin/profile-images/"):
                            response = JSONResponse({"status": "unauthorized", "reason": "Please sign in to the Admin Portal."}, status_code=401)
                        else:
                            response = _login_page()
                        await response(scope, receive, send)
                        return
            await self.app(scope, receive, send)

    app.routes.extend([
        Route("/admin/login", admin_login, methods=["GET", "POST"]),
        Route("/admin", admin_dashboard),
        Route("/admin/", admin_dashboard),
        Route("/admin/test-recognition", admin_test_recognition_page),
        Route("/admin/images/{img_type}/{request_id}", admin_serve_image),
        Route("/admin/api/requests", admin_api_requests),
        Route("/admin/api/profiles", admin_api_profiles),
        Route("/admin/api/test-recognition", admin_test_recognition, methods=["POST"]),
        Route("/admin/api/profiles", admin_create_profile, methods=["POST"]),
        Route("/admin/api/profiles/{profile_slug}", admin_update_profile, methods=["PATCH"]),
        Route("/admin/api/profiles/{profile_slug}", admin_delete_profile, methods=["DELETE"]),
        Route("/admin/api/profiles/{profile_slug}/images", admin_add_profile_image, methods=["POST"]),
        Route("/admin/api/profiles/{profile_slug}/images/{filename:path}", admin_update_profile_image, methods=["PATCH"]),
        Route("/admin/api/profiles/{profile_slug}/images/{filename:path}", admin_delete_profile_image, methods=["DELETE"]),
        Route("/admin/profile-images/{profile_slug}/{filename:path}", admin_profile_image),
        Route("/admin/logout", admin_logout),
        Route("/admin/api/approve/{request_id}", admin_approve, methods=["POST"]),
        Route("/admin/api/reject/{request_id}", admin_reject, methods=["POST"]),
    ])
    return AdminAuthMiddleware(app), admin_password
