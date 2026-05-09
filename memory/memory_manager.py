import logging
import threading
import time
from collections import OrderedDict
from datetime import datetime, timezone
from typing import Optional

from pydantic import BaseModel, Field

from memory.episodic import ConversationTurn, EpisodicMemory, EpisodicResult
from memory.relational import Entity, GraphResult, Relationship, RelationalMemory
from memory.semantic import SemanticMemory, SemanticResult

logger = logging.getLogger(__name__)

VALID_LAYERS = {'episodic', 'semantic', 'relational'}


class _EmbeddingCache:
    """Thread-safe LRU cache for embedding vectors."""
    def __init__(self, embed_fn, maxsize: int = 64, ttl: float = 300.0):
        self._embed_fn = embed_fn
        self._maxsize = maxsize
        self._ttl = ttl
        self._cache: OrderedDict[str, tuple[list[float], float]] = OrderedDict()
        self._lock = threading.Lock()

    def __call__(self, text: str) -> list[float]:
        now = time.monotonic()
        # Normalize whitespace for cache key
        key = text.strip()[:2000]
        with self._lock:
            if key in self._cache:
                vec, ts = self._cache[key]
                if now - ts < self._ttl:
                    self._cache.move_to_end(key)
                    return vec
                else:
                    del self._cache[key]
        # Cache miss — compute
        vec = self._embed_fn(text)
        with self._lock:
            self._cache[key] = (vec, now)
            if len(self._cache) > self._maxsize:
                self._cache.popitem(last=False)
        return vec


class MemoryContext(BaseModel):
    episodic: list[dict] = Field(default_factory=list)
    semantic: list[dict] = Field(default_factory=list)
    relational: list[dict] = Field(default_factory=list)
    query: str = ''
    layers_queried: list[str] = Field(default_factory=list)


class MemoryManager:
    def __init__(
        self,
        qdrant_url: str = 'http://localhost:6333',
        neo4j_uri: str = 'bolt://localhost:7687',
        neo4j_user: str = 'neo4j',
        neo4j_password: str = 'companion',
        embedding_fn: Optional[callable] = None,
    ):
        # Wrap embedding function with cache to avoid redundant Ollama calls
        self._raw_embed = embedding_fn
        self._embed = _EmbeddingCache(embedding_fn) if embedding_fn else None
        self._episodic = EpisodicMemory(
            qdrant_url=qdrant_url,
            embedding_fn=self._embed,
        )
        self._semantic = SemanticMemory(
            qdrant_url=qdrant_url,
            embedding_fn=self._embed,
        )

        # Relational layer with circuit breaker
        self._relational_available = True
        try:
            self._relational = RelationalMemory(
                uri=neo4j_uri,
                user=neo4j_user,
                password=neo4j_password,
            )
        except Exception as e:
            logger.warning('Neo4j unavailable, disabling relational memory: %s', e)
            self._relational = None
            self._relational_available = False

        # Cache layer emptiness to skip unnecessary queries
        self._layer_counts: dict[str, int] = {}
        self._layer_counts_ts: float = 0
        logger.info('MemoryManager initialized (relational=%s).', 'ok' if self._relational_available else 'disabled')

    @property
    def episodic(self) -> EpisodicMemory:
        return self._episodic

    @property
    def semantic(self) -> SemanticMemory:
        return self._semantic

    @property
    def relational(self) -> Optional[RelationalMemory]:
        return self._relational

    def _refresh_layer_counts(self) -> None:
        """Refresh cached counts every 60 seconds."""
        now = time.monotonic()
        if now - self._layer_counts_ts < 60:
            return
        self._layer_counts_ts = now
        try:
            self._layer_counts['episodic'] = self._episodic.count()
        except Exception:
            self._layer_counts['episodic'] = -1
        try:
            self._layer_counts['semantic'] = self._semantic.count()
        except Exception:
            self._layer_counts['semantic'] = -1

    def retrieve(
        self,
        query: str,
        layers: Optional[list[str]] = None,
        top_k: int = 5,
        session_filter: Optional[str] = None,
        file_type_filter: Optional[str] = None,
    ) -> MemoryContext:
        if layers is None:
            layers = list(VALID_LAYERS)

        for layer in layers:
            if layer not in VALID_LAYERS:
                raise ValueError(f'Invalid layer: {layer}. Must be one of {VALID_LAYERS}')

        # Refresh cached counts to skip empty layers
        self._refresh_layer_counts()

        context = MemoryContext(query=query, layers_queried=layers)

        query_vector = None
        if ('episodic' in layers or 'semantic' in layers) and self._embed:
            query_vector = self._embed(query)

        # Skip episodic if collection is known-empty
        if 'episodic' in layers and query_vector and self._layer_counts.get('episodic', -1) != 0:
            try:
                results = self._episodic.retrieve(
                    query_vector=query_vector,
                    top_k=top_k,
                    session_filter=session_filter,
                )
                context.episodic = [
                    {
                        'content': r.content,
                        'response': r.response,
                        'role': r.role,
                        'score': r.score,
                        'timestamp': r.timestamp.isoformat(),
                        'session_id': r.session_id,
                    }
                    for r in results
                ]
            except Exception as e:
                logger.error('Episodic retrieval failed: %s', e)

        # Skip semantic if collection is known-empty
        if 'semantic' in layers and query_vector and self._layer_counts.get('semantic', -1) != 0:
            try:
                results = self._semantic.search(
                    query_vector=query_vector,
                    top_k=top_k,
                    file_type_filter=file_type_filter,
                )
                context.semantic = [
                    {
                        'content': r.content,
                        'source_path': r.source_path,
                        'score': r.score,
                        'title': r.title,
                        'file_type': r.file_type,
                    }
                    for r in results
                ]
            except Exception as e:
                logger.error('Semantic retrieval failed: %s', e)

        # Skip relational if circuit breaker tripped or Neo4j unavailable
        if 'relational' in layers and self._relational_available and self._relational:
            try:
                search_results = self._relational.search_entities(query, limit=top_k)
                entities_data = []
                relationships_data = []

                for entity in search_results[:3]:
                    neighbors = self._relational.get_neighbors(entity['name'])
                    entities_data.append(entity)
                    relationships_data.extend(neighbors.relationships)

                context.relational = [
                    {
                        'entities': entities_data,
                        'relationships': relationships_data,
                    }
                ]
            except Exception as e:
                logger.error('Relational retrieval failed, disabling for session: %s', e)
                self._relational_available = False

        return context

    def store(
        self,
        turn: dict,
        entities: Optional[list[dict]] = None,
        relationships: Optional[list[dict]] = None,
        session_id: str = '',
        turn_index: int = 0,
    ) -> dict:
        result = {'episodic_id': None, 'entities_stored': 0, 'relationships_stored': 0}

        conv_turn = ConversationTurn(
            role=turn.get('role', 'user'),
            content=turn.get('content', ''),
            response=turn.get('response', ''),
            session_id=session_id,
            turn_index=turn_index,
            metadata=turn.get('metadata', {}),
        )

        try:
            point_id = self._episodic.store(conv_turn)
            result['episodic_id'] = point_id
        except Exception as e:
            logger.error('Failed to store episodic turn: %s', e)

        if entities:
            try:
                entity_objects = [
                    Entity(
                        name=e['name'],
                        entity_type=e.get('type', e.get('entity_type', 'Concept')),
                        properties=e.get('properties', {}),
                    )
                    for e in entities
                ]
                result['entities_stored'] = self._relational.upsert_entities_batch(entity_objects)
            except Exception as e:
                logger.error('Failed to store entities: %s', e)

        if relationships:
            try:
                rel_objects = [
                    Relationship(
                        source=r['source'],
                        source_type=r.get('source_type', 'Concept'),
                        target=r['target'],
                        target_type=r.get('target_type', 'Concept'),
                        relation=r.get('relation', 'RELATED_TO'),
                        properties=r.get('properties', {}),
                    )
                    for r in relationships
                ]
                result['relationships_stored'] = self._relational.add_relationships_batch(rel_objects)
            except Exception as e:
                logger.error('Failed to store relationships: %s', e)

        return result

    def format_context_for_prompt(self, context: MemoryContext) -> str:
        sections = []

        if context.episodic:
            lines = ['[Episodic]']
            for entry in context.episodic:
                ts = entry.get('timestamp', '')
                content = entry.get('content', '')[:150]
                lines.append(f'- ({ts}) {content}')
            sections.append('\n'.join(lines))

        if context.semantic:
            lines = ['[Semantic]']
            for entry in context.semantic:
                title = entry.get('title', 'untitled')
                content = entry.get('content', '')[:200]
                lines.append(f'- From {title}: {content}')
            sections.append('\n'.join(lines))

        if context.relational:
            lines = ['[Relational]']
            for group in context.relational:
                for entity in group.get('entities', []):
                    lines.append(f'- {entity.get("name")} ({entity.get("type", "")})')
                for rel in group.get('relationships', []):
                    lines.append(
                        f'- {rel.get("source")} --{rel.get("relation")}--> {rel.get("target")}'
                    )
            sections.append('\n'.join(lines))

        if not sections:
            return ''

        return '--- MEMORY CONTEXT ---\n\n' + '\n\n'.join(sections) + '\n\n--- END MEMORY CONTEXT ---'

    def index_documents(self, directory: str, recursive: bool = True) -> dict:
        stats = self._semantic.index_directory(directory, recursive=recursive)
        # Invalidate count cache so new documents are found immediately
        self._layer_counts_ts = 0
        return stats

    def get_stats(self) -> dict:
        stats = {}
        try:
            stats['episodic_count'] = self._episodic.count()
        except Exception:
            stats['episodic_count'] = -1
        try:
            stats['semantic_count'] = self._semantic.count()
        except Exception:
            stats['semantic_count'] = -1
        try:
            if self._relational_available and self._relational:
                stats['relational'] = self._relational.stats()
            else:
                stats['relational'] = {'status': 'disabled'}
        except Exception:
            stats['relational'] = {}
        return stats

    def close(self) -> None:
        if self._relational:
            self._relational.close()

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        self.close()
