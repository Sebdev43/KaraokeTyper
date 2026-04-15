// STT dispatcher — multi-backend abstraction over transcribe-rs.
// Several helpers are intentionally kept ahead of their callers (MistralApi
// variants, display_name, gpu_health recorder) so later lots can wire them
// without touching the core module.
#![allow(dead_code, unused_imports)]

pub mod engines;
pub mod error;
pub mod gpu_health;
pub mod types;

#[cfg(feature = "voxtral")]
pub mod voxtral_impl;

pub use engines::{load, suppress_backend_logs, transcribe, unload, LoadedEngine};
pub use error::SttError;
pub use types::EngineType;
