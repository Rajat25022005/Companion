"""Production-grade writer agent with style analysis, grammar checking, and citation management."""
import json
import logging
import re
from typing import Optional

from agents.base_agent import AgentResponse, BaseAgent
from agents.shared_utils import format_json_safe
from memory.memory_manager import MemoryManager

logger = logging.getLogger(__name__)


def search_semantic_memory(query: str, top_k: int = 5) -> str:
    """Find the user's past writing and related documents for style reference."""
    return format_json_safe({'info': 'semantic memory search stub', 'query': query})

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
    return format_json_safe({'info': 'episodic memory search stub', 'query': query})

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
    return format_json_safe({'info': 'knowledge graph query stub', 'cypher': cypher_query})

query_knowledge_graph._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'cypher_query': {'type': 'string', 'description': 'Cypher query for entity lookups'},
        },
        'required': ['cypher_query'],
    }
}


def analyze_style(text: str) -> str:
    """
    Analyze writing style metrics from sample text.
    Returns readability scores, tone indicators, and vocabulary stats.
    """
    try:
        sentences = [s.strip() for s in re.split(r'[.!?]+', text) if s.strip()]
        words = text.split()

        avg_sentence_length = len(words) / max(len(sentences), 1)
        avg_word_length = sum(len(w) for w in words) / max(len(words), 1)

        # Simple readability (Flesch Reading Ease approximation)
        flesch = 206.835 - 1.015 * avg_sentence_length - 84.6 * avg_word_length

        # Tone indicators
        formal_markers = ['furthermore', 'therefore', 'consequently', 'nevertheless', 
                         'however', 'additionally', 'specifically', 'regarding']
        casual_markers = ['actually', 'basically', 'honestly', 'literally', 
                         'pretty', 'really', 'totally', 'kind of']

        formal_count = sum(1 for m in formal_markers if m in text.lower())
        casual_count = sum(1 for m in casual_markers if m in text.lower())

        tone = 'formal' if formal_count > casual_count else 'casual' if casual_count > formal_count else 'neutral'

        return format_json_safe({
            'metrics': {
                'word_count': len(words),
                'sentence_count': len(sentences),
                'avg_sentence_length': round(avg_sentence_length, 1),
                'avg_word_length': round(avg_word_length, 2),
                'flesch_reading_ease': round(flesch, 1),
            },
            'tone': {
                'classification': tone,
                'formal_markers': formal_count,
                'casual_markers': casual_count,
            },
            'readability_level': 'easy' if flesch > 80 else 'standard' if flesch > 50 else 'difficult',
        })
    except Exception as e:
        return format_json_safe({'error': str(e)})

analyze_style._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'text': {'type': 'string', 'description': 'Text sample to analyze'},
        },
        'required': ['text'],
    }
}


def check_grammar(text: str) -> str:
    """
    Basic grammar and style checks.
    Returns list of potential issues with suggestions.
    """
    issues = []

    # Passive voice detection (simple heuristic)
    passive_patterns = [
        (r'\b(?:am|is|are|was|were|be|been|being)\s+\w+ed\b', 'Possible passive voice'),
        (r'\b(?:has|have|had)\s+been\s+\w+ed\b', 'Possible passive voice (perfect)'),
    ]

    for pattern, message in passive_patterns:
        matches = re.finditer(pattern, text, re.IGNORECASE)
        for match in matches:
            issues.append({
                'type': 'style',
                'message': message,
                'text': match.group(),
                'position': match.start(),
                'suggestion': 'Consider active voice for stronger writing',
            })

    # Repeated words
    repeated = re.finditer(r'\b(\w+)\s+\1\b', text, re.IGNORECASE)
    for match in repeated:
        issues.append({
            'type': 'grammar',
            'message': 'Repeated word',
            'text': match.group(),
            'position': match.start(),
            'suggestion': f'Remove duplicate "{match.group(1)}"',
        })

    # Long sentences
    sentences = re.split(r'[.!?]+', text)
    pos = 0
    for sent in sentences:
        words = sent.split()
        if len(words) > 40:
            issues.append({
                'type': 'readability',
                'message': 'Very long sentence',
                'text': sent.strip()[:50] + '...',
                'position': pos,
                'suggestion': f'Consider breaking into 2-3 sentences ({len(words)} words)',
            })
        pos += len(sent) + 1

    return format_json_safe({
        'issues': issues,
        'issue_count': len(issues),
        'categories': list(set(i['type'] for i in issues)),
    })

check_grammar._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'text': {'type': 'string', 'description': 'Text to check for grammar and style issues'},
        },
        'required': ['text'],
    }
}


def generate_outline(topic: str, sections: int = 5, genre: str = 'general') -> str:
    """
    Generate a structured outline for a writing project.
    Returns hierarchical outline with section descriptions.
    """
    return format_json_safe({
        'topic': topic,
        'genre': genre,
        'requested_sections': sections,
        'note': 'This is a stub. The LLM should generate the actual outline in its response.',
        'suggested_structure': {
            'introduction': 'Hook and thesis statement',
            'body': f'{sections - 2} main sections with supporting points',
            'conclusion': 'Synthesis and call to action',
        }
    })

generate_outline._tool_schema = {
    'parameters': {
        'type': 'object',
        'properties': {
            'topic': {'type': 'string', 'description': 'Writing topic or title'},
            'sections': {'type': 'integer', 'description': 'Number of body sections', 'default': 5},
            'genre': {'type': 'string', 'description': 'Writing genre', 'default': 'general'},
        },
        'required': ['topic'],
    }
}


WRITER_TOOLS = {
    'search_semantic_memory': search_semantic_memory,
    'search_episodic_memory': search_episodic_memory,
    'query_knowledge_graph': query_knowledge_graph,
    'analyze_style': analyze_style,
    'check_grammar': check_grammar,
    'generate_outline': generate_outline,
}


class WriterAgent(BaseAgent):
    """Agent specialized in writing, editing, and content creation."""

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

        # Track user's style preferences from past writing
        self._style_profile: Optional[dict] = None

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
                    {
                        'title': e.get('title', ''), 
                        'content': e.get('content', '')[:400], 
                        'source': e.get('source_path', ''),
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
            """Recall past writing conversations and feedback from the user."""
            try:
                context = memory.retrieve(query=query, layers=['episodic'], top_k=top_k)
                results = [
                    {
                        'content': e.get('content', '')[:200], 
                        'response': e.get('response', '')[:300], 
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
            """Look up entities, relationships, and timelines for factual grounding in writing."""
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

    def write(
        self,
        request: str,
        conversation_history: Optional[list[dict]] = None,
        genre: str = 'general',
        word_count: Optional[int] = None,
    ) -> AgentResponse:
        """Execute a writing task with genre-specific guidance."""
        extra = ''

        if genre == 'technical':
            extra = (
                'TECHNICAL WRITING MODE:\n'
                '- Use precise terminology and define acronyms on first use\n'
                '- Include concrete examples and code snippets where relevant\n'
                '- Follow proper heading hierarchy (H1 > H2 > H3)\n'
                '- Prefer showing over telling\n'
                '- Use active voice for procedures\n'
                '- Include diagrams or tables for complex concepts'
            )
        elif genre == 'blog':
            extra = (
                'BLOG WRITING MODE:\n'
                '- Use conversational-professional tone\n'
                '- Strong hook in the first 2 sentences\n'
                '- Short paragraphs (2-3 sentences max)\n'
                '- Clear sections with descriptive headers\n'
                '- Include a conclusion that lands with impact\n'
                '- Use rhetorical questions sparingly for engagement'
            )
        elif genre == 'academic':
            extra = (
                'ACADEMIC WRITING MODE:\n'
                '- Maintain formal register throughout\n'
                '- Cite sources from memory when available\n'
                '- Use hedging language appropriately ("suggests", "indicates")\n'
                '- Follow IMRAD structure if applicable\n'
                '- Avoid first person unless discipline permits\n'
                '- Include methodology and limitations discussion'
            )
        elif genre == 'edit':
            extra = (
                'EDITING MODE:\n'
                '- Summarize what you changed and why before presenting revised text\n'
                '- Preserve the author\'s voice and core argument\n'
                '- Check grammar, clarity, flow, and consistency\n'
                '- Flag structural issues, not just surface errors\n'
                '- Provide both tracked changes and clean version if possible'
            )
        elif genre == 'creative':
            extra = (
                'CREATIVE WRITING MODE:\n'
                '- Focus on sensory details and vivid imagery\n'
                '- Vary sentence structure for rhythm\n'
                '- Show emotion through action and dialogue\n'
                '- Maintain consistent point of view\n'
                '- Use metaphor and simile with restraint'
            )

        if word_count:
            extra += f"\n\nTARGET LENGTH: Approximately {word_count} words."

        return self.run(
            user_message=request,
            conversation_history=conversation_history,
            extra_context=extra,
        )

    def analyze_user_style(self, sample_text: str) -> dict:
        """Analyze and store user's writing style from a sample."""
        result = analyze_style(sample_text)
        try:
            self._style_profile = json.loads(result)
            logger.info("[%s] Style profile updated", self.agent_type)
        except json.JSONDecodeError:
            pass
        return self._style_profile or {}