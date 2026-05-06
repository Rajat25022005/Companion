import json
import logging
from typing import Optional

from agents.base_agent import AgentResponse, BaseAgent
from memory.memory_manager import MemoryManager

logger = logging.getLogger(__name__)


def search_semantic_memory(query: str, top_k: int = 5) -> str:
    """Find the user's past writing and related documents for style reference."""
    return json.dumps({'info': 'semantic memory search stub', 'query': query})

search_semantic_memory._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'query': {'type': 'string', 'description': 'Search query for writing samples'},
            'top_k': {'type': 'integer', 'description': 'Number of results', 'default': 5},
        },
        'required': ['query'],
    }
}


def search_episodic_memory(query: str, top_k: int = 5) -> str:
    """Recall past writing conversations and feedback from the user."""
    return json.dumps({'info': 'episodic memory search stub', 'query': query})

search_episodic_memory._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'query': {'type': 'string', 'description': 'Search query for past conversations'},
            'top_k': {'type': 'integer', 'description': 'Number of results', 'default': 5},
        },
        'required': ['query'],
    }
}


def query_knowledge_graph(cypher_query: str) -> str:
    """Look up entities, relationships, and timelines for factual grounding in writing."""
    return json.dumps({'info': 'knowledge graph query stub', 'cypher': cypher_query})

query_knowledge_graph._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'cypher_query': {'type': 'string', 'description': 'Cypher query for entity lookups'},
        },
        'required': ['cypher_query'],
    }
}


WRITER_TOOLS = {
    'search_semantic_memory': search_semantic_memory,
    'search_episodic_memory': search_episodic_memory,
    'query_knowledge_graph': query_knowledge_graph,
}


class WriterAgent(BaseAgent):
    def __init__(
        self,
        memory: Optional[MemoryManager] = None,
        tools: Optional[dict[str, callable]] = None,
        **kwargs,
    ):
        effective_tools = {**WRITER_TOOLS}
        if tools:
            effective_tools.update(tools)

        if memory:
            effective_tools['search_semantic_memory'] = self._make_semantic_search(memory)
            effective_tools['search_episodic_memory'] = self._make_episodic_search(memory)
            effective_tools['query_knowledge_graph'] = self._make_graph_query(memory)

        super().__init__(memory=memory, tools=effective_tools, **kwargs)

    @property
    def agent_type(self) -> str:
        return 'writer'

    @property
    def skill_name(self) -> str:
        return 'write'

    @property
    def memory_layers(self) -> list[str]:
        return ['episodic', 'semantic', 'relational']

    def get_available_tools(self) -> list[str]:
        return list(self._tools.keys())

    def _make_semantic_search(self, memory: MemoryManager) -> callable:
        def search(query: str, top_k: int = 5) -> str:
            """Find the user's past writing and related documents for style reference."""
            try:
                context = memory.retrieve(query=query, layers=['semantic'], top_k=top_k)
                results = [
                    {'title': e.get('title', ''), 'content': e.get('content', '')[:400], 'source': e.get('source_path', '')}
                    for e in context.semantic
                ]
                return json.dumps(results, default=str)
            except Exception as e:
                return json.dumps({'error': str(e)})

        search._tool_schema = search_semantic_memory._tool_schema
        return search

    def _make_episodic_search(self, memory: MemoryManager) -> callable:
        def search(query: str, top_k: int = 5) -> str:
            """Recall past writing conversations and feedback from the user."""
            try:
                context = memory.retrieve(query=query, layers=['episodic'], top_k=top_k)
                results = [
                    {'content': e.get('content', '')[:200], 'response': e.get('response', '')[:300], 'timestamp': e.get('timestamp', '')}
                    for e in context.episodic
                ]
                return json.dumps(results, default=str)
            except Exception as e:
                return json.dumps({'error': str(e)})

        search._tool_schema = search_episodic_memory._tool_schema
        return search

    def _make_graph_query(self, memory: MemoryManager) -> callable:
        def query(cypher_query: str) -> str:
            """Look up entities, relationships, and timelines for factual grounding in writing."""
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

    def write(
        self,
        request: str,
        conversation_history: Optional[list[dict]] = None,
        genre: str = 'general',
    ) -> AgentResponse:
        extra = ''
        if genre == 'technical':
            extra = (
                'This is technical documentation. Use precise terminology, concrete examples, '
                'and proper heading hierarchy. Prefer showing over telling.'
            )
        elif genre == 'blog':
            extra = (
                'This is a blog post. Use a conversational-professional tone, strong opening, '
                'clear sections, and a conclusion that lands. Keep paragraphs short.'
            )
        elif genre == 'academic':
            extra = (
                'This is academic writing. Maintain formal register, cite sources from memory '
                'when available, use precise language, and follow standard paper structure.'
            )
        elif genre == 'edit':
            extra = (
                'This is an editing request. Summarize what you changed and why before '
                'presenting the revised text. Preserve the author\'s voice and core argument.'
            )

        return self.run(
            user_message=request,
            conversation_history=conversation_history,
            extra_context=extra,
        )
