//! One module per media provider. **Adding a provider is exactly this
//! checklist:**
//!
//! 1. Create `backends/<name>.rs` with a `BackendDescriptor` const, a
//!    `list_models`, and a `generate`.
//! 2. Add one line to `Registry::new()` in `media/mod.rs`.
//! 3. If it uses `Credential::Media`, nothing else — the Settings key row,
//!    the picker group, the "add a key" empty state and the consent copy are
//!    all generated from the descriptor.
//!
//! Nothing outside `media/` should ever need to change for a new backend. If
//! it does, the seam (`media::mod`) is wrong — fix that before adding another
//! provider.

pub mod local;
pub mod openai;
pub mod openrouter;
