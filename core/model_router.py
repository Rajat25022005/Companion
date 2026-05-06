import logging
from typing import Optional

import yaml
from pathlib import Path

logger = logging.getLogger(__name__)

CONFIG_DIR = Path(__file__).parent.parent / 'config'


AGENT_MODEL_MAP = {
    'research': 'research',
    'code': 'code',
    'write': 'writer',
    'document': 'conductor',
    'email': 'conductor',
    'plan': 'conductor',
}


class ModelConfig:
    def __init__(self):
        self._config = {}
        config_path = CONFIG_DIR / 'models.yaml'
        if config_path.exists():
            with open(config_path, encoding='utf-8') as f:
                self._config = yaml.safe_load(f) or {}

    def get_model_for_agent(self, agent_type: str) -> dict:
        config_key = AGENT_MODEL_MAP.get(agent_type, agent_type)
        agent_config = self._config.get(config_key, {})

        return {
            'model': agent_config.get('model', 'gemma3:12b'),
            'temperature': agent_config.get('temperature', 0.3),
            'context_length': agent_config.get('context_length', 8192),
        }

    def get_embedding_config(self) -> dict:
        embed_config = self._config.get('embeddings', {})
        return {
            'model': embed_config.get('model', 'nomic-embed-text'),
            'dimensions': embed_config.get('dimensions', 768),
        }

    def list_models(self) -> dict[str, str]:
        return {key: val.get('model', 'unknown') for key, val in self._config.items()}


class ModelRouter:
    def __init__(self, config: Optional[ModelConfig] = None):
        self._config = config or ModelConfig()

    def route(self, intent: str) -> dict:
        return self._config.get_model_for_agent(intent)

    def get_model_name(self, intent: str) -> str:
        return self.route(intent)['model']

    def get_temperature(self, intent: str) -> float:
        return self.route(intent)['temperature']

    def get_context_length(self, intent: str) -> int:
        return self.route(intent)['context_length']

    def get_embedding_model(self) -> str:
        return self._config.get_embedding_config()['model']
