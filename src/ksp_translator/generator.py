"""Translated cfg file generator.

Produces a modified copy of the original cfg with translated values,
preserving the file structure (comments, whitespace, non-LOC lines).
"""

from __future__ import annotations

import codecs
import re
from pathlib import Path

LOC_PATTERN = re.compile(r"^(\s*)(#\S+)(\s*=\s*)(.*)$")


def generate_translated_cfg(
    source_file: Path,
    translations: dict[str, str],
    output_path: Path,
) -> Path:
    """Generate a translated copy of a cfg file.

    Replaces LOC values in the en-us block with translations,
    keeping everything else identical.
    """
    output_path.parent.mkdir(parents=True, exist_ok=True)
    text = _read_cfg(source_file)

    out_lines: list[str] = []
    in_en_us = False
    brace_depth = 0

    for line in text.splitlines():
        stripped = line.strip()

        if stripped.startswith("//") or not stripped:
            out_lines.append(line)
            continue

        if "en-us" in stripped and "{" not in stripped:
            in_en_us = True
            out_lines.append(line)
            continue

        if in_en_us and "{" in stripped and brace_depth == 0:
            brace_depth = 1
            out_lines.append(line)
            continue

        if in_en_us and brace_depth > 0:
            if "{" in stripped:
                brace_depth += 1
            if "}" in stripped:
                brace_depth -= 1
                if brace_depth == 0:
                    in_en_us = False
                    out_lines.append(line)
                    continue

            m = LOC_PATTERN.match(line)
            if m:
                indent, key, sep, _old_value = m.groups()
                if key in translations:
                    out_lines.append(f"{indent}{key}{sep}{translations[key]}")
                    continue

        out_lines.append(line)

    output_path.write_text("\n".join(out_lines), encoding="utf-8")
    return output_path


def _read_cfg(path: Path) -> str:
    raw = path.read_bytes()
    if raw.startswith(codecs.BOM_UTF8):
        raw = raw[len(codecs.BOM_UTF8):]
    return raw.decode("utf-8", errors="replace")
