import json
import logging
import time
import uuid
from abc import ABC, abstractmethod
from pathlib import Path
from typing import Any, Optional

import ollama
import yaml
from pydantic import BaseModel, Field

from memory.memory_manager import MemoryContext, MemoryManager
from skills.skill_registry import load_skill
from .shared_utils import (
    retry, estimate_tokens, truncate_to_tokens, MetricsCollector,
    ConversationManager, build_tool_schema, format_json_safe, ValidationError,
)

logger = logging.getLogger(__name__)

PERSONALITY_DIR = Path(__file__).parent.parent / 'personality'
CONFIG_DIR = Path(__file__).parent.parent / 'config'


class AgentResponse(BaseModel):
    """Structured agent response with full telemetry."""
    content: str
    tool_calls: list[dict] = Field(default_factory=list)
    tool_results: list[dict] = Field(default_factory=list)
    model: str = ''
    tokens_used: int = 0
    latency_ms: float = 0.0
    metadata: dict = Field(default_factory=dict)
    error: Optional[str] = None
    retries: int = 0


class ToolCall(BaseModel):
    tool: str
    args: dict = Field(default_factory=dict)


def _load_personality_file(name: str) -> str:
    path = PERSONALITY_DIR / f'{name}.md'
    if path.exists():
        return path.read_text(encoding='utf-8')
    logger.warning('Personality file not found: %s', path)
    return ''


def _load_models_config() -> dict:
    path = CONFIG_DIR / 'models.yaml'
    if path.exists():
        with open(path, encoding='utf-8') as f:
            return yaml.safe_load(f) or {}
    return {}


class BaseAgent(ABC):
    """Production-grade base agent with resilience, observability, and safety."""

    def __init__(
        self,
        memory: Optional[MemoryManager] = None,
        tools: Optional[dict[str, callable]] = None,
        model_override: Optional[str] = None,
        temperature_override: Optional[float] = None,
        max_tool_iterations: int = 5,
        enable_metrics: bool = True,
    ):
        self._memory = memory
        self._tools = tools or {}
        self._models_config = _load_models_config()
        self._max_tool_iterations = max_tool_iterations
        self._metrics = MetricsCollector() if enable_metrics else None

        agent_config = self._models_config.get(self.agent_type, {})
        self._model = model_override or agent_config.get('model', 'gemma3:12b')
        self._temperature = temperature_override if temperature_override is not None else agent_config.get('temperature', 0.3)
        self._context_length = agent_config.get('context_length', 8192)

        # Reserve tokens for system prompt and response
        self._conversation_manager = ConversationManager(
            max_tokens=int(self._context_length * 0.75),
            reserve_tokens=int(self._context_length * 0.25),
        )

        self._personality = self._build_personality()
        self._skill_content = load_skill(self.skill_name)

        # Session tracking
        self._session_id = ''
        self._run_id = ''

    @property
    @abstractmethod
    def agent_type(self) -> str:
        """Unique agent identifier used for config lookup."""
        ...

    @property
    @abstractmethod
    def skill_name(self) -> str:
        """Skill file name to load from skill registry."""
        ...

    @property
    @abstractmethod
    def memory_layers(self) -> list[str]:
        """Memory layers this agent queries."""
        ...

    @property
    def is_conductor(self) -> bool:
        return False

    def _build_personality(self) -> str:
        parts = [
            _load_personality_file('soul'),
            _load_personality_file('voice'),
            _load_personality_file('boundaries'),
        ]
        if self.is_conductor:
            parts.append(_load_personality_file('relationship'))
            parts.append(_load_personality_file('quirks'))
        return '\n\n---\n\n'.join(p for p in parts if p)

    def _build_system_prompt(self, memory_context: str = '', extra_context: str = '') -> str:
        """Compose system prompt from personality, skills, memory, and extra context."""
        sections = []
        if self._personality:
            sections.append(self._personality)
        if self._skill_content:
            sections.append(f"--- SKILL INSTRUCTIONS ---\n{self._skill_content}")
        if memory_context:
            sections.append(f"--- MEMORY CONTEXT ---\n{memory_context}")
        if extra_context:
            sections.append(f"--- ADDITIONAL CONTEXT ---\n{extra_context}")
        return '\n\n'.join(sections)

    def _retrieve_memory(self, query: str, top_k: int = 5) -> str:
        if not self._memory:
            return ''
        try:
            context = self._memory.retrieve(
                query=query,
                layers=self.memory_layers,
                top_k=top_k,
                session_filter=self._session_id,
            )
            formatted = self._memory.format_context_for_prompt(context)
            # Truncate if too large
            return truncate_to_tokens(formatted, 1500)
        except Exception as e:
            logger.error('Memory retrieval failed for %s: %s', self.agent_type, e)
            return ''

    @retry(max_attempts=3, backoff_seconds=1.0, exceptions=(Exception,))
    def _call_ollama(
        self,
        messages: list[dict],
        tools_schema: Optional[list[dict]] = None,
        stream: bool = False,
    ) -> dict:
        """Call Ollama with retries, proper error handling, and telemetry."""
        start = time.monotonic()

        kwargs = {
            'model': self._model,
            'messages': messages,
            'options': {
                'temperature': self._temperature,
                'num_ctx': self._context_length,
            },
        }
        if tools_schema:
            kwargs['tools'] = tools_schema
        if stream:
            kwargs['stream'] = True

        try:
            response = ollama.chat(**kwargs)
        except Exception as e:
            logger.error('Ollama call failed for model %s: %s', self._model, e)
            raise RetryableError(f"Ollama call failed: {e}") from e

        elapsed = (time.monotonic() - start) * 1000

        # Normalize response format
        if hasattr(response, 'message'):
            msg = response.message
            content = msg.content if hasattr(msg, 'content') else msg.get('content', '')
            tool_calls = msg.tool_calls if hasattr(msg, 'tool_calls') else msg.get('tool_calls', [])
            model = response.model if hasattr(response, 'model') else self._model
            tokens = response.eval_count if hasattr(response, 'eval_count') else 0
            prompt_tokens = response.prompt_eval_count if hasattr(response, 'prompt_eval_count') else 0
        else:
            msg = response.get('message', {})
            content = msg.get('content', '')
            tool_calls = msg.get('tool_calls', [])
            model = response.get('model', self._model)
            tokens = response.get('eval_count', 0)
            prompt_tokens = response.get('prompt_eval_count', 0)

        return {
            'content': content or '',
            'tool_calls': tool_calls or [],
            'model': model,
            'tokens_used': tokens,
            'prompt_tokens': prompt_tokens,
            'latency_ms': elapsed,
        }

    def _get_tools_schema(self) -> list[dict]:
        """Build validated tool schemas from registered tools."""
        return [build_tool_schema(fn) for fn in self._tools.values()]

    def _validate_tool_args(self, tool_name: str, args: dict) -> tuple[bool, str]:
        """Validate tool arguments against schema."""
        if tool_name not in self._tools:
            return False, f"Unknown tool: {tool_name}"
        schema = getattr(self._tools[tool_name], '_tool_schema', {})
        params = schema.get('parameters', {})
        required = params.get('required', [])
        for key in required:
            if key not in args:
                return False, f"Missing required argument '{key}' for tool '{tool_name}'"
        return True, ""

    def _execute_tool(self, tool_name: str, args: dict) -> dict:
        """Execute a tool with validation and timeout protection."""
        valid, error = self._validate_tool_args(tool_name, args)
        if not valid:
            return {'error': error, 'validation_failed': True}

        fn = self._tools[tool_name]
        try:
            logger.info('[%s] Executing tool: %s(%s)', self.agent_type, tool_name, 
                       json.dumps(args, default=str)[:200])
            result = fn(**args)
            # Normalize string results to dict
            if isinstance(result, str):
                try:
                    parsed = json.loads(result)
                    return {'result': parsed}
                except json.JSONDecodeError:
                    return {'result': result}
            return {'result': result}
        except ValidationError as e:
            logger.warning('Tool %s validation error: %s', tool_name, e)
            return {'error': str(e), 'validation_failed': True}
        except Exception as e:
            logger.error('Tool %s failed: %s\n%s', tool_name, e, traceback.format_exc())
            return {'error': str(e), 'traceback': traceback.format_exc()}

    def _run_tool_loop(
        self,
        messages: list[dict],
        tools_schema: list[dict],
        max_iterations: Optional[int] = None,
    ) -> tuple[str, list[dict], list[dict], int]:
        """
        Run the agent with tool-use loop.
        Returns: (content, tool_calls, tool_results, iterations_used)
        """
        max_iter = max_iterations or self._max_tool_iterations
        all_tool_calls = []
        all_tool_results = []
        iterations_used = 0

        for iteration in range(max_iter):
            iterations_used = iteration + 1
            response = self._call_ollama(messages, tools_schema)
            tool_calls = response.get('tool_calls', [])

            if not tool_calls:
                return response.get('content', ''), all_tool_calls, all_tool_results, iterations_used

            for tc in tool_calls:
                tool_name, tool_args = self._parse_tool_call(tc)
                if not tool_name:
                    continue

                logger.info(
                    '[%s] Tool call #%d: %s(%s)',
                    self.agent_type, len(all_tool_calls) + 1, tool_name, 
                    format_json_safe(tool_args)[:200],
                )

                all_tool_calls.append({'tool': tool_name, 'args': tool_args})
                result = self._execute_tool(tool_name, tool_args)
                all_tool_results.append({'tool': tool_name, 'result': result})

                # Append tool call and result to messages
                messages.append({
                    'role': 'assistant',
                    'content': '',
                    'tool_calls': [{'function': {'name': tool_name, 'arguments': tool_args}}],
                })
                messages.append({
                    'role': 'tool',
                    'content': format_json_safe(result),
                })

        # Max iterations reached - get final response without tools
        logger.warning('[%s] Max tool iterations (%d) reached. Getting final response.', 
                      self.agent_type, max_iter)
        final = self._call_ollama(messages, tools_schema=None)
        return final.get('content', ''), all_tool_calls, all_tool_results, iterations_used

    def _parse_tool_call(self, tc: Any) -> tuple[str, dict]:
        """Parse tool call from Ollama response format."""
        if hasattr(tc, 'function'):
            return tc.function.name, tc.function.arguments
        fn_data = tc.get('function', {}) if isinstance(tc, dict) else {}
        return fn_data.get('name', ''), fn_data.get('arguments', {})

    def run(
        self,
        user_message: str,
        conversation_history: Optional[list[dict]] = None,
        extra_context: str = '',
        session_id: str = '',
        max_tool_iterations: Optional[int] = None,
    ) -> AgentResponse:
        """
        Main entry point. Handles memory retrieval, prompt building, 
        tool loops, and response packaging.
        """
        start = time.monotonic()
        self._session_id = session_id or str(uuid.uuid4())
        self._run_id = str(uuid.uuid4())
        retries = 0

        try:
            # Memory retrieval
            memory_context = ''
            if not extra_context:  # Skip if conductor already provided context
                memory_context = self._retrieve_memory(user_message)

            system_prompt = self._build_system_prompt(memory_context, extra_context)

            # Token budget check for system prompt
            sys_tokens = estimate_tokens(system_prompt)
            if sys_tokens > self._context_length // 2:
                logger.warning('System prompt is %d tokens (limit %d). Truncating.', 
                             sys_tokens, self._context_length // 2)
                system_prompt = truncate_to_tokens(system_prompt, self._context_length // 2)

            # Build messages
            messages = [{'role': 'system', 'content': system_prompt}]

            if conversation_history:
                # Validate and sanitize history
                for msg in conversation_history:
                    if isinstance(msg, dict) and 'role' in msg and 'content' in msg:
                        messages.append({'role': msg['role'], 'content': str(msg['content'])})

            messages.append({'role': 'user', 'content': user_message})

            tools_schema = self._get_tools_schema() if self._tools else None

            if tools_schema:
                content, tool_calls, tool_results, iterations = self._run_tool_loop(
                    messages, tools_schema, max_iterations=max_tool_iterations
                )
            else:
                response = self._call_ollama(messages)
                content = response.get('content', '')
                tool_calls = []
                tool_results = []
                iterations = 0

            elapsed = (time.monotonic() - start) * 1000

            # Record metrics
            if self._metrics:
                self._metrics.record(
                    latency_ms=elapsed,
                    tokens=estimate_tokens(content),
                    tool_calls=len(tool_calls),
                )

            return AgentResponse(
                content=content,
                tool_calls=tool_calls,
                tool_results=tool_results,
                model=self._model,
                tokens_used=estimate_tokens(content),
                latency_ms=round(elapsed, 2),
                metadata={
                    'agent_type': self.agent_type,
                    'skill': self.skill_name,
                    'memory_layers': self.memory_layers,
                    'run_id': self._run_id,
                    'session_id': self._session_id,
                    'tool_iterations': iterations,
                    'metrics': self._metrics.to_dict() if self._metrics else None,
                },
            )

        except RetryableError as e:
            retries = 3  # Max retries from decorator
            elapsed = (time.monotonic() - start) * 1000
            logger.error('[%s] Failed after retries: %s', self.agent_type, e)
            if self._metrics:
                self._metrics.record(latency_ms=elapsed, error=True)
            return AgentResponse(
                content=f"I encountered a persistent error: {e}. Please try again in a moment.",
                model=self._model,
                latency_ms=round(elapsed, 2),
                error=str(e),
                retries=retries,
                metadata={'agent_type': self.agent_type, 'run_id': self._run_id},
            )
        except Exception as e:
            elapsed = (time.monotonic() - start) * 1000
            logger.error('[%s] Unexpected error: %s\n%s', self.agent_type, e, traceback.format_exc())
            if self._metrics:
                self._metrics.record(latency_ms=elapsed, error=True)
            return AgentResponse(
                content=f"An unexpected error occurred: {e}",
                model=self._model,
                latency_ms=round(elapsed, 2),
                error=str(e),
                metadata={'agent_type': self.agent_type, 'run_id': self._run_id},
            )

    def get_metrics(self) -> dict:
        """Return current metrics snapshot."""
        return self._metrics.to_dict() if self._metrics else {}

    @abstractmethod
    def get_available_tools(self) -> list[str]:
        """Return list of available tool names."""
        ...