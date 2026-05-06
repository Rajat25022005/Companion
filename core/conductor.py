import json
import logging
import time
import uuid
from typing import Optional

import ollama
import yaml
from pathlib import Path
from pydantic import BaseModel, Field

from agents.base_agent import AgentResponse, _load_personality_file
from core.intent_parser import Intent, IntentParser
from core.model_router import ModelRouter
from core.task_planner import PlannerResult, TaskPlanner
from memory.memory_manager import MemoryManager

logger = logging.getLogger(__name__)

CONFIG_DIR = Path(__file__).parent.parent / 'config'


class ConductorResponse(BaseModel):
    content: str
    intent: Optional[Intent] = None
    planner_result: Optional[PlannerResult] = None
    session_id: str = ''
    turn_index: int = 0
    latency_ms: float = 0.0
    model: str = ''


class Conductor:
    def __init__(
        self,
        memory: Optional[MemoryManager] = None,
        model_override: Optional[str] = None,
    ):
        config = {}
        config_path = CONFIG_DIR / 'models.yaml'
        if config_path.exists():
            with open(config_path, encoding='utf-8') as f:
                config = yaml.safe_load(f) or {}

        conductor_config = config.get('conductor', {})
        self._model = model_override or conductor_config.get('model', 'gemma3:12b')
        self._temperature = conductor_config.get('temperature', 0.3)
        self._context_length = conductor_config.get('context_length', 8192)

        self._memory = memory
        self._intent_parser = IntentParser(model=self._model)
        self._model_router = ModelRouter()
        self._task_planner = TaskPlanner(memory=memory)

        self._session_id = str(uuid.uuid4())[:8]
        self._turn_index = 0
        self._conversation_history: list[dict] = []

        self._personality = self._build_personality()

    def _build_personality(self) -> str:
        parts = [
            _load_personality_file('soul'),
            _load_personality_file('voice'),
            _load_personality_file('boundaries'),
            _load_personality_file('relationship'),
            _load_personality_file('quirks'),
        ]
        return '\n\n---\n\n'.join(p for p in parts if p)

    def _retrieve_memory_context(self, query: str) -> str:
        if not self._memory:
            return ''
        try:
            context = self._memory.retrieve(query=query, layers=['episodic', 'relational'], top_k=3)
            return self._memory.format_context_for_prompt(context)
        except Exception as e:
            logger.error('Conductor memory retrieval failed: %s', e)
            return ''

    def _synthesize_response(self, user_message: str, planner_result: PlannerResult) -> str:
        if not planner_result.agent_responses:
            return planner_result.final_output

        if len(planner_result.agent_responses) == 1:
            return planner_result.final_output

        agent_summaries = []
        for resp in planner_result.agent_responses:
            agent_summaries.append(
                f'[Step {resp["step"]} - {resp["agent"]}]\n{resp["content"]}'
            )

        synthesis_prompt = (
            'You are synthesizing results from multiple specialist agents into a single coherent response.\n\n'
            f'The user asked: {user_message}\n\n'
            'Agent results:\n\n' + '\n\n---\n\n'.join(agent_summaries) + '\n\n'
            'Combine these into one clear, unified response. Do not mention the agents or steps — '
            'the user should experience this as a single answer. Keep the structure and detail from '
            'the specialists, but make it flow as one response.'
        )

        try:
            response = ollama.chat(
                model=self._model,
                messages=[
                    {'role': 'system', 'content': self._personality},
                    {'role': 'user', 'content': synthesis_prompt},
                ],
                options={'temperature': self._temperature, 'num_ctx': self._context_length},
            )
            msg = response.message if hasattr(response, 'message') else response.get('message', {})
            return (msg.content if hasattr(msg, 'content') else msg.get('content', '')) or planner_result.final_output
        except Exception as e:
            logger.error('Synthesis failed, returning raw output: %s', e)
            return planner_result.final_output

    def _store_turn(self, user_message: str, response: str) -> None:
        if not self._memory:
            return
        try:
            self._memory.store(
                turn={
                    'role': 'user',
                    'content': user_message,
                    'response': response,
                },
                session_id=self._session_id,
                turn_index=self._turn_index,
            )
        except Exception as e:
            logger.error('Failed to store turn in memory: %s', e)

    def _is_simple_greeting(self, message: str) -> bool:
        greetings = {'hi', 'hello', 'hey', 'sup', 'yo', 'good morning', 'good evening', 'good afternoon'}
        return message.lower().strip().rstrip('!.') in greetings

    def chat(self, user_message: str) -> ConductorResponse:
        start = time.monotonic()
        self._turn_index += 1

        if self._is_simple_greeting(user_message):
            memory_context = self._retrieve_memory_context(user_message)

            messages = [{'role': 'system', 'content': self._personality}]
            if memory_context:
                messages[0]['content'] += f'\n\n{memory_context}'
            messages.extend(self._conversation_history[-6:])
            messages.append({'role': 'user', 'content': user_message})

            try:
                response = ollama.chat(
                    model=self._model,
                    messages=messages,
                    options={'temperature': self._temperature, 'num_ctx': self._context_length},
                )
                msg = response.message if hasattr(response, 'message') else response.get('message', {})
                content = msg.content if hasattr(msg, 'content') else msg.get('content', '')
            except Exception as e:
                content = 'Something went wrong on my end. Try again.'
                logger.error('Greeting response failed: %s', e)

            self._conversation_history.append({'role': 'user', 'content': user_message})
            self._conversation_history.append({'role': 'assistant', 'content': content})
            self._store_turn(user_message, content)

            return ConductorResponse(
                content=content,
                session_id=self._session_id,
                turn_index=self._turn_index,
                latency_ms=(time.monotonic() - start) * 1000,
                model=self._model,
            )

        intent = self._intent_parser.parse(user_message)
        logger.info(
            'Intent: primary=%s, multi_agent=%s, chain=%d steps',
            intent.primary, intent.requires_multi_agent, len(intent.task_chain),
        )

        if intent.requires_multi_agent:
            planner_result = self._task_planner.execute(
                intent=intent,
                user_message=user_message,
                conversation_history=self._conversation_history[-6:],
            )
            content = self._synthesize_response(user_message, planner_result)
        else:
            planner_result = self._task_planner.execute(
                intent=intent,
                user_message=user_message,
                conversation_history=self._conversation_history[-6:],
            )
            content = planner_result.final_output

        self._conversation_history.append({'role': 'user', 'content': user_message})
        self._conversation_history.append({'role': 'assistant', 'content': content})
        self._store_turn(user_message, content)

        elapsed = (time.monotonic() - start) * 1000

        return ConductorResponse(
            content=content,
            intent=intent,
            planner_result=planner_result,
            session_id=self._session_id,
            turn_index=self._turn_index,
            latency_ms=elapsed,
            model=self._model,
        )

    def reset_session(self) -> str:
        self._session_id = str(uuid.uuid4())[:8]
        self._turn_index = 0
        self._conversation_history = []
        logger.info('Session reset. New session: %s', self._session_id)
        return self._session_id

    @property
    def session_id(self) -> str:
        return self._session_id

    @property
    def turn_count(self) -> int:
        return self._turn_index
