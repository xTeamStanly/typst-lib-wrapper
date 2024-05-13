//! Contains some I/O parameters for the [crate::compiler::Compiler].

use std::path::PathBuf;

use ecow::EcoVec;
use typst::diag::SourceDiagnostic;

/// Typst input content/file. \
///
/// ## Content
/// Creates a typst input from anything convertable to [String]. \
/// This content is passed to the typst compiler.
///
/// # Example
/// Creates content input and builds the [Compiler].
/// ```
///     let content = r##"
///         #set page(paper: "a4");
///         = Hello World
///         Hello world from typst.
///     "##;
///     let input = Input::content(content);
///
///     CompilerBuilder::with_input(input)
///         .build()
///         .expect("Couldn't build the compiler");
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
///     let entry = "main.typ";
///     let root = "./project";
///     let input = Input::file(entry, root);
///
///     CompilerBuilder::with_input(input)
///         .build()
///         .expect("Couldn't build the compiler");
/// ```
#[derive(Debug, Clone)]
pub enum Input {
    /// Creates typst input from `entry` **filename** and project `root`.
    File { entry: String, root: PathBuf },

    /// Creates typst input from [String].
    Content(String)
}

impl Input {
    /// Creates [Input] variant [Input::Content] from anything convertable to [String].
    ///
    /// # Example
    /// ```
    ///     let content = r##"
    ///         #set page(paper: "a4");
    ///         = Hello World
    ///         Hello world from typst.
    ///     "##;
    ///     let input = Input::content(content);
    /// ```
    pub fn content(content: impl ToString) -> Self {
        Self::Content(content.to_string())
    }

    /// Creates [Input] variant [Input::File] from anything convertable to [String] \
    /// for `entry` and anything convertable [Into] [PathBuf] for `root`.
    ///
    /// `entry` is **_just_** the filename of the main (entry) typst file. \
    /// `root` is the path to the project root. It's used to resolve all relative paths, \
    /// including the entry file.
    ///
    /// # Example
    /// ```
    ///     let entry = "main.typ";
    ///     let root = "./project";
    ///     let input = Input::file(entry, root);
    /// ```
    pub fn file(entry: impl ToString, root: impl Into<PathBuf>) -> Self {
        Self::File { entry: entry.to_string(), root: root.into() }
    }
}

/// Output from the typst compiler. Consists of:
/// - `output`: Optional compilation result.
/// - `warnings`: Compiler warnings during compilation.
/// - `errors`: Compiler errors.
///
/// If there were errors during compilation or image encoding `output` field will be `None` \
/// and you should examine `errors` field. Otherwise just check the `warning` field, but \
/// that's not mandatory.
///
/// Currently `T` can be:
/// - [Vec\<u8\>](Vec) if the output is PDF.
/// - [Vec\<Vec\<u8\>\>](Vec) if the output is PNG/SVG, because every page is exported as an image.
///
/// # Example
/// Compiles document to PDF and writes it to the disk.
/// ```
///     let entry = "main.typ";
///     let root = "./project";
///     let input = Input::file(entry, root);
///
///     // Build the compiler and compile to PDF.
///     let compiler = CompilerBuilder::with_input(input)
///         .build()
///         .expect("Couldn't build the compiler");
///     let compiled = compiler.compile_pdf();
///
///     if let Some(pdf) = compiled.output {
///         // Writes PDF file.
///         std::fs::write("./main.pdf", pdf)
///             .expect("Couldn't write PDF");
///     } else {
///         // Compilation failed, show (and examine) errors.
///         dbg!(compiled.errors);
///     }
/// ```
#[derive(Debug)]
pub struct CompilerOutput<T> {
    /// Generic compilation output.
    pub output: Option<T>,
    /// Warnings during compilation.
    pub warnings: EcoVec<SourceDiagnostic>,
    /// Compilation errors.
    pub errors: EcoVec<SourceDiagnostic>
}
