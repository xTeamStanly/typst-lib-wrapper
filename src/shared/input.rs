use std::path::PathBuf;


/// # TODO
/// Can be made [From] ... show code example
/// ```
/// println!("HELLO")
///
/// ```
#[derive(Debug, Clone)]
pub enum Input {
    File { entry: String, root: PathBuf },
    Content(String)
}

/// Create [Input] from [String].
impl From<String> for Input {
    fn from(value: String) -> Self {
        Self::Content(value)
    }
}

/// Create [Input] from ([String], [PathBuf]) tuple. \
/// First tuple element is the the name of the main typst entry file. \
/// Second tuple element is the path to the project root. It is used to \
/// resolve all relative paths, including the entry file.
impl From<(String, PathBuf)> for Input {
    fn from(value: (String, PathBuf)) -> Self {
        Self::File { entry: value.0, root: value.1 }
    }
}