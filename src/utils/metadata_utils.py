"""Pure metadata, identity normalization, privacy classification, and time formatting helpers."""

from __future__ import annotations

import json
import re

PRIVATE_DETAIL_KEYS: set[str] = {
    "license_no",
    "dob",
    "date_of_birth",
    "ic",
    "nric",
    "passport",
    "ssn",
    "phone",
    "email",
    "exp",
}


def profile_slug(identity: str) -> str:
    """Convert an identity name into a filesystem- and URL-safe profile slug."""
    return re.sub(r"[^A-Za-z0-9._-]+", "_", identity).strip("._")


def format_identity_name(identity: str) -> str:
    """Normalize whitespace and capitalize the first letter of each word."""
    words = identity.strip().split()
    formatted: list[str] = []
    for word in words:
        if word.islower() or word.isupper():
            formatted.append(word[:1].upper() + word[1:].lower())
        else:
            formatted.append(word[:1].upper() + word[1:])
    return " ".join(formatted)


def format_time_12h(value: str) -> str:
    """Format a 24-hour time string (HH:MM or HH:MM:SS) as 12-hour AM/PM (e.g. '12:15 PM')."""
    text = value.strip()
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


def default_detail_visibility(key: str) -> str:
    """Classify a custom profile field key as 'private' or 'public'."""
    norm = re.sub(r"[^a-z0-9_]", "_", key.strip().lower())
    return "private" if norm in PRIVATE_DETAIL_KEYS else "public"


def parse_proposed_metadata(raw: str) -> dict:
    """Parse JSON metadata safely, recovering key-value pairs from truncated JSON."""
    text = raw.strip()
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
        pairs = re.findall(r'"([^"\\]+)"\s*:\s*"([^"\\]*)"', text)
        if pairs:
            return dict(pairs)
    return {"description": text}
# >>> rust-anything: metadata-utils-rs is on. `rust-anything disable ../../rust/metadata-utils-rs` removes this block.
def _rust_anything_metadata_utils_rs():
    import importlib, os, sys
    mode = os.environ.get("RUST_ANYTHING_MODE")
    if mode == "old" or os.environ.get("RUST_ANYTHING_UNIT_METADATA_UTILS") == "original":
        return
    at = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../rust/metadata-utils-rs/python"))
    sys.path.insert(0, at)
    try:
        native = importlib.import_module("metadata_utils_rs")
    except ImportError:
        if mode == "new":
            raise
        return
    finally:
        sys.path.remove(at)
    globals().update({name: getattr(native, name) for name in ["profile_slug", "format_identity_name", "format_time_12h", "default_detail_visibility", "parse_proposed_metadata"]})


_rust_anything_metadata_utils_rs()
del _rust_anything_metadata_utils_rs
# <<< rust-anything: metadata-utils-rs
