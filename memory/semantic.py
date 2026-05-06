import hashlib
import logging
import os
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional

from pydantic import BaseModel, Field
from qdrant_client import QdrantClient
from qdrant_client.http.exceptions import UnexpectedResponse
from qdrant_client.models import (
    Distance,
    FieldCondition,
    Filter,
    MatchValue,
    PayloadSchemaType,
    PointStruct,
    VectorParams,
)
from tenacity import retry, stop_after_attempt, wait_exponential

logger = logging.getLogger(__name__)

COLLECTION_NAME = 'semantic_memory'
EMBEDDING_DIM = 768
CHUNK_SIZE = 512
CHUNK_OVERLAP = 64

SUPPORTED_EXTENSIONS = {
    '.md', '.txt', '.py', '.js', '.ts', '.yaml', '.yml',
    '.json', '.toml', '.cfg', '.ini', '.sh', '.html',
    '.css', '.sql', '.rs', '.go', '.java', '.c', '.cpp',
    '.h', '.rb', '.tex', '.csv', '.xml', '.rst',
}


class DocumentChunk(BaseModel):
    content: str
    source_path: str
    chunk_index: int
    total_chunks: int
    file_type: str
    title: str = ''
    last_modified: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))
    metadata: dict = Field(default_factory=dict)


class SemanticResult(BaseModel):
    content: str
    source_path: str
    chunk_index: int
    score: float
    file_type: str
    title: str
    last_modified: datetime


class SemanticMemory:
    def __init__(
        self,
        qdrant_url: str = 'http://localhost:6333',
        collection_name: str = COLLECTION_NAME,
        embedding_fn: Optional[callable] = None,
        chunk_size: int = CHUNK_SIZE,
        chunk_overlap: int = CHUNK_OVERLAP,
    ):
        self._client = QdrantClient(url=qdrant_url, timeout=30)
        self._collection = collection_name
        self._embed = embedding_fn
        self._chunk_size = chunk_size
        self._chunk_overlap = chunk_overlap
        self._ensure_collection()

    def _ensure_collection(self) -> None:
        try:
            self._client.get_collection(self._collection)
        except (UnexpectedResponse, Exception):
            self._client.create_collection(
                collection_name=self._collection,
                vectors_config=VectorParams(size=EMBEDDING_DIM, distance=Distance.COSINE),
            )
            self._client.create_payload_index(
                collection_name=self._collection,
                field_name='source_path',
                field_schema=PayloadSchemaType.KEYWORD,
            )
            self._client.create_payload_index(
                collection_name=self._collection,
                field_name='file_type',
                field_schema=PayloadSchemaType.KEYWORD,
            )

    def _chunk_text(self, text: str) -> list[str]:
        if len(text) <= self._chunk_size:
            return [text]
        chunks = []
        start = 0
        while start < len(text):
            end = start + self._chunk_size
            if end < len(text):
                for sep in ['\n\n', '\n', '. ', ' ']:
                    bp = text.rfind(sep, start, end)
                    if bp > start:
                        end = bp + len(sep)
                        break
            chunks.append(text[start:end].strip())
            start = end - self._chunk_overlap
        return [c for c in chunks if c]

    def _make_point_id(self, source_path: str, chunk_index: int) -> str:
        return hashlib.md5(f'{source_path}:chunk:{chunk_index}'.encode()).hexdigest()

    def _extract_title(self, content: str, file_path: str) -> str:
        for line in content.split('\n')[:10]:
            stripped = line.strip()
            if stripped.startswith('# ') and not stripped.startswith('##'):
                return stripped[2:].strip()
        return Path(file_path).stem.replace('_', ' ').title()

    @retry(stop=stop_after_attempt(3), wait=wait_exponential(multiplier=0.5, max=5), reraise=True)
    def index_file(self, file_path: str, force: bool = False) -> int:
        path = Path(file_path)
        if not path.exists():
            raise FileNotFoundError(f'File not found: {file_path}')
        if path.suffix.lower() not in SUPPORTED_EXTENSIONS:
            return 0
        try:
            content = path.read_text(encoding='utf-8')
        except UnicodeDecodeError:
            return 0
        if not content.strip():
            return 0

        title = self._extract_title(content, file_path)
        chunks = self._chunk_text(content)
        file_type = path.suffix.lstrip('.')
        last_modified = datetime.fromtimestamp(path.stat().st_mtime, tz=timezone.utc)
        file_hash = hashlib.sha256(path.read_bytes()).hexdigest()

        if self._embed is None:
            raise ValueError('No embedding function configured.')

        points = []
        for i, chunk_text in enumerate(chunks):
            vector = self._embed(chunk_text)
            points.append(PointStruct(
                id=self._make_point_id(str(path.resolve()), i),
                vector=vector,
                payload={
                    'content': chunk_text,
                    'source_path': str(path.resolve()),
                    'chunk_index': i,
                    'total_chunks': len(chunks),
                    'file_type': file_type,
                    'title': title,
                    'last_modified': last_modified.isoformat(),
                    'file_hash': file_hash,
                },
            ))

        self._client.upsert(collection_name=self._collection, points=points, wait=True)
        logger.info('Indexed %s: %d chunks.', file_path, len(chunks))
        return len(chunks)

    def index_directory(self, directory: str, recursive: bool = True, force: bool = False) -> dict:
        dir_path = Path(directory)
        if not dir_path.is_dir():
            raise NotADirectoryError(f'Not a directory: {directory}')

        stats = {'files_indexed': 0, 'chunks_total': 0, 'files_skipped': 0, 'errors': []}
        pattern = '**/*' if recursive else '*'

        for fp in sorted(dir_path.glob(pattern)):
            if not fp.is_file() or fp.name.startswith('.'):
                continue
            if fp.suffix.lower() not in SUPPORTED_EXTENSIONS:
                stats['files_skipped'] += 1
                continue
            try:
                n = self.index_file(str(fp), force=force)
                if n > 0:
                    stats['files_indexed'] += 1
                    stats['chunks_total'] += n
                else:
                    stats['files_skipped'] += 1
            except Exception as e:
                stats['errors'].append({'file': str(fp), 'error': str(e)})

        return stats

    @retry(stop=stop_after_attempt(3), wait=wait_exponential(multiplier=0.5, max=5), reraise=True)
    def search(
        self,
        query_vector: list[float],
        top_k: int = 5,
        file_type_filter: Optional[str] = None,
        source_filter: Optional[str] = None,
        score_threshold: float = 0.0,
    ) -> list[SemanticResult]:
        conditions = []
        if file_type_filter:
            conditions.append(FieldCondition(key='file_type', match=MatchValue(value=file_type_filter)))
        if source_filter:
            conditions.append(FieldCondition(key='source_path', match=MatchValue(value=source_filter)))

        search_filter = Filter(must=conditions) if conditions else None
        results = self._client.query_points(
            collection_name=self._collection,
            query=query_vector,
            limit=top_k,
            query_filter=search_filter,
            score_threshold=score_threshold,
        )
        return [
            SemanticResult(
                content=hit.payload['content'],
                source_path=hit.payload['source_path'],
                chunk_index=hit.payload.get('chunk_index', 0),
                score=hit.score,
                file_type=hit.payload.get('file_type', ''),
                title=hit.payload.get('title', ''),
                last_modified=datetime.fromisoformat(hit.payload['last_modified']),
            )
            for hit in results.points
        ]

    def remove_file(self, file_path: str) -> None:
        resolved = str(Path(file_path).resolve())
        self._client.delete(
            collection_name=self._collection,
            points_selector=Filter(
                must=[FieldCondition(key='source_path', match=MatchValue(value=resolved))]
            ),
        )

    def list_indexed_files(self) -> list[dict]:
        all_points, _ = self._client.scroll(
            collection_name=self._collection, limit=10000, with_vectors=False,
        )
        files = {}
        for p in all_points:
            src = p.payload.get('source_path', '')
            if src not in files:
                files[src] = {
                    'source_path': src,
                    'title': p.payload.get('title', ''),
                    'file_type': p.payload.get('file_type', ''),
                    'total_chunks': p.payload.get('total_chunks', 0),
                }
        return list(files.values())

    def count(self) -> int:
        return self._client.get_collection(self._collection).points_count

    def clear(self) -> None:
        self._client.delete_collection(self._collection)
        self._ensure_collection()


if __name__ == '__main__':
    import sys
    logging.basicConfig(level=logging.INFO)
    if len(sys.argv) < 3 or sys.argv[1] != '--index':
        print('Usage: python -m memory.semantic --index <directory>')
        sys.exit(1)
    try:
        import ollama as _ollama
        def _embed(text: str) -> list[float]:
            return _ollama.embeddings(model='nomic-embed-text', prompt=text)['embedding']
        mem = SemanticMemory(embedding_fn=_embed)
        stats = mem.index_directory(sys.argv[2])
        print(f'Indexed {stats["files_indexed"]} files ({stats["chunks_total"]} chunks).')
    except ImportError:
        print('ollama package required: pip install ollama')
        sys.exit(1)
