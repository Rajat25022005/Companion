import json
import logging
import os
import subprocess
import uuid
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
    """Run Python code to generate documents (PDF, DOCX, PPTX). Files should be saved to the workspace directory."""
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
            capture_output=True, text=True, timeout=60,
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

        for f in WORKSPACE_DIR.iterdir():
            if f.suffix.lower() in ('.pdf', '.docx', '.pptx', '.png', '.jpg') and f.stat().st_mtime > (
                __import__('time').time() - 5
            ):
                output['file'] = f.name
                output['download_url'] = f'/files/{f.name}'
                break

        return json.dumps(output)
    except subprocess.TimeoutExpired:
        return json.dumps({'error': 'Execution timed out after 60 seconds'})
    except Exception as e:
        return json.dumps({'error': str(e)})

execute_python._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'code': {
                'type': 'string',
                'description': (
                    'Python code that generates a document. '
                    'Use reportlab for PDF, python-docx for DOCX, python-pptx for PPTX. '
                    f'Save files to: {WORKSPACE_DIR}'
                ),
            },
        },
        'required': ['code'],
    }
}


DOC_TOOLS = {
    'execute_python': execute_python,
}


class DocAgent(BaseAgent):
    def __init__(
        self,
        memory: Optional[MemoryManager] = None,
        tools: Optional[dict[str, callable]] = None,
        **kwargs,
    ):
        effective_tools = {**DOC_TOOLS}
        if tools:
            effective_tools.update(tools)
        super().__init__(memory=memory, tools=effective_tools, **kwargs)

    @property
    def agent_type(self) -> str:
        return 'conductor'

    @property
    def skill_name(self) -> str:
        return 'document'

    @property
    def memory_layers(self) -> list[str]:
        return ['episodic', 'semantic']

    def get_available_tools(self) -> list[str]:
        return list(self._tools.keys())

    def _build_system_prompt(self, memory_context: str = '') -> str:
        prompt = super()._build_system_prompt(memory_context)
        prompt += (
            '\n\nCRITICAL INSTRUCTIONS:\n'
            '- You MUST call execute_python with a complete Python script to generate the document.\n'
            '- Do NOT just show code as text. ALWAYS execute it.\n'
            '- Do NOT ask the user what format they want. Default to PDF using reportlab.\n'
            f'- Save all files to: {WORKSPACE_DIR}\n'
            '- After execution, tell the user the download URL: /files/filename.pdf\n'
        )
        return prompt
