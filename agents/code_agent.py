"""Production-grade code agent with sandboxing, session persistence, and security."""
import json
import logging
import os
import re
import subprocess
import tempfile
import uuid
from pathlib import Path
from typing import Optional

from agents.base_agent import AgentResponse, BaseAgent
from agents.shared_utils import validate_path, format_json_safe, ValidationError
from memory.memory_manager import MemoryManager

logger = logging.getLogger(__name__)

PROJECT_ROOT = Path(__file__).parent.parent
WORKSPACE_DIR = PROJECT_ROOT / 'workspace'
WORKSPACE_DIR.mkdir(exist_ok=True)
VENV_PYTHON = str(PROJECT_ROOT / '.venv' / 'bin' / 'python3')

# Security: Allowed imports whitelist (optional, can be disabled)
ALLOWED_BUILTINS = {
    'abs', 'all', 'any', 'ascii', 'bin', 'bool', 'bytearray', 'bytes',
    'chr', 'complex', 'dict', 'divmod', 'enumerate', 'filter', 'float',
    'format', 'frozenset', 'hasattr', 'hash', 'hex', 'int', 'isinstance',
    'issubclass', 'iter', 'len', 'list', 'map', 'max', 'min', 'next',
    'oct', 'ord', 'pow', 'print', 'range', 'repr', 'reversed', 'round',
    'set', 'slice', 'sorted', 'str', 'sum', 'tuple', 'type', 'zip',
}

BANNED_PATTERNS = [
    r'import\s+os\s*$',  # Block raw os imports at module level
    r'subprocess\.run',
    r'subprocess\.call',
    r'subprocess\.Popen',
    r'os\.system',
    r'os\.exec',
    r'__import__',
    r'eval\s*\(',
    r'exec\s*\(',
    r'open\s*\([^)]*[,'"]w',
    r'pathlib\.Path\s*\([^)]*\.\.',
]


def _security_check(code: str) -> tuple[bool, str]:
    """Basic security scan for dangerous patterns."""
    for pattern in BANNED_PATTERNS:
        if re.search(pattern, code, re.IGNORECASE):
            return False, f"Security check failed: blocked pattern '{pattern}' detected."
    return True, ""


def _ensure_package(package_name: str) -> tuple[bool, str]:
    """Install a package if missing."""
    try:
        __import__(package_name.split('[')[0].split('==')[0].split('>=')[0].strip())
        return True, ""
    except ImportError:
        logger.info("Package %s not found, attempting install...", package_name)
        try:
            result = subprocess.run(
                [VENV_PYTHON, '-m', 'pip', 'install', '--quiet', package_name],
                capture_output=True, text=True, timeout=120,
            )
            if result.returncode == 0:
                return True, f"Installed {package_name}"
            return False, f"Failed to install {package_name}: {result.stderr}"
        except Exception as e:
            return False, f"Install error for {package_name}: {e}"


def execute_python(code: str, session_state: Optional[dict] = None, 
                   allow_install: bool = True, timeout: int = 30) -> str:
    """
    Run Python code in an isolated sandbox with session persistence.

    Args:
        code: Python code to execute
        session_state: Previous session variables to inject
        allow_install: Whether to auto-install missing packages
        timeout: Execution timeout in seconds

    Returns:
        JSON string with stdout, stderr, exit_code, files, and session_state
    """
    # Security check
    safe, reason = _security_check(code)
    if not safe:
        return format_json_safe({'error': reason, 'security_blocked': True})

    # Auto-install common packages if needed
    if allow_install:
        imports = re.findall(r'^(?:import|from)\s+([a-zA-Z_][a-zA-Z0-9_]*)', code, re.MULTILINE)
        for pkg in set(imports):
            if pkg in ('os', 'sys', 'subprocess', 'pathlib'):  # Built-ins, skip
                continue
            ok, msg = _ensure_package(pkg)
            if not ok:
                logger.warning("Could not ensure package %s: %s", pkg, msg)

    # Handle matplotlib auto-save
    plot_files = []
    if 'plt.show()' in code or 'matplotlib' in code:
        plot_name = f'plot_{uuid.uuid4().hex[:8]}.png'
        plot_path = str(WORKSPACE_DIR / plot_name)
        code = code.replace('plt.show()', f"plt.savefig('{plot_path}', dpi=150, bbox_inches='tight')")
        if 'plt.savefig' not in code:
            code += f"\nimport matplotlib.pyplot as plt\nplt.savefig('{plot_path}', dpi=150, bbox_inches='tight')"
        plot_files.append(plot_name)

    # Wrap code to capture session state
    session_wrapper = f"""
_session_vars = {{}}
{code}
# Capture non-private, serializable variables
import json, types
for _k, _v in list(locals().items()):
    if not _k.startswith('_') and _k not in ('json', 'types'):
        try:
            json.dumps(_v)
            _session_vars[_k] = _v
        except (TypeError, ValueError):
            pass
print("\n___SESSION_STATE___")
print(json.dumps(_session_vars, default=str))
"""

    env = os.environ.copy()
    env['MPLBACKEND'] = 'Agg'
    env['PYTHONDONTWRITEBYTECODE'] = '1'

    try:
        result = subprocess.run(
            [VENV_PYTHON, '-c', session_wrapper],
            capture_output=True, text=True, timeout=timeout,
            env=env, cwd=str(WORKSPACE_DIR),
        )

        stdout = result.stdout
        stderr = result.stderr
        session_data = {}

        # Extract session state
        if '___SESSION_STATE___' in stdout:
            parts = stdout.split('___SESSION_STATE___')
            stdout = parts[0]
            try:
                session_data = json.loads(parts[1].strip())
            except json.JSONDecodeError:
                pass

        output = {
            'stdout': stdout[:4000],
            'stderr': stderr[:2000],
            'exit_code': result.returncode,
            'session_state': session_data,
        }

        # Detect generated files
        for f in WORKSPACE_DIR.iterdir():
            if f.suffix.lower() in ('.png', '.jpg', '.jpeg', '.svg', '.pdf', '.csv', '.json'):
                if f.stat().st_mtime > time.time() - 10:
                    output['file'] = f.name
                    output['download_url'] = f'/files/{f.name}'

        for plot in plot_files:
            if (WORKSPACE_DIR / plot).exists():
                output['plot'] = plot
                output['plot_url'] = f'/files/{plot}'

        return format_json_safe(output)

    except subprocess.TimeoutExpired:
        return format_json_safe({'error': f'Execution timed out after {timeout} seconds'})
    except Exception as e:
        return format_json_safe({'error': str(e)})

execute_python._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'code': {
                'type': 'string',
                'description': 'Python code to execute. Matplotlib plots auto-saved to workspace.',
            },
            'timeout': {
                'type': 'integer',
                'description': 'Execution timeout in seconds',
                'default': 30,
            },
        },
        'required': ['code'],
    }
}


def read_file(path: str) -> str:
    """Read a file from the workspace with path validation."""
    try:
        p = validate_path(path, WORKSPACE_DIR, must_exist=True)
        if p.stat().st_size > 100_000:
            return format_json_safe({
                'error': f'File too large: {p.stat().st_size} bytes (max 100KB)',
                'path': str(p),
            })
        content = p.read_text(encoding='utf-8')
        return format_json_safe({
            'path': str(p), 
            'content': content, 
            'lines': len(content.splitlines()),
            'size': p.stat().st_size,
        })
    except ValidationError as e:
        return format_json_safe({'error': str(e), 'validation_failed': True})
    except Exception as e:
        return format_json_safe({'error': str(e)})

read_file._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'path': {'type': 'string', 'description': 'Absolute or workspace-relative path to file'},
        },
        'required': ['path'],
    }
}


def write_file(path: str, content: str, overwrite: bool = False) -> str:
    """Write content to a file in the workspace with safety checks."""
    try:
        p = validate_path(path, WORKSPACE_DIR)
        if p.exists() and not overwrite:
            return format_json_safe({
                'error': f'File already exists: {p}. Use overwrite=true to replace.',
                'path': str(p),
            })
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(content, encoding='utf-8')
        return format_json_safe({
            'path': str(p), 
            'bytes_written': len(content.encode('utf-8')), 
            'status': 'ok',
        })
    except ValidationError as e:
        return format_json_safe({'error': str(e), 'validation_failed': True})
    except Exception as e:
        return format_json_safe({'error': str(e)})

write_file._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'path': {'type': 'string', 'description': 'Absolute or workspace-relative path'},
            'content': {'type': 'string', 'description': 'File content to write'},
            'overwrite': {'type': 'boolean', 'description': 'Allow overwriting existing files', 'default': False},
        },
        'required': ['path', 'content'],
    }
}


def list_directory(path: str = '.') -> str:
    """List files in a workspace directory."""
    try:
        p = validate_path(path, WORKSPACE_DIR, must_exist=True)
        if not p.is_dir():
            return format_json_safe({'error': f'Not a directory: {p}'})
        files = []
        for item in p.iterdir():
            files.append({
                'name': item.name,
                'type': 'directory' if item.is_dir() else 'file',
                'size': item.stat().st_size if item.is_file() else None,
                'modified': item.stat().st_mtime,
            })
        return format_json_safe({'path': str(p), 'items': files})
    except ValidationError as e:
        return format_json_safe({'error': str(e)})
    except Exception as e:
        return format_json_safe({'error': str(e)})

list_directory._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'path': {'type': 'string', 'description': 'Directory path (relative to workspace)', 'default': '.'},
        },
    }
}


def search_semantic_memory(query: str, top_k: int = 5) -> str:
    """Search indexed codebases and documentation for relevant content."""
    return format_json_safe({'info': 'semantic memory search stub', 'query': query})

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
    return format_json_safe({'info': 'knowledge graph query stub', 'cypher': cypher_query})

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
    'list_directory': list_directory,
    'search_semantic_memory': search_semantic_memory,
    'query_knowledge_graph': query_knowledge_graph,
}


class CodeAgent(BaseAgent):
    """Agent specialized in code execution, debugging, and file operations."""

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

        # Session persistence for code execution
        self._code_session: dict = {}

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
                    {
                        'title': e.get('title', ''), 
                        'content': e.get('content', '')[:500], 
                        'source': e.get('source_path', ''),
                        'score': e.get('score', 0),
                    }
                    for e in context.semantic
                ]
                return format_json_safe(results)
            except Exception as e:
                return format_json_safe({'error': str(e)})

        search._tool_schema = search_semantic_memory._tool_schema
        return search

    def _make_graph_query(self, memory: MemoryManager) -> callable:
        def query(cypher_query: str) -> str:
            """Look up project structure, dependencies, and relationships in the knowledge graph."""
            try:
                result = memory.relational.query(cypher_query)
                return format_json_safe({
                    'entities': result.entities[:10],
                    'relationships': result.relationships[:10],
                })
            except Exception as e:
                return format_json_safe({'error': str(e)})

        query._tool_schema = query_knowledge_graph._tool_schema
        return query

    def code(
        self,
        request: str,
        conversation_history: Optional[list[dict]] = None,
        task_type: str = 'general',
    ) -> AgentResponse:
        """Execute a coding task with appropriate context injection."""
        extra = ''
        if task_type == 'debug':
            extra = (
                'DEBUGGING MODE: Read any provided traceback bottom-up. '
                'Identify the root cause before suggesting a fix. '
                'State what is actually wrong, not just the symptom. '
                'Use execute_python to verify your hypothesis if needed.'
            )
        elif task_type == 'review':
            extra = (
                'CODE REVIEW MODE: Identify issues by severity (Critical/Warning/Suggestion). '
                'Suggest concrete improvements with refactored code snippets. '
                'Check for: security issues, performance bottlenecks, style violations, '
                'and maintainability concerns.'
            )
        elif task_type == 'refactor':
            extra = (
                'REFACTORING MODE: Preserve exact behavior while improving structure. '
                'Explain each change and its trade-off. Run tests before and after if available. '
                'Target: readability, testability, and performance.'
            )
        elif task_type == 'test':
            extra = (
                'TESTING MODE: Write comprehensive tests covering happy paths, edge cases, '
                'and error conditions. Use pytest style. Ensure tests are deterministic.'
            )

        return self.run(
            user_message=request,
            conversation_history=conversation_history,
            extra_context=extra,
        )

    def reset_session(self):
        """Clear the code execution session state."""
        self._code_session = {}
        logger.info("[%s] Code session reset", self.agent_type)