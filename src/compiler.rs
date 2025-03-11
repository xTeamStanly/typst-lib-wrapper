//! Provides a way to compile typst Document to PDF, PNG or SVG.

use std::collections::HashMap;
use std::path::PathBuf;

use parking_lot::Mutex;
use ecow::EcoVec;
use typst::diag::{FileResult, SourceDiagnostic, Warned};
use typst_pdf::{PdfOptions, PdfStandard, PdfStandards, Timestamp};
use typst::foundations::{Bytes, Datetime, Smart};
use typst::layout::PagedDocument;
use typst::html::HtmlDocument;
use typst::text::{Font, FontBook};
use typst::{Library, World};
use typst::visualize::{Color, Paint};
use typst_utils::LazyHash;
use typst_syntax::{FileId, Source, Span};

use crate::files::LazyFile;
use crate::fonts::{LazyFont, FontCache};
use crate::parameters::CompilerOutput;



/// [Compiler] instance build from [CompilerBuilder](crate::builder::CompilerBuilder).
///
/// Compiles Document to PDF, PNG or SVG by using `compile_` methods.
///
/// # Note
/// Please read all [CompilerBuilder](crate::builder::CompilerBuilder) warnings, also read
/// `compile_` methods warnings.
///
/// # Example
/// Compiles Document to PDF file and saves the result.
/// ```
/// let entry = "main.typ";
/// let root = "./project";
///
/// // Build the compiler and compile to PDF.
/// let compiler = CompilerBuilder::with_file_input(entry, root)
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
#[derive(Debug)]
pub struct Compiler {
    pub(crate) root: PathBuf,
    pub(crate) entry: Source,
    pub(crate) files: Mutex<HashMap<FileId, LazyFile>>,
    pub(crate) pdf_a: bool,

    pub(crate) library: LazyHash<Library>,
    pub(crate) book: LazyHash<FontBook>,
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
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    /// Metadata about all known fonts.
    fn book(&self) -> &LazyHash<FontBook> {
        &self.book
    }

    /// Access the main source file.
    fn main(&self) -> FileId {
        self.entry.id()
    }

    /// Try to access the specified source file. If the [FileId] points to a "file" with in memory
    /// contents, the contents are retrieved immediately. This is the case for the
    /// [Input::Content](crate::Input::Content).
    fn source(&self, id: FileId) -> FileResult<Source> {
        let in_memory_file = id
            .vpath()
            .as_rootless_path()
            .to_str()
            .map(|x| x.contains(crate::RESERVED_IN_MEMORY_IDENTIFIER))
            .unwrap_or(false);
        if in_memory_file { return Ok(self.entry.clone()); }

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

    /// Converts [chrono::Datelike] to [typst::foundations::Datetime].
    ///
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

    /// Converts [chrono::Datelike] and [chrono::Timelike] to [typst::foundations::Datetime].
    ///
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

    /// Compiles and consumes `self` into a paged typst document.
    ///
    /// Function returns a tuple with optional Document and [SourceDiagnostic] [EcoVec].
    ///
    /// Returns Document [CompilerOutput]. \
    /// If there's an error during compilation it will return `None` variant for `output`, also
    /// the `errors` vector will be populated. Even if the compilation is successfull the
    /// warnings can still occur.
    ///
    /// # Note / Warning
    /// This will lock the [FontCache](crate::fonts::FontCache) Mutex and update it with lazily
    /// loaded fonts. This mutex is **NOT ASYNC** so keep that in mind.
    /// Please use **'blocking task'** provided by your async runtime.
    ///
    /// If compiling with an opt-in feature (`"parallel_compilation"`) to PNGs or SVGs,
    /// the compiler tries to encode/convert images to bytes in parallel with `rayon`.
    /// To sync up compiled pages, again it uses **SYNC** mutex. \
    /// [On mixing `rayon` with `tokio`!](https://blog.dureuill.net/articles/dont-mix-rayon-tokio/)
    ///
    /// ### Used internally.
    fn compile_paged_document(self) -> CompilerOutput<PagedDocument> {
        let Warned { output, warnings } = typst::compile(&self);
        let compilation_result = output;

        // Tries to update the font cache, ignores errors.
        let _ = FontCache::update_cache(self.fonts);

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

    /// Compiles and consumes `self` into a HTML typst document.
    ///
    /// Function returns a tuple with optional Document and [SourceDiagnostic] [EcoVec].
    ///
    /// Returns Document [CompilerOutput]. \
    /// If there's an error during compilation it will return `None` variant for `output`, also
    /// the `errors` vector will be populated. Even if the compilation is successfull the
    /// warnings can still occur.
    ///
    /// # Note / Warning
    /// This will lock the [FontCache](crate::fonts::FontCache) Mutex and update it with lazily
    /// loaded fonts. This mutex is **NOT ASYNC** so keep that in mind.
    /// Please use **'blocking task'** provided by your async runtime.
    ///
    /// If compiling with an opt-in feature (`"parallel_compilation"`) to PNGs or SVGs,
    /// the compiler tries to encode/convert images to bytes in parallel with `rayon`.
    /// To sync up compiled pages, again it uses **SYNC** mutex. \
    /// [On mixing `rayon` with `tokio`!](https://blog.dureuill.net/articles/dont-mix-rayon-tokio/)
    ///
    /// ### Used internally.
    fn compile_html_document(self) -> CompilerOutput<HtmlDocument> {
        let Warned { output, warnings } = typst::compile(&self);
        let compilation_result = output;

        // Tries to update the font cache, ignores errors.
        let _ = FontCache::update_cache(self.fonts);

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

    /// Compiles typst Document into PDF bytes and consumes `self`.
    ///
    /// Returns [Vec\<u8\>](Vec) [CompilerOutput].
    ///
    /// # Note / Warning
    /// This will lock the [FontCache](crate::fonts::FontCache) Mutex and update it with lazily
    /// loaded fonts. This mutex is **NOT ASYNC** so keep that in mind.
    /// Please use **'blocking task'** provided by your async runtime.
    ///
    /// # Example
    /// Compiles Document to PDF file and saves the result.
    /// ```
    /// let entry = "main.typ";
    /// let root = "./project";
    ///
    /// // Build the compiler and compile to PDF.
    /// let compiler = CompilerBuilder::with_file_input(entry, root)
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
    pub fn compile_pdf(self) -> CompilerOutput<Vec<u8>> {
        let timestamp = Self::date_convert_ymd_hms(self.now);
        let pdf_a: bool = self.pdf_a;

        let compiler_output: CompilerOutput<PagedDocument> = self.compile_paged_document();
        let mut errors = compiler_output.errors;
        let warnings = compiler_output.warnings;

        let document: PagedDocument = match compiler_output.output {
            Some(doc) => doc,
            None => return CompilerOutput {
                output: None,
                errors,
                warnings
            }
        };

        // IMPORTANT NOTE: PdfStandards::new(...) should never panic, but we will handle it just in case.
        // https://github.com/typst/typst/blob/7add9b459a3ca54fca085e71f3dd4e611941c4cc/crates/typst-pdf/src/lib.rs#L114
        let pdf_standards = if pdf_a {
            match PdfStandards::new(&[PdfStandard::A_2b]) {
                Ok(pdf_stndr) => pdf_stndr,
                Err(err) => {
                    errors.push(SourceDiagnostic::error(Span::detached(), err));
                    return CompilerOutput {
                        output: None,
                        errors,
                        warnings
                    }
                }
            }
        } else {
            match PdfStandards::new(&[PdfStandard::V_1_7]) {
                Ok(pdf_stndr) => pdf_stndr,
                Err(err) => {
                    errors.push(SourceDiagnostic::error(Span::detached(), err));
                    return CompilerOutput {
                        output: None,
                        errors,
                        warnings
                    }
                }
            }
        };

        let pdf_options = PdfOptions {
            ident: Smart::Auto,
            timestamp: timestamp.map(Timestamp::new_utc),
            standards: pdf_standards,
            page_ranges: None // `None` exports all pages.
        };

        let mut pdf_bytes: Option<Vec<u8>> = None;

        match typst_pdf::pdf(&document, &pdf_options) {
            Ok(bytes) => { pdf_bytes = Some(bytes); },
            Err(err_vec) => { errors.extend(err_vec); }
        };

        return CompilerOutput {
            output: pdf_bytes,
            errors,
            warnings
        };
    }

    /// Compiles typst Document into a collection of PNG bytes and consumes `self`.
    ///
    /// One item for each page. Returns [Vec\<Vec\<u8\>\>](Vec) [CompilerOutput].
    ///
    /// # Note / Warning
    /// This will lock the [FontCache](crate::fonts::FontCache) Mutex and update it with lazily
    /// loaded fonts. This mutex is **NOT ASYNC** so keep that in mind.
    ///
    /// If compiling with an opt-in feature (`"parallel_compilation"`) to PNGs or SVGs,
    /// the compiler tries to encode/convert images to bytes in parallel with `rayon`.
    /// To sync up compiled pages, again it uses **SYNC** mutex. \
    /// [On mixing `rayon` with `tokio`!](https://blog.dureuill.net/articles/dont-mix-rayon-tokio/)
    ///
    /// # Example
    /// Compiles Document to multiple PNGs and saves them all.
    /// ```
    /// let entry = "main.typ";
    /// let root = "./project";
    ///
    /// // Build the compiler and compile to PNG.
    /// let compiler = CompilerBuilder::with_file_input(entry, root)
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
    pub fn compile_png(self) -> CompilerOutput<Vec<Vec<u8>>> {
        let ppi = self.ppi / 72.0;
        let background = self.background;
        let page_background = Smart::Custom(Some(Paint::Solid(background)));

        let compiler_output: CompilerOutput<PagedDocument> = self.compile_paged_document();
        let errors = compiler_output.errors;
        let warnings = compiler_output.warnings;

        let document: PagedDocument = match compiler_output.output {
            Some(doc) => doc,
            None => return CompilerOutput {
                output: None, // 'Bubbles up' `None` variant.
                errors,
                warnings
            }
        };

        let final_pages: Vec<Vec<u8>>;
        let final_errors: EcoVec<SourceDiagnostic>;

        // Sync compilation of pages.
        #[cfg(not(feature = "parallel_compilation"))]
        {
            // Gets number of pages in a document and allocates memory upfront.
            let pages_count = document.pages.len();
            let mut pages_buffer: Vec<Vec<u8>> = vec![Vec::new(); pages_count];
            let mut pages_errors = errors;

            for (page_index, mut page) in document.pages.into_iter().enumerate() {
                page.fill = page_background.clone();

                match typst_render::render(&page, ppi).encode_png() {
                    Ok(buf) => { // Write encoded PNG to the buffer.
                        pages_buffer[page_index] = buf;
                    },
                    Err(err) => { // Write error to the errors list.
                        let encoding_error = SourceDiagnostic::error(
                            Span::detached(), err.to_string()
                        );
                        pages_errors.push(encoding_error);
                    }
                }
            }

            final_pages = pages_buffer;
            final_errors = pages_errors;
        }

        // Parallel compilation of pages.
        #[cfg(feature = "parallel_compilation")]
        {
            use rayon::iter::{IndexedParallelIterator, IntoParallelIterator, ParallelIterator};

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
                .into_par_iter() // Tries to encode pages to PNG in parallel.
                .enumerate()
                .map(|(page_index, mut page)| {
                    page.fill = page_background.clone();

                    // Tries to encode page frame.
                    match typst_render::render(&page, ppi).encode_png() {
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

            // Takes pages and errors from the mutex
            final_pages = shared_pages_buffer.into_inner();
            final_errors = shared_errors.into_inner();
        }

        // Checks if any `page vector` is empty, which indicates
        // encoding error occured. Discards all pages if any encoutered an error.
        let encoding_error_occured = final_pages.iter().any(|x| x.is_empty());
        let output: Option<Vec<Vec<u8>>> = if encoding_error_occured {
            None
        } else {
            Some(final_pages)
        };

        return CompilerOutput {
            output,
            errors: final_errors,
            warnings
        };
    }

    /// Compiles typst Document into a collection of SVG bytes and consumes `self`.
    ///
    /// One item for each page. Returns [Vec\<Vec\<u8\>\>](Vec) [CompilerOutput].
    ///
    /// # Note / Warning
    /// This will lock the [FontCache](crate::fonts::FontCache) Mutex and update it with lazily
    /// loaded fonts. This mutex is **NOT ASYNC** so keep that in mind.
    ///
    /// If compiling with an opt-in feature (`"parallel_compilation"`) to PNGs or SVGs,
    /// the compiler tries to encode/convert images to bytes in parallel with `rayon`.
    /// To sync up compiled pages, again it uses **SYNC** mutex. \
    /// [On mixing `rayon` with `tokio`!](https://blog.dureuill.net/articles/dont-mix-rayon-tokio/)
    ///
    /// # Example
    /// Compiles Document to multiple SVGs and saves them all.
    /// ```
    /// let entry = "main.typ";
    /// let root = "./project";
    ///
    /// // Build the compiler and compile to SVG.
    /// let compiler = CompilerBuilder::with_file_input(entry, root)
    ///     .build()
    ///     .expect("Couldn't build the compiler");
    /// let compiled = compiler.compile_svg();
    ///
    /// if let Some(pages) = compiled.output {
    ///     // Writes images one by one.
    ///     pages.iter().enumerate().for_each(|(index, page)| {
    ///         let filename = format!("./output/{index}.svg");
    ///         std::fs::write(filename, page)
    ///             .expect("Couldn't write SVG");
    ///     });
    /// } else {
    ///     dbg!(compiled.errors); // Compilation failed, show errors.
    /// }
    /// ```
    pub fn compile_svg(self) -> CompilerOutput<Vec<Vec<u8>>> {
        let background = self.background;
        let page_background = Smart::Custom(Some(Paint::Solid(background)));

        let compiler_output: CompilerOutput<PagedDocument> = self.compile_paged_document();
        let errors = compiler_output.errors;
        let warnings = compiler_output.warnings;

        let document: PagedDocument = match compiler_output.output {
            Some(doc) => doc,
            None => return CompilerOutput {
                output: None, // 'Bubbles up' `None` variant.
                errors,
                warnings
            }
        };

        let final_pages: Vec<Vec<u8>>;
        let final_errors: EcoVec<SourceDiagnostic>;

        // Sync compilation of pages.
        #[cfg(not(feature = "parallel_compilation"))]
        {
            // Gets number of pages in a document and allocates memory upfront.
            let pages_count = document.pages.len();
            let mut pages_buffer: Vec<Vec<u8>> = vec![Vec::new(); pages_count];
            let pages_errors = errors;

            for (page_index, mut page) in document.pages.into_iter().enumerate() {
                page.fill = page_background.clone();
                let buf = typst_svg::svg(&page).into_bytes();
                pages_buffer[page_index] = buf;
            }

            final_pages = pages_buffer;
            final_errors = pages_errors;
        }

        // Parallel compilation of pages.
        #[cfg(feature = "parallel_compilation")]
        {
            use rayon::iter::{IndexedParallelIterator, IntoParallelIterator, ParallelIterator};

            // Gets number of pages in a document and allocates memory upfront.
            // Because of parallel SVG encoding, the pages buffer needs to be inside a mutex.
            // The same applies to errors.
            let pages_count = document.pages.len();
            let shared_pages_buffer: Mutex<Vec<Vec<u8>>> = Mutex::new(
                vec![Vec::new(); pages_count]
            );
            let shared_errors: Mutex<EcoVec<SourceDiagnostic>> = Mutex::new(errors);

            let _ = document
                .pages
                .into_par_iter() // Tries to encode pages to SVG in parallel.
                .enumerate()
                .map(|(page_index, mut page)| {
                    page.fill = page_background.clone();

                    // Write SVG to the shared buffer.
                    let buf = typst_svg::svg(&page).into_bytes();
                    {
                        shared_pages_buffer.lock()[page_index] = buf;
                    }
            }).collect::<Vec<()>>();

            // Takes pages and errors from the mutex
            final_pages = shared_pages_buffer.into_inner();
            final_errors = shared_errors.into_inner();
        }

        // Checks if any `page vector` is empty, which indicates
        // that error occured. Discards all pages if any encoutered an error.
        let encoding_error_occured = final_pages.iter().any(|x| x.is_empty());
        let output: Option<Vec<Vec<u8>>> = if encoding_error_occured {
            None
        } else {
            Some(final_pages)
        };

        return CompilerOutput {
            output,
            errors: final_errors,
            warnings
        };
    }

    /// Compiles typst Document into HTML bytes and consumes `self`.
    ///
    /// Returns [String](String) [CompilerOutput].
    ///
    /// # Note / Warning
    /// This will lock the [FontCache](crate::fonts::FontCache) Mutex and update it with lazily
    /// loaded fonts. This mutex is **NOT ASYNC** so keep that in mind.
    /// Please use **'blocking task'** provided by your async runtime.
    ///
    /// # Example
    /// Compiles Document to HTML file and saves the result.
    /// ```
    /// let entry = "main.typ";
    /// let root = "./project";
    ///
    /// // Build the compiler and compile to HTML.
    /// let compiler = CompilerBuilder::with_file_input(entry, root)
    ///     .build()
    ///     .expect("Couldn't build the compiler");
    /// let compiled = compiler.compile_html();
    ///
    /// if let Some(html) = compiled.output {
    ///     std::fs::write("./main.html", html.as_str())
    ///         .expect("Couldn't write HTML"); // Writes HTML file.
    /// } else {
    ///     dbg!(compiled.errors); // Compilation failed, show errors.
    /// }
    /// ```
    pub fn compile_html(self) -> CompilerOutput<String> {
        let compiler_output: CompilerOutput<HtmlDocument> = self.compile_html_document();
        let mut errors = compiler_output.errors;
        let warnings = compiler_output.warnings;

        let document: HtmlDocument = match compiler_output.output {
            Some(doc) => doc,
            None => return CompilerOutput {
                output: None,
                errors,
                warnings
            }
        };

        let mut html_string: Option<String> = None;

        match typst_html::html(&document) {
            Ok(text) => { html_string = Some(text); }
            Err(err_vec) => { errors.extend(err_vec); }
        };

        return CompilerOutput {
            output: html_string,
            errors,
            warnings
        };
    }
}
