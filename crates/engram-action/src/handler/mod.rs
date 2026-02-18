//! Action handler registry and trait definition.
//!
//! Defines the `ActionHandler` async trait and provides the handler
//! registry for dispatching actions to the correct implementation.

pub mod clipboard;
pub mod notification;
pub mod quick_note;
pub mod reminder;
pub mod shell_command;
pub mod url_open;
