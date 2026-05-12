# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

TAIS (Teacher-AI-Student) is a self-evolving AI teaching system. The core engine (`tais-core`) generates teaching workflow DAGs, executes them via an MCP-powered agent pipeline, and optimizes prompts through a TextGrad-style closed loop — all gated by teacher review.

## Build & Test

```bash
# Build
cargo build

# Run the server (defaults to 0.0.0.0:9527)
cargo run

# Configure LLM via CLI
cargo run -- --llm-provider ollama --llm-api-base http://localhost:11434 --llm-model llama3

# Or via env vars
TAIS_LLM_PROVIDER=openai TAIS_LLM_API_KEY=sk-xxx cargo run

# Or via homepage: http://localhost:9527/ (persistent config)

# Run all tests
cargo test

# Run tests for a specific module
cargo test --lib evolution::evaluator
cargo test --lib wecom
```

Configuration: CLI flags (highest priority) > env vars > `tais.toml` > homepage form. Homepage config is persisted to `tais.toml` and takes effect immediately without restart.

## Architecture

Six subsystems behind `Arc<>` / `Arc<RwLock<>>`, exposed through Axum HTTP/WebSocket:

### LLM Client (`llm/mod.rs`)
Unified LLM client supporting three backends: OpenAI-compatible, Anthropic, Ollama. Config is stored in `Arc<RwLock<Option<Arc<LlmClient>>>>` for runtime switching without restart. Provides `chat()` (full conversation) and `complete()` (system+user prompt) methods. Wired into:
- `parser.rs` — LLM extracts structured TeachingGoal from NL (fallback: keyword matching)
- `optimizer.rs` — LLM generates improved prompt variants (fallback: rule templates)
- `gene/mod.rs` — LLM rewrites output in persona style (fallback: string prepend)

### Orchestrator (`orchestrator/`)
Takes a teacher's natural-language goal, parses it into a `TeachingGoal` (`parser.rs` — LLM-first with keyword fallback), generates a linear DAG from mode-specific templates, wraps it in `petgraph::DiGraph` (`dag.rs`), and executes nodes in topological order (`executor.rs`). Each node assigns TAIS skill agents, gene capsules, MCP tools, and optional HITL triggers. DAG always ends with a teacher review node.

### MCP Gateway (`mcp/`)
Three-layer tool dispatch: (1) direct pipes for TAIS→OO calls, (2) tool registry, (3) external MCP servers via SSE/stdio. JSON-RPC protocol handler supports `tools/list`, `tools/call`, `initialize`. At startup, 21 tools registered (14 OO capsules + 7 gene capsules).

### Evolution Engine (`evolution/`)
Closed optimization loop: session records collected, 5-dimension composite metric computed (`evaluator.rs`), weaknesses diagnosed per agent, prompt variants generated (`optimizer.rs` — LLM-first with rule fallback), A/B tested with Welch's t-test. All prompt changes gated by teacher approval.

### Skills Bus & Gene Gateway (`skills/`, `gene/`)
Skills are `TaisSkill` async trait objects registered by name. `GeneGateway` wraps AI outputs with persona-specific styling (scholar/mentor/hacker) — LLM does the rewrites when available, falling back to string manipulation.

### WeCom Bot (`wecom/mod.rs`)
Enterprise WeChat (企业微信) group bot integration. Receives callbacks at `POST /api/wecom/callback` (JSON or XML), routes through safety check → Skills Bus → Gene wrap, and replies via webhook URL. URL verification at `GET /api/wecom/callback?echostr=...`.

### API Layer (`api/mod.rs`)
10 endpoints on Axum including homepage config UI, LLM config CRUD, WeCom config CRUD, workflow generation/execution, evolution metrics/review, skills/MCP tool lists, and WebSocket at `/api/session/{id}`.

### Core Types (`lib.rs`)
`TeachingGoal`, `Workflow`, `WorkflowNode`, `SessionRecord`, `EvolutionMetrics`, `GeneProfile`, MCP JSON-RPC types, HITL enums. All IDs are `Uuid`.
