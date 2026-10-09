use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use super::errors::{Error, Result};
use super::hdu::FitsHdu;
use super::images::ImageDescription;
use super::sys;
use crate::io::write_atomic;

/// Whether a file is opened for reading or writing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOpenMode {
    ReadOnly,
    ReadWrite,
}

/// An open FITS file.
///
/// A file opened with [`FitsFile::open`] is read on demand: opening it reads
/// the headers alone, and image reads fetch only the pixels they return.
/// Writable and in-memory files are held in memory whole.
///
/// The parse cache is a [`OnceLock`], so `&FitsFile` is `Sync` and one open
/// file can be shared across threads (reading every column of a table in
/// parallel, say) instead of reopening and reparsing the file per thread.
pub struct FitsFile {
    storage: Storage,
    filename: PathBuf,
    mode: FileOpenMode,
    /// No backing file, so `flush` and `Drop` never touch disk.
    in_memory: bool,
    cached_parse: OnceLock<crate::hdu::FitsData>,
}

/// Where an open file's bytes are.
enum Storage {
    /// The whole file, in memory.
    Memory(Vec<u8>),
    /// A file on disk opened read-only, read as needed. `loaded` holds the
    /// whole file once something has needed all of it.
    Disk {
        file: Mutex<std::fs::File>,
        len: usize,
        loaded: OnceLock<Vec<u8>>,
    },
}

/// Builder for creating a new FITS file.
pub struct NewFitsFile<'a> {
    path: PathBuf,
    image_description: Option<ImageDescription<'a>>,
    overwrite: bool,
}

/// Trait for types that can identify an HDU (by index or name).
///
/// A missing HDU is an error with cfitsio's status: `END_OF_FILE` for an index
/// past the last HDU, `BAD_HDU_NUM` for a name no HDU has.
pub trait DescribesHdu {
    fn get_hdu<'a>(
        &self,
        fits_data: &'a crate::hdu::FitsData,
    ) -> Result<(usize, &'a crate::hdu::Hdu)>;
}

impl DescribesHdu for usize {
    fn get_hdu<'a>(
        &self,
        fits_data: &'a crate::hdu::FitsData,
    ) -> Result<(usize, &'a crate::hdu::Hdu)> {
        fits_data
            .get(*self)
            .map(|hdu| (*self, hdu))
            .ok_or(Error::status(sys::END_OF_FILE))
    }
}

/// Matches the first HDU whose `EXTNAME`, or failing that `HDUNAME`, equals the
/// name ignoring case, as cfitsio does.
impl DescribesHdu for &str {
    fn get_hdu<'a>(
        &self,
        fits_data: &'a crate::hdu::FitsData,
    ) -> Result<(usize, &'a crate::hdu::Hdu)> {
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
            .ok_or(Error::status(sys::BAD_HDU_NUM))
    }
}

impl DescribesHdu for String {
    fn get_hdu<'a>(
        &self,
        fits_data: &'a crate::hdu::FitsData,
    ) -> Result<(usize, &'a crate::hdu::Hdu)> {
        self.as_str().get_hdu(fits_data)
    }
}

impl FitsFile {
    /// Open an existing FITS file in read-only mode.
    ///
    /// Only the headers are read here. Image reads then read just the pixels
    /// they return, so cutting a small region from a large image is fast and
    /// uses little memory. Table reads, and reads of tile-compressed images,
    /// read the whole file once.
    ///
    /// A gzip-compressed file (`.fits.gz`, `.fit.gz`) is decompressed into
    /// memory, as cfitsio does.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let not_opened = |_| Error::status(sys::FILE_NOT_OPENED);
        let mut file = std::fs::File::open(path).map_err(not_opened)?;
        let len = file.metadata().map_err(not_opened)?.len() as usize;
        let mut magic = [0u8; 2];
        let storage = if len >= magic.len()
            && file.read_exact(&mut magic).is_ok()
            && crate::gzip::is_gzip(&magic)
        {
            let mut data = Vec::with_capacity(len);
            file.seek(SeekFrom::Start(0))?;
            file.read_to_end(&mut data)?;
            Storage::Memory(crate::gzip::decompress(&data)?)
        } else {
            Storage::Disk {
                file: Mutex::new(file),
                len,
                loaded: OnceLock::new(),
            }
        };
        let file = FitsFile {
            storage,
            filename: path.to_path_buf(),
            mode: FileOpenMode::ReadOnly,
            in_memory: false,
            cached_parse: OnceLock::new(),
        };
        // As cfitsio does, fail to open a file that isn't FITS.
        let mut first = [0u8; 8];
        if file.byte_len() < crate::BLOCK_SIZE {
            return Err(Error::status(sys::READ_ERROR));
        }
        file.read_at(0, &mut first)?;
        if &first != b"SIMPLE  " {
            return Err(Error::status(sys::UNKNOWN_REC));
        }
        file.parsed()?;
        Ok(file)
    }

    /// Open an existing FITS file for editing.
    ///
    /// A gzip-compressed file is refused, as cfitsio refuses it: saving would
    /// replace it with uncompressed bytes. Decompress it first to edit it.
    pub fn edit<P: AsRef<Path>>(path: P) -> Result<Self> {
        let data = std::fs::read(path.as_ref()).map_err(|_| Error::status(sys::FILE_NOT_OPENED))?;
        if crate::gzip::is_gzip(&data) {
            return Err(Error::Message(format!(
                "{} is gzip-compressed and can only be opened read-only",
                path.as_ref().display()
            )));
        }
        Ok(FitsFile {
            storage: Storage::Memory(data),
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
            storage: Storage::Memory(gunzip_if_compressed(data.into())?),
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
        Self::new_in_memory(primary_hdu_bytes(None)?)
    }

    /// Create a new, writable in-memory FITS file whose primary HDU is an image
    /// described by `desc`, the in-memory twin of
    /// [`NewFitsFile::with_custom_primary`].
    ///
    /// Write its pixels through [`FitsFile::primary_hdu`].
    pub fn create_in_memory_with_custom_primary(desc: &ImageDescription) -> Result<Self> {
        Self::new_in_memory(primary_hdu_bytes(Some(desc))?)
    }

    fn new_in_memory(data: Vec<u8>) -> Result<Self> {
        Ok(FitsFile {
            storage: Storage::Memory(data),
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
        let parsed = match &self.storage {
            Storage::Memory(data) => crate::hdu::parse_fits(data)?,
            Storage::Disk { file, len, .. } => {
                crate::hdu::parse_fits_headers(*len, |offset, buf| read_file_at(file, offset, buf))?
            }
        };
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
    pub fn create<'a, P: AsRef<Path>>(path: P) -> NewFitsFile<'a> {
        NewFitsFile {
            path: path.as_ref().to_path_buf(),
            image_description: None,
            overwrite: false,
        }
    }

    /// Return a handle to the primary HDU (index 0).
    pub fn primary_hdu(&self) -> Result<FitsHdu> {
        FitsHdu::at(self, 0)
    }

    /// Return a handle to the HDU described by `desc` (index or name).
    pub fn hdu<D: DescribesHdu>(&self, desc: D) -> Result<FitsHdu> {
        let (idx, _) = desc.get_hdu(self.parsed()?)?;
        FitsHdu::at(self, idx)
    }

    /// Return the number of HDUs in this file.
    pub fn num_hdus(&self) -> Result<usize> {
        let fits_data = self.parsed()?;
        Ok(fits_data.len())
    }

    /// Return handles to all HDUs in the file.
    pub fn iter(&self) -> Result<Vec<FitsHdu>> {
        let count = self.parsed()?.len();
        (0..count).map(|i| FitsHdu::at(self, i)).collect()
    }

    /// Create a new image extension HDU with the given name and description.
    ///
    /// The name is anything that converts into a `String`, as in `fitsio`, so
    /// both `"SCI"` and `"SCI".to_string()` work.
    pub fn create_image<T: Into<String>>(
        &mut self,
        extname: T,
        desc: &ImageDescription,
    ) -> Result<FitsHdu> {
        self.check_writable()?;
        // `dimensions` are row-major, as in `fitsio`; FITS lists NAXIS1 first.
        let cards = crate::extension::build_extension_header(
            crate::extension::ExtensionType::Image,
            desc.data_type.to_bitpix(),
            &super::hdu::row_major(desc.dimensions),
            0,
            1,
        )?;
        let extname = extname.into();
        append_image_hdu(self.bytes_mut()?, cards, &extname, desc)?;

        let fits_data = self.parsed()?;
        let idx = fits_data.len() - 1;
        FitsHdu::at(self, idx)
    }

    /// Create a new binary table extension HDU.
    pub fn create_table(
        &mut self,
        extname: &str,
        columns: &[crate::bintable::BinaryColumnDescriptor],
    ) -> Result<FitsHdu> {
        self.check_writable()?;
        let mut cards = crate::bintable::build_binary_table_cards(columns, 0, 0)?;

        let extname_card = crate::header::Card {
            keyword: make_keyword("EXTNAME"),
            value: Some(crate::value::Value::String(extname.to_string())),
            comment: None,
        };
        cards.push(extname_card);

        let header_bytes = crate::header::serialize_header(&cards)?;
        self.bytes_mut()?.extend_from_slice(&header_bytes);

        self.invalidate_cache();
        let fits_data = self.parsed()?;
        let idx = fits_data.len() - 1;
        FitsHdu::at(self, idx)
    }

    /// Create a new ASCII table extension HDU.
    pub fn create_ascii_table(
        &mut self,
        extname: &str,
        columns: &[crate::table::AsciiColumnDescriptor],
    ) -> Result<FitsHdu> {
        self.check_writable()?;
        let mut cards = crate::table::build_ascii_table_cards(columns, 0)?;

        let extname_card = crate::header::Card {
            keyword: make_keyword("EXTNAME"),
            value: Some(crate::value::Value::String(extname.to_string())),
            comment: None,
        };
        cards.push(extname_card);

        let header_bytes = crate::header::serialize_header(&cards)?;
        self.bytes_mut()?.extend_from_slice(&header_bytes);

        self.invalidate_cache();
        let fits_data = self.parsed()?;
        let idx = fits_data.len() - 1;
        FitsHdu::at(self, idx)
    }

    /// Return the file's FITS bytes.
    ///
    /// For a file opened with [`FitsFile::open`], the first call reads the
    /// whole file into memory.
    pub fn data(&self) -> Result<&[u8]> {
        match &self.storage {
            Storage::Memory(data) => Ok(data),
            Storage::Disk { file, len, loaded } => {
                if let Some(data) = loaded.get() {
                    return Ok(data);
                }
                let mut data = vec![0u8; *len];
                read_file_at(file, 0, &mut data)?;
                // A concurrent caller may have loaded it first; keep either.
                Ok(loaded.get_or_init(|| data))
            }
        }
    }

    /// The length of the file in bytes.
    fn byte_len(&self) -> usize {
        match &self.storage {
            Storage::Memory(data) => data.len(),
            Storage::Disk { len, .. } => *len,
        }
    }

    /// Fill `buf` with the file's bytes from `offset`, reading from disk only
    /// what is asked for.
    pub(crate) fn read_at(&self, offset: usize, buf: &mut [u8]) -> Result<()> {
        let data = match &self.storage {
            Storage::Memory(data) => data,
            Storage::Disk { file, loaded, .. } => match loaded.get() {
                Some(data) => data,
                None => return Ok(read_file_at(file, offset, buf)?),
            },
        };
        let bytes = offset
            .checked_add(buf.len())
            .and_then(|end| data.get(offset..end))
            .ok_or(crate::Error::UnexpectedEof)?;
        buf.copy_from_slice(bytes);
        Ok(())
    }

    /// Replace the in-memory FITS bytes (used by write operations).
    pub fn set_data(&mut self, data: Vec<u8>) {
        self.storage = Storage::Memory(data);
        self.invalidate_cache();
    }

    /// The whole file in memory, to change it, reading it in first if needed.
    fn bytes_mut(&mut self) -> Result<&mut Vec<u8>> {
        if let Storage::Disk { .. } = self.storage {
            let data = self.data()?.to_vec();
            self.storage = Storage::Memory(data);
        }
        self.invalidate_cache();
        match &mut self.storage {
            Storage::Memory(data) => Ok(data),
            Storage::Disk { .. } => unreachable!("loaded into memory above"),
        }
    }

    /// Replace the bytes in `range` with `bytes`, moving what follows only if
    /// the length changes.
    pub(crate) fn replace_bytes(
        &mut self,
        range: std::ops::Range<usize>,
        bytes: &[u8],
    ) -> Result<()> {
        let data = self.bytes_mut()?;
        if range.len() == bytes.len() {
            data[range].copy_from_slice(bytes);
        } else {
            data.splice(range, bytes.iter().copied());
        }
        Ok(())
    }

    /// Mutable access to the FITS bytes, for writes that change bytes in
    /// place without moving anything.
    pub(crate) fn data_mut(&mut self) -> Result<&mut [u8]> {
        Ok(self.bytes_mut()?)
    }

    /// Flush the in-memory data to disk if opened for writing from a path.
    /// Does nothing for a file from [`FitsFile::create_in_memory`].
    ///
    /// The file is replaced atomically with [`crate::io::write_atomic`], so a
    /// failed or interrupted flush leaves the previous file intact.
    pub fn flush(&self) -> Result<()> {
        if self.mode == FileOpenMode::ReadWrite && !self.in_memory {
            write_atomic(&self.filename, self.data()?)?;
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
        Ok(std::mem::take(self.bytes_mut()?))
    }

    /// Return the file path. Empty for an in-memory file.
    pub fn filename(&self) -> &Path {
        &self.filename
    }

    /// Return the file path, as `fitsio`'s `file_path` does. Empty for an
    /// in-memory file.
    pub fn file_path(&self) -> &Path {
        &self.filename
    }

    /// Return the open mode.
    pub fn mode(&self) -> FileOpenMode {
        self.mode
    }

    /// Refuse to change a read-only file, with the status `fitsio` uses.
    pub(crate) fn check_writable(&self) -> Result<()> {
        match self.mode {
            FileOpenMode::ReadWrite => Ok(()),
            FileOpenMode::ReadOnly => Err(Error::status(READONLY_STATUS)),
        }
    }
}

impl Drop for FitsFile {
    fn drop(&mut self) {
        if let (FileOpenMode::ReadWrite, false, Storage::Memory(data)) =
            (self.mode, self.in_memory, &self.storage)
        {
            let _ = write_atomic(&self.filename, data);
        }
    }
}

impl<'a> NewFitsFile<'a> {
    /// Set whether to overwrite an existing file.
    pub fn overwrite(mut self) -> Self {
        self.overwrite = true;
        self
    }

    /// Make the primary HDU an image described by `description` instead of an
    /// empty one, so its pixels can be written through [`FitsFile::primary_hdu`].
    pub fn with_custom_primary(mut self, description: &ImageDescription<'a>) -> Self {
        self.image_description = Some(description.clone());
        self
    }

    /// Finalize creation: start the file with a primary HDU (minimal unless
    /// [`NewFitsFile::with_custom_primary`] was given) and return an open `FitsFile`.
    ///
    /// As with cfitsio, the file is created here but its contents are written
    /// when it is flushed or dropped, so the image isn't written twice. Until
    /// then a new file is empty, and with [`NewFitsFile::overwrite`] an
    /// existing file keeps its old contents. Each save replaces the file
    /// atomically.
    pub fn open(self) -> Result<FitsFile> {
        let data = primary_hdu_bytes(self.image_description.as_ref())?;

        if !(self.overwrite && self.path.exists()) {
            let created = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&self.path);
            match created {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    return Err(Error::ExistingFile(
                        self.path.to_string_lossy().into_owned(),
                    ))
                }
                Err(_) => return Err(Error::status(sys::FILE_NOT_CREATED)),
            }
        }

        Ok(FitsFile {
            storage: Storage::Memory(data),
            filename: self.path,
            mode: FileOpenMode::ReadWrite,
            in_memory: false,
            cached_parse: OnceLock::new(),
        })
    }
}

/// The bytes of a new primary HDU: minimal when `desc` is `None`, otherwise an
/// image named `_PRIMARY` (as rust-fitsio names it) with zeroed pixels.
fn primary_hdu_bytes(desc: Option<&ImageDescription>) -> Result<Vec<u8>> {
    match desc {
        None => {
            let cards = crate::primary::build_primary_header(8, &[])?;
            Ok(crate::header::serialize_header(&cards)?)
        }
        Some(desc) => {
            // `dimensions` are row-major, as in `fitsio`; FITS lists NAXIS1 first.
            let cards = crate::primary::build_primary_header(
                desc.data_type.to_bitpix(),
                &super::hdu::row_major(desc.dimensions),
            )?;
            let mut bytes = Vec::new();
            append_image_hdu(&mut bytes, cards, "_PRIMARY", desc)?;
            Ok(bytes)
        }
    }
}

/// Finish an image HDU from its structural `cards`: add the unsigned-type
/// scaling and `EXTNAME`, then append it to `out` with a zeroed data unit.
fn append_image_hdu(
    out: &mut Vec<u8>,
    mut cards: Vec<crate::header::Card>,
    extname: &str,
    desc: &ImageDescription,
) -> Result<()> {
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

    cards.push(crate::header::Card {
        keyword: make_keyword("EXTNAME"),
        value: Some(crate::value::Value::String(extname.to_string())),
        comment: None,
    });

    out.extend_from_slice(&crate::header::serialize_header(&cards)?);

    // NAXIS = 0 means no data unit, not the empty product's one pixel.
    let pixels = match desc.dimensions {
        [] => 0,
        dims => dims.iter().product::<usize>(),
    };
    let data_bytes = pixels * ((desc.data_type.to_bitpix().unsigned_abs() as usize) / 8);
    out.resize(out.len() + crate::block::padded_byte_len(data_bytes), 0u8);
    Ok(())
}

/// `fitsio`'s status for a write to a read-only file. Not a cfitsio status:
/// `fitsio` checks the open mode itself.
const READONLY_STATUS: u32 = 602;

/// Fill `buf` with the bytes of `file` from `offset`.
fn read_file_at(file: &Mutex<std::fs::File>, offset: usize, buf: &mut [u8]) -> crate::Result<()> {
    // A panic while holding the lock can't leave the handle inconsistent:
    // every read seeks first.
    let mut file = file.lock().unwrap_or_else(|e| e.into_inner());
    file.seek(SeekFrom::Start(offset as u64))?;
    file.read_exact(buf).map_err(|e| match e.kind() {
        std::io::ErrorKind::UnexpectedEof => crate::Error::UnexpectedEof,
        _ => crate::Error::Io(e),
    })
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
                dimensions: &[3, 2],
            };
            f.create_image("SCI", &desc).unwrap();
        }

        let from_disk = FitsFile::open(&path).unwrap();
        let from_mem = FitsFile::from_bytes(std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(from_mem.mode(), FileOpenMode::ReadOnly);
        assert_eq!(from_mem.filename(), Path::new(""));
        assert_eq!(from_mem.file_path(), Path::new(""));
        assert_eq!(from_disk.file_path(), path);
        assert_eq!(from_mem.data().unwrap(), from_disk.data().unwrap());
        assert_eq!(from_mem.num_hdus().unwrap(), from_disk.num_hdus().unwrap());
        assert_eq!(
            from_mem.hdu("SCI").unwrap().number,
            from_disk.hdu("SCI").unwrap().number
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
            dimensions: &[3, 2],
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
            dimensions: &[4],
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
                dimensions: &[3, 2],
            };
            f.create_image("SCI", &desc).unwrap();
        }
        let plain = std::fs::read(&path).unwrap();
        let gz = crate::gzip::encode_for_tests(&plain);
        let gz_path = dir.path().join("test.fits.gz");
        std::fs::write(&gz_path, &gz).unwrap();

        let from_file = FitsFile::open(&gz_path).unwrap();
        assert_eq!(from_file.data().unwrap(), &plain[..]);
        assert_eq!(from_file.hdu("SCI").unwrap().number, 1);

        let from_bytes = FitsFile::from_bytes(gz.clone()).unwrap();
        assert_eq!(from_bytes.data().unwrap(), &plain[..]);

        // Saving would replace the compressed file with plain bytes, so editing is refused.
        assert!(FitsFile::edit(&gz_path).is_err());
        assert_eq!(std::fs::read(&gz_path).unwrap(), gz);
    }

    #[test]
    fn flush_replaces_the_file_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        drop(FitsFile::create(&path).open().unwrap());

        let mut f = FitsFile::edit(&path).unwrap();
        let desc = ImageDescription {
            data_type: ImageType::Float,
            dimensions: &[2, 3],
        };
        f.create_image("SCI", &desc).unwrap();
        f.flush().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), f.data().unwrap());
        drop(f);

        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, [std::ffi::OsString::from("test.fits")]);
    }

    #[cfg(unix)]
    #[test]
    fn flush_keeps_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        drop(FitsFile::create(&path).open().unwrap());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();

        let f = FitsFile::edit(&path).unwrap();
        f.flush().unwrap();
        drop(f);
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o640);
    }

    #[test]
    fn create_and_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        let f = FitsFile::create(&path).open().unwrap();
        assert_eq!(f.mode(), FileOpenMode::ReadWrite);
        assert!(f.data().unwrap().len() >= 2880);
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
        assert_eq!(hdu.number, 0);
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
            dimensions: &[10, 10],
        };
        let hdu = f.create_image("SCI", &desc).unwrap();
        assert_eq!(hdu.number, 1);
        assert_eq!(f.num_hdus().unwrap(), 2);
    }

    #[test]
    fn hdu_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        let mut f = FitsFile::create(&path).open().unwrap();
        let desc = ImageDescription {
            data_type: ImageType::Float,
            dimensions: &[10],
        };
        f.create_image("SCI", &desc).unwrap();
        let hdu = f.hdu("SCI").unwrap();
        assert_eq!(hdu.number, 1);
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
            dimensions: &[10],
        };
        f.create_image("Events", &desc).unwrap();
        let second = f.create_image("WHT", &desc).unwrap();
        second.write_key(&mut f, "HDUNAME", "Weights").unwrap();

        assert_eq!(f.hdu("Events").unwrap().number, 1);
        assert_eq!(f.hdu("EVENTS").unwrap().number, 1);
        assert_eq!(f.hdu("events").unwrap().number, 1);
        assert_eq!(f.hdu("wht").unwrap().number, 2);
        assert_eq!(f.hdu("WEIGHTS").unwrap().number, 2);
        assert!(f.hdu("EVENT").is_err());
    }

    #[test]
    fn hdu_by_index() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        let f = FitsFile::create(&path).open().unwrap();
        let hdu = f.hdu(0usize).unwrap();
        assert_eq!(hdu.number, 0);
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
            dimensions: &[5],
        };
        f.create_image("EXT1", &desc).unwrap();
        f.create_image("EXT2", &desc).unwrap();
        let hdus = f.iter().unwrap();
        assert_eq!(hdus.len(), 3);
    }

    #[test]
    fn custom_primary_holds_the_image() {
        use crate::compat::hdu::HduInfo;
        use crate::compat::images::{ReadImage, WriteImage};

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        let desc = ImageDescription {
            data_type: ImageType::Float,
            dimensions: &[5, 7],
        };
        let pixels: Vec<f32> = (0..35).map(|i| i as f32 * 0.5 - 3.0).collect();
        {
            let mut f = FitsFile::create(&path)
                .with_custom_primary(&desc)
                .open()
                .unwrap();
            assert_eq!(f.num_hdus().unwrap(), 1);
            let hdu = f.primary_hdu().unwrap();
            f32::write_image(&mut f, &hdu, &pixels).unwrap();
        }

        let f = FitsFile::open(&path).unwrap();
        assert_eq!(f.num_hdus().unwrap(), 1);
        let hdu = f.hdu(0usize).unwrap();
        assert_eq!(
            hdu.info(&f).unwrap(),
            HduInfo::ImageInfo {
                shape: vec![5, 7],
                image_type: ImageType::Float,
            }
        );
        // Row-major [5, 7] is 5 rows of 7: NAXIS1 = 7, NAXIS2 = 5.
        assert_eq!(hdu.read_key::<i64>(&f, "NAXIS1").unwrap(), 7);
        assert_eq!(hdu.read_key::<i64>(&f, "NAXIS2").unwrap(), 5);
        assert_eq!(f32::read_image(&f, &hdu).unwrap(), pixels);
        assert_eq!(f.hdu("_PRIMARY").unwrap().number, 0);
    }

    #[test]
    fn custom_primary_unsigned_keeps_bzero() {
        use crate::compat::images::{ReadImage, WriteImage};

        let desc = ImageDescription {
            data_type: ImageType::UnsignedShort,
            dimensions: &[5, 7],
        };
        let pixels: Vec<u16> = (0..35).map(|i| i * 1871).collect();
        let mut f = FitsFile::create_in_memory_with_custom_primary(&desc).unwrap();
        let hdu = f.primary_hdu().unwrap();
        u16::write_image(&mut f, &hdu, &pixels).unwrap();

        let g = FitsFile::from_bytes(f.into_bytes().unwrap()).unwrap();
        let hdu = g.primary_hdu().unwrap();
        assert_eq!(hdu.read_key::<i64>(&g, "BITPIX").unwrap(), 16);
        assert_eq!(hdu.read_key::<i64>(&g, "BZERO").unwrap(), 32_768);
        assert_eq!(u16::read_image(&g, &hdu).unwrap(), pixels);
    }

    #[test]
    fn custom_primary_comes_before_extensions() {
        use crate::compat::images::{ReadImage, WriteImage};

        let primary = ImageDescription {
            data_type: ImageType::Short,
            dimensions: &[5, 7],
        };
        let mut f = FitsFile::create_in_memory_with_custom_primary(&primary).unwrap();
        let ext = f
            .create_image(
                "SCI",
                &ImageDescription {
                    data_type: ImageType::Double,
                    dimensions: &[3],
                },
            )
            .unwrap();
        // Writing the primary after an extension exists must not disturb it.
        let pixels: Vec<i16> = (0..35).map(|i| i * 100 - 1700).collect();
        f64::write_image(&mut f, &ext, &[1.5, -2.5, 3.5]).unwrap();
        let primary_hdu = f.primary_hdu().unwrap();
        i16::write_image(&mut f, &primary_hdu, &pixels).unwrap();

        assert_eq!(f.num_hdus().unwrap(), 2);
        assert_eq!(i16::read_image(&f, &primary_hdu).unwrap(), pixels);
        let ext = f.hdu("SCI").unwrap();
        assert_eq!(f64::read_image(&f, &ext).unwrap(), vec![1.5, -2.5, 3.5]);
    }

    #[test]
    fn custom_primary_chains_with_overwrite_in_either_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        let desc = ImageDescription {
            data_type: ImageType::Short,
            dimensions: &[2, 3],
        };
        FitsFile::create(&path).open().unwrap();
        assert!(FitsFile::create(&path)
            .with_custom_primary(&desc)
            .open()
            .is_err());

        let f = FitsFile::create(&path)
            .with_custom_primary(&desc)
            .overwrite()
            .open()
            .unwrap();
        assert_eq!(f.data().unwrap().len(), 2 * 2880);
        drop(f);
        let f = FitsFile::create(&path)
            .overwrite()
            .with_custom_primary(&desc)
            .open()
            .unwrap();
        assert_eq!(f.data().unwrap().len(), 2 * 2880);
    }

    #[test]
    fn custom_primary_without_axes_has_no_data_unit() {
        let desc = ImageDescription {
            data_type: ImageType::Float,
            dimensions: &[],
        };
        let mut f = FitsFile::create_in_memory_with_custom_primary(&desc).unwrap();
        assert_eq!(f.data().unwrap().len(), 2880);
        f.create_image("SCI", &desc).unwrap();
        assert_eq!(f.num_hdus().unwrap(), 2);
    }

    /// A file of two `u16` images (stored as `BZERO`-offset `i16`), 7×5 and
    /// 3×4, with distinct pixel values, as written by compat.
    fn two_image_file(path: &Path) -> (Vec<u16>, Vec<u16>) {
        let mut f = FitsFile::create(path).open().unwrap();
        let a: Vec<u16> = (0..35).map(|i| 40000 + i * 7).collect();
        let b: Vec<u16> = (0..12).map(|i| i * 3).collect();
        for (name, dims, pixels) in [("A", [5, 7], &a), ("B", [4, 3], &b)] {
            let desc = ImageDescription {
                data_type: ImageType::UnsignedShort,
                dimensions: &dims,
            };
            let hdu = f.create_image(name, &desc).unwrap();
            hdu.write_image(&mut f, pixels).unwrap();
        }
        (a, b)
    }

    #[test]
    fn open_reads_pixels_from_disk_as_from_bytes_does() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lazy.fits");
        let (a, b) = two_image_file(&path);

        let disk = FitsFile::open(&path).unwrap();
        let mem = FitsFile::from_bytes(std::fs::read(&path).unwrap()).unwrap();
        for f in [&disk, &mem] {
            let hdu = f.hdu("A").unwrap();
            assert_eq!(hdu.read_image::<Vec<u16>>(f).unwrap(), a);
            assert_eq!(hdu.read_section::<Vec<u16>>(f, 3, 11).unwrap(), a[3..11]);
            assert_eq!(hdu.read_rows::<Vec<u16>>(f, 1, 2).unwrap(), a[7..21]);
            // Columns 2..5 of rows 1..4: three runs, read separately.
            let region: Vec<u16> = hdu.read_region(f, &[&(2..5), &(1..4)]).unwrap();
            let expected: Vec<u16> = (1..4)
                .flat_map(|row| a[row * 7 + 2..row * 7 + 5].to_vec())
                .collect();
            assert_eq!(region, expected);
            // Whole rows: one run.
            let rows: Vec<u16> = hdu.read_region(f, &[&(0..7), &(2..4)]).unwrap();
            assert_eq!(rows, a[14..28]);

            let hdu = f.hdu("B").unwrap();
            assert_eq!(hdu.read_image::<Vec<u16>>(f).unwrap(), b);
            assert_eq!(hdu.read_key::<i64>(f, "NAXIS1").unwrap(), 3);
        }
        assert_eq!(disk.data().unwrap(), mem.data().unwrap());
    }

    #[test]
    fn open_reads_regions_from_threads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("threads.fits");
        let (a, _) = two_image_file(&path);

        let f = FitsFile::open(&path).unwrap();
        let hdu = f.hdu("A").unwrap();
        std::thread::scope(|scope| {
            for row in 0..5 {
                let (f, hdu, a) = (&f, &hdu, &a);
                scope.spawn(move || {
                    let got: Vec<u16> = hdu.read_region(f, &[&(0..7), &(row..row + 1)]).unwrap();
                    assert_eq!(got, a[row * 7..row * 7 + 7]);
                });
            }
        });
    }

    #[test]
    fn open_rejects_a_file_that_is_not_fits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("junk.fits");
        std::fs::write(&path, vec![b'x'; 2 * 2880]).unwrap();
        assert!(matches!(
            FitsFile::open(&path),
            Err(Error::Fits(crate::compat::errors::FitsError {
                status: 252,
                ..
            }))
        ));
    }

    #[test]
    fn open_reads_a_truncated_data_unit_as_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("truncated.fits");
        two_image_file(&path);
        let bytes = std::fs::read(&path).unwrap();
        // Keep the primary HDU and image A's header, and cut A's data short.
        let parsed = crate::hdu::parse_fits(&bytes).unwrap();
        let cut = parsed.hdus[1].data_start + 10;
        std::fs::write(&path, &bytes[..cut]).unwrap();
        assert!(FitsFile::open(&path).is_err());
    }

    #[test]
    fn create_writes_the_file_when_it_is_saved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new.fits");
        let desc = ImageDescription {
            data_type: ImageType::Short,
            dimensions: &[3, 2],
        };
        let f = FitsFile::create(&path)
            .with_custom_primary(&desc)
            .open()
            .unwrap();
        assert!(path.exists());
        assert!(matches!(
            FitsFile::create(&path).open(),
            Err(Error::ExistingFile(_))
        ));
        let bytes = f.data().unwrap().to_vec();
        drop(f);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn overwrite_keeps_the_old_file_until_saved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.fits");
        std::fs::write(&path, b"old contents").unwrap();
        let f = FitsFile::create(&path).overwrite().open().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"old contents");
        f.flush().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), f.data().unwrap());
    }
}
