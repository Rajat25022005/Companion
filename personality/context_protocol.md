# Context Protocol

> **INFRASTRUCTURE FILE — DO NOT INJECT INTO AGENT SYSTEM PROMPTS.**
> This file is read by your orchestration code, not by the model. Including it in a system prompt wastes ~400 tokens and causes the model to try to follow assembly instructions that were meant for your code.

How Companion manages context injection across the system. This file defines the rules for assembling the system prompt that every agent sees.

## Prompt Assembly Order

When any agent is invoked, its system prompt is assembled in this exact order:

```
1. soul.md          — core identity (always included, never truncated)
2. voice.md         — communication style (always included)
3. boundaries.md    — hard and soft limits (always included)
4. relationship.md  — collaboration principles (included for Conductor only)
5. quirks.md        — personality details (included for Conductor only)
6. [skill].md       — the relevant skill file for this specific agent
7. Memory context   — retrieved from episodic, semantic, and relational layers
8. Conversation     — the current message thread
```

## Truncation Rules

Context windows are limited. When truncation is necessary, cut from the bottom first:

1. **Never cut**: soul.md, boundaries.md
2. **Cut last**: voice.md, the active skill file
3. **Cut if needed**: relationship.md, quirks.md
4. **Cut first**: older memory context, older conversation turns

## Context Budget

With a typical 8192-token context window:

| Component | Approximate Budget |
|---|---|
| Personality (soul + voice + boundaries) | ~1500 tokens |
| Active skill file | ~800 tokens |
| Memory context (episodic + semantic + relational) | ~2000 tokens |
| Current conversation | ~3500 tokens |
| Generation headroom | ~400 tokens |

For agents with 16384-token context (research, code), double the memory and conversation budgets.

## Specialist vs. Conductor Context

### Conductor
Gets the full personality stack: soul + voice + boundaries + relationship + quirks.

### Specialist Agents
Get: soul + voice + boundaries + their specific skill file. Not relationship.md or quirks.md.

## Memory Injection Format

When memory context is injected into the prompt, it follows this format:

```
--- MEMORY CONTEXT ---

[Episodic]
- (2 days ago) User was debugging a FastAPI middleware issue with auth tokens
- (1 week ago) User decided to use Qdrant over Pinecone for vector storage

[Semantic]
- From project README: the system uses LangGraph for multi-agent orchestration
- From notes/architecture.md: the conductor pattern routes to 4 specialist types

[Relational]
- Hypnos (Project) --USES--> Mamba (Framework)
- Hypnos (Project) --TARGETS--> NeurIPS (Event)

--- END MEMORY CONTEXT ---
```

## Session Continuity

At the start of each new session:
1. Load the last 3 episodic entries to establish continuity
2. Check relational memory for any active projects with approaching deadlines
3. If the last session ended mid-task, proactively surface the unfinished context
