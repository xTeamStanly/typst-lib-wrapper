use std::path::PathBuf;

use thiserror::Error;

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
}


pub type WrapperResult<T> = Result<T, WrapperError>;