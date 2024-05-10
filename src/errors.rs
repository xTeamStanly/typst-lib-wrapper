use std::path::PathBuf;

use thiserror::Error;
pub use typst::diag::{FileError, PackageError}; // Re-exported typst errors.

pub type WrapperResult<T> = Result<T, WrapperError>;

#[derive(Debug, Error)]
#[error("{0}")]
pub enum WrapperError {

    /// Shouldn't happen, but just in case.
    #[error("Accessing uninitialized font storage")]
    UninitializedFontCache,

    // Font errors
    #[error("Cound't load font face with path: {0}")]
    FontFaceLoadingError(PathBuf),
    #[error("Coudn't load font: {0}")]
    FontLoadingError(std::io::Error),

    // Input errors
    #[error("Input `{0}` not found")]
    InputNotFound(PathBuf),
    #[error("Input `{0}` outside of root `{1}`")]
    InputOutsideRoot(PathBuf, PathBuf),

    // Wrapper around `std::io::Error`.
    #[error("IO: `{0}`")]
    Io(std::io::Error),

    // Boxed `ureq::Error` because it's too large.
    #[error("HTTP: `{0}`")]
    Http(Box<ureq::Error>),

    // Wrapper around typst `FileError`.
    #[error("File: `{0}`")]
    File(FileError),
    // Wrapper arount typst `PackageError`.
    #[error("Package: `{0}`")]
    Package(PackageError),

}

impl From<std::io::Error> for WrapperError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ureq::Error> for WrapperError {
    fn from(value: ureq::Error) -> Self {
        Self::Http(Box::new(value))
    }
}

impl From<FileError> for WrapperError {
    fn from(value: FileError) -> Self {
        Self::File(value)
    }
}

impl From<PackageError> for WrapperError {
    fn from(value: PackageError) -> Self {
        Self::Package(value)
    }
}
