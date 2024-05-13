//! Provides a way to interract with the file system. \
//! Files are lazily loaded on-demand.
//!
//! ### Used internally.

use std::path::{Path, PathBuf};

use typst::diag::{FileError, FileResult};
use typst::foundations::Bytes;
use typst_syntax::{FileId, Source};

use crate::package::prepare_package;

/// Same as [SlotCell](https://docs.rs/crate/typst-cli/latest/source/src/world.rs)
/// from [typst-cli](https://github.com/typst/typst/tree/main/crates/typst-cli).
///
/// Lazily processes data for a file.
#[derive(Debug)]
struct LazyCell<T> {
    /// The processed data.
    data: Option<FileResult<T>>,
    /// A hash of the raw file contents / access error.
    fingerprint: u128,
    /// Whether the slot has been accessed in the current compilation.
    accessed: bool
}

impl<T: Clone> LazyCell<T> {
    /// Creates a new, empty cell.
    fn new() -> Self {
        Self {
            data: None,
            fingerprint: 0,
            accessed: false
        }
    }

    /// Gets the contents of the cell or initialize them.
    fn get_or_init(
        &mut self,
        load: impl FnOnce() -> FileResult<Vec<u8>>,
        f: impl FnOnce(Vec<u8>, Option<T>) -> FileResult<T>
    ) -> FileResult<T> {
        // If we accessed the file already in this compilation, retrieve it.
        if std::mem::replace(&mut self.accessed, true) {
            if let Some(data) = &self.data {
                return data.clone();
            }
        }

        // Read and hash the file.
        let result = load();
        let fingerprint = typst::util::hash128(&result);

        // If the file contents didn't change, yield the old processed data.
        if std::mem::replace(&mut self.fingerprint, fingerprint) == fingerprint {
            if let Some(data) = &self.data {
                return data.clone();
            }
        }

        let prev = self.data.take().and_then(Result::ok);
        let value = result.and_then(|data| f(data, prev));
        self.data = Some(value.clone());

        value
    }

}

/// Same as [FileSlot](https://docs.rs/crate/typst-cli/latest/source/src/world.rs)
/// from [typst-cli](https://github.com/typst/typst/tree/main/crates/typst-cli).
///
/// Holds the processed data for a file ID.
///
/// Both fields can be populated if the file is both imported and read().
#[derive(Debug)]
pub(crate) struct LazyFile {
    /// The slot's file id.
    id: FileId,
    /// The lazily loaded and incrementally updated source file.
    source: LazyCell<Source>,
    /// The lazily loaded raw byte buffer.
    file: LazyCell<Bytes>
}

impl LazyFile {

    /// Read a file from disk.
    fn read_from_disk(path: &Path) -> FileResult<Vec<u8>> {
        let f = |e| FileError::from_io(e, path);
        if std::fs::metadata(path).map_err(f)?.is_dir() {
            Err(FileError::IsDirectory)?
        } else {
            std::fs::read(path).map_err(f)
        }
    }

    /// Resolves the path of a file id on the system, downloading a package if necessary.
    ///
    /// Determine the root path relative to which the file path will be resolved.
    fn system_path(
        project_root: &Path,
        id: FileId,
        http_client: &ureq::Agent
    ) -> FileResult<PathBuf> {
        if let Some(spec) = id.package() {
            let package_path: PathBuf = prepare_package(spec, http_client)?;
            return id.vpath().resolve(&package_path).ok_or(FileError::AccessDenied);
        }

        return id.vpath().resolve(project_root).ok_or(FileError::AccessDenied);
    }

    /// Decode UTF-8 with an optional BOM.
    fn decode_utf8(buf: &[u8]) -> FileResult<&str> {
        // Remove UTF-8 BOM.
        Ok(std::str::from_utf8(buf.strip_prefix(b"\xef\xbb\xbf").unwrap_or(buf))?)
    }

    /// Create a new file slot.
    pub(crate) fn new(id: FileId) -> Self {
        Self {
            id,
            source: LazyCell::new(),
            file: LazyCell::new()
        }
    }

    /// Retrieve the source for this file. Will download packages if necessary.
    pub(crate) fn source(
        &mut self,
        project_root: &Path,
        http_client: &ureq::Agent
    ) -> FileResult<Source> {
        self.source.get_or_init(
            || {
                let path = Self::system_path(project_root, self.id, http_client)?;
                Self::read_from_disk(&path)
            },

            |data, prev| {
                let text = Self::decode_utf8(&data)?;
                if let Some(mut prev) = prev {
                    prev.replace(text);
                    Ok(prev)
                } else {
                    Ok(Source::new(self.id, text.into()))
                }
            }
        )
    }

    /// Retrieve the file's bytes. Will download packages if necessary.
    pub(crate) fn file(
        &mut self,
        project_root: &Path,
        http_client: &ureq::Agent
    ) -> FileResult<Bytes> {
        self.file.get_or_init(
            || {
                let path = Self::system_path(project_root, self.id, http_client)?;
                Self::read_from_disk(&path)
            },

            |data, _| Ok(data.into())
        )
    }
}
