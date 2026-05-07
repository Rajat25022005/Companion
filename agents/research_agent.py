import json
import logging
import subprocess
from pathlib import Path
from typing import Optional

from agents.base_agent import AgentResponse, BaseAgent
from memory.memory_manager import MemoryManager

logger = logging.getLogger(__name__)

PROJECT_ROOT = Path(__file__).parent.parent
VENV_PYTHON = str(PROJECT_ROOT / '.venv' / 'bin' / 'python3')


def web_search(query: str, max_results: int = 5) -> str:
    """Search the web using DuckDuckGo and return results with titles, URLs, and snippets."""
    try:
        from duckduckgo_search import DDGS
        with DDGS() as ddgs:
            results = list(ddgs.text(query, max_results=max_results))
        formatted = [
            {'title': r.get('title', ''), 'url': r.get('href', ''), 'snippet': r.get('body', '')}
            for r in results
        ]
        return json.dumps(formatted, default=str)
    except Exception as e:
        logger.error('Web search failed: %s', e)
        return json.dumps({'error': str(e)})

web_search._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'query': {'type': 'string', 'description': 'Search query to look up on the web'},
            'max_results': {'type': 'integer', 'description': 'Number of results to return', 'default': 5},
        },
        'required': ['query'],
    }
}


def search_semantic_memory(query: str, top_k: int = 5) -> str:
    """Search the user's indexed documents and notes for relevant content."""
    return json.dumps({'info': 'semantic memory search stub', 'query': query, 'top_k': top_k})

search_semantic_memory._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'query': {'type': 'string', 'description': 'Search query for document corpus'},
            'top_k': {'type': 'integer', 'description': 'Number of results to return', 'default': 5},
        },
        'required': ['query'],
    }
}


def search_episodic_memory(query: str, top_k: int = 5) -> str:
    """Search past conversation history for relevant context."""
    return json.dumps({'info': 'episodic memory search stub', 'query': query, 'top_k': top_k})

search_episodic_memory._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'query': {'type': 'string', 'description': 'Search query for conversation history'},
            'top_k': {'type': 'integer', 'description': 'Number of results to return', 'default': 5},
        },
        'required': ['query'],
    }
}


def query_knowledge_graph(cypher_query: str) -> str:
    """Run a Cypher query against the Neo4j knowledge graph for structured lookups."""
    return json.dumps({'info': 'knowledge graph query stub', 'cypher': cypher_query})

query_knowledge_graph._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'cypher_query': {'type': 'string', 'description': 'Cypher query to execute against Neo4j'},
        },
        'required': ['cypher_query'],
    }
}


def execute_python(code: str) -> str:
    """Run Python code in an isolated sandbox for data analysis or verification."""
    try:
        result = subprocess.run(
            [VENV_PYTHON, '-c', code],
            capture_output=True, text=True, timeout=30,
        )
        return json.dumps({
            'stdout': result.stdout[:2000],
            'stderr': result.stderr[:1000],
            'exit_code': result.returncode,
        })
    except subprocess.TimeoutExpired:
        return json.dumps({'error': 'Execution timed out after 30 seconds'})
    except Exception as e:
        return json.dumps({'error': str(e)})

execute_python._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'code': {'type': 'string', 'description': 'Python code to execute'},
        },
        'required': ['code'],
    }
}


RESEARCH_TOOLS = {
    'web_search': web_search,
    'search_semantic_memory': search_semantic_memory,
    'search_episodic_memory': search_episodic_memory,
    'query_knowledge_graph': query_knowledge_graph,
    'execute_python': execute_python,
}


class ResearchAgent(BaseAgent):
    def __init__(
        self,
        memory: Optional[MemoryManager] = None,
        tools: Optional[dict[str, callable]] = None,
        **kwargs,
    ):
        effective_tools = {**RESEARCH_TOOLS}
        if tools:
            effective_tools.update(tools)

        if memory:
            effective_tools['search_semantic_memory'] = self._make_semantic_search(memory)
            effective_tools['search_episodic_memory'] = self._make_episodic_search(memory)
            effective_tools['query_knowledge_graph'] = self._make_graph_query(memory)

        super().__init__(memory=memory, tools=effective_tools, **kwargs)

    @property
    def agent_type(self) -> str:
        return 'research'

    @property
    def skill_name(self) -> str:
        return 'research'

    @property
    def memory_layers(self) -> list[str]:
        return ['episodic', 'semantic', 'relational']

    def get_available_tools(self) -> list[str]:
        return list(self._tools.keys())

    def _make_semantic_search(self, memory: MemoryManager) -> callable:
        def search(query: str, top_k: int = 5) -> str:
            """Search the user's indexed documents and notes for relevant content."""
            try:
                context = memory.retrieve(query=query, layers=['semantic'], top_k=top_k)
                results = [
                    {'title': e.get('title', ''), 'content': e.get('content', '')[:300], 'source': e.get('source_path', '')}
                    for e in context.semantic
                ]
                return json.dumps(results, default=str)
            except Exception as e:
                return json.dumps({'error': str(e)})

        search._tool_schema = search_semantic_memory._tool_schema
        return search

    def _make_episodic_search(self, memory: MemoryManager) -> callable:
        def search(query: str, top_k: int = 5) -> str:
            """Search past conversation history for relevant context."""
            try:
                context = memory.retrieve(
                    query=query, 
                    layers=['episodic'], 
                    top_k=top_k,
                    session_filter=getattr(self, '_current_session_id', '')
                )
                results = [
                    {'content': e.get('content', '')[:200], 'response': e.get('response', '')[:200], 'timestamp': e.get('timestamp', '')}
                    for e in context.episodic
                ]
                return json.dumps(results, default=str)
            except Exception as e:
                return json.dumps({'error': str(e)})

        search._tool_schema = search_episodic_memory._tool_schema
        return search

    def _make_graph_query(self, memory: MemoryManager) -> callable:
        def query(cypher_query: str) -> str:
            """Run a Cypher query against the Neo4j knowledge graph for structured lookups."""
            try:
                result = memory.relational.query(cypher_query)
                return json.dumps({
                    'entities': result.entities[:10],
                    'relationships': result.relationships[:10],
                    'records': [str(r) for r in result.raw_records[:10]],
                }, default=str)
            except Exception as e:
                return json.dumps({'error': str(e)})

        query._tool_schema = query_knowledge_graph._tool_schema
        return query

    def research(
        self,
        query: str,
        conversation_history: Optional[list[dict]] = None,
        depth: str = 'standard',
    ) -> AgentResponse:
        extra = ''
        if depth == 'deep':
            extra = (
                'This is a deep research request. Be thorough: check all memory layers, '
                'cross-reference findings, and provide comprehensive analysis with sources.'
            )
        elif depth == 'quick':
            extra = (
                'This is a quick lookup. Be concise: check memory first, give a direct answer. '
                'Skip the full analysis structure if the answer is straightforward.'
            )

        return self.run(
            user_message=query,
            conversation_history=conversation_history,
            extra_context=extra,
        )
