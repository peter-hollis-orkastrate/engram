# Engram

Local-first screen memory for Windows. Continuous screen OCR, app-gated audio via virtual mic device ("Engram Mic"), and hotkey-activated dictation. All powered by Whisper + RuVector semantic search. No cloud. No hot mic. No Alexa. Your memory, your machine.

---

## What It Does

Engram runs quietly in your system tray, capturing what's on your screen and what's said in meetings — all stored locally on your machine with semantic search.

- **Screen Capture** — Captures screenshots at configurable FPS with multi-monitor support, runs OCR via Windows WinRT, stores text in SQLite + HNSW vector index
- **Audio Transcription** — Captures audio via WASAPI, detects speech with Silero VAD, transcribes with Whisper — all local
- **Dictation** — Global hotkey (Ctrl+Shift+D) activates voice-to-text, injecting transcribed text into the active window
- **Semantic Search** — HNSW vector search (RuVector) + FTS5 full-text search with hybrid ranking, plus raw FTS and semantic-only modes
- **Privacy First** — PII redaction (credit cards, SSNs, emails, phone numbers) before storage, localhost-only API with Bearer token auth, no network connections
- **Summarization & Insights** — Extractive summarization, entity extraction (URLs, dates, money, projects, people), daily digests, topic clustering, Obsidian vault export
- **Action Engine** — Intent detection from captured text (80+ regex patterns), task lifecycle management (7-state machine), safety-gated action execution with confirmation flow
- **Conversational Interface** — Natural language chat over your memory: NLP query parsing (40+ regex patterns), follow-up resolution, pronoun handling, session management with SQLite persistence, real FTS5 search routing, action dispatch, analytics queries, domain event emission
- **Dashboard** — 8-tab web dashboard at `/ui` with real-time search, timeline, app activity, chat panel, and storage management

## Architecture

```
Screen Capture (1 FPS) ----\
  Multi-monitor + DPI       \
Audio Capture (WASAPI) -------> EngramPipeline --> SQLite + HNSW Vector Store
  VAD -> Whisper            /   (Safety Gate       |
                           /     Dedup              v
Dictation Engine ---------/      Embed         REST API (:3030, 41 endpoints)
  (Ctrl+Shift+D)                 Metadata)       |
                                    |             v
                          Intent Detector     Dashboard (/ui)
                            |                 System Tray + Webview
                            v                     |
                        Action Engine         Chat Interface
                        (Task Store,          (NLP Parser, Context,
                         Orchestrator,         FTS Search, Action
                         Scheduler,            Dispatch, Analytics,
                         Confirmation Gate)    Session Persistence)
```

**14 Rust crates** organized by DDD bounded contexts in a Cargo workspace:

| Crate | Purpose |
|-------|---------|
| `engram-core` | Shared types, config (25+ fields), error handling, 45 domain events, PII safety gate (credit cards, SSNs, emails, phones) |
| `engram-storage` | SQLite with WAL, FTS5 full-text search, tiered retention, migrations (v1-v6), vector metadata repository |
| `engram-vector` | HNSW vector index (RuVector), ONNX embeddings, ingestion pipeline with dual-write metadata |
| `engram-api` | axum REST API (41 endpoints), SSE streaming, auth middleware, rate limiting, dynamic CORS |
| `engram-capture` | Screen capture via Win32 GDI BitBlt, multi-monitor with DPI awareness |
| `engram-ocr` | OCR via Windows.Media.Ocr WinRT |
| `engram-audio` | Audio capture via cpal/WASAPI, ring buffer |
| `engram-whisper` | Whisper.cpp transcription (feature-gated) |
| `engram-dictation` | State machine, global hotkey, text injection via SendInput |
| `engram-insight` | Extractive summarization, entity extraction, daily digest, topic clustering, Obsidian vault export |
| `engram-action` | Intent detection (6 types, 80+ patterns), task store (7-state machine), 6 action handlers, orchestrator, scheduler, confirmation gate |
| `engram-chat` | NLP query parser (40+ patterns), conversation context manager, follow-up resolution, response generator, chat orchestrator with real FTS search, action dispatch, analytics, SQLite session persistence, domain events |
| `engram-ui` | Dashboard HTML (8 views + chat panel), tray panel webview, system tray icon |
| `engram-app` | Composition root — CLI (clap), config loading, pipeline wiring |

---

## Quick Start

### Prerequisites

- **Rust** stable toolchain (1.75+) — [rustup.rs](https://rustup.rs/)
- **Windows 10/11** for full functionality (compiles on Linux/macOS with platform stubs)
- **Git**

### Build and Run

```bash
git clone git@github.com:peter-hollis-orkastrate/engram.git
cd engram

# Build
cargo build --release --workspace

# Run tests (1325 tests across 14 crates)
cargo test --workspace

# Run the application
cargo run -p engram-app --release
# Or directly: .\target\release\engram.exe
```

### CLI Options

```bash
engram --port 8080                 # Custom API port (default: 3030)
engram --config /path/to/config    # Custom config file
engram --data-dir /path/to/data    # Custom data directory
engram --log-level debug           # Log verbosity
engram --headless                  # Run without system tray UI
```

### Access

- Dashboard: http://127.0.0.1:3030/ui
- Health: http://127.0.0.1:3030/health

Protected endpoints require `Authorization: Bearer <token>` (token auto-generated at `~/.engram/data/.api_token`).

### Configuration

Config file at `~/.engram/config.toml` (auto-created on install). Priority: CLI flags > env vars > config file > defaults.

| Setting | Default | Description |
|---------|---------|-------------|
| `general.port` | 3030 | API server port |
| `screen.capture_interval_secs` | 5 | Screen capture interval |
| `dictation.hotkey` | `"Ctrl+Shift+D"` | Dictation activation hotkey |
| `search.semantic_weight` | 0.7 | Weight for semantic vs FTS in hybrid search |
| `storage.retention_days` | 90 | Data retention period |
| `safety.redact_pii` | true | Enable PII redaction |
| `actions.enabled` | true | Enable action engine (intent detection + task execution) |
| `actions.auto_approve.passive` | true | Auto-approve passive (safe) actions |

---

## Current Status

### Completed Phases

| Phase | Status | Tests | Key Deliverables |
|-------|--------|-------|-----------------|
| **Initial Build** | Complete | — | 11-crate workspace, core types, config, pipeline architecture |
| **Phase 1: Critical Integration** | Complete | 241 | Audio pipeline wiring, real embeddings, dictation transcription, FTS5 injection fix, DB path fix |
| **Phase 2: Security Hardening** | Complete | 387 | Bearer token auth, rate limiting, error sanitization, config protection, graceful shutdown, Luhn validation, file permissions, system tray wiring |
| **Phase 3: Feature Completeness** | Complete | 560 | CLI (clap), 25+ config fields, phone PII, 3 search modes, 21 API routes, multi-monitor + DPI, webview HWND, WiX installer, `cargo deny`, criterion benchmarks, 66 integration tests |
| **Phase 4: Summarization & Insights** | Complete | 657 | engram-insight crate, extractive summarization, regex entity extraction (URLs, dates, money, projects, people), daily digest, topic clustering, Obsidian vault export, 6 new API routes (27 total), migration v4, SSE event bus activation |
| **Phase 5: Local Action Engine** | Complete | 987 | engram-action crate, intent detection (6 types, 80+ regex patterns), task store (7-state machine), 6 action handlers, orchestrator with safety routing, scheduler, confirmation gate, rate limiter, 9 new API routes (37 total), migration v5, 10 new domain events (45 total) |
| **Phase 6: Conversational Interface** | Complete | 1325 | engram-chat crate, NLP query parser (40+ regex patterns, time/person/app/topic extraction), conversation context manager with follow-up resolution and pronoun handling, response generator (extractive + analytics), chat orchestrator wired to real FTS5 search, action dispatch via IntentDetector + TaskStore, analytics via QueryService, SQLite session/message persistence (write-through), 4 domain events emitted, known entity loading, 5 new API routes (41 total), migration v6, dashboard chat panel with XSS-safe rendering |

### Upcoming Phases

| Phase | Focus |
|-------|-------|
| **Phase 7: Chat Response Quality** | Smart query routing to structured data stores (tasks, entities, summaries), future time parsing, FTS result truncation, relevance scoring fixes |
| **Phase 8: Workflow Integration** | Local tool integration (clipboard, Git, calendar, browser history, markdown watcher), trigger rules, template engine |
| **Phase 9: Ambient Intelligence** | Context tracking, proactive suggestions, pattern detection, focus mode, learning loop |
| **Phase 10: General Tidy-Up** | Accumulated fixes: system tray wiring, TaskStore SQLite backing, action_history persistence, unwrap/expect cleanup, plus Phase 8-9 findings |
| **Phase 11: LAN Mode** | One-way ingest from DevPods, VMs, and WSL instances over the local network |

Each phase has a full PRD in `docs/features/`.

---

## API Endpoints (41 routes)

All protected endpoints require `Authorization: Bearer <token>`.

### Core

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/health` | No | System health check |
| GET | `/ui` | No | Dashboard HTML |
| GET | `/stream` | Yes | SSE event stream (49 domain event types) |

### Search

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/search?q=&limit=&offset=` | Yes | Semantic + keyword search |
| GET | `/search/semantic?q=&limit=` | Yes | Vector-only semantic search |
| GET | `/search/hybrid?q=&limit=&weight=` | Yes | FTS5 + vector hybrid |
| GET | `/search/raw?q=&limit=` | Yes | FTS5-only with BM25 scores |

### Capture & Audio

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/recent?content_type=&limit=` | Yes | Recent captures |
| GET | `/apps` | Yes | App capture counts |
| GET | `/apps/{name}/activity` | Yes | Hourly activity for an app |
| GET | `/audio/status` | Yes | Audio capture status |
| GET | `/audio/device` | Yes | Current audio device |
| POST | `/ingest` | Yes | Ingest text content |

### Dictation

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/dictation/status` | Yes | Dictation engine state |
| GET | `/dictation/history` | Yes | Recent dictation entries |
| POST | `/dictation/start` | Yes | Start dictation |
| POST | `/dictation/stop` | Yes | Stop and transcribe |

### Storage & Config

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/storage/stats` | Yes | DB size and counts |
| POST | `/storage/purge` | Yes | Purge old captures |
| POST | `/storage/purge/dry-run` | Yes | Preview purge |
| GET | `/config` | Yes | Current config |
| PUT | `/config` | Yes | Update config |

### Insights (Phase 4)

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/insights/daily` | Yes | Latest daily digest |
| GET | `/insights/daily/{date}` | Yes | Digest for specific date |
| GET | `/insights/topics` | Yes | Topic clusters |
| GET | `/entities` | Yes | Extracted entities |
| GET | `/summaries` | Yes | Generated summaries |
| POST | `/insights/export` | Yes | Trigger Obsidian vault export |

### Action Engine (Phase 5)

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/tasks` | Yes | List tasks (filterable by status) |
| POST | `/tasks` | Yes | Create a task (returns 403 if actions disabled) |
| GET | `/tasks/{id}` | Yes | Get task by ID |
| PUT | `/tasks/{id}` | Yes | Update task |
| DELETE | `/tasks/{id}` | Yes | Delete task |
| GET | `/actions/history` | Yes | Action execution history |
| GET | `/intents` | Yes | Detected intents |
| POST | `/actions/{task_id}/approve` | Yes | Approve a queued action |
| POST | `/actions/{task_id}/dismiss` | Yes | Dismiss a queued action |

### Chat (Phase 6)

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| POST | `/chat` | Yes | Send a chat message (returns AI response) |
| GET | `/chat/history?session_id=` | Yes | Message history for a session |
| GET | `/chat/sessions` | Yes | List chat sessions |
| DELETE | `/chat/sessions/{id}` | Yes | Delete a chat session |

---

## Project Structure

```
engram/
  crates/
    engram-core/          # Shared kernel: types, config, errors, 49 domain events, safety
    engram-storage/       # SQLite, FTS5, tiered retention, migrations (v1-v6), repositories
    engram-vector/        # HNSW index, ONNX embeddings, ingestion pipeline
    engram-api/           # axum REST API, SSE, auth, rate limiting, 41 handlers
    engram-capture/       # Win32 GDI screen capture, multi-monitor, DPI
    engram-ocr/           # WinRT OCR
    engram-audio/         # cpal/WASAPI audio, Silero VAD
    engram-whisper/       # Whisper.cpp transcription
    engram-dictation/     # State machine, hotkey, text injection
    engram-insight/       # Summarization, entity extraction, digest, clustering, vault export
    engram-action/        # Intent detection, task store, action handlers, orchestrator, scheduler
    engram-chat/          # NLP query parser, context manager, chat orchestrator, session persistence
    engram-ui/            # Dashboard HTML (8 views + chat panel), tray panel, system tray
    engram-app/           # main.rs — CLI, config, composition root
  wix/                    # WiX installer configuration
  deny.toml               # Supply chain security policy
```

## Documentation

Full documentation lives in `docs/`:

- **[Product Requirements (PRD)](docs/Engram-PRD.md)** — Original product vision
- **[Architecture Decision Records](docs/base/adr/)** — 27 ADRs covering all major decisions
- **[DDD Domain Model](docs/base/ddd/)** — Bounded contexts, aggregates, entities, events
- **[API Contracts](docs/features/initial_build/specification/api-contracts.md)** — REST API specification
- **Phase PRDs** — `docs/features/phase*/` — Full specifications for each phase
- **Phase Reviews** — `docs/features/phase*/review/` — Requirements compliance, stubs audit, security audit

---

## License

MIT — See [LICENSE](LICENSE) for details.
