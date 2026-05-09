"""Production-grade research agent with caching, source deduplication, and fact extraction."""
import json
import logging
import os
import subprocess
import time
from pathlib import Path
from typing import Optional
from urllib.parse import urlparse

from agents.base_agent import AgentResponse, BaseAgent
from agents.shared_utils import retry, format_json_safe
from memory.memory_manager import MemoryManager

logger = logging.getLogger(__name__)

PROJECT_ROOT = Path(__file__).parent.parent
VENV_PYTHON = str(PROJECT_ROOT / '.venv' / 'bin' / 'python3')

# Simple in-memory cache for web searches
_search_cache: dict = {}
CACHE_TTL_SECONDS = 300  # 5 minutes


def _get_cached(query: str) -> Optional[list]:
    """Get cached search results if not expired."""
    if query in _search_cache:
        timestamp, results = _search_cache[query]
        if time.time() - timestamp < CACHE_TTL_SECONDS:
            logger.debug("Cache hit for query: %s", query)
            return results
        else:
            del _search_cache[query]
    return None


def _set_cached(query: str, results: list):
    """Cache search results with timestamp."""
    _search_cache[query] = (time.time(), results)


def _deduplicate_results(results: list) -> list:
    """Remove duplicate results by URL."""
    seen = set()
    unique = []
    for r in results:
        url = r.get('url', '')
        if url and url in seen:
            continue
        seen.add(url)
        unique.append(r)
    return unique


def _score_source(url: str) -> int:
    """Simple source credibility scoring."""
    domain = urlparse(url).netloc.lower()

    high_credibility = [
        'arxiv.org', 'ieee.org', 'acm.org', 'nature.com', 'science.org',
        'wikipedia.org', 'github.com', 'docs.python.org', 'developer.mozilla.org',
        'apache.org', 'spring.io', 'kubernetes.io',
    ]
    medium_credibility = [
        'medium.com', 'dev.to', 'stackoverflow.com', 'reddit.com',
        'news.ycombinator.com',
    ]

    if any(d in domain for d in high_credibility):
        return 3
    if any(d in domain for d in medium_credibility):
        return 2
    return 1


@retry(max_attempts=2, backoff_seconds=1.0, exceptions=(Exception,))
def web_search(query: str, max_results: int = 5, use_cache: bool = True) -> str:
    """
    Search the web using DuckDuckGo with caching and deduplication.

    Args:
        query: Search query
        max_results: Number of results to return
        use_cache: Whether to use cached results

    Returns:
        JSON string with ranked, deduplicated results
    """
    cache_key = f"{query}:{max_results}"

    if use_cache:
        cached = _get_cached(cache_key)
        if cached is not None:
            return format_json_safe({
                'results': cached,
                'source': 'cache',
                'count': len(cached),
            })

    try:
        from duckduckgo_search import DDGS
        with DDGS() as ddgs:
            raw_results = list(ddgs.text(query, max_results=max_results * 2))  # Fetch extra for dedup

        formatted = [
            {
                'title': r.get('title', ''), 
                'url': r.get('href', ''), 
                'snippet': r.get('body', ''),
                'source_score': _score_source(r.get('href', '')),
            }
            for r in raw_results
        ]

        # Deduplicate and sort by credibility
        deduped = _deduplicate_results(formatted)
        deduped.sort(key=lambda x: x['source_score'], reverse=True)
        deduped = deduped[:max_results]

        if use_cache:
            _set_cached(cache_key, deduped)

        return format_json_safe({
            'results': deduped,
            'source': 'live',
            'count': len(deduped),
            'query': query,
        })

    except ImportError:
        return format_json_safe({
            'error': 'duckduckgo_search not installed. Run: pip install duckduckgo-search',
        })
    except Exception as e:
        logger.error('Web search failed: %s', e)
        return format_json_safe({'error': str(e)})

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
    return format_json_safe({'info': 'semantic memory search stub', 'query': query, 'top_k': top_k})

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
    return format_json_safe({'info': 'episodic memory search stub', 'query': query, 'top_k': top_k})

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
    return format_json_safe({'info': 'knowledge graph query stub', 'cypher': cypher_query})

query_knowledge_graph._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'cypher_query': {'type': 'string', 'description': 'Cypher query to execute against Neo4j'},
        },
        'required': ['cypher_query'],
    }
}


def execute_python(code: str, timeout: int = 30) -> str:
    """Run Python code for data analysis or verification."""
    try:
        result = subprocess.run(
            [VENV_PYTHON, '-c', code],
            capture_output=True, text=True, timeout=timeout,
        )
        return format_json_safe({
            'stdout': result.stdout[:2000],
            'stderr': result.stderr[:1000],
            'exit_code': result.returncode,
        })
    except subprocess.TimeoutExpired:
        return format_json_safe({'error': f'Execution timed out after {timeout} seconds'})
    except Exception as e:
        return format_json_safe({'error': str(e)})

execute_python._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'code': {'type': 'string', 'description': 'Python code to execute'},
            'timeout': {'type': 'integer', 'description': 'Timeout in seconds', 'default': 30},
        },
        'required': ['code'],
    }
}


def extract_facts(text: str) -> str:
    """
    Extract factual claims from text for verification.
    Stub for future NLP integration.
    """
    sentences = [s.strip() for s in text.replace('!', '.').replace('?', '.').split('.') if len(s.strip()) > 20]
    facts = []
    for s in sentences[:10]:
        # Simple heuristic: sentences with numbers, dates, or proper nouns
        if any(c.isdigit() for c in s) or any(w[0].isupper() for w in s.split()[:3]):
            facts.append(s)
    return format_json_safe({'facts': facts, 'count': len(facts)})

extract_facts._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'text': {'type': 'string', 'description': 'Text to extract facts from'},
        },
        'required': ['text'],
    }
}


RESEARCH_TOOLS = {
    'web_search': web_search,
    'search_semantic_memory': search_semantic_memory,
    'search_episodic_memory': search_episodic_memory,
    'query_knowledge_graph': query_knowledge_graph,
    'execute_python': execute_python,
    'extract_facts': extract_facts,
}


class ResearchAgent(BaseAgent):
    """Agent specialized in research, fact-finding, and information synthesis."""

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
                    {
                        'title': e.get('title', ''), 
                        'content': e.get('content', '')[:300], 
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
                    {
                        'content': e.get('content', '')[:200], 
                        'response': e.get('response', '')[:200], 
                        'timestamp': e.get('timestamp', ''),
                    }
                    for e in context.episodic
                ]
                return format_json_safe(results)
            except Exception as e:
                return format_json_safe({'error': str(e)})

        search._tool_schema = search_episodic_memory._tool_schema
        return search

    def _make_graph_query(self, memory: MemoryManager) -> callable:
        def query(cypher_query: str) -> str:
            """Run a Cypher query against the Neo4j knowledge graph for structured lookups."""
            try:
                result = memory.relational.query(cypher_query)
                return format_json_safe({
                    'entities': result.entities[:10],
                    'relationships': result.relationships[:10],
                    'records': [str(r) for r in result.raw_records[:10]],
                })
            except Exception as e:
                return format_json_safe({'error': str(e)})

        query._tool_schema = query_knowledge_graph._tool_schema
        return query

    def research(
        self,
        query: str,
        conversation_history: Optional[list[dict]] = None,
        depth: str = 'standard',
    ) -> AgentResponse:
        """Execute a research query with configurable depth."""
        extra = ''
        if depth == 'deep':
            extra = (
                'DEEP RESEARCH MODE: Be thorough and exhaustive.\n'
                '1. Search all memory layers\n'
                '2. Perform multiple web searches with different query angles\n'
                '3. Cross-reference findings across sources\n'
                '4. Provide comprehensive analysis with citations\n'
                '5. Note confidence levels and conflicting information\n'
                '6. Synthesize into structured findings'
            )
        elif depth == 'quick':
            extra = (
                'QUICK LOOKUP MODE: Be concise and direct.\n'
                '1. Check memory first\n'
                '2. Give a direct answer if found\n'
                '3. Skip full analysis structure if straightforward\n'
                '4. Limit to 2-3 sources max'
            )
        elif depth == 'verify':
            extra = (
                'FACT VERIFICATION MODE:\n'
                '1. Extract factual claims from the query\n'
                '2. Search for corroborating evidence\n'
                '3. Note source credibility scores\n'
                '4. Flag uncertain or contested claims'
            )

        return self.run(
            user_message=query,
            conversation_history=conversation_history,
            extra_context=extra,
        )

    def clear_cache(self):
        """Clear the web search cache."""
        global _search_cache
        _search_cache.clear()
        logger.info("[%s] Search cache cleared", self.agent_type)