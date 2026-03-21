"""API cost tracking."""

from __future__ import annotations

import threading


# Gemini Flash Lite pricing (per 1M tokens, USD)
INPUT_PRICE_PER_M = 0.25
OUTPUT_PRICE_PER_M = 1.50


class CostTracker:
    """Thread-safe accumulator for API token usage and cost."""

    def __init__(self) -> None:
        self._lock = threading.Lock()
        self.input_tokens = 0
        self.output_tokens = 0

    def add(self, input_tokens: int, output_tokens: int) -> None:
        with self._lock:
            self.input_tokens += input_tokens
            self.output_tokens += output_tokens

    @property
    def input_cost(self) -> float:
        return self.input_tokens / 1_000_000 * INPUT_PRICE_PER_M

    @property
    def output_cost(self) -> float:
        return self.output_tokens / 1_000_000 * OUTPUT_PRICE_PER_M

    @property
    def total_cost(self) -> float:
        return self.input_cost + self.output_cost

    def format_cost(self) -> str:
        return f"${self.total_cost:.4f}"

    def format_detail(self) -> str:
        return (
            f"Input: {self.input_tokens:,} tokens (${self.input_cost:.4f}) | "
            f"Output: {self.output_tokens:,} tokens (${self.output_cost:.4f}) | "
            f"Total: {self.format_cost()}"
        )
