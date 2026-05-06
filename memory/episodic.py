import hashlib
import logging
import time
import uuid
from datetime import datetime, timezone
from typing import Optional

from pydantic import BaseModel, Field
from qdrant_client import QdrantClient
from qdrant_client.http.exceptions import UnexpectedResponse
from qdrant_client.models import (
    Distance,
    FieldCondition,
    Filter,
    MatchValue,
    PointStruct,
    VectorParams,
)
from tenacity import retry, stop_after_attempt, wait_exponential

logger = logging.getLogger(__name__)

COLLECTION_NAME = 'episodic_memory'
EMBEDDING_DIM = 768


class ConversationTurn(BaseModel):
    role: str
    content: str
    response: str
    timestamp: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))
    session_id: str = ''
    turn_index: int = 0
    metadata: dict = Field(default_factory=dict)


class EpisodicResult(BaseModel):
    content: str
    response: str
    role: str
    score: float
    timestamp: datetime
    session_id: str
    turn_index: int


class EpisodicMemory:
    def __init__(
        self,
        qdrant_url: str = 'http://localhost:6333',
        collection_name: str = COLLECTION_NAME,
        embedding_fn: Optional[callable] = None,
    ):
        self._client = QdrantClient(url=qdrant_url, timeout=30)
        self._collection = collection_name
        self._embed = embedding_fn
        self._ensure_collection()

    def _ensure_collection(self) -> None:
        try:
            self._client.get_collection(self._collection)
            logger.info('Episodic collection "%s" already exists.', self._collection)
        except (UnexpectedResponse, Exception):
            self._client.create_collection(
                collection_name=self._collection,
                vectors_config=VectorParams(
                    size=EMBEDDING_DIM,
                    distance=Distance.COSINE,
                ),
            )
            logger.info('Created episodic collection "%s".', self._collection)

    def _make_point_id(self, turn: ConversationTurn) -> str:
        raw = f'{turn.session_id}:{turn.turn_index}:{turn.content[:100]}'
        return hashlib.md5(raw.encode()).hexdigest()

    def _to_embedding_text(self, turn: ConversationTurn) -> str:
        text = f'{turn.role}: {turn.content}\nassistant: {turn.response}'
        # nomic-embed-text has ~8192 token limit; truncate to ~1500 tokens (6000 chars)
        if len(text) > 6000:
            text = text[:6000]
        return text

    @retry(
        stop=stop_after_attempt(3),
        wait=wait_exponential(multiplier=0.5, max=5),
        reraise=True,
    )
    def store(self, turn: ConversationTurn, vector: Optional[list[float]] = None) -> str:
        if vector is None:
            if self._embed is None:
                raise ValueError('No embedding function configured and no vector provided.')
            vector = self._embed(self._to_embedding_text(turn))

        point_id = self._make_point_id(turn)

        point = PointStruct(
            id=point_id,
            vector=vector,
            payload={
                'role': turn.role,
                'content': turn.content,
                'response': turn.response,
                'timestamp': turn.timestamp.isoformat(),
                'session_id': turn.session_id,
                'turn_index': turn.turn_index,
                'metadata': turn.metadata,
            },
        )

        self._client.upsert(
            collection_name=self._collection,
            points=[point],
            wait=True,
        )

        logger.debug('Stored episodic turn %s (session=%s, turn=%d)', point_id, turn.session_id, turn.turn_index)
        return point_id

    def store_batch(self, turns: list[ConversationTurn], vectors: Optional[list[list[float]]] = None) -> list[str]:
        if vectors is None:
            if self._embed is None:
                raise ValueError('No embedding function configured and no vectors provided.')
            texts = [self._to_embedding_text(t) for t in turns]
            vectors = [self._embed(text) for text in texts]

        if len(turns) != len(vectors):
            raise ValueError(f'Mismatch: {len(turns)} turns but {len(vectors)} vectors.')

        points = []
        point_ids = []
        for turn, vec in zip(turns, vectors):
            pid = self._make_point_id(turn)
            point_ids.append(pid)
            points.append(
                PointStruct(
                    id=pid,
                    vector=vec,
                    payload={
                        'role': turn.role,
                        'content': turn.content,
                        'response': turn.response,
                        'timestamp': turn.timestamp.isoformat(),
                        'session_id': turn.session_id,
                        'turn_index': turn.turn_index,
                        'metadata': turn.metadata,
                    },
                )
            )

        self._client.upsert(
            collection_name=self._collection,
            points=points,
            wait=True,
        )

        logger.info('Stored batch of %d episodic turns.', len(points))
        return point_ids

    @retry(
        stop=stop_after_attempt(3),
        wait=wait_exponential(multiplier=0.5, max=5),
        reraise=True,
    )
    def retrieve(
        self,
        query_vector: list[float],
        top_k: int = 5,
        session_filter: Optional[str] = None,
        score_threshold: float = 0.0,
    ) -> list[EpisodicResult]:
        search_filter = None
        if session_filter:
            search_filter = Filter(
                must=[
                    FieldCondition(
                        key='session_id',
                        match=MatchValue(value=session_filter),
                    )
                ]
            )

        results = self._client.query_points(
            collection_name=self._collection,
            query=query_vector,
            limit=top_k,
            query_filter=search_filter,
            score_threshold=score_threshold,
        )

        return [
            EpisodicResult(
                content=hit.payload['content'],
                response=hit.payload['response'],
                role=hit.payload['role'],
                score=hit.score,
                timestamp=datetime.fromisoformat(hit.payload['timestamp']),
                session_id=hit.payload.get('session_id', ''),
                turn_index=hit.payload.get('turn_index', 0),
            )
            for hit in results.points
        ]

    def get_recent(self, limit: int = 10, session_id: Optional[str] = None) -> list[EpisodicResult]:
        scroll_filter = None
        if session_id:
            scroll_filter = Filter(
                must=[
                    FieldCondition(
                        key='session_id',
                        match=MatchValue(value=session_id),
                    )
                ]
            )

        points, _ = self._client.scroll(
            collection_name=self._collection,
            scroll_filter=scroll_filter,
            limit=limit,
            with_vectors=False,
            order_by='timestamp',
        )

        results = [
            EpisodicResult(
                content=p.payload['content'],
                response=p.payload['response'],
                role=p.payload['role'],
                score=1.0,
                timestamp=datetime.fromisoformat(p.payload['timestamp']),
                session_id=p.payload.get('session_id', ''),
                turn_index=p.payload.get('turn_index', 0),
            )
            for p in points
        ]

        results.sort(key=lambda r: r.timestamp, reverse=True)
        return results[:limit]

    def count(self) -> int:
        info = self._client.get_collection(self._collection)
        return info.points_count

    def delete_session(self, session_id: str) -> int:
        self._client.delete(
            collection_name=self._collection,
            points_selector=Filter(
                must=[
                    FieldCondition(
                        key='session_id',
                        match=MatchValue(value=session_id),
                    )
                ]
            ),
        )
        logger.info('Deleted all turns for session %s.', session_id)
        return 0

    def clear(self) -> None:
        self._client.delete_collection(self._collection)
        self._ensure_collection()
        logger.warning('Cleared entire episodic memory collection.')
