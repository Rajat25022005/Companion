import logging
from abc import ABC, abstractmethod
from typing import Any, Optional

import yaml
from pathlib import Path

logger = logging.getLogger(__name__)

CONFIG_DIR = Path(__file__).parent.parent / 'config'


class BasePlugin(ABC):
    def __init__(self, name: str, config: Optional[dict] = None):
        self._name = name
        self._config = config or {}
        self._enabled = self._config.get('enabled', False)

    @property
    def name(self) -> str:
        return self._name

    @property
    def enabled(self) -> bool:
        return self._enabled

    @abstractmethod
    def get_tools(self) -> dict[str, callable]:
        ...

    @abstractmethod
    def health_check(self) -> dict:
        ...

    def __repr__(self) -> str:
        return f'<{self.__class__.__name__} name={self._name} enabled={self._enabled}>'


def load_plugin_configs() -> dict:
    path = CONFIG_DIR / 'plugins.yaml'
    if path.exists():
        with open(path, encoding='utf-8') as f:
            return yaml.safe_load(f) or {}
    return {}
