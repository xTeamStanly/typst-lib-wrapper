use std::collections::HashMap;
use std::path::PathBuf;

use chrono::Datelike;
use comemo::Prehashed;
use parking_lot::Mutex;
use rayon::iter::{IndexedParallelIterator, IntoParallelIterator, IntoParallelRefIterator, ParallelIterator};
use ecow::EcoVec;
use typst::diag::{FileResult, SourceDiagnostic};
use typst::eval::Tracer;
use typst::foundations::{Bytes, Datetime};
use typst::model::Document;
use typst::text::{Font, FontBook};
use typst::{Library, World};
use typst::visualize::Color;
use typst_syntax::{FileId, Source, Span};

use crate::files::LazyFile;
use crate::fonts::LazyFont;
use crate::parameters::{Format, CompilerOutput};

#[derive(Debug)]
pub struct Compiler {
    pub(crate) root: PathBuf,
    pub(crate) entry: Source,
    pub(crate) files: Mutex<HashMap<FileId, LazyFile>>,

    pub(crate) library: Prehashed<Library>,
    pub(crate) book: Prehashed<FontBook>,
    pub(crate) fonts: Vec<LazyFont>,

    pub(crate) http_client: ureq::Agent,

    pub(crate) format: Format,
    pub(crate) ppi: f32,
    pub(crate) background: Color,
    pub(crate) now: chrono::DateTime<chrono::Utc>,
}

/// A world that provides access to the operating system.
///
/// https://docs.rs/crate/typst-cli/latest/source/src/world.rs
impl World for Compiler {

    /// The standard library.
    ///
    /// Can be created through `Library::build()`.
    fn library(&self) -> &Prehashed<Library> {
        &self.library
    }

    /// Metadata about all known fonts.
    fn book(&self) -> &Prehashed<FontBook> {
        &self.book
    }

    /// Access the main source file.
    fn main(&self) -> Source {
        self.entry.clone()
    }

    /// Try to access the specified source file.
    fn source(&self, id: FileId) -> FileResult<Source> {
        self.slot(id, |slot| slot.source(&self.root, &self.http_client))
    }

    /// Try to access the specified file.
    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.slot(id, |slot| slot.file(&self.root, &self.http_client))
    }

    /// Try to access the font with the given index in the font book.
    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.get(index)?.get()
    }

    /// Get the current date.
    ///
    /// If no offset is specified, the local date should be chosen. Otherwise,
    /// the UTC date should be chosen with the corresponding offset in hours.
    ///
    /// If this function returns `None`, Typst's `datetime` function will
    /// return an error.
    fn today(&self, offset: Option<i64>) -> Option<Datetime> {
        // The time with the specified UTC offset, or within the local time zone.
        let with_offset = match offset {
            None => self.now.with_timezone(&chrono::Local).fixed_offset(),
            Some(hours) => {
                let seconds = i32::try_from(hours).ok()?.checked_mul(3600)?;
                self.now
                    .with_timezone(&chrono::FixedOffset::east_opt(seconds)?)
            }
        };

        Datetime::from_ymd(
            with_offset.year(),
            with_offset.month().try_into().ok()?,
            with_offset.day().try_into().ok()?,
        )
    }
}

impl Compiler {
    /// Access the canonical slot for the given file id.
    fn slot<F, T>(&self, id: FileId, f: F) -> T
    where
        F: FnOnce(&mut LazyFile) -> T,
    {
        let mut map = self.files.lock();
        f(map.entry(id).or_insert_with(|| LazyFile::new(id)))
    }

    pub fn save() {
    }

    pub fn save_pdf(&self, path: PathBuf) {
        let mut tracer = Tracer::new();
        let document = typst::compile(self, &mut tracer).unwrap();

        let bytes = typst_pdf::pdf(&document, typst::foundations::Smart::Auto, None);

        #[cfg(debug_assertions)]
        dbg!(tracer.warnings());

        std::fs::write(path, bytes).unwrap();
    }

    /// Compiles `self` into a typst document. \
    /// Function returns a tuple with optional [Document] and [SourceDiagnostic] [EcoVec].
    ///
    /// Returns [Document] [CompilerOutput].
    /// If there's an error during compilation it will return `None` variant for `data`, also
    /// the `errors` vector will be populated. Even if the compilation is successfull the
    /// warnings can still occur.
    ///
    /// ### Used internally
    fn compile_document(&self) -> CompilerOutput<Document> {
        let mut tracer = Tracer::new();
        let compilation_result = typst::compile(self, &mut tracer);
        let warnings = tracer.warnings();

        return match compilation_result {
            Ok(doc) => CompilerOutput {
                output: Some(doc),
                errors: EcoVec::new(),
                warnings
            },
            Err(err) => CompilerOutput {
                output: None,
                errors: err,
                warnings
            }
        };
    }
    // pub fn compile(&self) -> Output {
    //     Self::compile_document(self);
    //     todo!()
    // }

    pub fn compile_pdf(&self) -> Vec<u8> {
        todo!()
    }

    pub fn compile_png(&self) -> CompilerOutput<Vec<Vec<u8>>> {
        let compiler_output: CompilerOutput<Document> = self.compile_document();
        let mut errors = compiler_output.errors;
        let warnings = compiler_output.warnings;

        let document: Document = match compiler_output.output {
            Some(doc) => doc,
            None => return CompilerOutput {
                output: None,
                errors,
                warnings
            }
        };

        let pages_count = document.pages.len();
        let shared_pages_buffer: Mutex<Vec<Vec<u8>>> = Mutex::new(
            vec![Vec::new(); pages_count]
        );
        let shared_errors: Mutex<EcoVec<SourceDiagnostic>> = Mutex::new(errors);

        let _ = document
            .pages
            .par_iter()
            .enumerate()
            .map(|(page_index, page)| {
                // Tries to encode page frame
                match typst_render::render(&page.frame, self.ppi / 72.0, self.background)
                    .encode_png()
                {
                    Ok(buf) => { // Write encoded PNG to the shared buffer.
                        {
                            shared_pages_buffer.lock()[page_index] = buf;
                        }
                    },
                    Err(err) => { // Write error to the shared errors list.
                        let encoding_error = SourceDiagnostic::error(
                            Span::detached(), err.to_string()
                        );

                        {
                            shared_errors.lock().push(encoding_error);
                        }
                    }
                };
            }).collect::<Vec<()>>();


        // Gets pages from the mutex and checks if any `page vector` is empty, which indicates
        // encoding error occured.
        let pages = shared_pages_buffer.into_inner(); // Gets pages from the mutex.
        let encoding_error_occured = pages.iter().any(|x| x.is_empty());

        let output: Option<Vec<Vec<u8>>> = if encoding_error_occured {
            None
        } else {
            Some(pages)
        };

        return CompilerOutput {
            output,
            errors: shared_errors.into_inner(),
            warnings
        };
    }

    pub fn compile_svg(&self) -> Vec<Vec<u8>> {
        todo!()
    }
}
