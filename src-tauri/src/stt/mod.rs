// STT dispatcher — multi-backend abstraction over transcribe-rs.
// Lot 1: types + errors only. Wired into runtime in Lot 2.
#![allow(dead_code, unused_imports)]

pub mod error;
pub mod types;

pub use error::SttError;
pub use types::EngineType;
