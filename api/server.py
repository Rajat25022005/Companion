import logging
from pathlib import Path
from typing import Optional

from fastapi import FastAPI, HTTPException
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import FileResponse
from pydantic import BaseModel, Field

from core.conductor import Conductor, ConductorResponse
from memory.memory_manager import MemoryManager

logger = logging.getLogger(__name__)

WORKSPACE_DIR = Path(__file__).parent.parent / 'workspace'
WORKSPACE_DIR.mkdir(exist_ok=True)

app = FastAPI(
    title='Companion API',
    description='Locally-sovereign personal AI operating system',
    version='0.1.0',
)

app.add_middleware(
    CORSMiddleware,
    allow_origins=['*'],
    allow_credentials=True,
    allow_methods=['*'],
    allow_headers=['*'],
)

_conductor: Optional[Conductor] = None
_memory: Optional[MemoryManager] = None


def get_conductor() -> Conductor:
    global _conductor, _memory
    if _conductor is None:
        try:
            import ollama as _ollama
            def embed_fn(text: str) -> list[float]:
                return _ollama.embeddings(model='nomic-embed-text', prompt=text)['embedding']
            _memory = MemoryManager(embedding_fn=embed_fn)
        except Exception as e:
            logger.warning('Memory init failed, running without memory: %s', e)
            _memory = None

        _conductor = Conductor(memory=_memory)
    return _conductor


class ChatRequest(BaseModel):
    message: str
    session_id: Optional[str] = None


class ChatResponse(BaseModel):
    content: str
    session_id: str
    turn_index: int
    intent: Optional[str] = None
    latency_ms: float = 0.0
    model: str = ''


class IndexRequest(BaseModel):
    directory: str
    recursive: bool = True


class StatsResponse(BaseModel):
    session_id: str
    turn_count: int
    memory: dict = Field(default_factory=dict)


@app.post('/chat', response_model=ChatResponse)
async def chat(request: ChatRequest):
    conductor = get_conductor()
    try:
        result = conductor.chat(request.message)
        return ChatResponse(
            content=result.content,
            session_id=result.session_id,
            turn_index=result.turn_index,
            intent=result.intent.primary if result.intent else None,
            latency_ms=result.latency_ms,
            model=result.model,
        )
    except Exception as e:
        logger.error('Chat failed: %s', e)
        raise HTTPException(status_code=500, detail=str(e))


@app.post('/index')
async def index_documents(request: IndexRequest):
    conductor = get_conductor()
    if not _memory:
        raise HTTPException(status_code=503, detail='Memory not available')
    try:
        stats = _memory.index_documents(request.directory, recursive=request.recursive)
        return stats
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))


@app.post('/reset')
async def reset_session():
    conductor = get_conductor()
    new_id = conductor.reset_session()
    return {'session_id': new_id, 'status': 'reset'}


@app.get('/stats', response_model=StatsResponse)
async def get_stats():
    conductor = get_conductor()
    memory_stats = _memory.get_stats() if _memory else {}
    return StatsResponse(
        session_id=conductor.session_id,
        turn_count=conductor.turn_count,
        memory=memory_stats,
    )


@app.get('/files/{filename}')
async def serve_file(filename: str):
    """Serve generated files (PDFs, images, docs) from the workspace directory."""
    safe_name = Path(filename).name
    file_path = WORKSPACE_DIR / safe_name

    if not file_path.exists() or not file_path.is_file():
        raise HTTPException(status_code=404, detail=f'File not found: {filename}')

    ext = file_path.suffix.lower()
    media_types = {
        '.pdf': 'application/pdf',
        '.docx': 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
        '.pptx': 'application/vnd.openxmlformats-officedocument.presentationml.presentation',
        '.png': 'image/png',
        '.jpg': 'image/jpeg',
        '.jpeg': 'image/jpeg',
        '.svg': 'image/svg+xml',
        '.csv': 'text/csv',
        '.txt': 'text/plain',
        '.json': 'application/json',
    }
    media_type = media_types.get(ext, 'application/octet-stream')

    return FileResponse(
        path=str(file_path),
        filename=safe_name,
        media_type=media_type,
    )


@app.get('/files')
async def list_files():
    """List all files in the workspace."""
    files = []
    for f in sorted(WORKSPACE_DIR.iterdir()):
        if f.is_file() and not f.name.startswith('.'):
            files.append({
                'name': f.name,
                'size': f.stat().st_size,
                'url': f'/files/{f.name}',
            })
    return {'files': files}


@app.get('/health')
async def health():
    return {'status': 'ok', 'version': '0.1.0'}
