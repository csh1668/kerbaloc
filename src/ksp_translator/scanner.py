"""GameData directory scanner for localization cfg files."""

from __future__ import annotations

from pathlib import Path

from ksp_translator.config import EXCLUDED_DIRS


def scan_gamedata(gamedata: Path) -> list[Path]:
    """Recursively find cfg files containing en-us localization blocks."""
    results: list[Path] = []
    for cfg in gamedata.rglob("*.cfg"):
        if _is_excluded(cfg, gamedata):
            continue
        if _has_en_us_block(cfg):
            results.append(cfg)
    return sorted(results)


def _is_excluded(path: Path, root: Path) -> bool:
    rel = path.relative_to(root)
    return any(part in EXCLUDED_DIRS for part in rel.parts)


def _has_en_us_block(path: Path) -> bool:
    try:
        text = path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return False
    return "en-us" in text and "Localization" in text
