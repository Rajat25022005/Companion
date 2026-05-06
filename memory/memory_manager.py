import logging
from datetime import datetime, timezone
from typing import Optional

from pydantic import BaseModel, Field

from memory.episodic import ConversationTurn, EpisodicMemory, EpisodicResult
from memory.relational import Entity, GraphResult, Relationship, RelationalMemory
from memory.semantic import SemanticMemory, SemanticResult

logger = logging.getLogger(__name__)

VALID_LAYERS = {'episodic', 'semantic', 'relational'}


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
        self._embed = embedding_fn
        self._episodic = EpisodicMemory(
            qdrant_url=qdrant_url,
            embedding_fn=embedding_fn,
        )
        self._semantic = SemanticMemory(
            qdrant_url=qdrant_url,
            embedding_fn=embedding_fn,
        )
        self._relational = RelationalMemory(
            uri=neo4j_uri,
            user=neo4j_user,
            password=neo4j_password,
        )
        logger.info('MemoryManager initialized with all three layers.')

    @property
    def episodic(self) -> EpisodicMemory:
        return self._episodic

    @property
    def semantic(self) -> SemanticMemory:
        return self._semantic

    @property
    def relational(self) -> RelationalMemory:
        return self._relational

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

        context = MemoryContext(query=query, layers_queried=layers)

        query_vector = None
        if ('episodic' in layers or 'semantic' in layers) and self._embed:
            query_vector = self._embed(query)

        if 'episodic' in layers and query_vector:
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

        if 'semantic' in layers and query_vector:
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

        if 'relational' in layers:
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
                logger.error('Relational retrieval failed: %s', e)

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
        return self._semantic.index_directory(directory, recursive=recursive)

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
            stats['relational'] = self._relational.stats()
        except Exception:
            stats['relational'] = {}
        return stats

    def close(self) -> None:
        self._relational.close()

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        self.close()
