"""Gemini API translator with async parallel processing."""

from __future__ import annotations

import asyncio
import json
import logging
import re

from google import genai

from collections.abc import Callable

from ksp_translator.classifier import estimate_tokens
from ksp_translator.config import (
    GEMINI_API_KEY,
    GEMINI_MODEL,
    MAX_CONCURRENT_REQUESTS,
    SYSTEM_PROMPT,
)
from ksp_translator.cost import CostTracker

logger = logging.getLogger(__name__)

MAX_RETRIES = 5
BATCH_TOKEN_LIMIT = 2000


def _build_batches(entries: dict[str, str]) -> list[dict[str, str]]:
    """Group entries into batches that fit within the token limit."""
    batches: list[dict[str, str]] = []
    current_batch: dict[str, str] = {}
    current_tokens = 0

    for key, value in entries.items():
        # Estimate tokens for this entry (key + value + JSON overhead)
        entry_tokens = estimate_tokens(key) + estimate_tokens(value) + 10
        if current_batch and current_tokens + entry_tokens > BATCH_TOKEN_LIMIT:
            batches.append(current_batch)
            current_batch = {}
            current_tokens = 0
        current_batch[key] = value
        current_tokens += entry_tokens

    if current_batch:
        batches.append(current_batch)

    return batches


async def translate_batch(
    entries: dict[str, str],
    *,
    api_key: str = "",
    pseudo: bool = False,
    on_progress: Callable[[int], None] | None = None,
    cost_tracker: CostTracker | None = None,
) -> dict[str, str]:
    """Translate a dict of loc entries. Returns key -> translated value."""
    key = api_key or GEMINI_API_KEY
    if pseudo:
        if on_progress:
            on_progress(len(entries))
        return {k: f"[KO] {v}" for k, v in entries.items()}

    client = genai.Client(api_key=key)
    semaphore = asyncio.Semaphore(MAX_CONCURRENT_REQUESTS)
    tracker = cost_tracker or CostTracker()
    results: dict[str, str] = {}

    batches = _build_batches(entries)

    tasks = []
    for batch in batches:
        if len(batch) == 1:
            k = next(iter(batch))
            tasks.append(_translate_single(client, semaphore, k, batch[k], on_progress, tracker))
        else:
            tasks.append(_translate_batch_chunk(client, semaphore, batch, on_progress, tracker))

    for coro in asyncio.as_completed(tasks):
        try:
            partial = await coro
            results.update(partial)
        except Exception as e:
            logger.error(f"Translation task failed: {e}")

    return results


async def _translate_single(
    client: genai.Client,
    semaphore: asyncio.Semaphore,
    key: str,
    value: str,
    on_progress: Callable[[int], None] | None = None,
    tracker: CostTracker | None = None,
) -> dict[str, str]:
    prompt = (
        f"Translate the following KSP mod text from English to Korean.\n"
        f"Return ONLY the translated text, nothing else.\n\n"
        f"{value}"
    )
    async with semaphore:
        response = await _call_with_retry(client, prompt, tracker)
    if on_progress:
        on_progress(1)
    return {key: response.strip()}


async def _translate_batch_chunk(
    client: genai.Client,
    semaphore: asyncio.Semaphore,
    batch: dict[str, str],
    on_progress: Callable[[int], None] | None = None,
    tracker: CostTracker | None = None,
) -> dict[str, str]:
    items = [{"key": k, "value": v} for k, v in batch.items()]
    prompt = (
        f"Translate the following KSP mod texts from English to Korean.\n"
        f"Input is a JSON array. Return a JSON array with the same structure, "
        f"where each 'value' is replaced with the Korean translation.\n"
        f"Return ONLY the JSON array, no markdown fences or extra text.\n\n"
        f"{json.dumps(items, ensure_ascii=False)}"
    )
    async with semaphore:
        response = await _call_with_retry(client, prompt, tracker)

    try:
        cleaned = _strip_markdown_fences(response)
        translated = json.loads(cleaned)
        if on_progress:
            on_progress(len(batch))
        return {item["key"]: item["value"] for item in translated}
    except (json.JSONDecodeError, KeyError, TypeError):
        logger.warning("Batch response parse failed, falling back to individual translation")
        results: dict[str, str] = {}
        for k, v in batch.items():
            r = await _translate_single(client, semaphore, k, v, on_progress, tracker)
            results.update(r)
        return results


async def _call_with_retry(
    client: genai.Client,
    prompt: str,
    tracker: CostTracker | None = None,
) -> str:
    for attempt in range(MAX_RETRIES):
        try:
            response = await client.aio.models.generate_content(
                model=GEMINI_MODEL,
                contents=prompt,
                config=genai.types.GenerateContentConfig(
                    system_instruction=SYSTEM_PROMPT,
                    temperature=0.3,
                ),
            )
            if tracker and response.usage_metadata:
                tracker.add(
                    response.usage_metadata.prompt_token_count or 0,
                    response.usage_metadata.candidates_token_count or 0,
                )
            return response.text or ""
        except Exception as e:
            if "429" in str(e) or "RESOURCE_EXHAUSTED" in str(e):
                wait = 2 ** (attempt + 1)
                logger.warning(f"Rate limited, retrying in {wait}s (attempt {attempt + 1})")
                await asyncio.sleep(wait)
            else:
                if attempt < MAX_RETRIES - 1:
                    logger.warning(f"API error: {e}, retrying...")
                    await asyncio.sleep(1)
                else:
                    raise
    raise RuntimeError("Max retries exceeded")


def _strip_markdown_fences(text: str) -> str:
    text = text.strip()
    text = re.sub(r"^```(?:json)?\s*\n?", "", text)
    text = re.sub(r"\n?```\s*$", "", text)
    return text.strip()
