import json
import logging
from pathlib import Path
from typing import Optional

logger = logging.getLogger(__name__)

MAX_FILE_SIZE = 500_000


def read_file(path: str) -> dict:
    """Read a file from the workspace and return its contents."""
    try:
        p = Path(path)
        if not p.exists():
            return {'error': f'File not found: {path}'}
        if not p.is_file():
            return {'error': f'Not a file: {path}'}
        if p.stat().st_size > MAX_FILE_SIZE:
            return {'error': f'File too large: {p.stat().st_size} bytes (max {MAX_FILE_SIZE})'}

        content = p.read_text(encoding='utf-8')
        return {
            'path': str(p.resolve()),
            'content': content,
            'lines': len(content.splitlines()),
            'size_bytes': len(content.encode('utf-8')),
        }
    except UnicodeDecodeError:
        return {'error': f'Cannot read as text: {path}'}
    except Exception as e:
        return {'error': str(e)}

read_file._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'path': {'type': 'string', 'description': 'Absolute path to the file'},
        },
        'required': ['path'],
    }
}


def write_file(path: str, content: str) -> dict:
    """Write content to a file. Creates parent directories if needed."""
    try:
        p = Path(path)
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(content, encoding='utf-8')
        return {
            'path': str(p.resolve()),
            'bytes_written': len(content.encode('utf-8')),
            'status': 'ok',
        }
    except Exception as e:
        return {'error': str(e)}

write_file._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'path': {'type': 'string', 'description': 'Absolute path to write to'},
            'content': {'type': 'string', 'description': 'File content to write'},
        },
        'required': ['path', 'content'],
    }
}


def list_directory(path: str, recursive: bool = False) -> dict:
    """List files and directories at the given path."""
    try:
        p = Path(path)
        if not p.is_dir():
            return {'error': f'Not a directory: {path}'}

        pattern = '**/*' if recursive else '*'
        entries = []
        for item in sorted(p.glob(pattern)):
            if item.name.startswith('.'):
                continue
            entries.append({
                'name': item.name,
                'path': str(item.resolve()),
                'is_dir': item.is_dir(),
                'size': item.stat().st_size if item.is_file() else None,
            })

        return {'path': str(p.resolve()), 'entries': entries[:200], 'count': len(entries)}
    except Exception as e:
        return {'error': str(e)}

list_directory._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'path': {'type': 'string', 'description': 'Directory path to list'},
            'recursive': {'type': 'boolean', 'description': 'List recursively', 'default': False},
        },
        'required': ['path'],
    }
}


def append_file(path: str, content: str) -> dict:
    """Append content to an existing file."""
    try:
        p = Path(path)
        if not p.exists():
            return {'error': f'File not found: {path}'}
        with open(p, 'a', encoding='utf-8') as f:
            f.write(content)
        return {'path': str(p.resolve()), 'bytes_appended': len(content.encode('utf-8')), 'status': 'ok'}
    except Exception as e:
        return {'error': str(e)}

append_file._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'path': {'type': 'string', 'description': 'File to append to'},
            'content': {'type': 'string', 'description': 'Content to append'},
        },
        'required': ['path', 'content'],
    }
}

FILE_TOOLS = {
    'read_file': read_file,
    'write_file': write_file,
    'list_directory': list_directory,
    'append_file': append_file,
}
