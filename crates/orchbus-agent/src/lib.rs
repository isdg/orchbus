//! The agent driver: everything orchbus knows about a coding agent's CLI and screen,
//! free of orchbus's own state, so the CLI and the node share one implementation.

pub mod agent;
pub mod classify;
pub mod transcript;
