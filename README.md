# Engram

Local-first screen memory for Windows. Continuous screen OCR, app-gated audio via virtual mic device ("Engram Mic"), and hotkey-activated dictation. All powered by Whisper + RuVector semantic search. No cloud. No hot mic. No Alexa. Your memory, your machine.

---

## What It Does

Engram runs quietly in your system tray, capturing what's on your screen and what's said in meetings — all stored locally on your machine with semantic search.

- **Screen Capture** — Captures screenshots at configurable FPS, runs OCR via Windows WinRT, stores text in SQLite + HNSW vector index
- **Audio Transcription** — Captures audio via WASAPI, detects speech with Silero VAD, transcribes with Whisper — all local
- **Dictation** — Global hotkey (Ctrl+Shift+D) activates voice-to-text, injecting transcribed text into the active window
- **Semantic Search** — HNSW vector search (RuVector) + FTS5 full-text search with hybrid ranking
- **Privacy First** — PII redaction (credit cards, SSNs, emails) before storage, localhost-only API, no network connections

## Architecture

```
Screen Capture (1 FPS) ----\
                             \
Audio Capture (WASAPI) -------> EngramPipeline --> SQLite + HNSW Vector Store
  VAD → Whisper             /   (Safety Gate       |
                           /     Dedup              v
Dictation Engine ---------/      Embed)        REST API (:3030)
  (Ctrl+Shift+D)                                |
                                                v
                                           Dashboard (/ui)
                                           System Tray
```

**11 Rust crates** organized by DDD bounded contexts in a Cargo workspace:

| Crate | Purpose |
|-------|---------|
| `engram-core` | Shared types, config, error handling, domain events, PII safety gate |
| `engram-storage` | SQLite with WAL, FTS5 full-text search, tiered retention, migrations |
| `engram-vector` | HNSW vector index (RuVector), ONNX embeddings, ingestion pipeline |
| `engram-api` | axum REST API (16+ endpoints), SSE streaming, error middleware |
| `engram-capture` | Screen capture via Win32 GDI BitBlt |
| `engram-ocr` | OCR via Windows.Media.Ocr WinRT |
| `engram-audio` | Audio capture via cpal/WASAPI, ring buffer |
| `engram-whisper` | Whisper.cpp transcription (feature-gated) |
| `engram-dictation` | State machine, global hotkey, text injection via SendInput |
| `engram-ui` | Dashboard HTML (8 views), tray panel, system tray icon |
| `engram-app` | Composition root — wires everything together |

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

# Run tests
cargo test --workspace

# Run the application
cargo run -p engram-app --release
```

### Access

- Dashboard: http://127.0.0.1:3030/ui
- Health: http://127.0.0.1:3030/health
- Search: http://127.0.0.1:3030/search?q=hello
- Recent: http://127.0.0.1:3030/recent

### Configuration

Auto-created at `~/.engram/config.toml` on first run. Key settings:

| Setting | Default | Description |
|---------|---------|-------------|
| `screen.fps` | 1.0 | Screen capture rate |
| `dictation.hotkey` | `"Ctrl+Shift+D"` | Dictation activation hotkey |
| `search.dedup_threshold` | 0.95 | Cosine similarity threshold for dedup |
| `storage.retention_days` | 365 | Data retention period |
| `storage.hot_days` | 7 | Hot tier duration |
| `safety.pii_detection` | true | Enable PII redaction |

Override the API port with `ENGRAM_PORT=8080` environment variable.

---

## Current Status

### What's Built (Initial Build — Complete)

The initial build established the full architectural foundation:

| Milestone | Status | Summary |
|-----------|--------|---------|
| **M0: Foundation** | 100% | Workspace, core types, config, error handling, PII safety gate |
| **M1: Data Pipeline** | 90% | Screen capture, OCR, HNSW vector index, SQLite storage, FTS5, ingestion pipeline |
| **M2: API + Audio** | 75% | REST API (16 endpoints), audio capture, Silero VAD, Whisper transcription |
| **M3: UI + Installer** | 60% | Dashboard (8 views), system tray, tray panel, dictation state machine |

**Key metrics:** 11 crates, ~12,000 lines of Rust, all tests pass, zero circular dependencies, 17/23 planned tasks complete.

### What's Next

Three phases remain to bring Engram to production readiness. Each phase has a full PRD in `/docs/features/`.

#### Phase 1: Critical Integration & Security (P0)

*Resolves the 6 critical stubs and 2 critical security vulnerabilities found in code review.*

| Item | Description |
|------|-------------|
| Wire audio pipeline | Connect audio capture → VAD → Whisper → storage pipeline |
| Real embeddings | Replace MockEmbedding with OnnxEmbeddingService in production |
| Dictation transcription | Connect dictation engine to real Whisper output |
| API authentication | Bearer token middleware on all endpoints |
| FTS5 injection fix | Sanitize user input in search queries |
| CORS lockdown | Restrict to localhost only |
| Replace API stubs | Real handlers for audio/dictation status and control |
| Fix DB path | Store database under `~/.engram/data/` |

**PRD:** [`docs/features/phase1-critical-integration/Engram-Phase1-PRD.md`](docs/features/phase1-critical-integration/Engram-Phase1-PRD.md)

#### Phase 2: Security Hardening & Usability (P2)

*Addresses 6 High and 8 Medium security findings, adds lifecycle management.*

| Item | Description |
|------|-------------|
| Request limits | Body size limits and rate limiting |
| Error sanitization | No internal details in API error responses |
| Config protection | Safety settings immutable via API |
| Graceful shutdown | Clean Ctrl+C handling with data flush |
| Unsafe documentation | Document or remove all `unsafe impl Send/Sync` |
| Luhn validation | Credit card detection with checksum verification |
| File permissions | Owner-only ACLs on data directory |
| System tray wiring | Tray icon active with menu actions |
| Domain events | Complete all 30 specified events |

**PRD:** [`docs/features/phase2-hardening-usability/Engram-Phase2-PRD.md`](docs/features/phase2-hardening-usability/Engram-Phase2-PRD.md)

#### Phase 3: Feature Completeness & Polish (P3)

*Closes all remaining gaps from the original specification.*

| Item | Description |
|------|-------------|
| Missing API routes | 5 routes: /search/semantic, /search/hybrid, /search/raw, /audio/device, /storage/purge/dry-run |
| Missing DB tables | vectors_metadata and config tables |
| CLI arguments | clap-based `--port`, `--config`, `--data-dir`, `--log-level` |
| MSI installer | Windows installer via cargo-wix |
| Phone number PII | US phone number detection and redaction |
| cargo deny/audit | Supply chain security enforcement |
| Complete config | All PRD-specified config fields with correct defaults |
| Integration tests | Full API test suite for all 21 endpoints |
| Tray webview | Real window handle for tray panel |
| Multi-monitor | Capture from secondary monitors |

**PRD:** [`docs/features/phase3-completeness-polish/Engram-Phase3-PRD.md`](docs/features/phase3-completeness-polish/Engram-Phase3-PRD.md)

---

## Project Structure

```
engram/
  crates/
    engram-core/          # Shared kernel: types, config, errors, events, safety
    engram-storage/       # SQLite, FTS5, tiered retention, repositories
    engram-vector/        # HNSW index, ONNX embeddings, ingestion pipeline
    engram-api/           # axum REST API, SSE, handlers
    engram-capture/       # Win32 GDI screen capture
    engram-ocr/           # WinRT OCR
    engram-audio/         # cpal/WASAPI audio, Silero VAD
    engram-whisper/       # Whisper.cpp transcription
    engram-dictation/     # State machine, hotkey, text injection
    engram-ui/            # Dashboard HTML, tray panel, system tray
    engram-app/           # main.rs — composition root
  tests/                  # Workspace-level tests
  benches/                # Benchmarks
  examples/               # Example usage
```

## Documentation

Full documentation lives in `/docs/`:

- **[Product Requirements (PRD)](docs/Engram-PRD.md)** — Original product vision
- **[Architecture Decision Records](docs/base/adr/)** — 27 ADRs covering all major decisions
- **[DDD Domain Model](docs/base/ddd/)** — Bounded contexts, aggregates, entities, events
- **[API Contracts](docs/features/initial_build/specification/api-contracts.md)** — REST API specification
- **[Initial Build Review](docs/features/initial_build/review/)** — Deep code review results

---

## License

See [LICENSE](LICENSE) for details.
