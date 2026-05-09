import logging
from pathlib import Path
from typing import Optional

from fastapi import FastAPI, HTTPException, UploadFile, File
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


import asyncio
import uuid
from fastapi.responses import StreamingResponse
import json

active_tasks: dict[str, asyncio.Queue] = {}

class AsyncChatResponse(BaseModel):
    task_id: str


@app.post('/chat/async', response_model=AsyncChatResponse)
async def chat_async_endpoint(request: ChatRequest):
    conductor = get_conductor()
    if request.session_id and conductor.session_id != request.session_id:
        try:
            conductor.load_session(request.session_id)
        except Exception as e:
            logger.warning('Could not load session %s: %s', request.session_id, e)
    task_id = str(uuid.uuid4())
    queue = asyncio.Queue()
    active_tasks[task_id] = queue
    loop = asyncio.get_running_loop()

    def event_callback(event_data: dict):
        loop.call_soon_threadsafe(queue.put_nowait, event_data)

    def run_chat():
        try:
            event_callback({'type': 'task_start', 'task_id': task_id})
            result = conductor.chat(request.message, event_callback=event_callback)
            event_callback({
                'type': 'complete',
                'content': result.content,
                'intent': result.intent.primary if result.intent else None,
                'latency_ms': result.latency_ms,
                'model': result.model,
            })
        except Exception as e:
            logger.error('Chat async failed: %s', e)
            event_callback({'type': 'error', 'error': str(e)})
        finally:
            loop.call_soon_threadsafe(queue.put_nowait, None) # EOF marker

    # Run in background thread so we don't block the event loop
    asyncio.create_task(asyncio.to_thread(run_chat))

    return AsyncChatResponse(task_id=task_id)


@app.get('/tasks/{task_id}/stream')
async def stream_task(task_id: str):
    if task_id not in active_tasks:
        raise HTTPException(status_code=404, detail="Task not found")
    
    queue = active_tasks[task_id]

    async def event_generator():
        try:
            while True:
                event = await queue.get()
                if event is None:
                    break
                yield f"data: {json.dumps(event)}\n\n"
        finally:
            if task_id in active_tasks:
                del active_tasks[task_id]

    return StreamingResponse(event_generator(), media_type="text/event-stream")

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


@app.post('/upload')
async def upload_file(file: UploadFile = File(...)):
    """Save an uploaded file to the workspace and automatically index it."""
    get_conductor()  # Ensure memory is initialized
    
    file_path = WORKSPACE_DIR / file.filename
    # Save the file to disk
    with open(file_path, "wb") as f:
        f.write(await file.read())
        
    stats = {}
    if _memory:
        try:
            # We index the specific file by passing its parent directory, but memory manager
            # index_documents expects a directory. 
            # We should probably modify index_documents to accept a file or just index the whole workspace.
            # For simplicity, we just index the workspace non-recursively.
            stats = _memory.index_documents(str(WORKSPACE_DIR), recursive=False)
        except Exception as e:
            logger.warning(f"Failed to auto-index uploaded file {file.filename}: {e}")
            
    return {
        "filename": file.filename,
        "path": f"/files/{file.filename}",
        "size": file_path.stat().st_size,
        "indexed": bool(stats),
        "stats": stats
    }


@app.post('/reset')
async def reset_session():
    conductor = get_conductor()
    new_id = conductor.reset_session()
    return {'session_id': new_id, 'status': 'reset'}


# ── Session persistence ────────────────────────────────────────────────────────

class SaveSessionRequest(BaseModel):
    messages: list[dict]
    title: Optional[str] = None


@app.get('/sessions')
async def list_sessions():
    return {'sessions': get_conductor().list_sessions()}


@app.post('/sessions/{session_id}/save')
async def save_session(session_id: str, request: SaveSessionRequest):
    conductor = get_conductor()
    # Switch to this session id if it matches current, otherwise ignore
    if conductor.session_id != session_id:
        # Non-active session save — load it first, save, then restore current
        current_id = conductor.session_id
        try:
            conductor.load_session(session_id)
        except FileNotFoundError:
            pass
        result = conductor.save_session(request.messages, request.title)
        # Restore to original active session
        try:
            conductor.load_session(current_id)
        except Exception:
            pass
        return result
    return conductor.save_session(request.messages, request.title)


@app.post('/sessions/{session_id}/load')
async def load_session(session_id: str):
    conductor = get_conductor()
    try:
        data = conductor.load_session(session_id)
        return {
            'session_id': data['id'],
            'title': data.get('title', ''),
            'messages': data.get('messages', []),
        }
    except FileNotFoundError:
        raise HTTPException(status_code=404, detail='Session not found')


@app.delete('/sessions/{session_id}')
async def delete_session(session_id: str):
    get_conductor().delete_session(session_id)
    return {'status': 'deleted'}




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


@app.get("/github/repos")
async def list_github_repos():
    """List user's GitHub repos using configured token."""
    import os, httpx
    token = os.environ.get("GITHUB_TOKEN", "")
    headers = {"Accept": "application/vnd.github+json"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    try:
        r = httpx.get(
            "https://api.github.com/user/repos",
            headers=headers,
            params={"sort": "updated", "per_page": 50, "type": "all"},
            timeout=10,
        )
        return {"repos": r.json()}
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))


@app.post("/github/index-repo")
async def index_repo(request: dict):
    """Clone and index a GitHub repo into semantic memory."""
    import subprocess, tempfile, shutil
    repo_url = request.get("clone_url")
    repo_name = request.get("name", "repo")
    if not repo_url:
        raise HTTPException(status_code=400, detail="clone_url required")
    if not _memory:
        raise HTTPException(status_code=503, detail="Memory not available")
    
    # Add token to URL if available
    import os
    token = os.environ.get("GITHUB_TOKEN", "")
    if token:
        repo_url = repo_url.replace("https://", f"https://{token}@")
    
    clone_dir = None
    try:
        clone_dir = tempfile.mkdtemp(prefix=f"companion_{repo_name}_")
        subprocess.run(
            ["git", "clone", "--depth=1", repo_url, clone_dir],
            capture_output=True, text=True, timeout=120, check=True
        )
        stats = _memory.index_documents(clone_dir, recursive=True)
        return {"status": "indexed", "repo": repo_name, "stats": stats}
    except subprocess.CalledProcessError as e:
        raise HTTPException(status_code=500, detail=f"Clone failed: {e.stderr}")
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))
    finally:
        if clone_dir:
            shutil.rmtree(clone_dir, ignore_errors=True)


@app.get('/health')
async def health():
    return {'status': 'ok', 'version': '0.1.0'}
