"""Production-grade document agent with format detection and template management."""
import json
import logging
import os
import re
import subprocess
import time
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

# Supported document formats
SUPPORTED_FORMATS = {
    'pdf': {'ext': '.pdf', 'libs': ['reportlab'], 'default': True},
    'docx': {'ext': '.docx', 'libs': ['python-docx', 'docx']},
    'pptx': {'ext': '.pptx', 'libs': ['python-pptx', 'pptx']},
    'xlsx': {'ext': '.xlsx', 'libs': ['openpyxl']},
}


def _detect_format(request: str) -> str:
    """Detect desired document format from user request."""
    request_lower = request.lower()

    # Explicit mentions
    if any(k in request_lower for k in ['slides', 'deck', 'presentation', 'pptx', 'powerpoint']):
        return 'pptx'
    if any(k in request_lower for k in ['word', '.docx', 'editable', 'microsoft word']):
        return 'docx'
    if any(k in request_lower for k in ['spreadsheet', 'excel', 'xlsx', 'table', 'csv']):
        return 'xlsx'
    if any(k in request_lower for k in ['pdf', 'report', 'letter', 'memo']):
        return 'pdf'

    return 'pdf'  # Default


def _ensure_libraries(format_type: str) -> tuple[bool, str]:
    """Ensure required libraries are installed for a format."""
    info = SUPPORTED_FORMATS.get(format_type)
    if not info:
        return False, f"Unsupported format: {format_type}"

    for lib in info['libs']:
        try:
            __import__(lib)
            return True, ""
        except ImportError:
            logger.info("Installing %s for %s support...", lib, format_type)
            try:
                result = subprocess.run(
                    [VENV_PYTHON, '-m', 'pip', 'install', '--quiet', lib],
                    capture_output=True, text=True, timeout=120,
                )
                if result.returncode != 0:
                    return False, f"Failed to install {lib}: {result.stderr}"
            except Exception as e:
                return False, f"Install error: {e}"
    return True, ""


def execute_python(code: str, timeout: int = 60) -> str:
    """
    Execute Python code for document generation.
    Auto-detects and returns generated document files.
    """
    env = os.environ.copy()
    env['MPLBACKEND'] = 'Agg'
    env['PYTHONDONTWRITEBYTECODE'] = '1'

    try:
        result = subprocess.run(
            [VENV_PYTHON, '-c', code],
            capture_output=True, text=True, timeout=timeout,
            env=env, cwd=str(WORKSPACE_DIR),
        )

        output = {
            'stdout': result.stdout[:4000],
            'stderr': result.stderr[:2000],
            'exit_code': result.returncode,
        }

        # Detect generated document files (check last 10 seconds)
        cutoff = time.time() - 10
        for f in WORKSPACE_DIR.iterdir():
            if f.suffix.lower() in ('.pdf', '.docx', '.pptx', '.xlsx', '.png', '.jpg', '.html'):
                if f.stat().st_mtime > cutoff:
                    output['file'] = f.name
                    output['download_url'] = f'/files/{f.name}'
                    output['file_size'] = f.stat().st_size
                    break

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
                'description': (
                    'Python code that generates a document. '
                    'Use reportlab for PDF, python-docx for DOCX, python-pptx for PPTX, openpyxl for XLSX. '
                    f'Save files to: {WORKSPACE_DIR}'
                ),
            },
            'timeout': {
                'type': 'integer',
                'description': 'Execution timeout in seconds',
                'default': 60,
            },
        },
        'required': ['code'],
    }
}


def validate_document(path: str) -> str:
    """Validate that a generated document is readable and non-empty."""
    try:
        p = validate_path(path, WORKSPACE_DIR, must_exist=True)
        size = p.stat().st_size
        if size == 0:
            return format_json_safe({'valid': False, 'error': 'File is empty'})

        # Format-specific validation
        ext = p.suffix.lower()
        if ext == '.pdf':
            content = p.read_bytes()[:5]
            valid = content.startswith(b'%PDF-')
            return format_json_safe({'valid': valid, 'format': 'PDF', 'size': size})
        elif ext == '.docx':
            from zipfile import ZipFile
            try:
                with ZipFile(p, 'r') as z:
                    valid = 'word/document.xml' in z.namelist()
                return format_json_safe({'valid': valid, 'format': 'DOCX', 'size': size})
            except Exception:
                return format_json_safe({'valid': False, 'format': 'DOCX', 'error': 'Invalid ZIP structure'})
        elif ext == '.xlsx':
            from zipfile import ZipFile
            try:
                with ZipFile(p, 'r') as z:
                    valid = 'xl/workbook.xml' in z.namelist()
                return format_json_safe({'valid': valid, 'format': 'XLSX', 'size': size})
            except Exception:
                return format_json_safe({'valid': False, 'format': 'XLSX', 'error': 'Invalid ZIP structure'})
        elif ext == '.pptx':
            from zipfile import ZipFile
            try:
                with ZipFile(p, 'r') as z:
                    valid = 'ppt/presentation.xml' in z.namelist()
                return format_json_safe({'valid': valid, 'format': 'PPTX', 'size': size})
            except Exception:
                return format_json_safe({'valid': False, 'format': 'PPTX', 'error': 'Invalid ZIP structure'})
        else:
            return format_json_safe({'valid': True, 'format': ext, 'size': size})

    except ValidationError as e:
        return format_json_safe({'valid': False, 'error': str(e)})
    except Exception as e:
        return format_json_safe({'valid': False, 'error': str(e)})

validate_document._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'path': {'type': 'string', 'description': 'Path to document to validate'},
        },
        'required': ['path'],
    }
}


DOC_TOOLS = {
    'execute_python': execute_python,
    'validate_document': validate_document,
}


class DocAgent(BaseAgent):
    """Agent specialized in document generation and formatting."""

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
        return 'document'

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
            '\n\n--- DOCUMENT GENERATION PROTOCOL ---\n'
            '- You MUST call execute_python with a complete, runnable Python script.\n'
            '- Do NOT just show code as text. ALWAYS execute it.\n'
            '- Do NOT ask the user what format they want. Auto-detect from their request.\n'
            '- Default to PDF using reportlab if format is unclear.\n'
            f'- Save all files to: {WORKSPACE_DIR}\n'
            '- After execution, validate the document with validate_document if needed.\n'
            '- Tell the user the download URL: /files/filename.ext\n'
            '- Use descriptive filenames (e.g., q3_report.pdf, not doc1.pdf).\n'
        )
        return prompt

    def generate(
        self,
        request: str,
        conversation_history: Optional[list[dict]] = None,
        format_hint: str = '',
    ) -> AgentResponse:
        """Generate a document with automatic format detection."""
        detected = format_hint or _detect_format(request)
        ok, err = _ensure_libraries(detected)

        extra = f"Detected format: {detected}. "
        if not ok:
            extra += f"Warning: Could not ensure libraries ({err}). Fallback to basic generation. "

        extra += (
            f"Use {SUPPORTED_FORMATS[detected]['libs'][0]} for {detected.upper()}. "
            "Include proper error handling with try/except. "
            "Use professional styling with consistent colors and fonts."
        )

        return self.run(
            user_message=request,
            conversation_history=conversation_history,
            extra_context=extra,
        )