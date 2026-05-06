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

logger = logging.getLogger(__name__)

PERSONALITY_DIR = Path(__file__).parent.parent / 'personality'
CONFIG_DIR = Path(__file__).parent.parent / 'config'


class AgentResponse(BaseModel):
    content: str
    tool_calls: list[dict] = Field(default_factory=list)
    tool_results: list[dict] = Field(default_factory=list)
    model: str = ''
    tokens_used: int = 0
    latency_ms: float = 0.0
    metadata: dict = Field(default_factory=dict)


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
    def __init__(
        self,
        memory: Optional[MemoryManager] = None,
        tools: Optional[dict[str, callable]] = None,
        model_override: Optional[str] = None,
        temperature_override: Optional[float] = None,
    ):
        self._memory = memory
        self._tools = tools or {}
        self._models_config = _load_models_config()

        agent_config = self._models_config.get(self.agent_type, {})
        self._model = model_override or agent_config.get('model', 'gemma3:12b')
        self._temperature = temperature_override if temperature_override is not None else agent_config.get('temperature', 0.3)
        self._context_length = agent_config.get('context_length', 8192)

        self._personality = self._build_personality()
        self._skill_content = load_skill(self.skill_name)

    @property
    @abstractmethod
    def agent_type(self) -> str:
        ...

    @property
    @abstractmethod
    def skill_name(self) -> str:
        ...

    @property
    @abstractmethod
    def memory_layers(self) -> list[str]:
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

    def _build_system_prompt(self, memory_context: str = '') -> str:
        sections = [self._personality]

        if self._skill_content:
            sections.append(self._skill_content)

        if memory_context:
            sections.append(memory_context)

        return '\n\n'.join(sections)

    def _retrieve_memory(self, query: str, top_k: int = 5) -> str:
        if not self._memory:
            return ''
        try:
            context = self._memory.retrieve(
                query=query,
                layers=self.memory_layers,
                top_k=top_k,
            )
            return self._memory.format_context_for_prompt(context)
        except Exception as e:
            logger.error('Memory retrieval failed for %s: %s', self.agent_type, e)
            return ''

    def _call_ollama(
        self,
        messages: list[dict],
        tools_schema: Optional[list[dict]] = None,
    ) -> dict:
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

        try:
            response = ollama.chat(**kwargs)
        except Exception as e:
            logger.error('Ollama call failed for model %s: %s', self._model, e)
            raise

        elapsed = (time.monotonic() - start) * 1000

        if hasattr(response, 'message'):
            msg = response.message
            content = msg.content if hasattr(msg, 'content') else msg.get('content', '')
            tool_calls = msg.tool_calls if hasattr(msg, 'tool_calls') else msg.get('tool_calls', [])
            model = response.model if hasattr(response, 'model') else self._model
            tokens = response.eval_count if hasattr(response, 'eval_count') else 0
        else:
            msg = response.get('message', {})
            content = msg.get('content', '')
            tool_calls = msg.get('tool_calls', [])
            model = response.get('model', self._model)
            tokens = response.get('eval_count', 0)

        return {
            'content': content or '',
            'tool_calls': tool_calls or [],
            'model': model,
            'tokens_used': tokens,
            'latency_ms': elapsed,
        }

    def _get_tools_schema(self) -> list[dict]:
        schema = []
        for name, fn in self._tools.items():
            doc = fn.__doc__ or ''
            hints = {}
            if hasattr(fn, '_tool_schema'):
                hints = fn._tool_schema

            schema.append({
                'type': 'function',
                'function': {
                    'name': name,
                    'description': doc.strip().split('\n')[0] if doc else name,
                    'parameters': hints.get('parameters', {'type': 'object', 'properties': {}}),
                },
            })
        return schema

    def _execute_tool(self, tool_name: str, args: dict) -> dict:
        if tool_name not in self._tools:
            return {'error': f'Unknown tool: {tool_name}'}

        fn = self._tools[tool_name]
        try:
            result = fn(**args)
            return {'result': result}
        except Exception as e:
            logger.error('Tool %s failed: %s', tool_name, e)
            return {'error': str(e)}

    def _run_tool_loop(
        self,
        messages: list[dict],
        tools_schema: list[dict],
        max_iterations: int = 5,
    ) -> tuple[str, list[dict], list[dict]]:
        all_tool_calls = []
        all_tool_results = []

        for iteration in range(max_iterations):
            response = self._call_ollama(messages, tools_schema)
            tool_calls = response.get('tool_calls', [])

            if not tool_calls:
                return response.get('content', ''), all_tool_calls, all_tool_results

            for tc in tool_calls:
                if hasattr(tc, 'function'):
                    tool_name = tc.function.name
                    tool_args = tc.function.arguments
                else:
                    fn_data = tc.get('function', {})
                    tool_name = fn_data.get('name', '')
                    tool_args = fn_data.get('arguments', {})

                if not tool_name:
                    continue

                logger.info(
                    '[%s] Tool call #%d: %s(%s)',
                    self.agent_type, len(all_tool_calls) + 1, tool_name, json.dumps(tool_args, default=str)[:200],
                )

                all_tool_calls.append({'tool': tool_name, 'args': tool_args})
                result = self._execute_tool(tool_name, tool_args)
                all_tool_results.append({'tool': tool_name, 'result': result})

                messages.append({
                    'role': 'assistant',
                    'content': '',
                    'tool_calls': [{'function': {'name': tool_name, 'arguments': tool_args}}],
                })
                messages.append({
                    'role': 'tool',
                    'content': json.dumps(result, default=str),
                })

        final = self._call_ollama(messages, tools_schema=None)
        return final.get('content', ''), all_tool_calls, all_tool_results

    def run(
        self,
        user_message: str,
        conversation_history: Optional[list[dict]] = None,
        extra_context: str = '',
    ) -> AgentResponse:
        start = time.monotonic()

        memory_context = self._retrieve_memory(user_message)
        system_prompt = self._build_system_prompt(memory_context)

        if extra_context:
            system_prompt += f'\n\n--- ADDITIONAL CONTEXT ---\n{extra_context}\n--- END ADDITIONAL CONTEXT ---'

        messages = [{'role': 'system', 'content': system_prompt}]

        if conversation_history:
            messages.extend(conversation_history)

        messages.append({'role': 'user', 'content': user_message})

        tools_schema = self._get_tools_schema() if self._tools else None

        if tools_schema:
            content, tool_calls, tool_results = self._run_tool_loop(
                messages, tools_schema,
            )
        else:
            response = self._call_ollama(messages)
            content = response.get('content', '')
            tool_calls = []
            tool_results = []

        elapsed = (time.monotonic() - start) * 1000

        return AgentResponse(
            content=content,
            tool_calls=tool_calls,
            tool_results=tool_results,
            model=self._model,
            latency_ms=elapsed,
            metadata={
                'agent_type': self.agent_type,
                'skill': self.skill_name,
                'memory_layers': self.memory_layers,
            },
        )

    @abstractmethod
    def get_available_tools(self) -> list[str]:
        ...
