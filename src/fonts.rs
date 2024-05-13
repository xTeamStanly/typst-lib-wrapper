//! Provides a way to interract with the global [FontCache].

use std::path::PathBuf;
use std::sync::OnceLock;

use fontdb::{Database, Source as FontSource};
use parking_lot::{const_mutex, Mutex};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use typst::foundations::Bytes;
use typst::text::{Font, FontBook, FontInfo};

use crate::errors::{WrapperError, WrapperResult};

/// Holds details about the location of a font and lazily the font itself.
///
/// External docs: [FontSlot](https://docs.rs/crate/typst-cli/0.11.0/source/src/fonts.rs)
#[derive(Debug, Clone)]
pub(crate) struct LazyFont {
    /// The path at which the font can be found on the system.
    path: PathBuf,
    /// The index of the font in its collection. Zero if the path does not point to a collection.
    index: u32,
    /// The lazily loaded font.
    font: OnceLock<Option<Font>>,
    /// Used to indicate if the font it 'typst embedded font'.
    embedded: bool,
}

impl LazyFont {
    /// Gets the font data. \
    /// If the font is not loaded, loads the font from disk. \
    /// Returns `None` is error occurred.
    pub(crate) fn get(&self) -> Option<Font> {
        let font = self.font.get_or_init(|| {
            let raw_font: Vec<u8> = std::fs::read(&self.path).ok()?;
            let bytes: Bytes = Bytes::from(raw_font);
            Font::new(bytes, self.index)
        });
        return font.clone();
    }
}

/// Global font cache, initialized once on-demand. \
/// Many threads could access this cache so it's behind a [Mutex].
static FONT_CACHE: Mutex<Option<FontCache>> = const_mutex(None);

/// Caches and searches for fonts.
///
/// By default the cache will not load all system fonts. This can be enabled with \
/// custom initialization - `::init(true, None)`.
///
/// The cache doesn't need to be initialized explicitly, because it will initialize \
/// when compiling documents. Custom initialization method exists if you wish \
/// to include/exclude all system fonts or load custom fonts.
///
/// # Note / Warning
/// Overtime the cache accumulates allocated font bytes. This can happen when adding \
/// more and more fonts. One crude way to deal with this is to periodically empty cache \
/// so it releases memory. This is an extreme case and **probably** shouldn't be \
/// that big of a deal if you are not using an extreme amount of fonts.
///
/// ### Blocking [Mutex]
/// Any operation on the [FontCache] will lock the [Mutex]. This mutex is **NOT ASYNC** \
/// so keep that in mind. Use **'blocking task'** provided by your runtime \
/// if you wish to use it in an async environment.
///
/// # Examples
/// Initializes [FontCache] without system fonts including custom fonts directories.
/// ```
///     let font_dirs = vec![
///         "./assets/fonts",
///         "~/path/to/custom/fonts"
///     ];
///     FontCache::init(false, Some(font_dirs))
///         .expect("Cache error");
/// ```
///
/// Initializes [FontCache] with just the system fonts. Note the `None::<Vec<&str>>`.
/// ```
///     FontCache::init(true, None::<Vec<&str>>)
///         .expect("Cache error");
/// ```
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

    /// Returns the size of lazily loaded fonts currently in memory. \
    /// The size is in **bytes**.
    ///
    /// If you wish to include embedded fonts set `include_embedded_fonts` to `true`. \
    /// It is advised to set this to `false`.
    ///
    /// # Note / Warning
    /// This will lock the [FontCache] [Mutex]. This [Mutex] is **NOT ASYNC** \
    /// so keep that in mind. Use **'blocking task'** provided by your runtime \
    /// if you wish to use it in an async environment.
    ///
    /// # Example
    /// Clears cache if fonts take more then 64MB, excluding embedded fonts.
    /// ```
    ///     let size = FontCache::cache_size(false).expect("Cache error");
    ///     if size > 64_000_000 {
    ///         FontCache::clear_cache(false).expect("Cache error");
    ///     }
    /// ```
    pub fn cache_size(include_embedded_fonts: bool) -> WrapperResult<usize> {
        let mut font_cache_mutex = FONT_CACHE.lock();
        let font_cache: &mut FontCache = Self::get_mut_or_init(&mut font_cache_mutex)?;

        let cached_font_bytes: usize = font_cache.fonts
            .par_iter()
            .filter(|x| x.embedded == include_embedded_fonts)
            .map(|x| {
                if let Some(Some(font)) = x.font.get() {
                    font.data().len()
                } else {
                    0
                }
        }).sum();

        return Ok(cached_font_bytes);
    }

    /// Clears the [FontCache] by dropping all the lazily loaded font data.
    ///
    /// If you wish to drop embedded font data set `include_embedded_fonts` to `true`.
    /// It is advised to set this to `false`, because you can 'irreversably' unload them. \
    ///
    /// # Note / Warning
    /// If you choose to clear font data for embedded fonts, mind that you will \
    /// probably irreversably make them inaccessible. They are loaded on [FontCache] \
    /// initialization if the feature `embed_typst_fonts` is enabled, otherwise \
    /// you will need to manualy provide a path to them (or have them installed).
    ///
    /// This will lock the [FontCache] [Mutex]. This [Mutex] is **NOT ASYNC** \
    /// so keep that in mind. Use **'blocking task'** provided by your runtime \
    /// if you wish to use it in an async environment.
    ///
    /// # Example
    /// Clears cache if fonts take more then 64MB, excluding embedded fonts.
    /// ```
    ///     let size = FontCache::cache_size(false).expect("Cache error");
    ///     if size > 64_000_000 {
    ///         FontCache::clear_cache(false).expect("Cache error");
    ///     }
    /// ```
    pub fn clear_cache(include_embedded_fonts: bool) -> WrapperResult<()> {
        let mut font_cache_mutex = FONT_CACHE.lock();
        let font_cache: &mut FontCache = Self::get_mut_or_init(&mut font_cache_mutex)?;

        for lazyfont in font_cache.fonts
            .iter_mut()
            .filter(|x| x.embedded == include_embedded_fonts)
        {
            drop(lazyfont.font.take());
        }

        return Ok(());
    }

    /// TODO
    pub(crate) fn update_cache(font: Font) -> WrapperResult<()> {
        let mut font_cache_mutex = FONT_CACHE.lock();
        let font_cache: &mut FontCache = Self::get_mut_or_init(&mut font_cache_mutex)?;

        let info = font.info();

        // Search for the font if it exists in cache.
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
    ///
    /// ### Used internally.
    pub(crate) fn get_book_and_fonts() -> WrapperResult<(FontBook, Vec<LazyFont>)> {
        let mut font_cache_mutex = FONT_CACHE.lock();
        let font_cache: &mut FontCache = Self::get_mut_or_init(&mut font_cache_mutex)?;

        let book: FontBook = font_cache.book.clone();
        let fonts: Vec<LazyFont> = font_cache.fonts.to_vec();

        return Ok((book, fonts));
    }

    /// Returns mutable reference to existing [FontCache] or initializes new one.
    ///
    /// # Note / Warning
    /// [Global font cache](FONT_CACHE) must be **LOCKED** before calling this function.
    ///
    /// ### Used internally.
    fn get_mut_or_init(font_cache: &mut Option<FontCache>) -> WrapperResult<&mut Self> {
        if font_cache.is_none() {
            *font_cache = Some(Self::init_default_inner()?);
        }

        match font_cache {
            Some(fc) => Ok(fc),
            None => Err(WrapperError::UninitializedFontCache), // Shouldn't happen, just in case
        }
    }

    /// Inserts all fonts to the [global font cache](FONT_CACHE) from the provided `database`.
    ///
    /// # Note / Warning
    /// [Global font cache](FONT_CACHE) must be **LOCKED** before calling this function.
    ///
    /// ### Used internally.
    #[inline]
    fn insert_from_database(font_cache: &mut FontCache, database: Database) -> WrapperResult<()> {

        // Creates lazily loaded fonts for each font face.
        for face in database.faces() {
            let path = match &face.source {
                FontSource::File(path) | FontSource::SharedFile(path, _) => path,

                // typst-cli doesn't add binary sources to the database
                FontSource::Binary(_) => continue
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

        Ok(())
    }

    /// Creates a [LazyFont] and inserts it into [FontCache].
    ///
    /// - `font_path` - Anything that can be converted to [PathBuf] pointing \
    /// to a font file.
    ///
    /// # Note / Warning
    /// ### Blocking [Mutex]
    /// Any operation on the [FontCache] will lock the [Mutex]. This mutex is **NOT ASYNC** \
    /// so keep that in mind. Use **'blocking task'** provided by your runtime \
    /// if you wish to use it in an async environment.
    ///
    /// # Example
    /// Inserts a font into [FontCache].
    /// ```
    ///     FontCache::insert_one("./assets/fonts/times_new_roman.ttf")
    ///         .expect("Cache error");
    /// ```
    pub fn insert_one(font_path: impl Into<PathBuf>) -> WrapperResult<()> {
        let mut font_cache_mutex = FONT_CACHE.lock();
        let font_cache: &mut FontCache = Self::get_mut_or_init(&mut font_cache_mutex)?;

        let mut db = Database::new();
        db.load_font_file(font_path.into())
            .map_err(WrapperError::FontLoadingError)?;

        return Self::insert_from_database(font_cache, db);
    }

    /// For each font path in a [Vec] creates a [LazyFont] and inserts it into [FontCache].
    ///
    /// - `font_paths` - [Vec] containing anything that can be converted into [PathBuf]
    /// with each being a font path.
    ///
    /// # Note / Warning
    /// ### Blocking [Mutex]
    /// Any operation on the [FontCache] will lock the [Mutex]. This mutex is **NOT ASYNC** \
    /// so keep that in mind. Use **'blocking task'** provided by your runtime \
    /// if you wish to use it in an async environment.
    ///
    /// # Example
    /// Inserts some fonts into [FontCache].
    /// ```
    ///     let font_paths = vec![
    ///         "./assets/fonts/times_new_roman.ttf",
    ///         "~/path/to/custom/fonts/comic_sans.ttf"
    ///     ];
    ///
    ///     FontCache::insert_many(font_paths)
    ///         .expect("Cache error");
    /// ```
    pub fn insert_many(font_paths: Vec<impl Into<PathBuf>>) -> WrapperResult<()> {
        let mut font_cache_mutex = FONT_CACHE.lock();
        let font_cache: &mut FontCache = Self::get_mut_or_init(&mut font_cache_mutex)?;

        let mut db = Database::new();
        for font_path in font_paths {
            db.load_font_file(font_path.into())
                .map_err(WrapperError::FontLoadingError)?;
        }

        return Self::insert_from_database(font_cache, db);
    }

    /// For each font in a directory creates a [LazyFont] and inserts it into [FontCache].
    ///
    /// - `dir_path` - Anything that can be converted to [PathBuf] pointing to \
    /// a directory containing fonts.
    ///
    /// # Note / Warning
    /// ### Blocking [Mutex]
    /// Any operation on the [FontCache] will lock the [Mutex]. This mutex is **NOT ASYNC** \
    /// so keep that in mind. Use **'blocking task'** provided by your runtime \
    /// if you wish to use it in an async environment.
    ///
    /// # Example
    /// Inserts some directories into [FontCache].
    /// ```
    ///     FontCache::insert_dir("./assets/fonts")
    ///         .expect("Cache error");
    /// ```
    pub fn insert_dir(dir_path: impl Into<PathBuf>) -> WrapperResult<()> {
        let mut font_cache_mutex = FONT_CACHE.lock();
        let font_cache: &mut FontCache = Self::get_mut_or_init(&mut font_cache_mutex)?;

        let mut db = Database::new();
        db.load_fonts_dir(dir_path.into());

        return Self::insert_from_database(font_cache, db);
    }

    /// For each font in each directory creates a [LazyFont] and inserts it into [FontCache].
    ///
    /// - `dirs_path` - [Vec] containing anything that can be converted to [PathBuf] \
    /// with each being a fonts directory path.
    ///
    /// # Note / Warning
    /// ### Blocking [Mutex]
    /// Any operation on the [FontCache] will lock the [Mutex]. This mutex is **NOT ASYNC** \
    /// so keep that in mind. Use **'blocking task'** provided by your runtime \
    /// if you wish to use it in an async environment.
    ///
    /// # Example
    /// Inserts some directories into [FontCache].
    /// ```
    ///     let font_dirs = vec![
    ///         "./assets/fonts",
    ///         "~/path/to/custom/fonts"
    ///     ];
    ///
    ///     FontCache::insert_dirs(font_dirs)
    ///         .expect("Cache error");
    /// ```
    pub fn insert_dirs(dirs_path: Vec<impl Into<PathBuf>>) -> WrapperResult<()> {
        let mut font_cache_mutex = FONT_CACHE.lock();
        let font_cache: &mut FontCache = Self::get_mut_or_init(&mut font_cache_mutex)?;

        let mut db = Database::new();
        for dir_path in dirs_path {
            db.load_fonts_dir(dir_path.into());
        }

        return Self::insert_from_database(font_cache, db);
    }

    /// Loads all operating system fonts, custom fonts and returns [FontCache] struct. \
    ///
    /// You can choose to include all system fonts during the font search. If you have \
    /// a custom font directory or directories you can also pass them as a [Vec] of \
    /// [paths](PathBuf) to directories containing fonts.
    ///
    /// `include_system_fonts` - Notes if all system fonts should be loaded. \
    /// `dir_paths` - Optional [Vec] of [paths](PathBuf) to directories containing fonts.
    ///
    /// ### Used internally.
    #[inline]
    fn init_inner(
        include_system_fonts: bool,
        dir_paths: Option<Vec<PathBuf>>,
    ) -> WrapperResult<Self> {
        let mut db = Database::new();

        // Loads system fonts.
        if include_system_fonts {
            db.load_system_fonts();
        }

        // Loads custom fonts.
        if let Some(some_dir_paths) = dir_paths {
            for some_dir_path in some_dir_paths {
                db.load_fonts_dir(some_dir_path);
            }
        }

        let mut book: FontBook = FontBook::new();
        let mut fonts: Vec<LazyFont> = Vec::<LazyFont>::new();

        // Creates lazily loaded fonts for each font face.
        for face in db.faces() {
            let path = match &face.source {
                FontSource::File(path) | FontSource::SharedFile(path, _) => path,

                // typst-cli doesn't add binary sources to the database
                FontSource::Binary(_) => continue,
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

        // Optional preloaded typst embedded fonts.
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

        return Ok(Self { book, fonts });
    }

    /// Initializes [FontCache] without 'custom fonts' and excluding all system fonts.
    ///
    /// ### Used internally.
    fn init_default_inner() -> WrapperResult<Self> {
        Self::init_inner(false, None)
    }

    /// Initializes [FontCache] without 'custom fonts' and excluding all system fonts.
    ///
    /// # Note / Warning
    /// ### Blocking [Mutex]
    /// Any operation on the [FontCache] will lock the [Mutex]. This mutex is **NOT ASYNC** \
    /// so keep that in mind. Use **'blocking task'** provided by your runtime \
    /// if you wish to use it in an async environment.
    pub fn init_default() -> WrapperResult<()> {
        let mut font_cache_mutex = FONT_CACHE.lock();

        let font_cache: FontCache = Self::init_default_inner()?;
        *font_cache_mutex = Some(font_cache);

        return Ok(());
    }

    /// Loads all operating system fonts, custom fonts and initializes
    /// [global font cache](FONT_CACHE). \
    /// This will initialize the font cache with provided fonts which are lazily loaded on-demand.
    ///
    /// You can choose to include all system fonts during the font search. \
    /// If you have a custom font directory you can use [init_with_dirs](Self::init_with_dirs). \
    /// This function will automatically **overwrite** current global font cache.
    ///
    /// - `include_system_fonts` - Notes if all system fonts should be loaded.
    ///
    /// # Note / Warning
    /// ### Blocking [Mutex]
    /// Any operation on the [FontCache] will lock the [Mutex]. This mutex is **NOT ASYNC** \
    /// so keep that in mind. Use **'blocking task'** provided by your runtime \
    /// if you wish to use it in an async environment.
    ///
    /// # Example
    /// Initializes [FontCache] without system fonts.
    /// ```
    ///     FontCache::init(false).expect("Cache error");
    /// ```
    pub fn init(include_system_fonts: bool) -> WrapperResult<()> {
        let mut font_cache_mutex = FONT_CACHE.lock();

        let font_cache: FontCache = Self::init_inner(include_system_fonts, None)?;
        *font_cache_mutex = Some(font_cache);

        return Ok(());
    }

    /// Loads all operating system fonts, custom fonts and initializes
    /// [global font cache](FONT_CACHE). \
    /// This will initialize the font cache with provided fonts which are lazily loaded on-demand.
    ///
    /// You can choose to include all system fonts during the font search. If you have \
    /// a custom font directory or directories you can also pass them as a [Vec] of \
    /// anything cloneable that can be converted to [PathBuf]. This function will \
    /// automatically **overwrite** current global font cache.
    ///
    /// - `include_system_fonts` - Notes if all system fonts should be loaded.
    /// - `dir_paths` - [Vec] of anything clonable that can be converted into \
    /// [PathBuf] pointing to directories containing fonts.
    ///
    /// # Note / Warning
    /// ### Blocking [Mutex]
    /// Any operation on the [FontCache] will lock the [Mutex]. This mutex is **NOT ASYNC** \
    /// so keep that in mind. Use **'blocking task'** provided by your runtime \
    /// if you wish to use it in an async environment.
    ///
    /// # Example
    /// Initializes [FontCache] without system fonts including custom fonts directories.
    /// ```
    ///     let font_dirs = vec![
    ///         "./assets/fonts",
    ///         "~/path/to/custom/fonts"
    ///     ];
    ///     FontCache::init_with_dirs(false, font_dirs)
    ///         .expect("Cache error");
    /// ```
    pub fn init_with_dirs(include_system_fonts: bool, dir_paths: Vec<impl Into<PathBuf>>) -> WrapperResult<()> {
        let mut font_cache_mutex = FONT_CACHE.lock();

        let mapped: Vec<PathBuf> = dir_paths
            .into_iter()
            .map(|x| Into::<PathBuf>::into(x))
            .collect();

        let font_cache: FontCache = Self::init_inner(include_system_fonts, Some(mapped))?;
        *font_cache_mutex = Some(font_cache);

        return Ok(());
    }
}
