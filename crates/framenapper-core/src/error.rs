use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Invalid project file: {0}")]
    Json(#[from] serde_json::Error),
    #[error("QOI error: {0}")]
    Qoi(#[from] qoi::Error),
    #[error("Frame index {index} out of range (0..{len})")]
    FrameOutOfRange { index: usize, len: usize },
    #[error("Image size {got_w}x{got_h} does not match the project ({want_w}x{want_h})")]
    SizeMismatch { got_w: u32, got_h: u32, want_w: u32, want_h: u32 },
    #[error("Unsupported project version {0}")]
    UnsupportedVersion(u32),
    #[error("The project was changed in the meantime, please try again")]
    Busy,
    #[error("Invalid image operation (outside the image or size out of range)")]
    InvalidImageOp,
    #[error("Project archive: {0}")]
    Archive(String),
    #[error("Image: {0}")]
    Image(String),
}

pub type Result<T, E = CoreError> = std::result::Result<T, E>;

pub(crate) trait IoContext<T> {
    fn at(self, path: impl Into<PathBuf>) -> Result<T>;
}

impl<T> IoContext<T> for std::io::Result<T> {
    fn at(self, path: impl Into<PathBuf>) -> Result<T> {
        self.map_err(|source| CoreError::Io { path: path.into(), source })
    }
}
