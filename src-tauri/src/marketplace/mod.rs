//! Model marketplace (PRD §4.1): a unified catalog sourced from a curated
//! "recommended" overlay (D-5) plus dynamic discovery across Hugging Face and
//! GitHub releases, with a hardware-fit classifier (MKT-4).

pub mod catalog;
pub mod huggingface;
pub mod github;

pub use catalog::recommended_catalog;
