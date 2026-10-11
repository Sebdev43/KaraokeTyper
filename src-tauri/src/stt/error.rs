use thiserror::Error;

#[derive(Debug, Error)]
pub enum SttError {
    #[error("model not found: {0}")]
    ModelNotFound(String),

    #[error("failed to load model: {0}")]
    LoadError(String),

    /// The file exists but the engine cannot read this kind of model. Kept apart
    /// from `LoadError`: a format mismatch says nothing about the GPU, and
    /// counting it as a backend failure would auto-disable acceleration.
    #[error("unsupported model format: {0}")]
    UnsupportedFormat(String),

    #[error("transcription failed: {0}")]
    TranscribeError(String),

    #[error("backend unavailable: {0}")]
    BackendUnavailable(String),

    #[error("invalid audio: {0}")]
    InvalidAudio(String),

    #[error("api error: {0}")]
    ApiError(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
