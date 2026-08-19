# Contributing to Companion

Thank you for contributing to Companion! Companion is an enterprise agentic execution runtime written in Rust across 19 modular crates.

---

## Development Setup

### Prerequisites
- **Rust toolchain** (1.84+): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **PostgreSQL** (17+ recommended): Or run via Docker Compose (`deployments/docker/docker-compose.yml`).
- **Ollama / LLM Provider**: Running locally or configured via `config/secrets.toml`.

### Building & Testing
```bash
# Check compilation across all crates
cargo check --workspace

# Run the full test suite
cargo test --workspace

# Run tests for a specific crate (e.g. capabilities)
cargo test -p companion-capabilities
```

---

## Architecture Principles

1. **Deterministic Contracts First**: All agent actions must be governed by a `TaskContract` with bounded token budgets, tool whitelists, and verification conditions.
2. **Memory Safety & Zero Unchecked IO**: Never invoke arbitrary shell commands or write files outside configured workspace paths without checking `companion-policy`.
3. **No Crate Bloat**: When adding new built-in tools, implement them in `crates/companion-capabilities/src/builtins/` rather than creating a new crate.
4. **Self-Awareness Blueprint**: When adding new tools or crates, register them in `crates/companion-context/src/blueprint.rs` so the runtime maintains live self-awareness.

---

## Commit Guidelines

Follow conventional commit messages:
- `feat(capabilities)`: Add new built-in tool or provider
- `fix(runtime)`: Fix execution loop or verifier state transition
- `perf(context)`: Optimize ContextOS compilation or token packing
- `test(memory)`: Add integration tests for vector store or session store
