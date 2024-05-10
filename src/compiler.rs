use std::collections::HashMap;
use std::path::PathBuf;

use chrono::Datelike;
use comemo::Prehashed;
use parking_lot::Mutex;

use typst::diag::FileResult;
use typst::eval::Tracer;
use typst::foundations::{Bytes, Datetime};
use typst::text::{Font, FontBook};
use typst::{Library, World};
use typst_syntax::{FileId, Source};

use crate::files::LazyFile;
use crate::fonts::LazyFont;
use crate::parameters::Format;

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
    pub(crate) now: chrono::DateTime<chrono::Utc>,
}

// todo: https://docs.rs/crate/typst-cli/latest/source/src/world.rs
impl World for Compiler {
    fn library(&self) -> &Prehashed<Library> {
        &self.library
    }

    fn book(&self) -> &Prehashed<FontBook> {
        &self.book
    }

    fn main(&self) -> Source {
        self.entry.clone()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        self.slot(id, |slot| slot.source(&self.root, &self.http_client))
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.slot(id, |slot| slot.file(&self.root, &self.http_client))
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.get(index)?.get()
    }

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

    pub fn save_pdf(&self, path: PathBuf) {
        let mut tracer = Tracer::new();
        let document = typst::compile(self, &mut tracer).unwrap();

        let bytes = typst_pdf::pdf(&document, typst::foundations::Smart::Auto, None);

        #[cfg(debug_assertions)]
        dbg!(tracer.warnings());

        std::fs::write(path, bytes).unwrap();
    }

    fn compile_document(&self) {}
    pub fn compile(&self) {}
}
