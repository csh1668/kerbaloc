"""Configuration and environment variable handling."""

import os
from pathlib import Path

from dotenv import load_dotenv

load_dotenv()

GEMINI_API_KEY = os.environ.get("GEMINI_API_KEY", "")
KSP_GAMEDATA = os.environ.get("KSP_GAMEDATA", r"C:\Program Files (x86)\Steam\steamapps\common\Kerbal Space Program\GameData")

MAX_CONCURRENT_REQUESTS = 10

EXCLUDED_DIRS = {"PluginData", "cache", "Cache", "CommunityResourcePack", "Squad", "SquadExpansion"}

SYSTEM_PROMPT = """\
You are a professional translator for Kerbal Space Program mod localization.
Translate English text to Korean following these rules:

1. Preserve all formatting tags: <<1>>, <<2>>, <b>, </b>, <i>, </i>, <color=#hex>, </color>, \n
2. Preserve placeholder tokens like ^N (%.0f, %s, etc.) exactly as-is.
3. Use natural Korean that fits the KSP space/engineering context.
4. Keep proper nouns (part names, mod names, planet names) in English.
5. Use 존댓말 (formal polite speech) for UI text.
6. Common KSP term translations:
   - thrust = 추력, delta-v = 델타-V, orbit = 궤도, apoapsis = 원점, periapsis = 근점
   - burn = 연소, stage = 단계, fuel = 연료, oxidizer = 산화제
   - maneuver node = 기동 노드, trajectory = 궤적, inclination = 궤도경사각
"""

GEMINI_MODEL = "gemini-3.1-flash-lite-preview"
