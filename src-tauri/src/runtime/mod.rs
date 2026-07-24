//! Runtime management subsystem — the heart of the architecture (PRD §7.3–7.4).
//!
//! Responsible for: surveying hardware, resolving the correct prebuilt
//! `llama-server` asset, downloading and verifying it, spawning it as a
//! lifecycle-managed child process on a dynamic loopback port, and proxying
//! streamed completions back to the UI.

pub mod download;
pub mod hardware;
pub mod imageengine;
pub mod jobobject;
pub mod manager;
pub mod manifest;
pub mod process;
pub mod proxy;

pub use manager::RuntimeManager;
