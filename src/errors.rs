use std::path::PathBuf;

use thiserror::Error;
use typst::diag::FileError;

#[derive(Debug, Error)]
#[error("{0}")]
pub enum WrapperError {

    /// Shouldn't happen, but just in case
    #[error("Accessing uninitialized font storage")]
    UninitializedFontCache,


    #[error("Cound't load font face with path: {0}")]
    FontFaceLoadingError(PathBuf),
    #[error("Coudn't load font: {0}")]
    FontLoadingError(std::io::Error),


    #[error("Input `{0}` not found")]
    InputNotFound(PathBuf),
    #[error("Input `{0}` outside of root `{1}`")]
    InputOutsideRoot(PathBuf, PathBuf),


    #[error("IO: `{0}`")]
    Io(std::io::Error),

    #[error("HTTP: `{0}`")]
    Http(ureq::Error),

    #[error("File: `{0}`")]
    File(FileError)
}

impl From<std::io::Error> for WrapperError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ureq::Error> for WrapperError {
    fn from(value: ureq::Error) -> Self {
        Self::Http(value)
    }
}

impl From<FileError> for WrapperError {
    fn from(value: FileError) -> Self {
        Self::File(value)
    }
}

pub type WrapperResult<T> = Result<T, WrapperError>;