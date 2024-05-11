use std::collections::HashMap;
use std::fmt::Debug;
use std::path::PathBuf;

use comemo::Prehashed;
use native_tls::Certificate;
use parking_lot::Mutex;
use typst::foundations::{Capturer, IntoValue};
use typst::foundations::{Dict, Value};
use typst::text::FontBook;
use typst::visualize::Color;
use typst::LibraryBuilder;
use typst_syntax::{FileId, Source, VirtualPath};

// use super::{Format, Input};

use crate::compiler::Compiler;
use crate::errors::{WrapperError, WrapperResult};
use crate::files::LazyFile;
use crate::fonts::FontCache;
use crate::package::create_http_agent;
use crate::parameters::Input;

/// [Compiler] factory, which can be used in order to configure the properties \
/// of a new [Compiler].
///
/// Configured by using method chaining. Uses `with_` and `add_` method names. \
/// Finalize the configuration by using `build` method, which \
/// takes ownership of the [CompilerBuilder].
///
/// Available configurations:
/// - `input`: Compilation [Input] (File or String).
/// - `sys_inputs`: Provides data to `sys.inputs` dictionary.
/// - `custom_data`: Overrides typst standard library with custom symbol definitions.
/// - `font_paths`: If needed, additional font paths, will be inserted into [FontCache].
/// - `ppi`: Pixels per inch when compiling to PNG, ignored otherwise.
/// - `background`: Backgroud color when compiling to PNG, ignored otherwise.
/// - `certificate`: X509 [Certificate] used when downloading packages from the repository.
///
///  `add_` methods exists for `sys_inputs`, `custom_data` and `font_paths`. \
/// They are used if you wish to add items one by one (extending vector) without rebuilding.
///
/// # Note / Warning
/// ### Blocking [Mutex]
/// It is recommended to add fonts using [FontCache] to avoid locking the font mutex \
/// for too long. This mutex is **NOT ASYNC** so keep that in mind. Use **'blocking task'** \
/// if you wish to compile documents in an async environment. Also, when compiling to \
/// PNGs or SVGs, the compiler tries to encode/convert images to bytes in parallel. \
/// To sync up compiled pages, again it uses **SYNC** mutex. \
///
/// ### [FontCache] cloning
/// Before building the compiler, builder locks [FontCache] and **clones** it for itself. \
/// To reduce [FontCache] size it is recommended to avoid loading all system fonts and \
/// to load only needed fonts. In practise this won't be that big of a deal, because \
/// all fonts are lazily loaded into memory, but they stay there, so **manually empty** \
/// the [FontCache].
///
/// **⚠ You have been warned ⚠**
///
/// # Examples
/// ## Compiling PDF
/// Shows how to compile existing typst file to PDF. Saves the result to disk.
/// ```
///     let entry: String = "main.typ".to_string();
///     let root: PathBuf = "./project".into();
///     let input = Input::from((entry, root));
///
///     // Build the compiler and compile to PDF.
///     let compiler = CompilerBuilder::with_input(input).build()
///         .expect("Couldn't build the compiler");
///     let compiled = compiler.compile_pdf();
///
///     if let Some(pdf) = compiled.output {
///         std::fs::write("./main.pdf", pdf)
///             .expect("Couldn't write PDF"); // Writes PDF file.
///     } else {
///         dbg!(compiled.errors); // Compilation failed, show errors.
///     }
/// ```
/// ## Compiling PNGs (SVGs)
/// Shows how to compile existing typst file to PNG. Saves the result to disk, \
/// one image per page. Same applies to SVGs, just use `compile_svg` method \
/// on the [Compiler] instead.
///
/// For this example let's output transparent PNGs.
/// ```
///     let entry: String = "main.typ".to_string();
///     let root: PathBuf = "./project".into();
///     let input = Input::from((entry, root));
///
///     // Build the compiler and compile to PNG.
///     let compiler = CompilerBuilder::with_input(input)
///         .with_background(Color::from_u8(0, 0, 0, 0))
///         .build()
///         .expect("Couldn't build the compiler");
///     let compiled = compiler.compile_png();
///
///     if let Some(pages) = compiled.output {
///         // Writes images one by one.
///         pages.iter().enumerate().for_each(|(index, page)| {
///             let filename = format!("./output/{index}.png");
///             std::fs::write(filename, page)
///                 .expect("Couldn't write PNG");
///         });
///     } else {
///         dbg!(compiled.errors); // Compilation failed, show errors.
///     }
/// ```
#[derive(Clone)]
pub struct CompilerBuilder {
    /// Compilation [Input] (File or String).
    input: Input,

    /// Provides data to `sys.inputs` dictionary.
    sys_inputs: Vec<(String, String)>,
    /// Overrides typst standard library with custom symbol definitions.
    custom_data: Vec<(String, Value)>,

    /// If needed, additional font paths, will be inserted into [FontCache].
    font_paths: Vec<PathBuf>,
    /// Optional PNG PPI.
    ppi: Option<f32>,
    /// Optional PNG background [Color].
    background: Option<Color>,
    /// Optional X509 [certificate](Certificate).
    certificate: Option<Certificate>,
}
/// Custom [Debug] implementation, because [Certificate] doesn't implement [Debug].
impl Debug for CompilerBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let certificate_fmt: &'static str = match &self.certificate {
            Some(_) => "Some(...)",
            None => "None",
        };

        f.debug_struct("CompilerBuilder")
            .field("input", &self.input)
            .field("sys_inputs", &self.sys_inputs)
            .field("custom_data", &self.custom_data)
            .field("font_paths", &self.font_paths)
            .field("ppi", &self.ppi)
            .field("background", &self.background)
            .field("certificate", &certificate_fmt)
            .finish()
    }
}

impl CompilerBuilder {
    /// Creates default instance of [CompilerBuilder] with `input`.
    ///
    /// # Example
    /// ```
    ///     let entry: String = "main.typ".to_string();
    ///     let root: PathBuf = "./project".into();
    ///     let input = Input::from((entry, root));
    ///     let compiler = CompilerBuilder::with_input(input)
    ///         .build()
    ///         .expect("Couldn't build the compiler");
    /// ```
    pub fn with_input(input: Input) -> Self {
        Self {
            input,

            sys_inputs: Vec::new(),
            custom_data: Vec::new(),

            font_paths: Vec::new(),
            ppi: None,
            background: None,
            certificate: None,
        }
    }

    /// Creates default instance of [CompilerBuilder] with file input. \
    /// `entry` is a **filename, not a path**.
    ///
    /// # Example
    /// ```
    ///     let entry: String = "main.typ".to_string();
    ///     let root: PathBuf = "./project".into();
    ///     let compiler = CompilerBuilder::with_file_input(entry, root)
    ///         .build()
    ///         .expect("Couldn't build the compiler");
    /// ```
    pub fn with_file_input(entry: String, root: PathBuf) -> Self {
        let input = Input::File { entry, root };
        return Self::with_input(input);
    }

    /// Creates default instance of [CompilerBuilder] with content input.
    ///
    /// # Example
    /// ```
    ///     let content = r##"
    ///         #set page(paper: "a4");
    ///         = Hello World
    ///         Hello world from typst.
    ///     "##.to_string();
    ///     let compiler = CompilerBuilder::with_content_input(content)
    ///         .build()
    ///         .expect("Couldn't build the compiler");
    /// ```
    pub fn with_content_input(content: String) -> Self {
        let input = Input::Content(content);
        return Self::with_input(input);
    }

    /// Provides data to `sys.inputs` dictionary. [String] tuple in a [vector](Vec).
    ///
    /// # Example
    /// This creates a document with text "rust world".
    /// ```
    ///     let content = r##"
    ///         #set page(paper: "a4");
    ///
    ///         #text(sys.inputs.at("language"));
    ///         #text(sys.inputs.at("hello"));
    ///     "##.to_string();
    ///
    ///     let sys_inputs: Vec<(String, String)> = vec![
    ///         ("language", "rust"),
    ///         ("hello", "world")
    ///     ]
    ///     .into_iter()
    ///     .map(|(key, value)| (String::from(key), String::from(value)))
    ///     .collect();
    ///
    ///     let compiler = CompilerBuilder::with_content_input(content)
    ///         .with_sys_inputs(sys_inputs)
    ///         .build()
    ///         .expect("Couldn't build the compiler");
    /// ```
    pub fn with_sys_inputs(mut self, sys_inputs: Vec<(String, String)>) -> Self {
        self.sys_inputs = sys_inputs;
        self
    }

    /// Adds a single value to `sys.inputs` dictionary. [String] tuple.
    ///
    /// # Example
    /// This creates a document with text "rust".
    /// ```
    ///     let content = r##"
    ///         #set page(paper: "a4");
    ///
    ///         #text(sys.inputs.at("language"));
    ///     "##.to_string();
    ///
    ///     let compiler = CompilerBuilder::with_content_input(content)
    ///         .add_sys_input(("language".to_string(), "rust".to_string()))
    ///         .build()
    ///         .expect("Couldn't build the compiler");
    /// ```
    pub fn add_sys_input(mut self, sys_input: (String, String)) -> Self {
        self.sys_inputs.push(sys_input);
        self
    }

    /// Adds multiple values to `sys.inputs` dictionary. [String] tuple in a [vector](Vec).
    ///
    /// # Example
    /// This creates a document with text "rust world".
    /// ```
    ///     let content = r##"
    ///         #set page(paper: "a4");
    ///
    ///         #text(sys.inputs.at("language"));
    ///         #text(sys.inputs.at("hello"));
    ///     "##.to_string();
    ///
    ///     let sys_inputs_1: Vec<(String, String)> = vec![
    ///         ("language".to_string(), "rust".to_string())
    ///     ];
    ///
    ///     let sys_inputs_2: Vec<(String, String)> = vec![
    ///         ("hello".to_string(), "world".to_string())
    ///     ];
    ///
    ///     let compiler = CompilerBuilder::with_content_input(content)
    ///         .add_sys_inputs(sys_inputs_1)
    ///         .add_sys_inputs(sys_inputs_2)
    ///         .build()
    ///         .expect("Couldn't build the compiler");
    /// ```
    pub fn add_sys_inputs(mut self, sys_inputs: Vec<(String, String)>) -> Self {
        self.sys_inputs.extend(sys_inputs);
        self
    }

    pub fn with_custom_data(mut self, custom_data: Vec<(String, Value)>) -> Self {
        self.custom_data = custom_data;
        self
    }
    pub fn add_custom_data_one(mut self, custom_data: (String, Value)) -> Self {
        self.custom_data.push(custom_data);
        self
    }
    pub fn add_custom_data_many(mut self, custom_data: Vec<(String, Value)>) -> Self {
        self.custom_data.extend(custom_data);
        self
    }

    pub fn with_font_paths(mut self, font_paths: Vec<PathBuf>) -> Self {
        self.font_paths = font_paths;
        self
    }
    pub fn add_font_path(mut self, font_paths: PathBuf) -> Self {
        self.font_paths.push(font_paths);
        self
    }
    pub fn add_font_paths(mut self, font_paths: Vec<PathBuf>) -> Self {
        self.font_paths.extend(font_paths);
        self
    }

    /// ## Pixels per inch.
    /// Default value: 144.0
    /// # Note
    /// Ignored if not compiling to PNG.
    pub fn with_ppi(mut self, ppi: f32) -> Self {
        self.ppi = Some(ppi);
        self
    }

    /// ## Background [Color]
    /// Default value: [Color::WHITE]
    ///
    /// If you wish to create transparent PNGs use:
    /// ```
    ///     Color::from_u8(0, 0, 0, 0)
    /// ```
    /// # Note
    /// Ignored if not compiling to PNG.
    pub fn with_background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }

    /// X509 [certificate](Certificate). \
    /// Used when building [ureq::Agent] for downloading packages from the repository.
    ///
    /// # Example
    /// How to load and parse the [Certificate].
    /// ```
    ///     let bytes = std::fs::read("my_certificate.pem")
    ///         .expect("Coudn't read the certificate");
    ///     let certificate = Certificate::from_pem(&bytes)
    ///         .expect("Invalid certificate");
    ///
    ///     let content = r##"
    ///         #set page(paper: "a4");
    ///         = Hello World
    ///         Hello world from typst.
    ///     "##.to_string();
    ///
    ///     let compiler = CompilerBuilder::with_content_input(content)
    ///         .with_certificate(certificate)
    ///         .build()
    ///         .expect("Couldn't build the compiler");
    /// ```
    pub fn with_certificate(mut self, certificate: Certificate) -> Self {
        self.certificate = Some(certificate);
        self
    }

    /// Finalizes the configuration and takes ownership of the [CompilerBuilder]. \
    /// Returns an error if something goes wrong.
    ///
    /// # Note / Warning
    /// ### Blocking [Mutex]
    /// It is recommended to add fonts using [FontCache] to avoid locking the font mutex \
    /// for too long. This mutex is **NOT ASYNC** so keep that in mind. Use **'blocking task'** \
    /// if you wish to compile documents in an async environment. Also, when compiling to \
    /// PNGs or SVGs, the compiler tries to encode/convert images to bytes in parallel. \
    /// To sync up compiled pages, again it uses **SYNC** mutex. \
    ///
    /// ### [FontCache] cloning
    /// Before building the compiler, builder locks [FontCache] and **clones** it for itself. \
    /// To reduce [FontCache] size it is recommended to avoid loading all system fonts and \
    /// to load only needed fonts. In practise this won't be that big of a deal, because \
    /// all fonts are lazily loaded into memory, but they stay there, so **manually empty** \
    /// the [FontCache].
    ///
    /// **⚠ You have been warned ⚠**
    pub fn build(self) -> WrapperResult<Compiler> {
        let http_client = create_http_agent(self.certificate)?;

        let now = chrono::Utc::now();
        let ppi: f32 = self.ppi.unwrap_or(144.0); // default typst ppi: 144.0
        let background = self.background.unwrap_or(Color::WHITE);
        let mut files: HashMap<FileId, LazyFile> = HashMap::new();

        // Convert the input pairs to a dictionary.
        let sys_inputs: Dict = self
            .sys_inputs
            .into_iter()
            .map(|(key, value)| (key.into(), value.into_value()))
            .collect();
        let mut library = LibraryBuilder::default().with_inputs(sys_inputs).build();

        // Provides a way to load custom data into the library, by overriding `keys`.
        for (key, value) in self.custom_data.into_iter() {
            library
                .global
                .scope_mut()
                .define_captured(key, value, Capturer::Function);
        }

        let root_path: PathBuf;
        let entry: Source = match self.input {
            Input::Content(c) => {
                root_path = PathBuf::from(".");
                Source::detached(c)
            }
            Input::File { entry, root } => {
                // Appends `entry` filename to `root`
                let mut entry_path = root.clone();
                entry_path.push(entry);

                // Resolve the system-global root directory.
                let canon_root_path: PathBuf =
                    root.canonicalize().map_err(|err| match err.kind() {
                        std::io::ErrorKind::NotFound => WrapperError::InputNotFound(root),
                        _ => WrapperError::from(err),
                    })?;

                // Resolve the system-global input path.
                let canon_entry_path: PathBuf =
                    entry_path.canonicalize().map_err(|err| match err.kind() {
                        std::io::ErrorKind::NotFound => WrapperError::InputNotFound(entry_path),
                        _ => WrapperError::from(err),
                    })?;

                // Resolve the virtual path of the main file within the project root.
                let main_path =
                    VirtualPath::within_root(&canon_entry_path, &canon_root_path).ok_or(
                        WrapperError::InputOutsideRoot(canon_entry_path, canon_root_path.clone()),
                    )?;
                let main_file_id = FileId::new(None, main_path);

                let entry_file: &mut LazyFile = files
                    .entry(main_file_id)
                    .or_insert_with(|| LazyFile::new(main_file_id));

                let entry_source = entry_file
                    .source(&canon_root_path, &http_client)
                    .map_err(WrapperError::from)?;

                root_path = canon_root_path;
                entry_source
            }
        };

        // TODO: LOAD FONTS + FONT CACHE THINGS
        // let book = FontBook::new();
        // let fonts = Vec::<LazyFont>::new();
        let (book, fonts) = FontCache::get_book_and_fonts().unwrap();

        Ok(Compiler {
            root: root_path,
            entry,
            files: Mutex::new(files),

            library: Prehashed::new(library),
            book: Prehashed::new(book),
            fonts,

            http_client,

            ppi,
            background,
            now,
        })
    }
}
