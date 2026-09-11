use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("unsupported conversion: {0} -> {1}")]
    Unsupported(String, String),

    #[error("engine binary not found: {0} (looked under {1})")]
    EngineMissing(String, String),

    #[error("input file not found: {0}")]
    InputMissing(String),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("image: {0}")]
    Image(#[from] image::ImageError),

    #[error("{engine} exited with code {code}: {stderr}")]
    ProcessFailed {
        engine: String,
        code: i32,
        stderr: String,
    },

    #[error("job cancelled")]
    Cancelled,

    #[error("{0}")]
    Other(String),
}
