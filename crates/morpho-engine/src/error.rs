use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Stable i18n key for a user-facing hint. The raw error message stays
    /// available for troubleshooting; the hint is what the UI shows.
    pub fn hint_key(&self) -> &'static str {
        match self {
            Error::Unsupported(_, _) => "hintUnsupported",
            Error::EngineMissing(_, _) => "hintEngineMissing",
            Error::InputMissing(_) => "hintInputMissing",
            Error::Io(_) => "hintIo",
            Error::Image(_) => "hintImage",
            Error::ProcessFailed { stderr, .. } => {
                if stderr.contains("Option not found") || stderr.contains("No such filter") {
                    "hintProcessBadInput"
                } else if stderr.contains("Invalid data") || stderr.contains("moov atom not found") {
                    "hintProcessCorrupt"
                } else {
                    "hintProcessFailed"
                }
            }
            Error::Cancelled => "hintCancelled",
            Error::Other(msg) => {
                if msg.contains("needs a quality or preset option") {
                    "hintReencodeNeedsOption"
                } else if msg.contains("no text layer") {
                    "hintCancelled"
                } else {
                    "hintGeneric"
                }
            }
        }
    }
}

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
