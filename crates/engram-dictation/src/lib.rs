//! Engram Dictation crate - Dictation state machine, session management, and text injection.
//!
//! Provides the dictation engine that manages the lifecycle of a dictation session
//! through a strict state machine: Idle -> Listening -> Processing -> Typing -> Idle.
//! Thread-safe state management is handled via `Arc<Mutex<>>`.

pub mod engine;
pub mod hotkey;
pub mod state;
pub mod text_inject;

pub use engine::{DictationEngine, DictationSession, TranscriptionFn};
pub use hotkey::{HotkeyConfig, HotkeyService};
pub use state::DictationState;
pub use text_inject::TextInjector;
