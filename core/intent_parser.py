import json
import logging
from typing import Optional

import ollama
import yaml
from pathlib import Path
from pydantic import BaseModel, Field

logger = logging.getLogger(__name__)

CONFIG_DIR = Path(__file__).parent.parent / 'config'

INTENT_TYPES = {
    'research': {
        'keywords': ['research', 'find', 'look up', 'what is', 'explain', 'how does', 'why does', 'compare', 'analyze'],
        'description': 'Deep research, information synthesis, analysis',
    },
    'code': {
        'keywords': ['code', 'write code', 'fix', 'debug', 'implement', 'refactor', 'function', 'class', 'script', 'bug', 'error', 'traceback'],
        'description': 'Code generation, debugging, review, refactoring',
    },
    'write': {
        'keywords': ['write', 'draft', 'compose', 'blog', 'article', 'essay', 'summary', 'rewrite', 'edit', 'proofread'],
        'description': 'Long-form writing, editing, summarization',
    },
    'document': {
        'keywords': ['pdf', 'docx', 'pptx', 'presentation', 'slides', 'report', 'document', 'powerpoint', 'word doc'],
        'description': 'Document generation: PDF, DOCX, PPTX',
    },
    'email': {
        'keywords': ['email', 'mail', 'inbox', 'reply', 'send', 'compose email', 'draft email', 'follow up'],
        'description': 'Email composition, replies, inbox management',
    },
    'plan': {
        'keywords': ['plan', 'schedule', 'roadmap', 'breakdown', 'tasks', 'timeline', 'prioritize', 'organize', 'todo'],
        'description': 'Project planning, task decomposition, scheduling',
    },
}


class Intent(BaseModel):
    primary: str
    secondary: list[str] = Field(default_factory=list)
    confidence: float = 0.0
    reasoning: str = ''
    requires_multi_agent: bool = False
    task_chain: list[dict] = Field(default_factory=list)


class IntentParser:
    def __init__(self, model: Optional[str] = None):
        config = {}
        config_path = CONFIG_DIR / 'models.yaml'
        if config_path.exists():
            with open(config_path, encoding='utf-8') as f:
                config = yaml.safe_load(f) or {}

        conductor_config = config.get('conductor', {})
        self._model = model or conductor_config.get('model', 'gemma3:12b')
        self._temperature = conductor_config.get('temperature', 0.3)

    def _keyword_classify(self, message: str) -> dict[str, float]:
        lower = message.lower()
        scores = {}

        for intent_name, intent_data in INTENT_TYPES.items():
            score = 0.0
            for keyword in intent_data['keywords']:
                if keyword in lower:
                    score += 1.0
                    if lower.startswith(keyword):
                        score += 0.5
            if score > 0:
                scores[intent_name] = score

        return scores

    def _llm_classify(self, message: str) -> Intent:
        system_prompt = (
            'You are an intent classifier. Given a user message, determine what type(s) of work are needed.\n\n'
            'Available intent types:\n'
        )
        for name, data in INTENT_TYPES.items():
            system_prompt += f'- {name}: {data["description"]}\n'

        system_prompt += (
            '\nRespond with ONLY valid JSON, no markdown:\n'
            '{\n'
            '  "primary": "the main intent type",\n'
            '  "secondary": ["other intent types needed, if any"],\n'
            '  "confidence": 0.0 to 1.0,\n'
            '  "reasoning": "brief explanation",\n'
            '  "requires_multi_agent": true/false,\n'
            '  "task_chain": [\n'
            '    {"step": 1, "agent": "intent_type", "task": "what this agent does", "depends_on": []},\n'
            '    {"step": 2, "agent": "intent_type", "task": "what this agent does", "depends_on": [1]}\n'
            '  ]\n'
            '}\n\n'
            'If only one agent is needed, task_chain has one entry and requires_multi_agent is false.\n'
            'If multiple agents are needed in sequence, set requires_multi_agent to true and define the chain.'
        )

        try:
            response = ollama.chat(
                model=self._model,
                messages=[
                    {'role': 'system', 'content': system_prompt},
                    {'role': 'user', 'content': message},
                ],
                options={'temperature': 0.1, 'num_ctx': 2048},
                format='json',
            )

            msg = response.message if hasattr(response, 'message') else response.get('message', {})
            content = (msg.content if hasattr(msg, 'content') else msg.get('content', '')) or '{}'
            data = json.loads(content)

            return Intent(
                primary=data.get('primary', 'research'),
                secondary=data.get('secondary', []),
                confidence=data.get('confidence', 0.5),
                reasoning=data.get('reasoning', ''),
                requires_multi_agent=data.get('requires_multi_agent', False),
                task_chain=data.get('task_chain', []),
            )

        except (json.JSONDecodeError, Exception) as e:
            logger.error('LLM classification failed: %s', e)
            return self._fallback_classify(message)

    def _fallback_classify(self, message: str) -> Intent:
        scores = self._keyword_classify(message)

        if not scores:
            return Intent(
                primary='research',
                confidence=0.3,
                reasoning='No clear intent detected, defaulting to research.',
                task_chain=[{'step': 1, 'agent': 'research', 'task': message, 'depends_on': []}],
            )

        sorted_intents = sorted(scores.items(), key=lambda x: x[1], reverse=True)
        primary = sorted_intents[0][0]
        secondary = [s[0] for s in sorted_intents[1:] if s[1] > 0]

        requires_multi = len(sorted_intents) > 1 and sorted_intents[1][1] > 0.5

        chain = [{'step': 1, 'agent': primary, 'task': message, 'depends_on': []}]
        if requires_multi:
            for i, (intent, _) in enumerate(sorted_intents[1:], start=2):
                chain.append({'step': i, 'agent': intent, 'task': message, 'depends_on': [i - 1]})

        return Intent(
            primary=primary,
            secondary=secondary,
            confidence=min(sorted_intents[0][1] / 3.0, 1.0),
            reasoning=f'Keyword match: {sorted_intents}',
            requires_multi_agent=requires_multi,
            task_chain=chain,
        )

    def parse(self, message: str, use_llm: bool = True) -> Intent:
        keyword_scores = self._keyword_classify(message)

        if keyword_scores:
            top_score = max(keyword_scores.values())
            if top_score >= 2.0 and len([s for s in keyword_scores.values() if s > 0]) == 1:
                intent_name = max(keyword_scores, key=keyword_scores.get)
                return Intent(
                    primary=intent_name,
                    confidence=min(top_score / 3.0, 1.0),
                    reasoning=f'High-confidence keyword match for {intent_name}.',
                    task_chain=[{'step': 1, 'agent': intent_name, 'task': message, 'depends_on': []}],
                )

        if use_llm:
            return self._llm_classify(message)

        return self._fallback_classify(message)
