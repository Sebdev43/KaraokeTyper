use thiserror::Error;

#[derive(Debug, Error)]
pub enum SttError {
    #[error("model not found: {0}")]
    ModelNotFound(String),

    #[error("failed to load model: {0}")]
    LoadError(String),

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
