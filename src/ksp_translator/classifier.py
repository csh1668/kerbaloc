"""Classify localization entries for translation strategy."""

from __future__ import annotations

import re
from enum import Enum, auto

FORMAT_PATTERN = re.compile(r"%(\.?\d*[dfsex]|s)")
TAG_PATTERN = re.compile(r"<<\d+>>")
NUMERIC_PATTERN = re.compile(r"^[\d.,/%°]+\s*\w{0,5}$")


class Category(Enum):
    SKIP = auto()
    TRANSLATE = auto()


def classify(key: str, value: str) -> Category:
    """Classify a loc entry into skip or translate."""
    if _should_skip(key, value):
        return Category.SKIP
    return Category.TRANSLATE


def estimate_tokens(text: str) -> int:
    """Rough token estimate: ~1 token per 4 chars for English."""
    return max(1, len(text) // 4)


def _should_skip(key: str, value: str) -> bool:
    if not value or value.isspace():
        return True
    if key.endswith("^N"):
        return True
    stripped = value.strip()
    if NUMERIC_PATTERN.match(stripped):
        return True
    # Pure format string with no natural language
    no_formats = FORMAT_PATTERN.sub("", stripped)
    no_tags = TAG_PATTERN.sub("", no_formats)
    if not no_tags.strip():
        return True
    return False
