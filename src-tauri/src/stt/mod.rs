// STT dispatcher — multi-backend abstraction over transcribe-rs.
// Several helpers are intentionally kept ahead of their callers (MistralApi
// variants, display_name, gpu_health recorder) so later lots can wire them
// without touching the core module.
#![allow(dead_code, unused_imports)]

pub mod engines;
pub mod error;
pub mod gpu_health;
pub mod hybrid;
pub mod runtime;
pub mod types;

pub use engines::{load, transcribe, GpuChoice, LoadedEngine};
pub use error::SttError;
pub use hybrid::{HybridPolicy, SttLane};
pub use runtime::{capture_secs, EngineSlots, SttEngine};
pub use types::EngineType;
