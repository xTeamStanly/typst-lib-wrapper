use std::collections::HashMap;
use std::fmt::Debug;
use std::path::{Path, PathBuf};

use comemo::Prehashed;
use native_tls::Certificate;
use parking_lot::Mutex;
use typst::foundations::{Capturer, IntoValue};
use typst::foundations::{Dict, Value};
use typst::text::FontBook;
use typst::{Library, LibraryBuilder};
use typst_syntax::{FileId, Source, VirtualPath};

// use super::{Format, Input};

use crate::compiler::Compiler;
use crate::errors::{WrapperError, WrapperResult};
use crate::files::LazyFile;
use crate::fonts::FontCache;
use crate::package::create_http_agent;
use crate::parameters::{Format, Input};

#[derive(Clone)]
pub struct CompilerBuilder {
    pub(crate) input: Input,
    pub(crate) format: Option<Format>,

    pub(crate) sys_inputs: Vec<(String, String)>,
    pub(crate) custom_data: Vec<(String, Value)>,

    pub(crate) font_paths: Vec<PathBuf>,
    pub(crate) ppi: Option<f32>,

    /// Optional X509 [certificate](Certificate).
    /// # Example
    ///
    /// ```rust
    /// // Loads and parses the certificate.
    /// let bytes = std::fs::read("my_certificate.pem").expect("Coudn't read the certificate");
    /// let certificate = Certificate::from_pem(&bytes).expect("Invalid certificate");
    /// // Creates HTTP agent.
    /// let agent = create_http_agent(Some(certificate)).expect("Coudn't create HTTP Agent");
    /// ```
    pub(crate) certificate: Option<Certificate>,
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
            .field("format", &self.format)
            .field("sys_inputs", &self.sys_inputs)
            .field("custom_data", &self.custom_data)
            .field("font_paths", &self.font_paths)
            .field("ppi", &self.ppi)
            .field("certificate", &certificate_fmt)
            .finish()
    }
}

impl CompilerBuilder {
    pub fn with_input(input: Input) -> Self {
        Self {
            input,
            format: None,

            sys_inputs: Vec::new(),
            custom_data: Vec::new(),

            font_paths: Vec::new(),
            ppi: None,
            certificate: None,
        }
    }

    pub fn with_file_input(entry: String, root: PathBuf) -> Self {
        let input = Input::File { entry, root };
        return Self::with_input(input);
    }
    pub fn with_content_input(content: String) -> Self {
        let input = Input::Content(content);
        return Self::with_input(input);
    }

    pub fn with_format(mut self, format: Format) -> Self {
        self.format = Some(format);
        self
    }

    pub fn with_sys_inputs(mut self, sys_inputs: Vec<(String, String)>) -> Self {
        self.sys_inputs = sys_inputs;
        self
    }
    pub fn add_sys_input(mut self, sys_input: (String, String)) -> Self {
        self.sys_inputs.push(sys_input);
        self
    }
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

    pub fn with_ppi(mut self, ppi: f32) -> Self {
        self.ppi = Some(ppi);
        self
    }

    pub fn with_certificate(mut self, certificate: Certificate) -> Self {
        self.certificate = Some(certificate);
        self
    }

    pub fn build(self) -> WrapperResult<Compiler> {
        let http_client = create_http_agent(self.certificate)?;

        let now = chrono::Utc::now();
        let format: Format = self.format.unwrap_or_default();
        let ppi: f32 = self.ppi.unwrap_or(144.0); // default typst ppi: 144.0
        let mut files: HashMap<FileId, LazyFile> = HashMap::new();

        // Convert the input pairs to a dictionary.
        let sys_inputs: Dict = self
            .sys_inputs
            .into_iter()
            .map(|(key, value)| (key.into(), value.into_value()))
            .collect();
        let mut library = LibraryBuilder::default().with_inputs(sys_inputs).build();

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

            format,
            ppi,
            now,
        })
    }
}
