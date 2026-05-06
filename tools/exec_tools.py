import json
import logging
import subprocess
import tempfile
from pathlib import Path
from typing import Optional

logger = logging.getLogger(__name__)

TIMEOUT_SECONDS = 30
MAX_OUTPUT_CHARS = 5000

PROJECT_ROOT = Path(__file__).parent.parent
WORKSPACE_DIR = PROJECT_ROOT / 'workspace'
WORKSPACE_DIR.mkdir(exist_ok=True)
VENV_PYTHON = str(PROJECT_ROOT / '.venv' / 'bin' / 'python3')


def execute_python(code: str, timeout: int = TIMEOUT_SECONDS) -> dict:
    """Execute Python code in an isolated subprocess sandbox. Plots are auto-saved to workspace."""
    import os
    import re
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
            capture_output=True,
            text=True,
            timeout=timeout,
            env=env,
        )
        output = {
            'stdout': result.stdout[:MAX_OUTPUT_CHARS],
            'stderr': result.stderr[:MAX_OUTPUT_CHARS],
            'exit_code': result.returncode,
        }
        if plot_file and (WORKSPACE_DIR / plot_file).exists():
            output['file'] = plot_file
            output['download_url'] = f'/files/{plot_file}'
        return output
    except subprocess.TimeoutExpired:
        return {'stdout': '', 'stderr': f'Execution timed out after {timeout}s', 'exit_code': -1}
    except Exception as e:
        return {'stdout': '', 'stderr': str(e), 'exit_code': -1}

execute_python._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'code': {'type': 'string', 'description': 'Python code to execute. Plots are auto-saved to workspace.'},
            'timeout': {'type': 'integer', 'description': 'Timeout in seconds', 'default': 30},
        },
        'required': ['code'],
    }
}


def execute_shell(command: str, timeout: int = TIMEOUT_SECONDS) -> dict:
    """Execute a shell command in a sandboxed subprocess."""
    blocked = ['rm -rf', 'sudo', 'mkfs', 'dd if=', ':(){', 'fork']
    lower = command.lower()
    for b in blocked:
        if b in lower:
            return {'stdout': '', 'stderr': f'Blocked command pattern: {b}', 'exit_code': -1}

    try:
        result = subprocess.run(
            command,
            shell=True,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
        return {
            'stdout': result.stdout[:MAX_OUTPUT_CHARS],
            'stderr': result.stderr[:MAX_OUTPUT_CHARS],
            'exit_code': result.returncode,
        }
    except subprocess.TimeoutExpired:
        return {'stdout': '', 'stderr': f'Timed out after {timeout}s', 'exit_code': -1}
    except Exception as e:
        return {'stdout': '', 'stderr': str(e), 'exit_code': -1}

execute_shell._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'command': {'type': 'string', 'description': 'Shell command to execute'},
            'timeout': {'type': 'integer', 'description': 'Timeout in seconds', 'default': 30},
        },
        'required': ['command'],
    }
}


def execute_python_file(path: str, args: str = '', timeout: int = TIMEOUT_SECONDS) -> dict:
    """Execute a Python file in a subprocess."""
    p = Path(path)
    if not p.exists():
        return {'stdout': '', 'stderr': f'File not found: {path}', 'exit_code': -1}

    cmd = [VENV_PYTHON, str(p)]
    if args:
        cmd.extend(args.split())

    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
        return {
            'stdout': result.stdout[:MAX_OUTPUT_CHARS],
            'stderr': result.stderr[:MAX_OUTPUT_CHARS],
            'exit_code': result.returncode,
        }
    except subprocess.TimeoutExpired:
        return {'stdout': '', 'stderr': f'Timed out after {timeout}s', 'exit_code': -1}
    except Exception as e:
        return {'stdout': '', 'stderr': str(e), 'exit_code': -1}

execute_python_file._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'path': {'type': 'string', 'description': 'Path to Python file'},
            'args': {'type': 'string', 'description': 'Command line arguments', 'default': ''},
            'timeout': {'type': 'integer', 'description': 'Timeout in seconds', 'default': 30},
        },
        'required': ['path'],
    }
}

EXEC_TOOLS = {
    'execute_python': execute_python,
    'execute_shell': execute_shell,
    'execute_python_file': execute_python_file,
}
