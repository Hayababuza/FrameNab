use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("E/A-Fehler bei {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Ungültige Projektdatei: {0}")]
    Json(#[from] serde_json::Error),
    #[error("QOI-Fehler: {0}")]
    Qoi(#[from] qoi::Error),
    #[error("Frame-Index {index} außerhalb des Bereichs (0..{len})")]
    FrameOutOfRange { index: usize, len: usize },
    #[error("Bildgröße {got_w}x{got_h} passt nicht zum Projekt ({want_w}x{want_h})")]
    SizeMismatch { got_w: u32, got_h: u32, want_w: u32, want_h: u32 },
    #[error("Nicht unterstützte Projektversion {0}")]
    UnsupportedVersion(u32),
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
