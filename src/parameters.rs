//! Contains some I/O parameters for the [Compiler]. \
//! ### Exports [Input], [Format] and [Output].

use std::path::PathBuf;

use ecow::EcoVec;
use typst::{diag::SourceDiagnostic, model::Document};

/// Typst input content/file. \
///
/// ## Content
/// Creates a typst input from [String]. \
/// This content is passed to the typst compiler.
///
/// # Example
/// Creates content input and builds the [Compiler].
/// ```
/// let content = r##"
///     #set page(paper: "a4");
///     = Hello World
///     Hello world from typst.
/// "##.to_string();
/// let input = Input::from(content);
///
/// CompilerBuilder::with_input(input).build()
///     .expect("Couldn't build the compiler");
/// ```
///
/// ## File
/// Defined by two values:
/// - `entry`: Main (entry) typst **filename**.
/// - `root`: Typst project root directory path.
///
/// The compiler will use those values to load needed \
/// files during compilation.
///
/// # Example
/// Creates file input and builds the [Compiler].
/// ```
/// let entry: String = "main.typ".to_string();
/// let root: PathBuf = "./project".into();
/// let input = Input::from((entry, root));
///
/// CompilerBuilder::with_input(input).build()
///     .expect("Couldn't build the compiler");
/// ```
#[derive(Debug, Clone)]
pub enum Input {
    File { entry: String, root: PathBuf },

    /// Creates typst input from [String].
    Content(String)
}

/// Creates [Input] variant [Input::Content] from [String].
impl From<String> for Input {
    fn from(value: String) -> Self {
        Self::Content(value)
    }
}

/// Creates [Input] variant [Input::File] from ([String], [PathBuf]) tuple. \
/// First tuple element is **_just_** the filename of the main (entry) typst file. \
/// Second tuple element is the path to the project root. It's used to resolve \
/// all relative paths, including the entry file.
impl From<(String, PathBuf)> for Input {
    fn from(value: (String, PathBuf)) -> Self {
        Self::File { entry: value.0, root: value.1 }
    }
}




#[derive(Debug)]
pub struct CompilerOutput<T> {
    pub output: Option<T>,
    pub warnings: EcoVec<SourceDiagnostic>,
    pub errors: EcoVec<SourceDiagnostic>
}



#[derive(Debug, Clone, Default)]
pub enum Format {
    #[default]
    Pdf,
    Png,
    Svg
}

pub enum Output {
    Single(Vec<u8>),
    Paged { pages: Vec<Vec<u8>> }
}