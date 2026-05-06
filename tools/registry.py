import logging

from tools.file_tools import FILE_TOOLS
from tools.exec_tools import EXEC_TOOLS
from tools.doc_tools import DOC_TOOLS

logger = logging.getLogger(__name__)


ALL_TOOLS: dict[str, callable] = {}
ALL_TOOLS.update(FILE_TOOLS)
ALL_TOOLS.update(EXEC_TOOLS)
ALL_TOOLS.update(DOC_TOOLS)


def get_tools(*categories: str) -> dict[str, callable]:
    """Get tools by category. Categories: 'file', 'exec', 'doc', 'all'."""
    tool_map = {
        'file': FILE_TOOLS,
        'exec': EXEC_TOOLS,
        'doc': DOC_TOOLS,
        'all': ALL_TOOLS,
    }

    if not categories or 'all' in categories:
        return dict(ALL_TOOLS)

    result = {}
    for cat in categories:
        if cat in tool_map:
            result.update(tool_map[cat])
        else:
            logger.warning('Unknown tool category: %s', cat)
    return result


def get_tool(name: str) -> callable:
    """Get a single tool by name."""
    if name not in ALL_TOOLS:
        raise KeyError(f'Unknown tool: {name}. Available: {list(ALL_TOOLS.keys())}')
    return ALL_TOOLS[name]


def list_tools() -> list[dict]:
    """List all available tools with their descriptions."""
    tools = []
    for name, fn in ALL_TOOLS.items():
        doc = fn.__doc__ or ''
        schema = getattr(fn, '_tool_schema', {})
        tools.append({
            'name': name,
            'description': doc.strip().split('\n')[0] if doc else name,
            'parameters': schema.get('parameters', {}),
        })
    return tools
