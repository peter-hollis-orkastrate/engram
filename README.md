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
- **Dashboard** — 8-tab web dashboard at `/ui` with real-time search, timeline, app activity, and storage management

## Architecture

```
Screen Capture (1 FPS) ----\
  Multi-monitor + DPI       \
Audio Capture (WASAPI) -------> EngramPipeline --> SQLite + HNSW Vector Store
  VAD -> Whisper            /   (Safety Gate       |
                           /     Dedup              v
Dictation Engine ---------/      Embed         REST API (configurable port)
  (Ctrl+Shift+D)                 Metadata)       |
                                                  v
                                             Dashboard (/ui)
                                             System Tray + Webview
```

**11 Rust crates** organized by DDD bounded contexts in a Cargo workspace:

| Crate | Purpose |
|-------|---------|
| `engram-core` | Shared types, config (25+ fields), error handling, domain events, PII safety gate (credit cards, SSNs, emails, phones) |
| `engram-storage` | SQLite with WAL, FTS5 full-text search, tiered retention, migrations (v1-v3), vector metadata repository |
| `engram-vector` | HNSW vector index (RuVector), ONNX embeddings, ingestion pipeline with dual-write metadata |
| `engram-api` | axum REST API (21 endpoints), SSE streaming, auth middleware, rate limiting, dynamic CORS |
| `engram-capture` | Screen capture via Win32 GDI BitBlt, multi-monitor with DPI awareness |
| `engram-ocr` | OCR via Windows.Media.Ocr WinRT |
| `engram-audio` | Audio capture via cpal/WASAPI, ring buffer |
| `engram-whisper` | Whisper.cpp transcription (feature-gated) |
| `engram-dictation` | State machine, global hotkey, text injection via SendInput |
| `engram-ui` | Dashboard HTML (8 views), tray panel webview, system tray icon |
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

# Run tests (560 tests)
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

---

## Current Status

### Completed Phases

| Phase | Status | Tests | Key Deliverables |
|-------|--------|-------|-----------------|
| **Initial Build** | Complete | — | 11-crate workspace, core types, config, pipeline architecture |
| **Phase 1: Critical Integration** | Complete | 241 | Audio pipeline wiring, real embeddings, dictation transcription, FTS5 injection fix, DB path fix |
| **Phase 2: Security Hardening** | Complete | 387 | Bearer token auth, rate limiting, error sanitization, config protection, graceful shutdown, Luhn validation, file permissions, system tray wiring |
| **Phase 3: Feature Completeness** | Complete | 560 | CLI (clap), 25+ config fields, phone PII, 3 search modes, 21 API routes, multi-monitor + DPI, webview HWND, WiX installer, `cargo deny`, criterion benchmarks, 66 integration tests |

### Upcoming Phases (PRDs Ready)

| Phase | Name | Focus |
|-------|------|-------|
| **Phase 4** | Intelligent Summarization & Insight Extraction | Auto-summarization of captured content, pattern detection, insight generation |
| **Phase 5** | Local Action Engine | Automated actions triggered by captured context, local command execution |
| **Phase 6** | Conversational Interface | Natural language queries over your memory, chat-based interaction |
| **Phase 7** | Workflow Automation & Integration | Third-party app integration, automated workflows triggered by context |
| **Phase 8** | Ambient Intelligence & Proactive Assistant | Proactive suggestions, context-aware notifications, anticipatory assistance |
| **Phase 9** | Cross-Device Sync (Privacy-Preserving) | Encrypted sync across devices, federated search, zero-knowledge architecture |

Each phase has a full PRD in `docs/features/`.

---

## API Endpoints (21 routes)

All protected endpoints require `Authorization: Bearer <token>`.

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/health` | No | System health check |
| GET | `/ui` | No | Dashboard HTML |
| GET | `/search?q=&limit=&offset=` | Yes | Semantic + keyword search |
| GET | `/search/semantic?q=&limit=` | Yes | Vector-only semantic search |
| GET | `/search/hybrid?q=&limit=&weight=` | Yes | FTS5 + vector hybrid |
| GET | `/search/raw?q=&limit=` | Yes | FTS5-only with BM25 scores |
| GET | `/recent?content_type=&limit=` | Yes | Recent captures |
| GET | `/apps` | Yes | App capture counts |
| GET | `/apps/{name}/activity` | Yes | Hourly activity for an app |
| GET | `/audio/status` | Yes | Audio capture status |
| GET | `/audio/device` | Yes | Current audio device |
| GET | `/dictation/status` | Yes | Dictation engine state |
| GET | `/dictation/history` | Yes | Recent dictation entries |
| POST | `/dictation/start` | Yes | Start dictation |
| POST | `/dictation/stop` | Yes | Stop and transcribe |
| GET | `/storage/stats` | Yes | DB size and counts |
| POST | `/storage/purge` | Yes | Purge old captures |
| POST | `/storage/purge/dry-run` | Yes | Preview purge |
| GET | `/config` | Yes | Current config |
| PUT | `/config` | Yes | Update config |
| POST | `/ingest` | Yes | Ingest text content |
| GET | `/stream` | Yes | SSE event stream |

---

## Project Structure

```
engram/
  crates/
    engram-core/          # Shared kernel: types, config, errors, events, safety
    engram-storage/       # SQLite, FTS5, tiered retention, repositories
    engram-vector/        # HNSW index, ONNX embeddings, ingestion pipeline
    engram-api/           # axum REST API, SSE, handlers
    engram-capture/       # Win32 GDI screen capture, multi-monitor, DPI
    engram-ocr/           # WinRT OCR
    engram-audio/         # cpal/WASAPI audio, Silero VAD
    engram-whisper/       # Whisper.cpp transcription
    engram-dictation/     # State machine, hotkey, text injection
    engram-ui/            # Dashboard HTML, tray panel webview, system tray
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
- **Phase PRDs** — `docs/features/phase{1-9}*/` — Full specifications for each phase

---

## License

MIT — See [LICENSE](LICENSE) for details.
