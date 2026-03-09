use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO error at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("config parse error at line {line}: {message}")]
    ConfigParse { line: usize, message: String },

    #[error("ROM not found: {0}")]
    RomNotFound(PathBuf),

    #[error("system not found: {0}")]
    SystemNotFound(String),

    #[error("favorite already exists: {0}")]
    FavoriteExists(PathBuf),

    #[error("storage not found: no valid storage location detected")]
    StorageNotFound,

    #[error("duplicate ROM detected: {original} and {duplicate}")]
    DuplicateRom { original: PathBuf, duplicate: PathBuf },

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
