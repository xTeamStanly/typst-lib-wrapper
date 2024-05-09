use std::{collections::HashMap, fmt::Debug, hash::Hash, path::{Path, PathBuf}};

use crate::{blocking::{CompilerOptions as CompilerOptionsSync, FontCache, LazyFile as LazyFileSync, LazyFont}, errors::{WrapperError, WrapperResult}, shared::create_http_agent};

use comemo::Prehashed;
use parking_lot::Mutex;
use typst::{foundations::{Capturer, IntoValue}, text::FontBook, Library, LibraryBuilder};
use typst_syntax::{FileId, Source, VirtualPath};

use super::{Format, Input};
use native_tls::Certificate;
use typst::foundations::{Dict, Value};

#[derive(Clone)]
pub struct CompilerOptionsBuilder {
    pub(crate) input: Input,
    pub(crate) format: Option<Format>,

    pub(crate) sys_inputs: Vec<(String, String)>,
    pub(crate) custom_data: Vec<(String, Value)>,

    pub(crate) font_paths: Vec<PathBuf>,
    pub(crate) ppi: Option<f32>,
    pub(crate) certificate: Option<Certificate>
}
/// Custom [Debug] implementation, because [Certificate] doesn't implement [Debug].
impl Debug for CompilerOptionsBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let certificate_fmt: &'static str = match &self.certificate {
            Some(_) => "Some(...)",
            None => "None"
        };

        f.debug_struct("CompilerOptionsBuilder")
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

impl CompilerOptionsBuilder {
    pub fn with_input(input: Input) -> Self {
        Self {
            input,
            format: None,

            sys_inputs: Vec::new(),
            custom_data: Vec::new(),

            font_paths: Vec::new(),
            ppi: None,
            certificate: None
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
    pub fn with_sys_inputs_ref(mut self, sys_inputs: &[(&str, &str)]) -> Self {
        let cloned: Vec<(String, String)> = sys_inputs
            .iter()
            .map(|x| (x.0.to_string(), x.1.to_string()))
            .collect();
        self.sys_inputs = cloned;
        self
    }
    pub fn add_sys_input(mut self, sys_input: (String, String)) -> Self {
        self.sys_inputs.push(sys_input);
        self
    }
    pub fn add_sys_input_ref(mut self, sys_input: (&str, &str)) -> Self {
        let cloned = (sys_input.0.to_string(), sys_input.1.to_string());
        self.sys_inputs.push(cloned);
        self
    }
    pub fn add_sys_inputs(mut self, sys_inputs: Vec<(String, String)>) -> Self {
        self.sys_inputs.extend(sys_inputs);
        self
    }
    pub fn add_sys_inputs_ref(mut self, sys_inputs: &[(&str, &str)]) -> Self {
        let cloned = sys_inputs.iter().map(|x| (x.0.to_string(), x.1.to_string()));
        self.sys_inputs.extend(cloned);
        self
    }

    pub fn with_custom_data(mut self, custom_data: Vec<(String, Value)>) -> Self {
        self.custom_data = custom_data;
        self
    }
    pub fn with_custom_data_ref(mut self, custom_data: &[(&str, &Value)]) -> Self {
        let cloned: Vec<(String, Value)> = custom_data
            .iter()
            .map(|x| (x.0.to_string(), x.1.clone()))
            .collect();
        self.custom_data = cloned;
        self
    }
    pub fn add_custom_data_one(mut self, custom_data: (String, Value)) -> Self {
        self.custom_data.push(custom_data);
        self
    }
    pub fn add_custom_data_ref_one(mut self, custom_data: (&str, &Value)) -> Self {
        let cloned = (custom_data.0.to_string(), custom_data.1.clone());
        self.custom_data.push(cloned);
        self
    }
    pub fn add_custom_data_many(mut self, custom_data: Vec<(String, Value)>) -> Self {
        self.custom_data.extend(custom_data);
        self
    }
    pub fn add_custom_data_ref_many(mut self, custom_data: &[(&str, &Value)]) -> Self {
        let cloned = custom_data
            .iter()
            .map(|x| (x.0.to_string(), x.1.clone()));
        self.custom_data.extend(cloned);
        self
    }

    pub fn with_font_paths(mut self, font_paths: Vec<PathBuf>) -> Self {
        self.font_paths = font_paths;
        self
    }
    pub fn with_font_paths_ref(mut self, font_paths: &[&Path]) -> Self {
        let cloned = font_paths
            .iter()
            .map(|x| x.to_path_buf())
            .collect();
        self.font_paths = cloned;
        self
    }
    pub fn add_font_path(mut self, font_paths: PathBuf) -> Self {
        self.font_paths.push(font_paths);
        self
    }
    pub fn add_font_path_ref(mut self, font_path: &Path) -> Self {
        let cloned = font_path.to_path_buf();
        self.font_paths.push(cloned);
        self
    }
    pub fn add_font_paths(mut self, font_paths: Vec<PathBuf>) -> Self {
        self.font_paths.extend(font_paths);
        self
    }
    pub fn add_font_paths_ref(mut self, font_paths: &[&Path]) -> Self {
        let cloned = font_paths
            .iter()
            .map(|x| x.to_path_buf());
        self.font_paths.extend(cloned);
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

    pub fn build_sync(self) -> WrapperResult<CompilerOptionsSync> {
        let http_client = create_http_agent(self.certificate)?;

        let now = chrono::Utc::now();
        let format: Format = self.format.unwrap_or_default();
        let ppi: f32 = self.ppi.unwrap_or(144.0); // default typst ppi: 144.0
        let mut files: HashMap<FileId, LazyFileSync> = HashMap::new();

        // Convert the input pairs to a dictionary.
        let sys_inputs: Dict = self.sys_inputs
            .into_iter()
            .map(|(key, value)| (key.into(), value.into_value()))
            .collect();
        let mut library = LibraryBuilder::default().with_inputs(sys_inputs).build();

        for (key, value) in self.custom_data.into_iter() {
            library.global.scope_mut().define_captured(key, value, Capturer::Function);
        }

        let root_path: PathBuf;
        let entry: Source = match self.input {
            Input::Content(c) => {
                root_path = PathBuf::from(".");
                Source::detached(c)
            },
            Input::File { entry, root } => {

                // Appends `entry` filename to `root`
                let mut entry_path = root.clone();
                entry_path.push(entry);

                // Resolve the system-global root directory.
                let canon_root_path: PathBuf = root.canonicalize().map_err(|err| {
                    match err.kind() {
                        std::io::ErrorKind::NotFound => WrapperError::InputNotFound(root),
                        _ => WrapperError::from(err)
                    }
                })?;

                // Resolve the system-global input path.
                let canon_entry_path: PathBuf = entry_path.canonicalize().map_err(|err| {
                    match err.kind() {
                        std::io::ErrorKind::NotFound => WrapperError::InputNotFound(entry_path),
                        _ => WrapperError::from(err)
                    }
                })?;

                // Resolve the virtual path of the main file within the project root.
                let main_path = VirtualPath::within_root(&canon_entry_path, &canon_root_path)
                    .ok_or(WrapperError::InputOutsideRoot(
                        canon_entry_path,
                        canon_root_path.clone()
                    ))?;
                let main_file_id = FileId::new(None, main_path);

                let entry_file: &mut LazyFileSync = files
                    .entry(main_file_id)
                    .or_insert_with(|| LazyFileSync::new(main_file_id));

                let entry_source = entry_file
                    .source(&canon_root_path, &http_client).map_err(WrapperError::from)?;

                root_path = canon_root_path;
                entry_source
            }
        };

        // TODO: LOAD FONTS + FONT CACHE THINGS
        // let book = FontBook::new();
        // let fonts = Vec::<LazyFont>::new();
        let (book, fonts) = FontCache::get_book_and_fonts().unwrap();

        Ok(CompilerOptionsSync {
            root: root_path,
            entry,
            files: Mutex::new(files),

            library: Prehashed::new(library),
            book: Prehashed::new(book),
            fonts,

            http_client,

            format,
            ppi,
            now
        })
    }

    // pub fn build_async(mut self) -> ! {

    // }
}