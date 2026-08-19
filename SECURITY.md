# Security Policy

## Reporting Security Vulnerabilities

We take the security of Companion and its agentic execution runtime seriously. If you discover a security vulnerability or exploit within this codebase, please **do not open a public GitHub issue**.

Please report vulnerabilities privately:
- **Email**: `security@companion-ai.org` (or directly to repository maintainers)
- **Response Target**: Within 48 hours with an initial assessment and mitigation timeline.

---

## Security Architecture & Defenses

Companion is designed with defense-in-depth principles across its 19 crates:

### 1. Zero-Trust Capability & Tool Gate (`companion-capabilities` + `companion-policy`)
- **Strict Permission Tiers**: Every tool invocation is checked against the task contract's `allowed_tools` and required `CapabilityPermission` (e.g. `NetworkRead`, `FileSystemWrite`, `Execute`).
- **Human-in-the-Loop (HITL) Dual-Control Gates**: High-risk actions (e.g., sending emails via `gmail.send_reply`, dropping databases, executing remote destructive payloads) are gated and require manual authorization via the CRP Approval API (`/v1/approvals/:id/decide`).

### 2. Secrets & Credential Vault (`companion-profile`)
- **Air-Gapped Vault**: Raw API keys and credentials are stored strictly in `config/secrets.toml` or OS environment variables, parsed into an in-memory `SecretsVault` with zero plaintext leakage in log sinks.
- **Regex PII & Token Redaction**: All context compiler streams, model completions, and audit ledger entries pass through the `companion-policy` redactor to sanitize tokens (`sk-...`, `Bearer ...`, API secrets).

### 3. Execution Sandboxing (`companion-capabilities`)
- **WASM Isolation**: WASM modules execute in memory-isolated sandboxes with bounded fuel limits and memory caps.
- **Chroot & Path Traversal Guards**: Native filesystem tools restrict writes to configured workspace roots.

### 4. Cryptographic Tamper-Evident Audit Ledger (`companion-observability`)
- **SHA-256 Hash Chain**: Every task transition, tool execution result, and model response is hashed into a monotonic, cryptographically verifiable Merkle chain stored in PostgreSQL.

---

## Supported Versions

| Version | Supported |
|---|---|
| `0.1.x` (Current) | :white_check_mark: |
| `< 0.1.0` | :x: |
