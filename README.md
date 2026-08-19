# Companion — Enterprise Autonomous Agent Runtime

[![Rust](https://img.shields.io/badge/rust-1.84%2B-blue.svg)](https://www.rust-lang.org)
[![PostgreSQL](https://img.shields.io/badge/postgresql-17-336791.svg)](https://www.postgresql.org)
[![Workspace Crates](https://img.shields.io/badge/crates-19%20modular-orange.svg)]()
[![Tests](https://img.shields.io/badge/tests-100%25%20passing-brightgreen.svg)]()
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

**Companion** is a high-performance, deterministic, memory-safe agentic execution runtime written in Rust. Designed for production workloads, Companion combines strict contract enforcement, a 7-tier hierarchical memory engine, 8-stage context compilation with zero-cost codebase self-awareness, Human-in-the-Loop (HITL) safety gates, and cryptographic SHA-256 audit compliance.

---

## 🏛️ System Architecture

```
┌──────────────────────────────────────────────────────────────────────────────────────────────────┐
│                                COMPANION RUNTIME ARCHITECTURE                                    │
├──────────────────────────────────────────────────────────────────────────────────────────────────┤
│                                                                                                  │
│  [ Enterprise Gateways & CLI ]                                                                   │
│   ├── REST + Server-Sent Events (CRP Gateway)   ├── CLI (`companion run/goal/memory/skill/audit`)│
│   ├── Web Dashboard UI (`http://localhost:8000`)├── Prometheus `/metrics` Real-time Exporter    │
│                                                                                                  │
│  [ ContextOS & Live Workspace Blueprint ]                                                        │
│   ├── 8-Stage Context Compiler                  ├── Zero-Cost Live Codebase Self-Awareness Block │
│   ├── Dynamic Budget Manager & Sensitivity Gate ├── Prompt Prefix Cache (Stable Hash Keys)       │
│                                                                                                  │
│  [ MemoryOS — 7 Hierarchical Tiers ]                                                             │
│   ├── L0: Working Memory (Scratchpad)           ├── L4: Relational Knowledge Graph Store         │
│   ├── L1: Session Store (Sliding Turn Window)   ├── L5: Procedural Skill Graph Storage           │
│   ├── L2: Episodic Recorder (Execution Traces)  └── L6: Offline Dream Cycle Consolidator         │
│   ├── L3: Semantic Embeddings & Vector Store                                                     │
│                                                                                                  │
│  [ Capabilities & Extensibility ]                                                                │
│   ├── Filesystem (`read`, `write`, `list`)      ├── Gmail (`fetch_unread`, `create_draft`)       │
│   ├── Process Execution (`process.execute`)     ├── Web Scraping (`web.fetch`, `extract_links`)  │
│   ├── HITL Gated Actions (`gmail.send_reply`)   ├── Memory-Isolated WASM Sandbox (Fuel Budgeted) │
│   └── Model Context Protocol (MCP) JSON-RPC 2.0 └── Rate Limiter (TokenBucket/SlidingWindow)    │
│                                                                                                  │
│  [ SkillOS & Self-Improvement ]                                                                  │
│   ├── Multi-Version Immutable Skill Registry    ├── Trace Mining & Skill Synthesizer             │
│   ├── Automated Safety & Regression Evaluator   └── Canary Controller & Auto-Promotion/Rollback  │
│                                                                                                  │
│  [ Security, Policy & Audit Ledger ]                                                             │
│   ├── Regex Secret & PII Sanitizer / Redactor   ├── Multi-Tenant Workspace Chroot Isolation      │
│   ├── Dual-Control HITL Approval Gate           └── Cryptographic SHA-256 Hash-Chained Ledger    │
│                                                                                                  │
└──────────────────────────────────────────────────────────────────────────────────────────────────┘
```

---

## 📦 Workspace Crates (19 Crates)

| Crate | Path | Responsibility |
|---|---|---|
| `companion-domain` | `crates/companion-domain` | Core domain types: `TaskContract`, `TaskState`, `ToolResult`, `Message`, error models |
| `companion-capabilities` | `crates/companion-capabilities` | Native built-ins (`filesystem`, `process`, `gmail`, `web`), WASM sandboxing, MCP 2.0 |
| `companion-runtime` | `crates/companion-runtime` | Deterministic execution loop, contract compiler, self-healing loop, HITL gate |
| `companion-context` | `crates/companion-context` | 8-stage ContextOS compiler, token budgeting, prompt caching, Live Workspace Blueprint |
| `companion-memory` | `crates/companion-memory` | 7-tier MemoryOS (working, session, episodic, vector, knowledge graph, dream cycle) |
| `companion-models` | `crates/companion-models` | Multi-provider LLM router (Ollama, Nvidia NIM, Anthropic, OpenAI) |
| `companion-rate-limiter` | `crates/companion-rate-limiter` | Token bucket, sliding window log, and leaky bucket rate limiters |
| `companion-skills` | `crates/companion-skills` | SkillOS: procedural execution, synthesis from traces, canary rollouts |
| `companion-profile` | `crates/companion-profile` | User profile context, agent persona management, `SecretsVault` |
| `companion-agents` | `crates/companion-agents` | Actor-model `AgentTeam` with specialized roles (Architect, Engineer, Reviewer) |
| `companion-workflow` | `crates/companion-workflow` | DAG multi-agent orchestration, milestone tracking, persistent checkpointing |
| `companion-policy` | `crates/companion-policy` | `PolicyEvaluator`, PII & token redactor, tenant security enforcement |
| `companion-observability` | `crates/companion-observability` | Cryptographic SHA-256 audit ledger, Prometheus metrics collector |
| `companion-storage` | `crates/companion-storage` | PostgreSQL persistence (`PgEventStore`, `PgTaskStore`, migrations) |
| `companion-events` | `crates/companion-events` | Event-sourced `TaskEvent` structures and event store interfaces |
| `companion-protocol` | `crates/companion-protocol` | Wire serialization protocols, CRP envelopes |
| `companion-cap` | `crates/companion-cap` | Companion Agent Protocol (CAP) inter-agent messaging |
| `companion-api` | `services/api` | Axum HTTP/SSE server, REST endpoints, real-time Web Dashboard (`:8000`) |
| `companion-cli` | `bins/companion-cli` | CLI binary (`companion run/goal/memory/audit/skill`) |

---

## 🛠️ Built-in Capabilities

Companion ships with 9 production built-in tools across 4 core domains:

- **Filesystem**:
  - `filesystem.read`: Read file contents safely with size limits.
  - `filesystem.write`: Create or overwrite files within the workspace root.
  - `filesystem.list`: List directory contents with recursive depth controls.
- **Process**:
  - `process.execute`: Execute shell commands with timeout enforcement and captured stdio.
- **Gmail Automation**:
  - `gmail.fetch_unread`: Fetch inbox messages with heuristic spam/newsletter filtering.
  - `gmail.create_draft`: Create properly formatted reply drafts.
  - `gmail.send_reply`: SMTP delivery protected by a **Human-in-the-Loop (HITL) dual-control approval gate**.
- **Web Scraping & Extraction**:
  - `web.fetch`: Fetch URLs, strip boilerplate (`<script>`, `<style>`, `<nav>`, `<footer>`), and convert the main body to clean Markdown.
  - `web.extract_links`: Extract all anchor tags with URLs and labels, resolving relative links and tagging internal vs. external.

---

## 🚀 Quick Start

### 1. Launch Infrastructure
Start PostgreSQL with vector support:
```bash
docker compose -f deployments/docker/docker-compose.yml up -d
```

### 2. Configure Secrets (Optional)
Copy the example vault configuration:
```bash
cp config/secrets.toml.example config/secrets.toml
```
Configure your database URL and model provider keys (e.g. Ollama, Nvidia, OpenAI).

### 3. Run the API Gateway & Web Dashboard
```bash
cargo run -p companion-api
```
Open **[http://localhost:8000](http://localhost:8000)** in your browser to access the real-time CRP Web Dashboard.

---

## 💻 CLI Usage

Companion provides a unified CLI for all runtime and memory subsystems:

```bash
# Execute a single task under contract enforcement
cargo run -p companion-cli -- run "#code Build a REST health check in Axum"

# Execute a multi-agent DAG workflow
cargo run -p companion-cli -- goal "Create a web scraper pipeline with unit tests"

# Store semantic memory
cargo run -p companion-cli -- memory remember "API endpoints must follow REST standards"

# Semantic memory recall
cargo run -p companion-cli -- memory recall "API guidelines"

# Consolidate memories via Dream Cycle
cargo run -p companion-cli -- memory consolidate

# Inspect and verify cryptographic audit ledger
cargo run -p companion-cli -- audit view
cargo run -p companion-cli -- audit verify
```

---

## 📡 HTTP & SSE API Reference

| Method | Endpoint | Description |
|---|---|---|
| `GET` | `/` | Real-time Web Dashboard (SSE feed, approval requests, audit logs) |
| `GET` | `/v1/health` | Service health status and provider connectivity |
| `POST` | `/v1/tasks` | Create and execute an agentic task |
| `GET` | `/v1/tasks/:id` | Get task state, contract, and execution result |
| `GET` | `/v1/tasks/:id/events` | Retrieve complete event-sourced trail for a task |
| `GET` | `/v1/tasks/:id/stream` | Stream live Server-Sent Events (SSE) for a running task |
| `GET` | `/v1/approvals` | List pending Human-in-the-Loop approval requests |
| `POST` | `/v1/approvals/:id/decide` | Approve or reject a gated tool execution |
| `GET` | `/v1/audit/ledger` | Retrieve tamper-evident audit ledger entries |
| `POST` | `/v1/audit/verify` | Cryptographically verify SHA-256 Merkle chain integrity |
| `GET` | `/v1/skills` | List all registered SkillOS procedural skills |
| `GET` | `/metrics` | Prometheus metrics scrape endpoint |

---

## 🧪 Testing & Verification

Run the comprehensive test suite across all 19 workspace crates:

```bash
cargo test --workspace
```

---

## 📄 License

Licensed under the [MIT License](LICENSE).
