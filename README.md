# Companion

A locally-sovereign personal AI operating system. One interface, multiple specialized models, persistent memory, and plugin-based actions — all running on your own hardware via Ollama.

---

## What This Is

Companion is not a chatbot wrapper. It is a multi-agent orchestration system built around a single principle: you always talk to one model, and that model silently coordinates a team of specialists to get work done.

You type a message. The Conductor model reads it, figures out what kind of work is involved, builds a task graph, delegates to the right specialist agents, collects results, and replies to you. The specialist calls — to DeepSeek for research, Qwen-Coder for code, a document builder for files — are invisible. From your side it feels like one coherent conversation with something that actually knows how to do things.

Everything runs locally. No API keys, no data leaving your machine, no per-token costs.

---

## Architecture

### The Conductor Pattern

The system has one entry point: the Conductor model (Gemma 3 or Llama 3.1). It is the only model you ever interact with directly. Its job is not to do the work — it is to understand what work needs to be done and coordinate who does it.

```
You
 |
 v
Conductor (Gemma / Llama)
 |
 +-- Intent Parser  -->  Task Planner (LangGraph DAG)
                              |
              +---------------+---------------+
              |               |               |
         Research          Coder          Writer       Doc Builder
        (DeepSeek)     (Qwen-Coder)   (Qwen/DS)      (Gemma fast)
              |               |               |               |
              +---------------+---------------+---------------+
                                    |
                           Memory Manager
                      (reads/writes all three layers)
                                    |
              +---------------------+---------------------+
              |                     |                     |
          Episodic              Semantic             Relational
          (Qdrant)           (RAG / Qdrant)          (Neo4j)
                                    |
                           Plugin Layer
              +----------+----------+----------+----------+
              |          |          |          |          |
           Gmail     Calendar    Drive      GitHub     + More
```

### How Execution Actually Works

Models generate text. Your system executes it. This separation is absolute.

When a specialist agent needs to perform an action — write a file, run code, send an email, create a calendar event — it outputs a structured tool call (JSON). LangGraph intercepts that call, runs the registered Python function, and feeds the result back to the model. The model sees the output and continues. This loop repeats until the task is complete.

For code execution specifically, the agent generates a Python script as text, your sandbox runner executes it in an isolated subprocess, captures stdout and stderr, and returns that to the agent. The agent then reads the output, refines if needed, and produces the final result. The model never touches your filesystem directly.

```
Model output: { "tool": "execute_python", "args": { "code": "..." } }
LangGraph:    runs execute_python(code) in subprocess
Return:       { "stdout": "...", "stderr": "...", "exit_code": 0 }
Model:        reads result, continues
```

---

## Tech Stack

| Component | Technology | Notes |
|---|---|---|
| Conductor model | Gemma 3 12B / Llama 3.1 8B | Via Ollama, local |
| Research specialist | DeepSeek-R1 14B | Best open-source reasoning |
| Code specialist | Qwen2.5-Coder 14B | Purpose-built for code tasks |
| Writer specialist | Qwen2.5 14B | Long-form generation |
| Embeddings | nomic-embed-text | Ollama-native, 8192 context |
| Orchestration | LangGraph + LangChain | Multi-agent DAG execution |
| Vector store | Qdrant | Local Docker instance |
| Graph store | Neo4j | GraphRAG, relationship memory |
| Task queue | Celery + Redis | Async plugin calls |
| API server | FastAPI | Tool and plugin endpoints |
| Frontend | Streamlit | Chat UI, memory browser |
| Document generation | python-docx, fpdf2, python-pptx | Local, no external services |
| Code sandbox | subprocess + Docker (Phase 3) | Isolated execution |

---

## Memory System

Companion uses three memory layers, unified behind a single `MemoryManager` interface. Every agent reads from and writes to memory through this interface — no agent accesses any store directly.

### Episodic Memory
Stores every conversation turn. When you ask something, the memory manager retrieves the top-k most semantically similar past exchanges and prepends them to the agent's context. This is what makes the system remember that you prefer FastAPI over Flask, or that you were debugging a specific issue last Tuesday.

- Store: Qdrant
- Indexed by: nomic-embed-text embeddings
- Retrieval: cosine similarity, top-5 per query
- Written: after every conversation turn

### Semantic Memory
A RAG corpus built from your own documents — research notes, paper PDFs, markdown files, code comments, exported browser bookmarks. The research agent queries this before hitting the web. The writer agent queries it for your past writing style.

- Store: Qdrant (separate collection)
- Indexed by: nomic-embed-text embeddings
- Sources: any folder you point it at, re-indexed on change
- Retrieval: similarity search with metadata filtering

### Relational Memory (GraphRAG)
A knowledge graph that tracks entities and their relationships across conversations. People, projects, concepts, tools, and the connections between them. This is what lets the system know that Hypnos is your current project, that it uses Mamba and JEPA, and that it targets NeurIPS.

- Store: Neo4j
- Nodes: Person, Project, Concept, Tool, File, Event
- Edges: WORKS_ON, USES, RELATED_TO, MENTIONED_WITH, CREATED_BY
- Updated: after each session via an extraction pass over the conversation

### Memory Manager Interface

```python
memory = MemoryManager()

# read — used by all agents before generating
context = memory.retrieve(
    query="user's current research project",
    layers=["episodic", "semantic", "relational"],
    top_k=5
)

# write — used by conductor after each turn
memory.store(
    turn={"role": "user", "content": "...", "response": "..."},
    entities=extracted_entities
)
```

---

## Skill System

Skills are markdown files. Each file describes how a specialist agent should behave for a particular task type — what model to use, what tools are available, what the user's preferences are, and what output format is expected.

When the Conductor routes a task, the orchestrator loads the relevant skill file, injects it as the agent's system prompt prefix, pulls memory context, and calls the model. Swapping a skill is as simple as editing a text file.

```
skills/
├── research.md
├── code.md
├── write.md
├── document.md
├── email.md
└── plan.md
```

A skill file contains four sections:

```markdown
# skill:code

## Model
qwen2.5-coder:14b

## Memory retrieval
- Pull last 5 code-related episodes from episodic memory
- Query CodeMind graph for active project structure
- Check semantic memory for related files in current repo

## Constraints
- Always include file path as comment on line 1
- Use type hints on all function signatures
- Prefer the user's existing stack: Python, FastAPI, Neo4j, Qdrant
- Never suggest: raw SQL, class-based Django views, jQuery

## Output format
State the approach in 2 lines before any code block.
If fixing a bug, state the root cause before the fix.
```

The skill loader is intentionally minimal:

```python
def load_skill(skill_name: str) -> str:
    path = Path(f"skills/{skill_name}.md")
    return path.read_text() if path.exists() else ""
```

Over time, skill files become the most valuable part of the system. They encode your preferences, your environment's constraints, and your working style in a format any model can read instantly.

---

## Plugin System

Plugins are Python functions registered as LangChain tools. Each plugin wraps a real external service — Gmail, Google Calendar, Google Drive, GitHub — behind a clean function interface. The Conductor and specialist agents call these tools by name. LangGraph executes the real function and returns the result.

```
plugins/
├── base_plugin.py
├── gmail_plugin.py
├── calendar_plugin.py
├── drive_plugin.py
└── github_plugin.py
```

All Google plugins share a single OAuth credentials file. Plugin availability is controlled via `config/plugins.yaml` — you toggle plugins on or off without touching code.

### Phase 1 Plugins (current)
- Gmail — read threads, search inbox, compose, send
- Google Calendar — list events, create events, find free slots
- Google Drive — list, read, create, and upload files
- GitHub — list issues, read PRs, view recent commits

### Phase 2 Plugins (planned)
- Web search (SearXNG, self-hosted)
- Local filesystem (sandboxed read/write)
- Terminal execution (sandboxed subprocess)
- Notion
- Slack

---

## File Structure

```
companion/
|
+-- core/
|   +-- conductor.py          # Conductor agent (Gemma / Llama)
|   +-- intent_parser.py      # Classifies and routes intent
|   +-- task_planner.py       # LangGraph DAG builder
|   +-- model_router.py       # Maps intent to Ollama model
|
+-- agents/
|   +-- base_agent.py         # Abstract agent class
|   +-- research_agent.py     # DeepSeek + RAG
|   +-- code_agent.py         # Qwen-Coder + CodeMind graph
|   +-- writer_agent.py       # Qwen/DeepSeek + style memory
|   +-- doc_agent.py          # PDF, DOCX, PPTX generation
|
+-- memory/
|   +-- episodic.py           # Qdrant conversation store
|   +-- semantic.py           # RAG over documents
|   +-- relational.py         # Neo4j GraphRAG
|   +-- memory_manager.py     # Unified read/write interface
|
+-- skills/
|   +-- skill_registry.py     # Loads and indexes all skills
|   +-- research.md
|   +-- code.md
|   +-- write.md
|   +-- document.md
|   +-- email.md
|   +-- plan.md
|
+-- plugins/
|   +-- base_plugin.py
|   +-- gmail_plugin.py
|   +-- calendar_plugin.py
|   +-- drive_plugin.py
|   +-- github_plugin.py
|
+-- tools/
|   +-- file_tools.py         # read_file, write_file
|   +-- exec_tools.py         # execute_python (sandboxed)
|   +-- doc_tools.py          # create_pdf, create_docx, create_pptx
|   +-- registry.py           # registers all tools with LangGraph
|
+-- api/
|   +-- server.py             # FastAPI application
|
+-- ui/
|   +-- app.py                # Streamlit chat interface
|
+-- config/
|   +-- models.yaml           # Model names, temperatures, context lengths
|   +-- plugins.yaml          # Plugin credentials and toggles
|   +-- skills.yaml           # Skill trigger keywords
|
+-- docker-compose.yml        # Qdrant, Neo4j, Redis
+-- requirements.txt
+-- main.py
```

---

## Setup

### Prerequisites

- Python 3.11+
- Docker and Docker Compose
- Ollama installed and running
- Google Cloud project with OAuth credentials (for plugins)

### 1. Pull models

```bash
ollama pull gemma3:12b
ollama pull deepseek-r1:14b
ollama pull qwen2.5-coder:14b
ollama pull qwen2.5:14b
ollama pull nomic-embed-text
```

### 2. Start infrastructure

```bash
docker-compose up -d
# starts Qdrant on :6333, Neo4j on :7687, Redis on :6379
```

### 3. Install dependencies

```bash
python -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
```

### 4. Configure

```bash
cp config/plugins.yaml.example config/plugins.yaml
# add your Google OAuth credentials path
# toggle which plugins are active
```

### 5. Index your documents (semantic memory)

```bash
python -m memory.semantic --index ./your-notes-folder
```

### 6. Run

```bash
python main.py
# opens Streamlit UI at localhost:8501
```

---

## Build Phases

### Phase 1 — Core conductor and memory (Weeks 1-3)
Conductor model running with intent classification and model routing. Episodic and semantic memory operational. Basic Streamlit UI. The system can route research questions to DeepSeek and code questions to Qwen-Coder, and it remembers across sessions.

### Phase 2 — LangGraph orchestration and specialists (Weeks 4-6)
Full multi-agent DAG replacing the simple router. Each specialist is a LangGraph node. Neo4j relational memory tracks projects and entities. Skill files injected per agent. Memory manager unifies all three layers. The system can execute multi-step tasks like "research X then write a summary" as a real two-node graph.

### Phase 3 — Plugins (Weeks 7-9)
Google OAuth setup. Gmail, Calendar, Drive, and GitHub plugins registered as tools. The conductor can read emails, create calendar events, and write to Drive as part of any task chain.

### Phase 4 — Intelligence and polish (Weeks 10-12)
Writing style memory extracted from your past documents and injected into the writer agent. Web search via self-hosted SearXNG. Local filesystem and terminal plugins (sandboxed). Docker-based code execution sandbox. Proactive suggestions from the conductor based on detected patterns in your work.

---

## Configuration Reference

### models.yaml

```yaml
conductor:
  model: gemma3:12b
  temperature: 0.3
  context_length: 8192

research:
  model: deepseek-r1:14b
  temperature: 0.2
  context_length: 16384

code:
  model: qwen2.5-coder:14b
  temperature: 0.1
  context_length: 16384

writer:
  model: qwen2.5:14b
  temperature: 0.7
  context_length: 8192

embeddings:
  model: nomic-embed-text
  dimensions: 768
```

### plugins.yaml

```yaml
gmail:
  enabled: true
  credentials_path: ~/.config/companion/google_credentials.json
  scopes:
    - https://www.googleapis.com/auth/gmail.modify

calendar:
  enabled: true
  credentials_path: ~/.config/companion/google_credentials.json

drive:
  enabled: true
  credentials_path: ~/.config/companion/google_credentials.json

github:
  enabled: false
  token_env: GITHUB_TOKEN
```

---

## Design Decisions

**Why one front-facing model instead of exposing all models directly**
Cognitive consistency. If you are switching mental models every time you switch tasks, you are doing context management work that the system should be doing. One model, one voice, one relationship. The routing complexity is the system's problem.

**Why markdown skill files instead of code-defined agent personalities**
Skills change often. Your preferences evolve, your stack changes, you discover what phrasing works better with a given model. Editing a markdown file takes thirty seconds. Refactoring a Python class takes thirty minutes and a test run. Skills as text also means you can version them in git, diff them, and read them like documentation.

**Why three memory layers instead of one vector store**
Vector similarity is good at finding relevant past content but blind to structure. It cannot tell you that two projects are related, or that a person you mentioned last week is the same person you mentioned today in a different context. Graph memory handles exactly that. Episodic, semantic, and relational memory answer three different questions: what happened, what do I know, and how does everything connect.

**Why local models via Ollama instead of hosted APIs**
Your research notes, emails, code, and documents are private. Running everything locally means no third party ever sees your data. It also means no latency spikes, no rate limits, and no monthly bills that scale with usage.

---

## Hardware Notes

The system was designed for an Apple M2 Air 16GB running Ollama with MPS backend. With 16GB unified memory, running a 14B model at Q4 quantization leaves enough headroom for Qdrant, Neo4j, and the Python process simultaneously. Avoid running two 14B models at the same time — have the conductor offload context before invoking a specialist.

For heavier research tasks, the system supports routing to a remote Ollama instance, such as a GCP instance, by setting a `OLLAMA_HOST` override per agent in `models.yaml`.

---

## Roadmap

- [ ] Phase 1: Conductor + memory
- [ ] Phase 2: LangGraph multi-agent DAG
- [ ] Phase 3: Google plugins + GitHub
- [ ] Phase 4: Web search + terminal sandbox
- [ ] Voice interface via local Whisper + TTS
- [ ] Browser plugin for passive memory ingestion (save pages you read)
- [ ] Mobile companion via FastAPI + React Native thin client
- [ ] Scheduled agents (daily digest, weekly research summary)
- [ ] Cross-device sync via encrypted Qdrant snapshots

---

## Author

Rajat Malik
GitHub: Rajat25022005
HuggingFace: rajat5039