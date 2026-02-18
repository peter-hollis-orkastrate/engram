//! Task state machine with validated transitions.
//!
//! Enforces the allowed state transitions for task lifecycle:
//! Detected -> Pending -> Active -> Done/Failed/Expired/Dismissed.
