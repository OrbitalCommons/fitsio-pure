use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::errors::{Error, Result};
use super::hdu::FitsHdu;
use super::images::ImageDescription;

/// Whether a file is opened for reading or writing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOpenMode {
    ReadOnly,
    ReadWrite,
}

/// An in-memory representation of an open FITS file.
///
/// The parse cache is a [`OnceLock`], so `&FitsFile` is `Sync` and one open
/// file can be shared across threads (reading every column of a table in
/// parallel, say) instead of reopening and reparsing the file per thread.
pub struct FitsFile {
    data: Vec<u8>,
    filename: PathBuf,
    mode: FileOpenMode,
    /// No backing file, so `flush` and `Drop` never touch disk.
    in_memory: bool,
    cached_parse: OnceLock<crate::hdu::FitsData>,
}

/// Builder for creating a new FITS file.
pub struct NewFitsFile {
    path: PathBuf,
    overwrite: bool,
}

/// Trait for types that can identify an HDU (by index or name).
pub trait DescribesHdu {
    fn get_hdu<'a>(
        &self,
        fits_data: &'a crate::hdu::FitsData,
    ) -> Option<(usize, &'a crate::hdu::Hdu)>;
}

impl DescribesHdu for usize {
    fn get_hdu<'a>(
        &self,
        fits_data: &'a crate::hdu::FitsData,
    ) -> Option<(usize, &'a crate::hdu::Hdu)> {
        fits_data.get(*self).map(|hdu| (*self, hdu))
    }
}

/// Matches the first HDU whose `EXTNAME`, or failing that `HDUNAME`, equals the
/// name ignoring case, as cfitsio does.
impl DescribesHdu for &str {
    fn get_hdu<'a>(
        &self,
        fits_data: &'a crate::hdu::FitsData,
    ) -> Option<(usize, &'a crate::hdu::Hdu)> {
        // Like cfitsio, only the first card with the keyword counts.
        let named = |hdu: &crate::hdu::Hdu, keyword: &str| {
            matches!(
                hdu.cards
                    .iter()
                    .find(|card| card.keyword_str() == keyword)
                    .and_then(|card| card.value.as_ref()),
                Some(crate::value::Value::String(s)) if s.trim().eq_ignore_ascii_case(self)
            )
        };
        fits_data
            .iter()
            .enumerate()
            .find(|(_, hdu)| named(hdu, "EXTNAME") || named(hdu, "HDUNAME"))
    }
}

impl DescribesHdu for String {
    fn get_hdu<'a>(
        &self,
        fits_data: &'a crate::hdu::FitsData,
    ) -> Option<(usize, &'a crate::hdu::Hdu)> {
        self.as_str().get_hdu(fits_data)
    }
}

impl FitsFile {
    /// Open an existing FITS file in read-only mode.
    ///
    /// A gzip-compressed file (`.fits.gz`, `.fit.gz`) is decompressed
    /// transparently, as cfitsio does.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let data = gunzip_if_compressed(std::fs::read(path.as_ref())?)?;
        Ok(FitsFile {
            data,
            filename: path.as_ref().to_path_buf(),
            mode: FileOpenMode::ReadOnly,
            in_memory: false,
            cached_parse: OnceLock::new(),
        })
    }

    /// Open an existing FITS file for editing.
    ///
    /// A gzip-compressed file is refused, as cfitsio refuses it: saving would
    /// replace it with uncompressed bytes. Decompress it first to edit it.
    pub fn edit<P: AsRef<Path>>(path: P) -> Result<Self> {
        let data = std::fs::read(path.as_ref())?;
        if crate::gzip::is_gzip(&data) {
            return Err(Error::Message(format!(
                "{} is gzip-compressed and can only be opened read-only",
                path.as_ref().display()
            )));
        }
        Ok(FitsFile {
            data,
            filename: path.as_ref().to_path_buf(),
            mode: FileOpenMode::ReadWrite,
            in_memory: false,
            cached_parse: OnceLock::new(),
        })
    }

    /// Open FITS data that is already in memory, such as an upload, a network
    /// response or an embedded asset, in read-only mode.
    ///
    /// Nothing is written to disk. Gzip-compressed bytes are decompressed. The
    /// data is parsed immediately, so bytes that are not a valid FITS file are
    /// rejected here rather than on first use.
    pub fn from_bytes(data: impl Into<Vec<u8>>) -> Result<Self> {
        let file = FitsFile {
            data: gunzip_if_compressed(data.into())?,
            filename: PathBuf::new(),
            mode: FileOpenMode::ReadOnly,
            in_memory: true,
            cached_parse: OnceLock::new(),
        };
        file.parsed()?;
        Ok(file)
    }

    /// Create a new, writable FITS file that lives only in memory.
    ///
    /// It starts with a minimal primary HDU, like [`FitsFile::create`]. Build it
    /// with the usual methods and take the result with [`FitsFile::into_bytes`];
    /// `flush` and `Drop` never write it to disk.
    pub fn create_in_memory() -> Result<Self> {
        let cards = crate::primary::build_primary_header(8, &[])?;
        Ok(FitsFile {
            data: crate::header::serialize_header(&cards)?,
            filename: PathBuf::new(),
            mode: FileOpenMode::ReadWrite,
            in_memory: true,
            cached_parse: OnceLock::new(),
        })
    }

    /// Return the cached parse of the FITS data, parsing if needed.
    pub fn parsed(&self) -> Result<&crate::hdu::FitsData> {
        if let Some(cached) = self.cached_parse.get() {
            return Ok(cached);
        }
        let parsed = crate::hdu::parse_fits(&self.data)?;
        // A concurrent caller may have won the race; either parse is equivalent,
        // so keep whichever landed first.
        let _ = self.cached_parse.set(parsed);
        Ok(self
            .cached_parse
            .get()
            .expect("cache is populated immediately above"))
    }

    /// Invalidate the cached parse (called after data mutations).
    fn invalidate_cache(&mut self) {
        self.cached_parse.take();
    }

    /// Return a builder for creating a new FITS file.
    pub fn create<P: AsRef<Path>>(path: P) -> NewFitsFile {
        NewFitsFile {
            path: path.as_ref().to_path_buf(),
            overwrite: false,
        }
    }

    /// Return a handle to the primary HDU (index 0).
    pub fn primary_hdu(&self) -> Result<FitsHdu> {
        Ok(FitsHdu { hdu_index: 0 })
    }

    /// Return a handle to the HDU described by `desc` (index or name).
    pub fn hdu<D: DescribesHdu>(&self, desc: D) -> Result<FitsHdu> {
        let fits_data = self.parsed()?;
        let (idx, _) = desc
            .get_hdu(fits_data)
            .ok_or(Error::Message("HDU not found".to_string()))?;
        Ok(FitsHdu { hdu_index: idx })
    }

    /// Return the number of HDUs in this file.
    pub fn num_hdus(&self) -> Result<usize> {
        let fits_data = self.parsed()?;
        Ok(fits_data.len())
    }

    /// Return handles to all HDUs in the file.
    pub fn iter(&self) -> Result<Vec<FitsHdu>> {
        let fits_data = self.parsed()?;
        Ok((0..fits_data.len())
            .map(|i| FitsHdu { hdu_index: i })
            .collect())
    }

    /// Create a new image extension HDU with the given name and description.
    pub fn create_image(&mut self, extname: &str, desc: &ImageDescription) -> Result<FitsHdu> {
        let bitpix = desc.data_type.to_bitpix();
        let naxes = &desc.dimensions;

        let mut cards = crate::extension::build_extension_header(
            crate::extension::ExtensionType::Image,
            bitpix,
            naxes,
            0,
            1,
        )?;

        // For unsigned pixel types, record the cfitsio storage convention
        // (signed BITPIX offset by BZERO) so readers recover unsigned values.
        if let Some(bzero) = desc.data_type.unsigned_bzero() {
            cards.push(crate::header::Card {
                keyword: make_keyword("BZERO"),
                value: Some(bzero),
                comment: None,
            });
            cards.push(crate::header::Card {
                keyword: make_keyword("BSCALE"),
                value: Some(crate::value::Value::Integer(1)),
                comment: None,
            });
        }

        let extname_card = crate::header::Card {
            keyword: make_keyword("EXTNAME"),
            value: Some(crate::value::Value::String(extname.to_string())),
            comment: None,
        };
        cards.push(extname_card);

        let header_bytes = crate::header::serialize_header(&cards)?;

        let data_bytes = desc.dimensions.iter().copied().product::<usize>()
            * ((bitpix.unsigned_abs() as usize) / 8);
        let padded_data = crate::block::padded_byte_len(data_bytes);

        self.data.extend_from_slice(&header_bytes);
        self.data.resize(self.data.len() + padded_data, 0u8);

        self.invalidate_cache();
        let fits_data = self.parsed()?;
        let idx = fits_data.len() - 1;
        Ok(FitsHdu { hdu_index: idx })
    }

    /// Create a new binary table extension HDU.
    pub fn create_table(
        &mut self,
        extname: &str,
        columns: &[crate::bintable::BinaryColumnDescriptor],
    ) -> Result<FitsHdu> {
        let mut cards = crate::bintable::build_binary_table_cards(columns, 0, 0)?;

        let extname_card = crate::header::Card {
            keyword: make_keyword("EXTNAME"),
            value: Some(crate::value::Value::String(extname.to_string())),
            comment: None,
        };
        cards.push(extname_card);

        let header_bytes = crate::header::serialize_header(&cards)?;
        self.data.extend_from_slice(&header_bytes);

        self.invalidate_cache();
        let fits_data = self.parsed()?;
        let idx = fits_data.len() - 1;
        Ok(FitsHdu { hdu_index: idx })
    }

    /// Create a new ASCII table extension HDU.
    pub fn create_ascii_table(
        &mut self,
        extname: &str,
        columns: &[crate::table::AsciiColumnDescriptor],
    ) -> Result<FitsHdu> {
        let mut cards = crate::table::build_ascii_table_cards(columns, 0)?;

        let extname_card = crate::header::Card {
            keyword: make_keyword("EXTNAME"),
            value: Some(crate::value::Value::String(extname.to_string())),
            comment: None,
        };
        cards.push(extname_card);

        let header_bytes = crate::header::serialize_header(&cards)?;
        self.data.extend_from_slice(&header_bytes);

        self.invalidate_cache();
        let fits_data = self.parsed()?;
        let idx = fits_data.len() - 1;
        Ok(FitsHdu { hdu_index: idx })
    }

    /// Return a reference to the in-memory FITS bytes.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Replace the in-memory FITS bytes (used by write operations).
    pub fn set_data(&mut self, data: Vec<u8>) {
        self.data = data;
        self.invalidate_cache();
    }

    /// Flush the in-memory data to disk if opened for writing from a path.
    /// Does nothing for a file from [`FitsFile::create_in_memory`].
    pub fn flush(&self) -> Result<()> {
        if self.mode == FileOpenMode::ReadWrite && !self.in_memory {
            std::fs::write(&self.filename, &self.data)?;
        }
        Ok(())
    }

    /// Consume the file and return its FITS bytes.
    ///
    /// A writable file opened from a path is flushed first, so the file on disk
    /// matches the returned bytes.
    pub fn into_bytes(mut self) -> Result<Vec<u8>> {
        self.flush()?;
        // The bytes are handed back, so Drop must not write them out again.
        self.in_memory = true;
        Ok(std::mem::take(&mut self.data))
    }

    /// Return the file path. Empty for an in-memory file.
    pub fn filename(&self) -> &Path {
        &self.filename
    }

    /// Return the open mode.
    pub fn mode(&self) -> FileOpenMode {
        self.mode
    }
}

impl Drop for FitsFile {
    fn drop(&mut self) {
        if self.mode == FileOpenMode::ReadWrite && !self.in_memory {
            let _ = std::fs::write(&self.filename, &self.data);
        }
    }
}

impl NewFitsFile {
    /// Set whether to overwrite an existing file.
    pub fn overwrite(mut self) -> Self {
        self.overwrite = true;
        self
    }

    /// Finalize creation: write a minimal primary HDU and return an open `FitsFile`.
    pub fn open(self) -> Result<FitsFile> {
        if !self.overwrite && self.path.exists() {
            return Err(Error::Message(format!(
                "file already exists: {}",
                self.path.display()
            )));
        }

        let cards = crate::primary::build_primary_header(8, &[])?;
        let header_bytes = crate::header::serialize_header(&cards)?;

        std::fs::write(&self.path, &header_bytes)?;

        Ok(FitsFile {
            data: header_bytes,
            filename: self.path,
            mode: FileOpenMode::ReadWrite,
            in_memory: false,
            cached_parse: OnceLock::new(),
        })
    }
}

fn gunzip_if_compressed(data: Vec<u8>) -> Result<Vec<u8>> {
    if crate::gzip::is_gzip(&data) {
        Ok(crate::gzip::decompress(&data)?)
    } else {
        Ok(data)
    }
}

fn make_keyword(name: &str) -> [u8; 8] {
    let mut kw = [b' '; 8];
    let bytes = name.as_bytes();
    let len = bytes.len().min(8);
    kw[..len].copy_from_slice(&bytes[..len]);
    kw
}

#[cfg(test)]
mod tests {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}

    /// `&FitsFile` crossing a thread boundary is the whole point of the
    /// `OnceLock` cache; a `RefCell` here would fail to compile.
    #[test]
    fn fits_file_is_send_and_sync() {
        assert_send::<super::FitsFile>();
        assert_sync::<super::FitsFile>();
    }

    use super::*;
    use crate::compat::images::ImageType;

    #[test]
    fn from_bytes_reads_what_open_reads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        {
            let mut f = FitsFile::create(&path).open().unwrap();
            let desc = ImageDescription {
                data_type: ImageType::Short,
                dimensions: vec![3, 2],
            };
            f.create_image("SCI", &desc).unwrap();
        }

        let from_disk = FitsFile::open(&path).unwrap();
        let from_mem = FitsFile::from_bytes(std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(from_mem.mode(), FileOpenMode::ReadOnly);
        assert_eq!(from_mem.filename(), Path::new(""));
        assert_eq!(from_mem.data(), from_disk.data());
        assert_eq!(from_mem.num_hdus().unwrap(), from_disk.num_hdus().unwrap());
        assert_eq!(
            from_mem.hdu("SCI").unwrap().hdu_index,
            from_disk.hdu("SCI").unwrap().hdu_index
        );
    }

    #[test]
    fn from_bytes_rejects_non_fits() {
        assert!(FitsFile::from_bytes(b"not a FITS file".to_vec()).is_err());
    }

    #[test]
    fn in_memory_file_round_trips_through_bytes() {
        use crate::compat::images::{ReadImage, WriteImage};

        let mut f = FitsFile::create_in_memory().unwrap();
        assert_eq!(f.mode(), FileOpenMode::ReadWrite);
        assert_eq!(f.filename(), Path::new(""));

        let desc = ImageDescription {
            data_type: ImageType::Short,
            dimensions: vec![3, 2],
        };
        let hdu = f.create_image("SCI", &desc).unwrap();
        let pixels: Vec<i16> = vec![-3, -2, -1, 0, 1, 2];
        i16::write_image(&mut f, &hdu, &pixels).unwrap();
        f.flush().unwrap();

        let g = FitsFile::from_bytes(f.into_bytes().unwrap()).unwrap();
        let hdu = g.hdu("SCI").unwrap();
        let back: Vec<i16> = i16::read_image(&g, &hdu).unwrap();
        assert_eq!(back, pixels);
    }

    #[test]
    fn into_bytes_flushes_a_file_opened_for_editing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        drop(FitsFile::create(&path).open().unwrap());

        let mut f = FitsFile::edit(&path).unwrap();
        let desc = ImageDescription {
            data_type: ImageType::UnsignedByte,
            dimensions: vec![4],
        };
        f.create_image("EXTRA", &desc).unwrap();
        let bytes = f.into_bytes().unwrap();

        // The file holds the edit, and Drop did not overwrite it with an empty buffer.
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn gzip_compressed_files_open_transparently() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        {
            let mut f = FitsFile::create(&path).open().unwrap();
            let desc = ImageDescription {
                data_type: ImageType::Short,
                dimensions: vec![3, 2],
            };
            f.create_image("SCI", &desc).unwrap();
        }
        let plain = std::fs::read(&path).unwrap();
        let gz = crate::gzip::encode_for_tests(&plain);
        let gz_path = dir.path().join("test.fits.gz");
        std::fs::write(&gz_path, &gz).unwrap();

        let from_file = FitsFile::open(&gz_path).unwrap();
        assert_eq!(from_file.data(), &plain[..]);
        assert_eq!(from_file.hdu("SCI").unwrap().hdu_index, 1);

        let from_bytes = FitsFile::from_bytes(gz.clone()).unwrap();
        assert_eq!(from_bytes.data(), &plain[..]);

        // Saving would replace the compressed file with plain bytes, so editing is refused.
        assert!(FitsFile::edit(&gz_path).is_err());
        assert_eq!(std::fs::read(&gz_path).unwrap(), gz);
    }

    #[test]
    fn create_and_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        let f = FitsFile::create(&path).open().unwrap();
        assert_eq!(f.mode(), FileOpenMode::ReadWrite);
        assert!(f.data().len() >= 2880);
    }

    #[test]
    fn create_exists_without_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        FitsFile::create(&path).open().unwrap();
        assert!(FitsFile::create(&path).open().is_err());
    }

    #[test]
    fn create_with_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        FitsFile::create(&path).open().unwrap();
        FitsFile::create(&path).overwrite().open().unwrap();
    }

    #[test]
    fn open_readonly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        FitsFile::create(&path).open().unwrap();
        let f = FitsFile::open(&path).unwrap();
        assert_eq!(f.mode(), FileOpenMode::ReadOnly);
    }

    #[test]
    fn edit_mode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        FitsFile::create(&path).open().unwrap();
        let f = FitsFile::edit(&path).unwrap();
        assert_eq!(f.mode(), FileOpenMode::ReadWrite);
    }

    #[test]
    fn primary_hdu() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        let f = FitsFile::create(&path).open().unwrap();
        let hdu = f.primary_hdu().unwrap();
        assert_eq!(hdu.hdu_index, 0);
    }

    #[test]
    fn num_hdus() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        let f = FitsFile::create(&path).open().unwrap();
        assert_eq!(f.num_hdus().unwrap(), 1);
    }

    #[test]
    fn create_image_extension() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        let mut f = FitsFile::create(&path).open().unwrap();
        let desc = ImageDescription {
            data_type: ImageType::Float,
            dimensions: vec![10, 10],
        };
        let hdu = f.create_image("SCI", &desc).unwrap();
        assert_eq!(hdu.hdu_index, 1);
        assert_eq!(f.num_hdus().unwrap(), 2);
    }

    #[test]
    fn hdu_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        let mut f = FitsFile::create(&path).open().unwrap();
        let desc = ImageDescription {
            data_type: ImageType::Float,
            dimensions: vec![10],
        };
        f.create_image("SCI", &desc).unwrap();
        let hdu = f.hdu("SCI").unwrap();
        assert_eq!(hdu.hdu_index, 1);
    }

    /// cfitsio compares names ignoring case and falls back to HDUNAME, and
    /// callers rely on it (one pipeline asks for both "Orbit_Attitude" and
    /// "ORBIT_ATTITUDE").
    #[test]
    fn hdu_by_name_ignores_case_and_falls_back_to_hduname() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        let mut f = FitsFile::create(&path).open().unwrap();
        let desc = ImageDescription {
            data_type: ImageType::Float,
            dimensions: vec![10],
        };
        f.create_image("Events", &desc).unwrap();
        let second = f.create_image("WHT", &desc).unwrap();
        second
            .write_key(&mut f, "HDUNAME", &"Weights".to_string())
            .unwrap();

        assert_eq!(f.hdu("Events").unwrap().hdu_index, 1);
        assert_eq!(f.hdu("EVENTS").unwrap().hdu_index, 1);
        assert_eq!(f.hdu("events").unwrap().hdu_index, 1);
        assert_eq!(f.hdu("wht").unwrap().hdu_index, 2);
        assert_eq!(f.hdu("WEIGHTS").unwrap().hdu_index, 2);
        assert!(f.hdu("EVENT").is_err());
    }

    #[test]
    fn hdu_by_index() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        let f = FitsFile::create(&path).open().unwrap();
        let hdu = f.hdu(0usize).unwrap();
        assert_eq!(hdu.hdu_index, 0);
    }

    #[test]
    fn hdu_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        let f = FitsFile::create(&path).open().unwrap();
        assert!(f.hdu("MISSING").is_err());
    }

    #[test]
    fn iter_hdus() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        let mut f = FitsFile::create(&path).open().unwrap();
        let desc = ImageDescription {
            data_type: ImageType::Short,
            dimensions: vec![5],
        };
        f.create_image("EXT1", &desc).unwrap();
        f.create_image("EXT2", &desc).unwrap();
        let hdus = f.iter().unwrap();
        assert_eq!(hdus.len(), 3);
    }
}
