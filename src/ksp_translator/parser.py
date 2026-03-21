"""KSP cfg localization file parser.

Extracts #LOC_ and #autoLOC_ key-value pairs from cfg files.
"""

from __future__ import annotations

import codecs
import re
from pathlib import Path

LOC_PATTERN = re.compile(r"^\s*(#\S+)\s*=\s*(.*?)\s*$")


def parse_loc_file(path: Path) -> dict[str, str]:
    """Parse a cfg file and return a dict of LOC key -> English value."""
    text = _read_cfg(path)
    entries: dict[str, str] = {}
    in_en_us = False
    brace_depth = 0

    for line in text.splitlines():
        stripped = line.strip()

        if stripped.startswith("//"):
            continue

        if "en-us" in stripped and "{" not in stripped:
            in_en_us = True
            continue

        if in_en_us and "{" in stripped and brace_depth == 0:
            brace_depth = 1
            continue

        if in_en_us and brace_depth > 0:
            if "{" in stripped:
                brace_depth += 1
            if "}" in stripped:
                brace_depth -= 1
                if brace_depth == 0:
                    in_en_us = False
                    continue

            m = LOC_PATTERN.match(stripped)
            if m:
                entries[m.group(1)] = m.group(2)

    return entries


def _read_cfg(path: Path) -> str:
    """Read a cfg file, handling UTF-8 BOM."""
    raw = path.read_bytes()
    if raw.startswith(codecs.BOM_UTF8):
        raw = raw[len(codecs.BOM_UTF8) :]
    return raw.decode("utf-8", errors="replace")
