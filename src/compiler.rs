//! Provides a way to compile typst [Document] to PDF, PNG or SVG.

use std::collections::HashMap;
use std::path::PathBuf;

use comemo::Prehashed;
use parking_lot::Mutex;
use rayon::iter::{IndexedParallelIterator, IntoParallelRefIterator, ParallelIterator};
use ecow::EcoVec;
use typst::diag::{FileResult, SourceDiagnostic};
use typst::eval::Tracer;
use typst::foundations::{Bytes, Datetime, Smart};
use typst::model::Document;
use typst::text::{Font, FontBook};
use typst::{Library, World};
use typst::visualize::Color;
use typst_syntax::{FileId, Source, Span};

use crate::files::LazyFile;
use crate::fonts::LazyFont;
use crate::parameters::CompilerOutput;

/// [Compiler] instance build from [CompilerBuilder](crate::builder::CompilerBuilder). \
/// Compiles [Document] to PDF, PNG or SVG by using `compile_` methods.
///
/// # Note
/// Please read all [CompilerBuilder](crate::builder::CompilerBuilder) warnings, also read
/// `compile_` methods warnings.
///
/// # Example
/// Compiles [Document] to PDF file and saves the result.
/// ```
///     let entry = "main.typ";
///     let root = "./project";
///
///     // Build the compiler and compile to PDF.
///     let compiler = CompilerBuilder::with_file_input(entry, root)
///         .build()
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
#[derive(Debug)]
pub struct Compiler {
    pub(crate) root: PathBuf,
    pub(crate) entry: Source,
    pub(crate) files: Mutex<HashMap<FileId, LazyFile>>,

    pub(crate) library: Prehashed<Library>,
    pub(crate) book: Prehashed<FontBook>,
    pub(crate) fonts: Vec<LazyFont>,

    pub(crate) http_client: ureq::Agent,

    pub(crate) ppi: f32,
    pub(crate) background: Color,
    pub(crate) now: chrono::DateTime<chrono::Utc>,
}

/// A world that provides access to the operating system.
///
/// [Docs](https://docs.rs/crate/typst-cli/latest/source/src/world.rs)
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
    /// If no offset is specified, the local date should be chosen. Otherwise, the UTC
    /// date should be chosen with the corresponding offset in hours.
    ///
    /// If this function returns `None`, Typst's `datetime` function will return an error.
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

        return Self::date_convert_ymd(with_offset);
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

    /// Converts [chrono::Datelike] to [typst::foundations::Datetime]. \
    /// Ignores time, uses just date. If the conversion fails, returns `None`.
    ///
    /// ### Used internally.
    fn date_convert_ymd(input: impl chrono::Datelike) -> Option<Datetime> {
        Datetime::from_ymd(
            input.year(),
            input.month().try_into().ok()?,
            input.day().try_into().ok()?,
        )
    }

    /// Converts [chrono::Datelike] and [chrono::Timelike] to [typst::foundations::Datetime]. \
    /// Uses both date and time. If the conversion fails, returns `None`.
    ///
    /// ### Used internally.
    fn date_convert_ymd_hms(input: impl chrono::Datelike + chrono::Timelike) -> Option<Datetime> {
        Datetime::from_ymd_hms(
            input.year(),
            input.month().try_into().ok()?,
            input.day().try_into().ok()?,
            input.hour().try_into().ok()?,
            input.minute().try_into().ok()?,
            input.second().try_into().ok()?,
        )
    }

    /// Compiles `self` into a typst document. \
    /// Function returns a tuple with optional [Document] and [SourceDiagnostic] [EcoVec].
    ///
    /// Returns [Document] [CompilerOutput]. \
    /// If there's an error during compilation it will return `None` variant for `output`, also \
    /// the `errors` vector will be populated. Even if the compilation is successfull the \
    /// warnings can still occur.
    ///
    /// # Note / Warning
    /// This will lock the [FontCache](crate::fonts::FontCache) [Mutex] and update it with lazily \
    /// loaded fonts. This mutex is **NOT ASYNC** so keep that in mind. \
    /// Please use **'blocking task'** provided by your async runtime.
    ///
    /// ### Used internally
    fn compile_document(&self) -> CompilerOutput<Document> {
        let mut tracer = Tracer::new();
        let compilation_result = typst::compile(self, &mut tracer);
        let warnings = tracer.warnings();

        // TODO: UPDATE FONT CACHE and add warning to all methods using this one.

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

    /// Compiles typst [Document] into PDF bytes. \
    /// Returns [Vec\<u8\>](Vec) [CompilerOutput].
    ///
    /// # Note / Warning
    /// This will lock the [FontCache](crate::fonts::FontCache) [Mutex] and update it with lazily \
    /// loaded fonts. This mutex is **NOT ASYNC** so keep that in mind. \
    /// Please use **'blocking task'** provided by your async runtime.
    ///
    /// # Example
    /// Compiles [Document] to PDF file and saves the result.
    /// ```
    ///     let entry = "main.typ";
    ///     let root = "./project";
    ///
    ///     // Build the compiler and compile to PDF.
    ///     let compiler = CompilerBuilder::with_file_input(entry, root)
    ///         .build()
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
    pub fn compile_pdf(&self) -> CompilerOutput<Vec<u8>> {
        let compiler_output: CompilerOutput<Document> = self.compile_document();
        let errors = compiler_output.errors;
        let warnings = compiler_output.warnings;

        let document: Document = match compiler_output.output {
            Some(doc) => doc,
            None => return CompilerOutput {
                output: None,
                errors,
                warnings
            }
        };
        let timestamp = Self::date_convert_ymd_hms(self.now);
        let pdf_bytes = typst_pdf::pdf(&document, Smart::Auto, timestamp);

        return CompilerOutput {
            output: Some(pdf_bytes),
            errors,
            warnings
        };
    }

    /// Compiles typst [Document] into a collection of PNG bytes. One item for each page. \
    /// Returns [Vec\<Vec\<u8\>\>](Vec) [CompilerOutput].
    ///
    /// # Note / Warning
    /// This will lock the [FontCache](crate::fonts::FontCache) [Mutex] and update it with lazily \
    /// loaded fonts. This mutex is **NOT ASYNC** so keep that in mind. \
    /// When compiling to PNGs or SVGs, the compiler tries \
    /// to encode/convert images to bytes in parallel. \
    /// To sync up compiled pages, again it uses **SYNC** mutex. \
    /// Please use **'blocking task'** provided by your async runtime.
    ///
    /// # Example
    /// Compiles [Document] to multiple PNGs and saves them all.
    /// ```
    ///     let entry = "main.typ";
    ///     let root = "./project";
    ///
    ///     // Build the compiler and compile to PNG.
    ///     let compiler = CompilerBuilder::with_file_input(entry, root)
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
    pub fn compile_png(&self) -> CompilerOutput<Vec<Vec<u8>>> {
        let compiler_output: CompilerOutput<Document> = self.compile_document();
        let errors = compiler_output.errors;
        let warnings = compiler_output.warnings;

        let document: Document = match compiler_output.output {
            Some(doc) => doc,
            None => return CompilerOutput {
                output: None, // 'Bubbles up' `None` variant.
                errors,
                warnings
            }
        };

        // Gets number of pages in a document and allocates memory upfront.
        // Because of parallel PNG encoding, the pages buffer needs to be inside a mutex.
        // The same applies to errors.
        let pages_count = document.pages.len();
        let shared_pages_buffer: Mutex<Vec<Vec<u8>>> = Mutex::new(
            vec![Vec::new(); pages_count]
        );
        let shared_errors: Mutex<EcoVec<SourceDiagnostic>> = Mutex::new(errors);

        let _ = document
            .pages
            .par_iter() // Tries to encode pages to PNG in parallel.
            .enumerate()
            .map(|(page_index, page)| {
                // Tries to encode page frame.
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
        // encoding error occured. Discards all pages if any encoutered an error.
        let pages = shared_pages_buffer.into_inner(); // Takes pages from the mutex.
        let encoding_error_occured = pages.iter().any(|x| x.is_empty());
        let output: Option<Vec<Vec<u8>>> = if encoding_error_occured {
            None
        } else {
            Some(pages)
        };

        return CompilerOutput {
            output,
            errors: shared_errors.into_inner(), // Takes errors from the mutex.
            warnings
        };
    }

    /// Compiles typst [Document] into a collection of SVG bytes. One item for each page. \
    /// Returns [Vec\<Vec\<u8\>\>](Vec) [CompilerOutput].
    ///
    /// # Note / Warning
    /// This will lock the [FontCache](crate::fonts::FontCache) [Mutex] and update it with lazily \
    /// loaded fonts. This mutex is **NOT ASYNC** so keep that in mind. \
    /// When compiling to PNGs or SVGs, the compiler tries \
    /// to encode/convert images to bytes in parallel. \
    /// To sync up compiled pages, again it uses **SYNC** mutex. \
    /// Please use **'blocking task'** provided by your async runtime.
    ///
    /// # Example
    /// Compiles [Document] to multiple SVGs and saves them all.
    /// ```
    ///     let entry = "main.typ";
    ///     let root = "./project";
    ///
    ///     // Build the compiler and compile to SVG.
    ///     let compiler = CompilerBuilder::with_file_input(entry, root)
    ///         .build()
    ///         .expect("Couldn't build the compiler");
    ///     let compiled = compiler.compile_svg();
    ///
    ///     if let Some(pages) = compiled.output {
    ///         // Writes images one by one.
    ///         pages.iter().enumerate().for_each(|(index, page)| {
    ///             let filename = format!("./output/{index}.svg");
    ///             std::fs::write(filename, page)
    ///                 .expect("Couldn't write SVG");
    ///         });
    ///     } else {
    ///         dbg!(compiled.errors); // Compilation failed, show errors.
    ///     }
    /// ```
    pub fn compile_svg(&self) -> CompilerOutput<Vec<Vec<u8>>> {
        let compiler_output: CompilerOutput<Document> = self.compile_document();
        let errors = compiler_output.errors;
        let warnings = compiler_output.warnings;

        let document: Document = match compiler_output.output {
            Some(doc) => doc,
            None => return CompilerOutput {
                output: None, // 'Bubbles up' `None` variant.
                errors,
                warnings
            }
        };

        // Gets number of pages in a document and allocates memory upfront.
        // Because of parallel SVG compiling, the pages buffer needs to be inside a mutex.
        // The same applies to errors.
        let pages_count = document.pages.len();
        let shared_pages_buffer: Mutex<Vec<Vec<u8>>> = Mutex::new(
            vec![Vec::new(); pages_count]
        );
        let shared_errors: Mutex<EcoVec<SourceDiagnostic>> = Mutex::new(errors);

        let _ = document
            .pages
            .par_iter() // Tries to compile pages to SVG in parallel.
            .enumerate()
            .map(|(page_index, page)| {
                // Write SVG to the shared buffer.
                let buf = typst_svg::svg(&page.frame).into_bytes();
                {
                    shared_pages_buffer.lock()[page_index] = buf;
                }
            }).collect::<Vec<()>>();


        // Gets pages from the mutex and checks if any `page vector` is empty, which indicates
        // encoding error occured. Discards all pages if any encoutered an error.
        let pages = shared_pages_buffer.into_inner(); // Takes pages from the mutex.
        let encoding_error_occured = pages.iter().any(|x| x.is_empty());
        let output: Option<Vec<Vec<u8>>> = if encoding_error_occured {
            None
        } else {
            Some(pages)
        };

        return CompilerOutput {
            output,
            errors: shared_errors.into_inner(), // Takes errors from the mutex.
            warnings
        };
    }
}
