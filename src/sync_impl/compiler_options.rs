use std::{collections::HashMap, path::PathBuf, str::Bytes};

use comemo::Prehashed;

use typst::foundations::Bytes as TypstBytes;
use typst::foundations::Datetime as TypstDateTime;

use std::sync::OnceLock;

// use crate::fonts::FONT_CACHE as FontCache;

use typst::{
    diag::FileResult,
    syntax::{FileId, Source},
    text::{Font, FontBook},
    Library, World,
};

// TODO: Global Font storage
// use parking_lot::Mutex;
// static INNER_FONT_STORAGE_MUTEX
// Global font storage cache. Initialized once and reused.
pub static FONT_STORAGE: OnceLock<u32> = OnceLock::new();

pub enum Input {
    Content(String),
    File(PathBuf),
}

pub struct CompilerOptionsBuilder<'a> {
    pub input: Option<Input>,


    /// false by default, include your own fonts
    /// because loading all fonts can be demanding
    pub include_system_fonts: bool,


    font_paths: &'a [PathBuf]
}

impl<'a> CompilerOptionsBuilder<'a> {
    pub fn with_input(mut self, input: Input) -> Self {
        self.input = Some(input);
        self
    }

    pub fn with_file_input(mut self, path: PathBuf) -> Self {
        self.input = Some(Input::File(path));
        self
    }

    pub fn with_content_input(mut self, content: String) -> Self {
        self.input = Some(Input::Content(content));
        self
    }
}

pub struct CompilerOptions {
    input: Input,
    root: Option<PathBuf>,
    inputs: Vec<(String, String)>,

    library: Prehashed<Library>,

    main_source: Source,



    font_book: Prehashed<FontBook>,
    fonts: Vec<Font>,
    // fonts: Vec<FontSlot

    // files / sources
    ppi: Option<f32>,
    cache_directory: Option<PathBuf>,
}

impl CompilerOptions {
    pub fn new() -> Self {
        todo!()
    }
}

impl World for CompilerOptions {
    fn library(&self) -> &Prehashed<Library> {
        return &self.library;
    }

    fn book(&self) -> &Prehashed<FontBook> {
        todo!()
    }

    fn main(&self) -> Source {
        match &self.input {
            Input::Content(c) => Source::detached(c),
            Input::File(f) => todo!()
        };

        todo!()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        todo!()
    }

    fn file(&self, id: FileId) -> FileResult<TypstBytes> {
        todo!()
    }

    fn font(&self, index: usize) -> Option<Font> {
        todo!()
    }

    fn today(&self,offset:Option<i64>) -> Option<TypstDateTime> {
        todo!()
    }
}
