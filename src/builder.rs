//! Provides a way to build a typst [Compiler].

use std::collections::HashMap;
use std::fmt::Debug;
use std::path::PathBuf;

use parking_lot::Mutex;
use typst::foundations::{Capturer, IntoValue};
use typst::foundations::{Dict, Value};
use typst::visualize::Color;
use typst::LibraryBuilder;
use typst_syntax::{FileId, Source, Span, VirtualPath};
use typst_utils::LazyHash;

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
/// Finalize the configuration by using `build` method, which
/// takes ownership of the [CompilerBuilder].
///
/// Available configurations:
/// - `input`: Compilation [Input] (File or String).
/// - `sys_inputs`: Provides data to `sys.inputs` dictionary.
/// - `custom_data`: Overrides typst standard library with custom symbol definitions.
/// - `font_paths`: If needed, additional font paths, will be inserted into [FontCache].
/// - `ppi`: Pixels per inch when compiling to PNG, ignored otherwise.
/// - `background`: Backgroud color when compiling to PNG, ignored otherwise.
/// - `agent`: Overrides default [ureq::Agent] with provided one.
///
///  `add_` methods exists for `sys_inputs`, `custom_data` and `font_paths`. \
/// They are used if you wish to add items one by one (extending vector) without rebuilding.
///
/// # Note / Warning
/// ### Blocking Mutex
/// It is recommended to add fonts using [FontCache] to avoid locking the font mutex
/// for too long. This mutex is **NOT ASYNC** so keep that in mind. Use **'blocking task'**
/// if you wish to compile documents in an async environment. Also, if compiling with an
/// opt-in feature (`"parallel_compilation"`) to PNGs or SVGs, the compiler tries to
/// encode/convert images to bytes in parallel with `rayon`. To sync up compiled pages,
/// again it uses **SYNC** mutex. \
/// [On mixing `rayon` with `tokio`!](https://blog.dureuill.net/articles/dont-mix-rayon-tokio/)
///
/// ### [FontCache] cloning
/// Before building the compiler, builder locks [FontCache] and **clones** it for itself.
/// To reduce [FontCache] size it is recommended to avoid loading all system fonts and
/// to load only needed fonts. In practise this won't be that big of a deal, because
/// all fonts are lazily loaded into memory, but they stay there, so **manually empty**
/// the [FontCache].
///
/// ### Filename restrictions
/// Do not use any filenames or paths that contain text
/// **`"CUSTOM_SOURCE_CONTENT_INPUT_IN_MEMORY_FILE"`**. \
/// For more information check the main ReadMe file.
///
/// **⚠ You have been warned ⚠**
///
/// # Examples
/// ## Compiling PDF
/// Shows how to compile existing typst file to PDF. Saves the result to disk.
/// ```
/// let entry = "main.typ";
/// let root = "./project";
/// let input = Input::file(entry, root);
///
/// // Build the compiler and compile to PDF.
/// let compiler = CompilerBuilder::with_input(input)
///     .build()
///     .expect("Couldn't build the compiler");
/// let compiled = compiler.compile_pdf();
///
/// if let Some(pdf) = compiled.output {
///     std::fs::write("./main.pdf", pdf)
///         .expect("Couldn't write PDF"); // Writes PDF file.
/// } else {
///     dbg!(compiled.errors); // Compilation failed, show errors.
/// }
/// ```
/// ## Compiling PNGs (SVGs)
/// Shows how to compile existing typst file to PNG. Saves the result to disk,
/// one image per page. Same applies to SVGs, just use `compile_svg` method
/// on the [Compiler] instead.
///
/// For this example let's output transparent PNGs.
/// ```
/// let entry = "main.typ";
/// let root = "./project";
/// let input = Input::file(entry, root);
///
/// // Build the compiler and compile to PNG.
/// let compiler = CompilerBuilder::with_input(input)
///     .with_background(Color::from_u8(0, 0, 0, 0))
///     .build()
///     .expect("Couldn't build the compiler");
/// let compiled = compiler.compile_png();
///
/// if let Some(pages) = compiled.output {
///     // Writes images one by one.
///     pages.iter().enumerate().for_each(|(index, page)| {
///         let filename = format!("./output/{index}.png");
///         std::fs::write(filename, page)
///             .expect("Couldn't write PNG");
///     });
/// } else {
///     dbg!(compiled.errors); // Compilation failed, show errors.
/// }
/// ```
#[derive(Clone, Debug)]
pub struct CompilerBuilder {
    /// Compilation [Input] (File or String).
    input: Input,

    /// Provides data to `sys.inputs` dictionary.
    sys_inputs: Vec<(String, String)>,
    /// Overrides typst standard library with custom symbol definitions.
    custom_data: Vec<(String, Value)>,
    /// Generate PDF/A output. Only used if compiler compiles to PDF.
    pdf_a: Option<bool>,

    /// If needed, additional font paths, will be inserted into [FontCache].
    font_paths: Vec<PathBuf>,
    /// Optional PNG PPI.
    ppi: Option<f32>,
    /// Optional PNG background [Color].
    background: Option<Color>,
    /// Optional [ureq::Agent].
    agent: Option<ureq::Agent>
}

impl CompilerBuilder {
    /// Creates default instance of [CompilerBuilder] with `input`.
    ///
    /// # Example
    /// ```
    /// let entry = "main.typ";
    /// let root = "./project";
    /// let input = Input::file(entry, root);
    /// let compiler = CompilerBuilder::with_input(input)
    ///     .build()
    ///     .expect("Couldn't build the compiler");
    /// ```
    ///
    /// # Note / Warning
    /// Do not use any filenames or paths that contain text
    /// **`"CUSTOM_SOURCE_CONTENT_INPUT_IN_MEMORY_FILE"`**. \
    /// For more information check the main ReadMe file.
    pub fn with_input(input: Input) -> Self {
        Self {
            input,

            sys_inputs: Vec::new(),
            custom_data: Vec::new(),
            pdf_a: Some(false),

            font_paths: Vec::new(),
            ppi: None,
            background: None,
            agent: None
        }
    }

    /// Creates default instance of [CompilerBuilder] with file input.
    /// `entry` is a **filename, not a path**.
    ///
    /// # Example
    /// ```
    /// let entry = "main.typ";
    /// let root = "./project";
    /// let compiler = CompilerBuilder::with_file_input(entry, root)
    ///     .build()
    ///     .expect("Couldn't build the compiler");
    /// ```
    ///
    /// # Note / Warning
    /// Do not use any filenames or paths that contain text
    /// **`"CUSTOM_SOURCE_CONTENT_INPUT_IN_MEMORY_FILE"`**. \
    /// For more information check the main ReadMe file.
    pub fn with_file_input(entry: impl ToString, root: impl Into<PathBuf>) -> Self {
        let input = Input::File { entry: entry.to_string(), root: root.into() };
        return Self::with_input(input);
    }

    /// Creates default instance of [CompilerBuilder] with content input.
    ///
    /// # Example
    /// ```
    /// let content = r##"
    ///     #set page(paper: "a4");
    ///     = Hello World
    ///     Hello world from typst.
    /// "##;
    /// let compiler = CompilerBuilder::with_content_input(content)
    ///     .build()
    ///     .expect("Couldn't build the compiler");
    /// ```
    pub fn with_content_input(content: impl ToString) -> Self {
        let input = Input::Content(content.to_string());
        return Self::with_input(input);
    }

    /// Provides data to `sys.inputs` dictionary.
    ///
    /// # Example
    /// This creates a document with text _"rust world"_.
    /// ```
    /// let content = r##"
    ///     #set page(paper: "a4");
    ///
    ///     #text(sys.inputs.at("language"));
    ///     #text(sys.inputs.at("hello"));
    /// "##;
    ///
    /// let sys_inputs = vec![
    ///     ("language", "rust"),
    ///     ("hello", "world")
    /// ];
    ///
    /// let compiler = CompilerBuilder::with_content_input(content)
    ///     .with_sys_inputs(sys_inputs)
    ///     .build()
    ///     .expect("Couldn't build the compiler");
    /// ```
    pub fn with_sys_inputs(mut self, sys_inputs: Vec<(impl ToString, impl ToString)>) -> Self {
        let mapped = sys_inputs.into_iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        self.sys_inputs = mapped;
        self
    }

    /// Adds a single value to `sys.inputs` dictionary.
    ///
    /// # Example
    /// This creates a document with text _"rust"_.
    /// ```
    /// let content = r##"
    ///     #set page(paper: "a4");
    ///
    ///     #text(sys.inputs.at("language"));
    /// "##;
    ///
    /// let compiler = CompilerBuilder::with_content_input(content)
    ///     .add_sys_input(("language", "rust"))
    ///     .build()
    ///     .expect("Couldn't build the compiler");
    /// ```
    pub fn add_sys_input(mut self, sys_input: (impl ToString, impl ToString)) -> Self {
        self.sys_inputs.push((sys_input.0.to_string(), sys_input.1.to_string()));
        self
    }

    /// Adds multiple values to `sys.inputs` dictionary.
    ///
    /// # Example
    /// This creates a document with text _"rust world"_.
    /// ```
    /// let content = r##"
    ///     #set page(paper: "a4");
    ///
    ///     #text(sys.inputs.at("language"));
    ///     #text(sys.inputs.at("hello"));
    /// "##;
    ///
    /// let sys_inputs_1 = vec![("language", "rust")];
    /// let sys_inputs_2 = vec![("hello", "world")];
    ///
    /// let compiler = CompilerBuilder::with_content_input(content)
    ///     .add_sys_inputs(sys_inputs_1)
    ///     .add_sys_inputs(sys_inputs_2)
    ///     .build()
    ///     .expect("Couldn't build the compiler");
    /// ```
    pub fn add_sys_inputs(mut self, sys_inputs: Vec<(impl ToString, impl ToString)>) -> Self {
        let mapped: Vec<(String, String)> = sys_inputs
            .into_iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        self.sys_inputs.extend(mapped);
        self
    }

    /// Provides a way to override typst standard library and add custom symbols to the
    /// global context.
    ///
    /// # Note / Warning
    /// Mind that this will overload ANY symbol, so use it with caution. It is recommended
    /// that **all custom data starts with prefix "_".**
    ///
    /// # Example
    /// This creates a document with text _"Rust 1.0 was released on 15.05.2015."_.
    /// The text "Rust" is orange. Currently typst floats aren't displaying a decimal
    /// point when passed a floating point that can be converted to integer without loss.
    /// That's why there's ".0" after "#_VERSION", it is not a tuple index.
    /// ```
    /// use typst_lib_wrapper::reexports::{IntoValue, Datetime, Color};
    ///
    /// let content = r##"
    ///     #set page(paper: "a4");
    ///
    ///     #text(fill: _COLOR)[#_LANGUAGE]
    ///     #_VERSION.0 was released on
    ///     #_DATE.display("[day].[month].[year]").
    /// "##;
    ///
    /// let language = "Rust".into_value();
    /// let date = Datetime::from_ymd(2015, 5, 15).expect("Invalid date").into_value();
    /// let color = Color::from_u8(247, 75, 0, 255).into_value();
    /// let version = 1.0_f64.into_value();
    ///
    /// let custom_data = vec![
    ///     ("_LANGUAGE", language),
    ///     ("_DATE", date),
    ///     ("_COLOR", color),
    ///     ("_VERSION", version)
    /// ];
    ///
    /// let compiler = CompilerBuilder::with_content_input(content)
    ///     .with_custom_data(custom_data)
    ///     .build()
    ///     .expect("Couldn't build the compiler");
    /// ```
    pub fn with_custom_data(mut self, custom_data: Vec<(impl ToString, impl IntoValue)>) -> Self {
        let mapped: Vec<(String, Value)> = custom_data
            .into_iter()
            .map(|(key, value)| (key.to_string(), value.into_value()))
            .collect();
        self.custom_data = mapped;
        self
    }

    /// Adds a single value to global typst symbols overriding the typst library.
    ///
    /// # Note / Warning
    /// Mind that this will overload ANY symbol, so use it with caution. It is recommended
    /// that **all custom data starts with prefix "_".**
    ///
    /// # Example
    /// This creates a document with text _"Rust 1.0 was released on 15.05.2015."_.
    /// The text "Rust" is orange. Currently typst floats aren't displaying a decimal
    /// point when passed a floating point that can be converted to integer without loss.
    /// That's why there's ".0" after "#_VERSION", it is not a tuple index.
    /// ```
    /// use typst_lib_wrapper::reexports::{IntoValue, Datetime, Color};
    ///
    /// let content = r##"
    ///     #set page(paper: "a4");
    ///
    ///     #text(fill: _COLOR)[#_LANGUAGE]
    ///     #_VERSION.0 was released on
    ///     #_DATE.display("[day].[month].[year]").
    /// "##;
    ///
    /// let language = "Rust";
    /// let date = Datetime::from_ymd(2015, 5, 15).expect("Invalid date");
    /// let color = Color::from_u8(247, 75, 0, 255);
    /// let version = 1.0_f64;
    ///
    /// let compiler = CompilerBuilder::with_content_input(content)
    ///     .add_custom_data_one(("_LANGUAGE", language))
    ///     .add_custom_data_one(("_DATE", date))
    ///     .add_custom_data_one(("_COLOR", color))
    ///     .add_custom_data_one(("_VERSION", version))
    ///     .build()
    ///     .expect("Couldn't build the compiler");
    /// ```
    pub fn add_custom_data_one(mut self, custom_data: (impl ToString, impl IntoValue)) -> Self {
        self.custom_data.push((custom_data.0.to_string(), custom_data.1.into_value()));
        self
    }

    /// Adds multiple values to global typst symbols overriding the typst library.
    ///
    /// # Note / Warning
    /// Mind that this will overload ANY symbol, so use it with caution. It is recommended
    /// that **all custom data starts with prefix "_".**
    ///
    /// # Example
    /// This creates a document with text _"Rust 1.0 was released on 15.05.2015."_.
    /// The text "Rust" is orange. Currently typst floats aren't displaying a decimal
    /// point when passed a floating point that can be converted to integer without loss.
    /// That's why there's ".0" after "#_VERSION", it is not a tuple index.
    /// ```
    /// use typst_lib_wrapper::reexports::{IntoValue, Datetime, Color};
    ///
    /// let content = r##"
    ///     #set page(paper: "a4");
    ///
    ///     #text(fill: _COLOR)[#_LANGUAGE]
    ///     #_VERSION.0 was released on
    ///     #_DATE.display("[day].[month].[year]").
    /// "##;
    ///
    /// let language = "Rust".into_value();
    /// let date = Datetime::from_ymd(2015, 5, 15).expect("Invalid date").into_value();
    /// let color = Color::from_u8(247, 75, 0, 255).into_value();
    /// let version = 1.0_f64.into_value();
    ///
    /// let custom_data_1 = vec![("_LANGUAGE", language), ("_DATE", date)];
    /// let custom_data_2 = vec![("_COLOR", color), ("_VERSION", version)];
    ///
    /// let compiler = CompilerBuilder::with_content_input(content)
    ///     .add_custom_data_many(custom_data_1)
    ///     .add_custom_data_many(custom_data_2)
    ///     .build()
    ///     .expect("Couldn't build the compiler");
    /// ```
    pub fn add_custom_data_many(mut self, custom_data: Vec<(impl ToString, impl IntoValue)>) -> Self {
        let mapped: Vec<(String, Value)> = custom_data
            .into_iter()
            .map(|(key, value)| (key.to_string(), value.into_value()))
            .collect();

        self.custom_data.extend(mapped);
        self
    }

    /// Provides a way to add additional fonts to the [FontCache].
    ///
    /// # Note / Warning
    /// ### [FontCache] cloning
    /// Before building the compiler, builder locks [FontCache] and **clones** it for itself.
    /// To reduce [FontCache] size it is recommended to avoid loading all system fonts and
    /// to load only needed fonts. In practise this won't be that big of a deal, because
    /// all fonts are lazily loaded into memory, but they stay there, so **manually empty**
    /// the [FontCache].
    ///
    /// # Example
    /// Loads custom fonts into [FontCache].
    /// ```
    /// let content = r##"
    ///     #set page(paper: "a4");
    ///
    ///     #set text(font: "Times New Roman");
    ///     This is Times New Roman.
    ///
    ///     #set text(font: "Comic Sans");
    ///     This is Comic Sans.
    /// "##;
    ///
    /// let font_paths = vec![
    ///     "./fonts/times_new_roman.ttf",
    ///     "./fonts/comic_sans.ttf"
    /// ];
    ///
    /// let compiler = CompilerBuilder::with_content_input(content)
    ///     .with_font_paths(font_paths)
    ///     .build()
    ///     .expect("Couldn't build the compiler");
    /// ```
    pub fn with_font_paths(mut self, font_paths: Vec<impl Into<PathBuf>>) -> Self {
        self.font_paths = font_paths.into_iter().map(|x| x.into()).collect();
        self
    }

    /// Adds additional font path to the [FontCache].
    ///
    /// # Note / Warning
    /// ### [FontCache] cloning
    /// Before building the compiler, builder locks [FontCache] and **clones** it for itself.
    /// To reduce [FontCache] size it is recommended to avoid loading all system fonts and
    /// to load only needed fonts. In practise this won't be that big of a deal, because
    /// all fonts are lazily loaded into memory, but they stay there, so **manually empty**
    /// the [FontCache].
    ///
    /// # Example
    /// Loads custom fonts into [FontCache].
    /// ```
    /// let content = r##"
    ///     #set page(paper: "a4");
    ///
    ///     #set text(font: "Times New Roman");
    ///     This is Times New Roman.
    ///
    ///     #set text(font: "Comic Sans");
    ///     This is Comic Sans.
    /// "##;
    ///
    /// let compiler = CompilerBuilder::with_content_input(content)
    ///     .add_font_path("./fonts/times_new_roman.ttf")
    ///     .add_font_path("./fonts/comic_sans.ttf")
    ///     .build()
    ///     .expect("Couldn't build the compiler");
    /// ```
    pub fn add_font_path(mut self, font_paths: impl Into<PathBuf>) -> Self {
        self.font_paths.push(font_paths.into());
        self
    }

    /// Adds additional font paths to the [FontCache].
    ///
    /// # Note / Warning
    /// ### [FontCache] cloning
    /// Before building the compiler, builder locks [FontCache] and **clones** it for itself.
    /// To reduce [FontCache] size it is recommended to avoid loading all system fonts and
    /// to load only needed fonts. In practise this won't be that big of a deal, because
    /// all fonts are lazily loaded into memory, but they stay there, so **manually empty**
    /// the [FontCache].
    ///
    /// # Example
    /// Loads custom fonts into [FontCache].
    /// ```
    /// let content = r##"
    ///     #set page(paper: "a4");
    ///
    ///     #set text(font: "Times New Roman");
    ///     This is Times New Roman.
    ///
    ///     #set text(font: "Comic Sans");
    ///     This is Comic Sans.
    /// "##;
    ///
    /// let font_paths_1 = vec!["./fonts/times_new_roman.ttf"];
    /// let font_paths_2 = vec!["./fonts/comic_sans.ttf"];
    ///
    /// let compiler = CompilerBuilder::with_content_input(content)
    ///     .add_font_paths(font_paths_1)
    ///     .add_font_paths(font_paths_2)
    ///     .build()
    ///     .expect("Couldn't build the compiler");
    /// ```
    pub fn add_font_paths(mut self, font_paths: Vec<impl Into<PathBuf>>) -> Self {
        let mapped: Vec<PathBuf> = font_paths.into_iter().map(|x| x.into()).collect();
        self.font_paths.extend(mapped);
        self
    }

    /// ## Pixels per inch.
    /// Default value: 144.0
    ///
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
    /// Color::from_u8(0, 0, 0, 0)
    /// ```
    /// # Note
    /// Ignored if not compiling to PNG.
    pub fn with_background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }

    /// ## PDF/A output
    /// Default value: false
    ///
    /// Enables creation of PDF/A files.
    ///
    /// # Note
    /// Ignored if not compiling to PDF.
    pub fn with_pdf_a(mut self, pdf_a: bool) -> Self {
        self.pdf_a = Some(pdf_a);
        self
    }

    /// Optional [ureq::Agent]
    ///
    /// Used for downloading packages from the repository. Primarily exists to enable loading
    /// custom TLS configuration.
    ///
    /// # Example
    /// How to create [ureq::Agent].
    /// ```
    /// let agent = ureq::AgentBuilder::new().build();
    ///
    /// let content = r##"
    ///     #set page(paper: "a4");
    ///     = Hello World
    ///     Hello world from typst.
    /// "##;
    ///
    /// let compiler = CompilerBuilder::with_content_input(content)
    ///     .with_agent(agent)
    ///     .build()
    ///     .expect("Couldn't build the compiler");
    /// ```
    pub fn with_agent(mut self, agent: ureq::Agent) -> Self {
        self.agent = Some(agent);
        self
    }

    /// Finalizes the configuration and takes ownership of the [CompilerBuilder].
    /// Returns an error if something goes wrong.
    ///
    /// # Note / Warning
    /// ### Blocking Mutex
    /// It is recommended to add fonts using [FontCache] to avoid locking the font mutex
    /// for too long. This mutex is **NOT ASYNC** so keep that in mind. Use **'blocking task'**
    /// if you wish to compile documents in an async environment. Also, if compiling with an
    /// opt-in feature (`"parallel_compilation"`) to PNGs or SVGs, the compiler tries to
    /// encode/convert images to bytes in parallel with `rayon`. To sync up compiled pages,
    /// again it uses **SYNC** mutex. \
    /// [On mixing `rayon` with `tokio`!](https://blog.dureuill.net/articles/dont-mix-rayon-tokio/)
    ///
    /// ### [FontCache] cloning
    /// Before building the compiler, builder locks [FontCache] and **clones** it for itself.
    /// To reduce [FontCache] size it is recommended to avoid loading all system fonts and
    /// to load only needed fonts. In practise this won't be that big of a deal, because
    /// all fonts are lazily loaded into memory, but they stay there, so **manually empty**
    /// the [FontCache].
    ///
    /// ### Filename restrictions
    /// Do not use any filenames or paths that contain text
    /// **`"CUSTOM_SOURCE_CONTENT_INPUT_IN_MEMORY_FILE"`**. \
    /// For more information check the main ReadMe file.
    ///
    /// **⚠ You have been warned ⚠**
    pub fn build(self) -> WrapperResult<Compiler> {

        // Prevents forbidden filename/path input.
        if self.input.is_forbidden() {
            return Err(WrapperError::ForbiddenFilenamePathText);
        }

        let http_client = create_http_agent(self.agent);

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

            let key_eco = ecow::EcoString::from(key);
            library
                .global
                .scope_mut()
                .define_captured(key_eco, value, Capturer::Function, Span::detached());
        }

        let root_path: PathBuf;
        let entry: Source = match self.input {
            Input::Content(c) => {
                root_path = PathBuf::from(".");
                let vpath = VirtualPath::new(crate::RESERVED_IN_MEMORY_IDENTIFIER);
                Source::new(FileId::new(None, vpath), c)
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

        // Skips adding fonts to the font cache if no custom paths provided.
        if !self.font_paths.is_empty() {
            FontCache::insert_many(self.font_paths)?;
        }
        // Gets all necessary font information.
        let (book, fonts) = FontCache::get_book_and_fonts()?;

        Ok(Compiler {
            root: root_path,
            entry,
            files: Mutex::new(files),
            pdf_a: self.pdf_a.unwrap_or(false),

            library: LazyHash::new(library),
            book: LazyHash::new(book),
            fonts,

            http_client,

            ppi,
            background,
            now,
        })
    }
}
