import json
import logging
import subprocess
from pathlib import Path
from typing import Optional

from agents.base_agent import AgentResponse, BaseAgent
from memory.memory_manager import MemoryManager

logger = logging.getLogger(__name__)


PROJECT_ROOT = Path(__file__).parent.parent
WORKSPACE_DIR = PROJECT_ROOT / 'workspace'
WORKSPACE_DIR.mkdir(exist_ok=True)
VENV_PYTHON = str(PROJECT_ROOT / '.venv' / 'bin' / 'python3')


def execute_python(code: str) -> str:
    """Run Python code in an isolated sandbox. Plots are auto-saved to workspace. Returns stdout/stderr/exit_code and download_url if a plot was generated."""
    import os
    import uuid

    plot_file = None
    if 'plt.show()' in code or 'matplotlib' in code:
        plot_name = f'plot_{uuid.uuid4().hex[:8]}.png'
        plot_path = str(WORKSPACE_DIR / plot_name)
        code = code.replace('plt.show()', f"plt.savefig('{plot_path}', dpi=150, bbox_inches='tight')")
        if 'plt.savefig' not in code and 'plt.show' not in code:
            code += f"\nimport matplotlib.pyplot as plt\nplt.savefig('{plot_path}', dpi=150, bbox_inches='tight')"
        plot_file = plot_name

    env = os.environ.copy()
    env['MPLBACKEND'] = 'Agg'

    try:
        result = subprocess.run(
            [VENV_PYTHON, '-c', code],
            capture_output=True, text=True, timeout=30,
            env=env,
        )
        output = {
            'stdout': result.stdout[:4000],
            'stderr': result.stderr[:2000],
            'exit_code': result.returncode,
        }
        if plot_file and (WORKSPACE_DIR / plot_file).exists():
            output['file'] = plot_file
            output['download_url'] = f'/files/{plot_file}'
            output['message'] = f'Plot saved. View at: /files/{plot_file}'
        return json.dumps(output)
    except subprocess.TimeoutExpired:
        return json.dumps({'error': 'Execution timed out after 30 seconds'})
    except Exception as e:
        return json.dumps({'error': str(e)})

execute_python._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'code': {'type': 'string', 'description': 'Python code to execute. Matplotlib plots are auto-saved to workspace.'},
        },
        'required': ['code'],
    }
}


def read_file(path: str) -> str:
    """Read a file from the workspace and return its contents."""
    try:
        p = Path(path)
        if not p.exists():
            return json.dumps({'error': f'File not found: {path}'})
        if p.stat().st_size > 100_000:
            return json.dumps({'error': f'File too large: {p.stat().st_size} bytes (max 100KB)'})
        content = p.read_text(encoding='utf-8')
        return json.dumps({'path': str(p), 'content': content, 'lines': len(content.splitlines())})
    except Exception as e:
        return json.dumps({'error': str(e)})

read_file._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'path': {'type': 'string', 'description': 'Absolute path to file to read'},
        },
        'required': ['path'],
    }
}


def write_file(path: str, content: str) -> str:
    """Write content to a file in the workspace. Creates parent directories if needed."""
    try:
        p = Path(path)
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(content, encoding='utf-8')
        return json.dumps({'path': str(p), 'bytes_written': len(content.encode('utf-8')), 'status': 'ok'})
    except Exception as e:
        return json.dumps({'error': str(e)})

write_file._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'path': {'type': 'string', 'description': 'Absolute path to write file to'},
            'content': {'type': 'string', 'description': 'File content to write'},
        },
        'required': ['path', 'content'],
    }
}


def search_semantic_memory(query: str, top_k: int = 5) -> str:
    """Search indexed codebases and documentation for relevant content."""
    return json.dumps({'info': 'semantic memory search stub', 'query': query})

search_semantic_memory._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'query': {'type': 'string', 'description': 'Search query'},
            'top_k': {'type': 'integer', 'description': 'Number of results', 'default': 5},
        },
        'required': ['query'],
    }
}


def query_knowledge_graph(cypher_query: str) -> str:
    """Look up project structure, dependencies, and relationships in the knowledge graph."""
    return json.dumps({'info': 'knowledge graph query stub', 'cypher': cypher_query})

query_knowledge_graph._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'cypher_query': {'type': 'string', 'description': 'Cypher query to run'},
        },
        'required': ['cypher_query'],
    }
}


CODE_TOOLS = {
    'execute_python': execute_python,
    'read_file': read_file,
    'write_file': write_file,
    'search_semantic_memory': search_semantic_memory,
    'query_knowledge_graph': query_knowledge_graph,
}


class CodeAgent(BaseAgent):
    def __init__(
        self,
        memory: Optional[MemoryManager] = None,
        tools: Optional[dict[str, callable]] = None,
        **kwargs,
    ):
        effective_tools = {**CODE_TOOLS}
        if tools:
            effective_tools.update(tools)

        if memory:
            effective_tools['search_semantic_memory'] = self._make_semantic_search(memory)
            effective_tools['query_knowledge_graph'] = self._make_graph_query(memory)

        super().__init__(memory=memory, tools=effective_tools, **kwargs)

    @property
    def agent_type(self) -> str:
        return 'code'

    @property
    def skill_name(self) -> str:
        return 'code'

    @property
    def memory_layers(self) -> list[str]:
        return ['episodic', 'semantic', 'relational']

    def get_available_tools(self) -> list[str]:
        return list(self._tools.keys())

    def _make_semantic_search(self, memory: MemoryManager) -> callable:
        def search(query: str, top_k: int = 5) -> str:
            """Search indexed codebases and documentation for relevant content."""
            try:
                context = memory.retrieve(query=query, layers=['semantic'], top_k=top_k)
                results = [
                    {'title': e.get('title', ''), 'content': e.get('content', '')[:500], 'source': e.get('source_path', '')}
                    for e in context.semantic
                ]
                return json.dumps(results, default=str)
            except Exception as e:
                return json.dumps({'error': str(e)})

        search._tool_schema = search_semantic_memory._tool_schema
        return search

    def _make_graph_query(self, memory: MemoryManager) -> callable:
        def query(cypher_query: str) -> str:
            """Look up project structure, dependencies, and relationships in the knowledge graph."""
            try:
                result = memory.relational.query(cypher_query)
                return json.dumps({
                    'entities': result.entities[:10],
                    'relationships': result.relationships[:10],
                }, default=str)
            except Exception as e:
                return json.dumps({'error': str(e)})

        query._tool_schema = query_knowledge_graph._tool_schema
        return query

    def code(
        self,
        request: str,
        conversation_history: Optional[list[dict]] = None,
        task_type: str = 'general',
    ) -> AgentResponse:
        extra = ''
        if task_type == 'debug':
            extra = (
                'This is a debugging request. Read any provided traceback bottom-up. '
                'Identify the root cause before suggesting a fix. State what is actually wrong.'
            )
        elif task_type == 'review':
            extra = (
                'This is a code review request. Identify issues by severity. '
                'Suggest concrete improvements with refactored code.'
            )
        elif task_type == 'refactor':
            extra = (
                'This is a refactoring request. Preserve behavior while improving structure. '
                'Explain each change and its trade-offs.'
            )

        return self.run(
            user_message=request,
            conversation_history=conversation_history,
            extra_context=extra,
        )
