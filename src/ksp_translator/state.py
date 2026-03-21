"""Translation state and cache management."""

from __future__ import annotations

import hashlib
import json
import shutil
from pathlib import Path

STATE_FILE = Path(".ksp-translator-state.json")


class TranslationState:
    """Tracks translation progress, caches, and backup state."""

    def __init__(self, state_path: Path = STATE_FILE):
        self.path = state_path
        self.data: dict = self._load()

    def _load(self) -> dict:
        if self.path.exists():
            return json.loads(self.path.read_text(encoding="utf-8"))
        return {"files": {}, "cache": {}, "failed": {}, "backups": {}}

    def save(self) -> None:
        self.path.write_text(
            json.dumps(self.data, ensure_ascii=False, indent=2),
            encoding="utf-8",
        )

    def file_hash(self, path: Path) -> str:
        return hashlib.sha256(path.read_bytes()).hexdigest()[:16]

    def is_file_changed(self, path: Path) -> bool:
        key = str(path)
        current_hash = self.file_hash(path)
        return self.data["files"].get(key, {}).get("hash") != current_hash

    def mark_file_done(self, path: Path, translated_keys: list[str]) -> None:
        self.data["files"][str(path)] = {
            "hash": self.file_hash(path),
            "translated_keys": translated_keys,
        }

    def get_cached(self, loc_key: str) -> str | None:
        return self.data["cache"].get(loc_key)

    def set_cached(self, loc_key: str, value: str) -> None:
        self.data["cache"][loc_key] = value

    def get_failed_keys(self) -> dict[str, str]:
        return self.data.get("failed", {})

    def mark_failed(self, loc_key: str, original: str) -> None:
        self.data.setdefault("failed", {})[loc_key] = original

    def clear_failed(self, loc_key: str) -> None:
        self.data.get("failed", {}).pop(loc_key, None)

    # --- Backup management ---

    def get_translated_files(self) -> dict[str, dict]:
        """Return dict of source_path -> {translated_path, rel_path}."""
        return self.data.get("translated", {})

    def register_translated(
        self, source_path: Path, translated_path: Path, rel_path: str
    ) -> None:
        self.data.setdefault("translated", {})[str(source_path)] = {
            "translated_path": str(translated_path),
            "rel_path": rel_path,
        }

    def is_applied(self) -> bool:
        return self.data.get("applied", False)

    def set_applied(self, applied: bool) -> None:
        self.data["applied"] = applied

    def get_backup_dir(self) -> str | None:
        return self.data.get("backup_dir")

    def set_backup_dir(self, path: str) -> None:
        self.data["backup_dir"] = path

    def clear(self) -> None:
        self.data = {"files": {}, "cache": {}, "failed": {}, "backups": {}}
        self.save()

    def summary(self) -> dict[str, int | bool]:
        cached = len(self.data.get("cache", {}))
        files = len(self.data.get("files", {}))
        failed = len(self.data.get("failed", {}))
        translated = len(self.data.get("translated", {}))
        applied = self.data.get("applied", False)
        return {
            "cached_translations": cached,
            "completed_files": files,
            "failed_keys": failed,
            "translated_files": translated,
            "applied": applied,
        }
