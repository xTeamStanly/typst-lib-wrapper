use std::path::PathBuf;
use std::sync::OnceLock;

use fontdb::{Database, Source as FontSource};
use parking_lot::{const_mutex, Mutex};
use typst::foundations::Bytes;
use typst::text::{Font, FontBook, FontInfo};

use crate::errors::{WrapperError, WrapperResult};









#[derive(Debug, Clone)]
pub(crate) struct LazyFont {
    path: PathBuf,
    index: u32,
    font: OnceLock<Option<Font>>,
    embedded: bool,
}

impl LazyFont {
    pub(crate) fn get(&self) -> Option<Font> {
        let font = self.font.get_or_init(|| {
            let raw_font: Vec<u8> = std::fs::read(&self.path).ok()?;
            let bytes: Bytes = Bytes::from(raw_font);
            Font::new(bytes, self.index)
        });
        return font.clone();
    }
}

/// Global font cache, initialized once on demand. \
/// Many threads could access this cache so it's behind a [Mutex](parking_lot::Mutex).
static FONT_CACHE: Mutex<Option<FontCache>> = const_mutex(None);

/// Holds details about the location (path) of a font and lazily the font itself.
///
/// Docs: [FontSlot](https://docs.rs/crate/typst-cli/0.11.0/source/src/fonts.rs)
// #[derive(Debug, Clone)]
// pub(crate) struct LazyFont {
//     /// The path at which the font can be found on the system.
//     path: PathBuf,
//     /// The index of the font in its collection. Zero if the path does not point
//     /// to a collection.
//     index: u32,
//     /// The lazily loaded font.
//     pub font: OnceLock<Option<Font>>,

//     pub embeded: bool
// }
// impl LazyFont {
//     /// Get the font for this slot.
//     pub fn get(&self) -> Option<Font> {

//         // let mut first_time = false;

//         let retval = self.font
//             .get_or_init(|| {
//                 let data = TypstBytes::from(fs::read(&self.path).ok()?);
//                 // println!("\x1b[1;31m MISS: {} \x1b[0m", self.path.to_str().unwrap());
//                 // first_time = true;
//                 Font::new(data, self.index)
//             })
//             .clone();

//         // if (!first_time) {
//         //     println!("\x1b[1;32m HIT: {} \x1b[0m", self.path.to_str().unwrap());
//         // }

//         return retval;
//     }
// }

/// Caches and searches for fonts.
///
/// ### Important
/// Overtime the cache accumulates allocated font bytes. This can happen \
/// when keep loading more and more fonts. One way to deal with this is to \
/// periodically reconstruct this struct so it releases memory. \
/// This is an extreme case and **probably** shouldn't be that big of a deal \
/// if you are using a normal amount of fonts.
///
/// Docs: [FontSearcher](https://docs.rs/crate/typst-cli/0.11.0/source/src/fonts.rs)
#[derive(Debug)]
pub struct FontCache {
    /// Metadata about all discovered fonts.
    book: FontBook,
    /// Slots that the fonts are loaded into.
    fonts: Vec<LazyFont>,
}

impl FontCache {
    pub fn empty_cache() -> WrapperResult<()> {
        let mut font_cache_mutex = FONT_CACHE.lock();
        let font_cache: &mut FontCache = Self::get_mut_or_init(&mut font_cache_mutex)?;

        for lazyfont in font_cache.fonts.iter_mut() {
            drop(lazyfont.font.take());
        }

        return Ok(());
    }

    pub fn update_cache(font: Font) -> WrapperResult<()> {
        let mut font_cache_mutex = FONT_CACHE.lock();
        let font_cache: &mut FontCache = Self::get_mut_or_init(&mut font_cache_mutex)?;

        let info = font.info();
        // println!("\x1b[1;35m CACHED REQ: {:?} \x1b[0m", font.info());
        if let Some(buffer_index) = font_cache
            .book
            .select(&info.family.to_lowercase(), info.variant)
        {
            if let Some(old_font) = font_cache.fonts.get_mut(buffer_index) {
                let old_font_optional = old_font.font.take();
                match old_font_optional {
                    None => {
                        // println!("\x1b[1;33m CACHED: {:?} \x1b[0m", font.info());
                        old_font.font.set(Some(font)).unwrap();
                    }
                    Some(fff) => {
                        old_font.font.set(fff).unwrap();
                        // println!("\x1b[1;36m ALREADY CACHED: {:?} \x1b[0m", font.info());
                    }
                };
                // println!("\x1b[1;33m CACHED: {:?} \x1b[0m", font.info());
                // old_font.font.set(Some(font));
            }
        }

        Ok(())
    }

    /// Acquires [global font cache](FONT_CACHE), **clones** [FontBook] and creates
    /// [LazyFont] [Vec] by **cloning** and returns them as tuple.
    pub(crate) fn get_book_and_fonts() -> WrapperResult<(FontBook, Vec<LazyFont>)> {
        let mut font_cache_mutex = FONT_CACHE.lock();
        let font_cache: &mut FontCache = Self::get_mut_or_init(&mut font_cache_mutex)?;

        let book: FontBook = font_cache.book.clone();
        let fonts: Vec<LazyFont> = font_cache.fonts.to_vec();

        return Ok((book, fonts));
    }

    /// [Global font cache](FONT_CACHE) **must be locked** before calling this function.
    #[inline]
    fn get_mut_or_init(font_cache: &mut Option<FontCache>) -> WrapperResult<&mut Self> {
        if font_cache.is_none() {
            *font_cache = Some(Self::init_default()?);
        }

        match font_cache {
            Some(fc) => Ok(fc),
            None => Err(WrapperError::UninitializedFontCache), // Shouldn't happen, just in case
        }
    }

    /// Inserts all fonts to the [global font cache](FONT_CACHE) from the provided `database`.
    /// Global font cache **must be locked** before calling this function.
    #[inline]
    fn insert_from_database(font_cache: &mut FontCache, database: Database) -> WrapperResult<()> {
        for face in database.faces() {
            let path = match &face.source {
                FontSource::File(path) | FontSource::SharedFile(path, _) => path,
                FontSource::Binary(_) => continue, // typst-cli doesn't add binary sources to the database
            };

            let info: Option<FontInfo> = database
                .with_face_data(face.id, FontInfo::new)
                .ok_or(WrapperError::FontFaceLoadingError(path.to_owned()))?;

            if let Some(font_info) = info {
                font_cache.book.push(font_info);
                font_cache.fonts.push(LazyFont {
                    path: path.clone(),
                    index: face.index,
                    font: OnceLock::new(),
                    embedded: false,
                });
            }
        }

        // dbg!(font_cache);

        Ok(())
    }
    /// Creates a [LazyFont] and inserts it into [FontCache].
    ///
    /// * `font_path` - [PathBuf] to a font file.
    ///
    /// Acquires global font cache, adds font with the provided `font_path` path and \
    /// updates font cache.
    pub fn insert_one(font_path: &PathBuf) -> WrapperResult<()> {
        let mut font_cache_mutex = FONT_CACHE.lock();
        let font_cache: &mut FontCache = Self::get_mut_or_init(&mut font_cache_mutex)?;

        let mut db = Database::new();
        db.load_font_file(font_path)
            .map_err(WrapperError::FontLoadingError)?;

        return Self::insert_from_database(font_cache, db);
    }

    /// For each font path in a [LazyFont] slice creates a [LazyFont] and \
    /// inserts it into [FontCache].
    ///
    /// * `font_paths` - Slice containing [PathBuf]s with each being a font path.
    ///
    /// Acquires global font cache, adds fonts with the provided `font_paths` paths and \
    /// updates font cache.
    pub fn insert_many(font_paths: &[PathBuf]) -> WrapperResult<()> {
        let mut font_cache_mutex = FONT_CACHE.lock();
        let font_cache: &mut FontCache = Self::get_mut_or_init(&mut font_cache_mutex)?;

        let mut db = Database::new();
        for font_path in font_paths {
            db.load_font_file(font_path)
                .map_err(WrapperError::FontLoadingError)?;
        }

        return Self::insert_from_database(font_cache, db);
    }

    /// For each font in a directory creates a [LazyFont] and \
    /// inserts it into [FontCache].
    ///
    /// * `dir_path` - [PathBuf] to a directory containing fonts.
    ///
    /// Acquires global font cache, adds fonts located inside the provided `dir_path` \
    /// directory path and updates font cache.
    pub fn insert_dir(dir_path: &PathBuf) -> WrapperResult<()> {
        let mut font_cache_mutex = FONT_CACHE.lock();
        let font_cache: &mut FontCache = Self::get_mut_or_init(&mut font_cache_mutex)?;

        let mut db = Database::new();
        db.load_fonts_dir(dir_path);

        return Self::insert_from_database(font_cache, db);
    }

    /// For each font in a directory creates a [LazyFont] and \
    /// inserts it into [FontCache].
    ///
    /// * `dirs_path` - Slice containing [PathBuf]s with each being a fonts directory path.
    ///
    /// Acquires global font cache, adds fonts located inside the provided `dirs_path` \
    /// directories path and updates font cache.
    pub fn insert_dirs(dirs_path: &[PathBuf]) -> WrapperResult<()> {
        let mut font_cache_mutex = FONT_CACHE.lock();
        let font_cache: &mut FontCache = Self::get_mut_or_init(&mut font_cache_mutex)?;

        let mut db = Database::new();
        for dir_path in dirs_path {
            db.load_fonts_dir(dir_path);
        }

        return Self::insert_from_database(font_cache, db);
    }

    /// Loads all operating system fonts, custom fonts and returns [FontCache] struct. \
    ///
    /// You can choose to include all system fonts during the font search. \
    /// If you have a custom font directory or directories you can also pass \
    /// them as optional slice of type [PathBuf](std::path::PathBuf).
    ///
    /// `include_system_fonts` - Notes if all system fonts should be loaded.
    /// `dir_paths` - Optional slice of paths to directories containing fonts.
    #[inline]
    fn init_inner(
        include_system_fonts: bool,
        dir_paths: Option<&[PathBuf]>,
    ) -> WrapperResult<Self> {
        let mut db = Database::new();

        if include_system_fonts {
            db.load_system_fonts();
        }

        if let Some(some_dir_paths) = dir_paths {
            for some_dir_path in some_dir_paths {
                db.load_fonts_dir(some_dir_path);
            }
        }

        let mut book: FontBook = FontBook::new();
        let mut fonts: Vec<LazyFont> = Vec::<LazyFont>::new();

        for face in db.faces() {
            let path = match &face.source {
                FontSource::File(path) | FontSource::SharedFile(path, _) => path,
                FontSource::Binary(_) => continue, // typst-cli doesn't add binary sources to the database
            };

            let info: Option<FontInfo> = db
                .with_face_data(face.id, FontInfo::new)
                .ok_or(WrapperError::FontFaceLoadingError(path.to_owned()))?;

            if let Some(font_info) = info {
                book.push(font_info);
                fonts.push(LazyFont {
                    path: path.clone(),
                    index: face.index,
                    font: OnceLock::new(),
                    embedded: false,
                });
            }
        }

        #[cfg(feature = "embed_typst_fonts")]
        for data in typst_assets::fonts() {
            let buffer = typst::foundations::Bytes::from_static(data);
            for (i, font) in Font::iter(buffer).enumerate() {
                book.push(font.info().clone());
                fonts.push(LazyFont {
                    path: PathBuf::new(),
                    index: i as u32,
                    font: OnceLock::from(Some(font)),
                    embedded: true,
                })
            }
        }

        // dbg!(&book);

        // println!("------------------------------------");
        // for (family_name, family_fonts) in book.families() {
        //     // println!("{family_name}");
        //     for index in book.select_family(family_name.to_lowercase().as_str()) {
        //         println!("{family_name}: {index}");
        //     }
        //     // for index in book.select_family(family_name) {
        //     //     println!("{family_name}: {index}");
        //     // }

        //     // for font_info in family_fonts {

        //     // }
        // }
        // println!("------------------------------------");
        // println!("");
        // println!("");
        // println!("");

        // for (family_name, family_fonts) in book.families() {
        //     for font in family_fonts {

        //     }
        // }

        // dbg!(&book);
        // println!("");
        // println!("");
        // println!("");
        // dbg!(&fonts);

        return Ok(Self { book, fonts });
    }

    /// Initializes [FontCache] without 'custom fonts' and including all system fonts.
    #[inline]
    pub fn init_default() -> WrapperResult<Self> {
        Self::init_inner(true, None)
    }

    /// Loads all operating system fonts, custom fonts and initializes
    /// [global font cache](FONT_CACHE). This will initialize the \
    /// font cache with provided fonts which are lazily loaded on demand.
    ///
    /// You can choose to include all system fonts during the font search. \
    /// If you have a custom font directory or directories you can also pass \
    /// them as optional slice of type [PathBuf](std::path::PathBuf). \
    /// This function will automatically **overwrite** current global font cache.
    ///
    /// `include_system_fonts` - Notes if all system fonts should be loaded.
    /// `dir_paths` - Optional slice of paths to directories containing fonts.
    pub fn init(include_system_fonts: bool, dir_paths: Option<&[PathBuf]>) -> WrapperResult<()> {
        let mut font_cache_mutex = FONT_CACHE.lock();

        let font_cache: FontCache = Self::init_inner(include_system_fonts, dir_paths)?;
        *font_cache_mutex = Some(font_cache);

        return Ok(());
    }
}
