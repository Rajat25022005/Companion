"""Shared utilities and mixins for all agents."""
import json
import logging
import os
import re
import time
import traceback
from functools import wraps
from pathlib import Path
from typing import Any, Callable, Optional

logger = logging.getLogger(__name__)


class RetryableError(Exception):
    """Errors that should trigger a retry."""
    pass


class ValidationError(Exception):
    """Input validation errors that should not retry."""
    pass


def retry(max_attempts: int = 3, backoff_seconds: float = 1.0, 
          exceptions: tuple = (Exception,)):
    """Exponential backoff retry decorator."""
    def decorator(fn: Callable) -> Callable:
        @wraps(fn)
        def wrapper(*args, **kwargs):
            last_exception = None
            for attempt in range(1, max_attempts + 1):
                try:
                    return fn(*args, **kwargs)
                except exceptions as e:
                    last_exception = e
                    if attempt == max_attempts:
                        break
                    sleep_time = backoff_seconds * (2 ** (attempt - 1))
                    logger.warning(
                        '[retry] %s failed (attempt %d/%d): %s. Retrying in %.1fs...',
                        fn.__name__, attempt, max_attempts, e, sleep_time
                    )
                    time.sleep(sleep_time)
            raise last_exception
        return wrapper
    return decorator


def estimate_tokens(text: str) -> int:
    """Rough token estimation: ~4 chars per token for English text."""
    if not text:
        return 0
    return len(text) // 4


def truncate_to_tokens(text: str, max_tokens: int, suffix: str = "...") -> str:
    """Truncate text to approximate token limit."""
    estimated = estimate_tokens(text)
    if estimated <= max_tokens:
        return text
    char_limit = max_tokens * 4
    if len(text) <= char_limit:
        return text
    return text[:char_limit - len(suffix)] + suffix


def sanitize_filename(name: str) -> str:
    """Sanitize a string for use as a filename."""
    name = re.sub(r'[^\w\s-]', '_', name)
    name = re.sub(r'\s+', '_', name)
    return name.strip('_')[:64]


def validate_path(path: str, allowed_base: Path, must_exist: bool = False) -> Path:
    """Validate that a path is within allowed_base. Prevents directory traversal."""
    try:
        target = Path(path).resolve()
        allowed = allowed_base.resolve()
        if not str(target).startswith(str(allowed)):
            raise ValidationError(f"Path {path} is outside allowed directory {allowed_base}")
        if must_exist and not target.exists():
            raise ValidationError(f"Path does not exist: {path}")
        return target
    except ValidationError:
        raise
    except Exception as e:
        raise ValidationError(f"Invalid path {path}: {e}")


def format_json_safe(obj: Any, max_length: int = 4000) -> str:
    """JSON serialize with truncation and fallback."""
    try:
        text = json.dumps(obj, default=str, ensure_ascii=False)
        if len(text) > max_length:
            text = text[:max_length] + "... [truncated]"
        return text
    except Exception as e:
        return json.dumps({"error": f"Serialization failed: {e}"})


class MetricsCollector:
    """Simple metrics collection for agent operations."""

    def __init__(self):
        self.calls = 0
        self.errors = 0
        self.total_latency_ms = 0.0
        self.total_tokens = 0
        self.tool_calls = 0

    def record(self, latency_ms: float, tokens: int = 0, error: bool = False, 
               tool_calls: int = 0):
        self.calls += 1
        self.total_latency_ms += latency_ms
        self.total_tokens += tokens
        self.tool_calls += tool_calls
        if error:
            self.errors += 1

    @property
    def avg_latency_ms(self) -> float:
        return self.total_latency_ms / max(self.calls, 1)

    def to_dict(self) -> dict:
        return {
            'calls': self.calls,
            'errors': self.errors,
            'error_rate': self.errors / max(self.calls, 1),
            'avg_latency_ms': round(self.avg_latency_ms, 2),
            'total_tokens': self.total_tokens,
            'tool_calls': self.tool_calls,
        }


class ConversationManager:
    """Manages conversation history with token budgeting."""

    def __init__(self, max_tokens: int = 6000, reserve_tokens: int = 2000):
        self.max_tokens = max_tokens
        self.reserve_tokens = reserve_tokens
        self.history: list[dict] = []

    def add_message(self, role: str, content: str, **metadata):
        msg = {"role": role, "content": content, **metadata}
        self.history.append(msg)
        self._enforce_limit()

    def _enforce_limit(self):
        """Trim oldest non-system messages when over token budget."""
        while self.history and self._estimate_total() > self.max_tokens - self.reserve_tokens:
            # Keep system prompt, remove oldest user/assistant
            for i, msg in enumerate(self.history):
                if msg["role"] != "system":
                    self.history.pop(i)
                    break
            else:
                # Only system messages left, truncate the last one
                if self.history:
                    sys_msg = self.history[0]
                    sys_msg["content"] = truncate_to_tokens(
                        sys_msg["content"], 
                        self.max_tokens - self.reserve_tokens
                    )
                    break

    def _estimate_total(self) -> int:
        return sum(estimate_tokens(m.get("content", "")) for m in self.history)

    def to_messages(self) -> list[dict]:
        return [{"role": m["role"], "content": m["content"]} for m in self.history]

    def clear(self):
        self.history = []


def build_tool_schema(fn: Callable) -> dict:
    """Build OpenAI-compatible tool schema from function."""
    doc = fn.__doc__ or ""
    description = doc.strip().split("\n")[0] if doc else fn.__name__

    schema = getattr(fn, "_tool_schema", {})
    parameters = schema.get("parameters", {"type": "object", "properties": {}})

    return {
        "type": "function",
        "function": {
            "name": fn.__name__,
            "description": description,
            "parameters": parameters,
        },
    }