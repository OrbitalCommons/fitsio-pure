//! Streaming FITS reading over [`std::io::Read`].
//!
//! [`FitsReader`](crate::stream::FitsReader) walks a FITS stream one HDU at a time without holding the
//! file in memory. Each call to [`FitsReader::next_hdu`](crate::stream::FitsReader::next_hdu) reads only the next
//! header, so the cards, [`HduInfo`](crate::hdu::HduInfo) and image dimensions of an HDU are
//! available without touching its data unit. The data unit can then be
//! skipped, or an image decoded from it in fixed-size chunks straight into
//! typed pixels.
//!
//! HDU discovery follows [`parse_fits`](crate::hdu::parse_fits): the first HDU
//! must be primary, a later header that cannot be parsed ends the file, and
//! trailing padding may be missing after the last data unit. The [`Hdu`](crate::hdu::Hdu)s
//! yielded are the ones `parse_fits` returns for the same bytes, with offsets
//! counted from where the reader started.
//!
//! Tile-compressed images are decoded by reading their compressed data unit
//! (table and heap) into memory and decompressing it, so their peak memory is
//! the compressed size plus the output, not a fixed chunk.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::vec::Vec;

use crate::block::{padded_byte_len, BLOCK_SIZE};
use crate::error::{Error, Result};
use crate::hdu::{compute_data_byte_len, is_primary_hdu, parse_hdu_info, Hdu, HduInfo};
use crate::header::{header_byte_len, parse_header_blocks, Card};
use crate::image::{bytes_per_pixel, ImageData};

/// Bytes of the data unit read and converted at a time.
const CHUNK_SIZE: usize = 1 << 20;

/// Moves a seekable reader forward by `n` bytes.
type SkipFn<R> = fn(&mut R, u64) -> io::Result<()>;

fn seek_forward<R: Seek>(inner: &mut R, n: u64) -> io::Result<()> {
    let n = i64::try_from(n).map_err(|_| io::Error::other("seek distance overflows i64"))?;
    inner.seek(SeekFrom::Current(n)).map(|_| ())
}

fn to_usize(n: u64) -> Result<usize> {
    usize::try_from(n).map_err(|_| Error::InvalidHeader("file offset exceeds usize"))
}

fn alloc_error<E>(_: E) -> Error {
    Error::InvalidHeader("data unit too large to allocate")
}

/// A FITS file read one HDU at a time from any [`Read`].
///
/// ```no_run
/// use fitsio_pure::hdu::HduInfo;
/// use fitsio_pure::stream::FitsReader;
///
/// let mut reader = FitsReader::open("image.fits")?;
/// while let Some(hdu) = reader.next_hdu()? {
///     if let HduInfo::Image { naxes, .. } = &hdu.info {
///         println!("image extension {naxes:?}");
///         let pixels = reader.read_image()?;
///     }
/// }
/// # Ok::<(), fitsio_pure::Error>(())
/// ```
pub struct FitsReader<R> {
    inner: R,
    /// Bytes consumed from `inner` since the reader was created.
    pos: u64,
    /// Total bytes `inner` will yield, when known.
    len: Option<u64>,
    /// Seek-based skipping, set when `inner` is [`Seek`].
    skip: Option<SkipFn<R>>,
    current: Option<Hdu>,
    /// HDUs read so far.
    count: usize,
    /// Unread bytes of the current data unit.
    data_left: u64,
    /// Unread padding after the current data unit.
    pad_left: u64,
    done: bool,
}

impl<R: Read> FitsReader<R> {
    /// Reads FITS from `inner`, whose length is unknown.
    ///
    /// Data units are skipped by reading and discarding them. A truncated data
    /// unit is only detected when it is read or skipped; prefer
    /// [`with_len`](Self::with_len), [`seekable`](Self::seekable) or
    /// [`open`](FitsReader::open) when the length is available, so a declared
    /// size larger than the input is rejected before anything is allocated.
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            pos: 0,
            len: None,
            skip: None,
            current: None,
            count: 0,
            data_left: 0,
            pad_left: 0,
            done: false,
        }
    }

    /// Reads FITS from `inner`, which will yield exactly `len` bytes.
    ///
    /// Each HDU's declared data size is checked against `len` when its header
    /// is read, so a truncated or hostile file is an error before any pixel
    /// buffer is allocated.
    pub fn with_len(inner: R, len: u64) -> Self {
        Self {
            len: Some(len),
            ..Self::new(inner)
        }
    }

    /// Reads the next HDU's header, skipping whatever is left of the current
    /// HDU's data unit first.
    ///
    /// Only header blocks are read: the reader is left at the start of the new
    /// HDU's data unit. Returns `Ok(None)` at the end of the file, or when a
    /// header after the first cannot be parsed, as
    /// [`parse_fits`](crate::hdu::parse_fits) does.
    pub fn next_hdu(&mut self) -> Result<Option<&Hdu>> {
        if self.done {
            return Ok(None);
        }
        if self.count > 0 {
            self.skip_data()?;
        }
        self.current = None;
        let first = self.count == 0;
        let header_start = self.pos;

        let raw = match self.read_header_bytes()? {
            Some(raw) => raw,
            None if first => return Err(Error::UnexpectedEof),
            None => {
                self.done = true;
                return Ok(None);
            }
        };
        let parsed = parse_header_blocks(&raw).and_then(|cards| {
            let is_primary = first && is_primary_hdu(&cards);
            if first && !is_primary {
                return Err(Error::InvalidHeader("first HDU must be primary"));
            }
            let info = parse_hdu_info(&cards, is_primary)?;
            let data_len = compute_data_byte_len(&cards, is_primary)?;
            Ok((cards, info, data_len))
        });
        let (cards, info, data_len) = match parsed {
            Ok(parsed) => parsed,
            Err(e) if first => return Err(e),
            Err(_) => {
                self.done = true;
                return Ok(None);
            }
        };

        let data_start = self.pos;
        if let Some(len) = self.len {
            let data_end = data_start
                .checked_add(data_len as u64)
                .ok_or(Error::InvalidHeader("data size overflow"))?;
            if data_len > 0 && data_end > len {
                return Err(Error::UnexpectedEof);
            }
        }

        self.count += 1;
        self.data_left = data_len as u64;
        self.pad_left = (padded_byte_len(data_len) - data_len) as u64;
        self.current = Some(Hdu {
            info,
            header_start: to_usize(header_start)?,
            data_start: to_usize(data_start)?,
            data_len,
            cards,
        });
        Ok(self.current.as_ref())
    }

    /// The HDU whose header was read last, if any.
    pub fn current(&self) -> Option<&Hdu> {
        self.current.as_ref()
    }

    /// Moves past the rest of the current data unit and its padding without
    /// keeping any of it.
    ///
    /// Seekable readers seek; others read and discard. Missing padding at the
    /// end of the file is allowed; missing data is [`Error::UnexpectedEof`].
    /// Does nothing before the first HDU or after the data has been skipped.
    pub fn skip_data(&mut self) -> Result<()> {
        let data_left = self.data_left;
        let skipped = self.discard(data_left)?;
        self.data_left -= skipped;
        if skipped < data_left {
            return Err(Error::UnexpectedEof);
        }
        let pad_left = self.pad_left;
        self.discard(pad_left)?;
        self.pad_left = 0;
        Ok(())
    }

    /// Decodes the current HDU's image into native-endian pixels typed by
    /// BITPIX.
    ///
    /// Gives the same result as [`read_image_data`](crate::image::read_image_data).
    /// The data unit is read in 1 MiB chunks and converted into the output as
    /// it arrives, so peak memory is the output plus one chunk. The padding
    /// after the data unit is not read. BSCALE/BZERO are not applied.
    ///
    /// Tile-compressed images are read whole and decompressed (see the
    /// [module docs](crate::stream)). Errors if the HDU is not an image, or its data has
    /// already been read or skipped.
    pub fn read_image(&mut self) -> Result<ImageData> {
        let (bitpix, npixels) = match self.image_layout()? {
            Layout::Plain { bitpix, npixels } => (bitpix, npixels),
            Layout::Compressed => return self.read_compressed(),
        };
        Ok(match bitpix {
            8 => ImageData::U8(self.read_pixels(npixels)?),
            16 => ImageData::I16(self.read_pixels(npixels)?),
            32 => ImageData::I32(self.read_pixels(npixels)?),
            64 => ImageData::I64(self.read_pixels(npixels)?),
            -32 => ImageData::F32(self.read_pixels(npixels)?),
            -64 => ImageData::F64(self.read_pixels(npixels)?),
            other => return Err(Error::InvalidBitpix(other)),
        })
    }

    /// Decodes the current HDU's image into `buf`, converting each pixel to
    /// `f32`.
    ///
    /// `buf` must hold exactly the image's pixel count, else
    /// [`Error::InvalidValue`]. Gives the same values as
    /// [`read_image_data_into_f32`](crate::image::read_image_data_into_f32),
    /// reading in chunks as [`read_image`](Self::read_image) does.
    pub fn read_image_into_f32(&mut self, buf: &mut [f32]) -> Result<()> {
        self.read_image_into(buf)
    }

    /// Decodes the current HDU's image into `buf`, converting each pixel to
    /// `f64`.
    ///
    /// `buf` must hold exactly the image's pixel count, else
    /// [`Error::InvalidValue`]. Gives the same values as
    /// [`read_image_data_into_f64`](crate::image::read_image_data_into_f64).
    pub fn read_image_into_f64(&mut self, buf: &mut [f64]) -> Result<()> {
        self.read_image_into(buf)
    }

    /// Returns the underlying reader, positioned wherever reading stopped.
    pub fn into_inner(self) -> R {
        self.inner
    }

    fn read_image_into<O: Output>(&mut self, buf: &mut [O]) -> Result<()> {
        let (bitpix, npixels) = match self.image_layout()? {
            Layout::Plain { bitpix, npixels } => (bitpix, npixels),
            Layout::Compressed => {
                if buf.len() != compressed_pixel_count(self.current_hdu()?) {
                    return Err(Error::InvalidValue);
                }
                return match self.read_compressed()? {
                    ImageData::U8(v) => fill_from(buf, &v),
                    ImageData::I16(v) => fill_from(buf, &v),
                    ImageData::I32(v) => fill_from(buf, &v),
                    ImageData::I64(v) => fill_from(buf, &v),
                    ImageData::F32(v) => fill_from(buf, &v),
                    ImageData::F64(v) => fill_from(buf, &v),
                };
            }
        };
        if buf.len() != npixels {
            return Err(Error::InvalidValue);
        }
        match bitpix {
            8 => self.fill_pixels::<u8, O>(buf),
            16 => self.fill_pixels::<i16, O>(buf),
            32 => self.fill_pixels::<i32, O>(buf),
            64 => self.fill_pixels::<i64, O>(buf),
            -32 => self.fill_pixels::<f32, O>(buf),
            -64 => self.fill_pixels::<f64, O>(buf),
            other => Err(Error::InvalidBitpix(other)),
        }
    }

    fn current_hdu(&self) -> Result<&Hdu> {
        self.current
            .as_ref()
            .ok_or(Error::InvalidHeader("no current HDU"))
    }

    /// The shape of the current image, checking its data is still unread.
    fn image_layout(&self) -> Result<Layout> {
        let hdu = self.current_hdu()?;
        if self.data_left != hdu.data_len as u64 {
            return Err(Error::InvalidHeader("data unit already read"));
        }
        match &hdu.info {
            HduInfo::Primary { bitpix, .. } | HduInfo::Image { bitpix, .. } => {
                let bpp = bytes_per_pixel(*bitpix)?;
                Ok(Layout::Plain {
                    bitpix: *bitpix,
                    npixels: hdu.data_len / bpp,
                })
            }
            HduInfo::CompressedImage { .. } => Ok(Layout::Compressed),
            _ => Err(Error::InvalidHeader("not an image HDU")),
        }
    }

    /// Reads the whole tile-compressed data unit and decompresses it.
    fn read_compressed(&mut self) -> Result<ImageData> {
        let mut hdu = self.current_hdu()?.clone();
        let mut raw = Vec::new();
        let initial = if self.len.is_some() {
            hdu.data_len
        } else {
            hdu.data_len.min(CHUNK_SIZE)
        };
        raw.try_reserve_exact(initial).map_err(alloc_error)?;
        self.stream_data(hdu.data_len, |chunk| {
            raw.try_reserve(chunk.len()).map_err(alloc_error)?;
            raw.extend_from_slice(chunk);
            Ok(())
        })?;
        hdu.data_start = 0;
        crate::tiled::read_tiled_image(&raw, &hdu)
    }

    /// Reads `npixels` big-endian values of `T` from the data unit.
    ///
    /// With a known length the output is reserved up front (the size has been
    /// checked against the input). Otherwise it grows as data arrives, so a
    /// header declaring more data than the stream holds cannot force a large
    /// allocation.
    fn read_pixels<T: Pixel>(&mut self, npixels: usize) -> Result<Vec<T>> {
        let mut out: Vec<T> = Vec::new();
        let initial = if self.len.is_some() {
            npixels
        } else {
            npixels.min(CHUNK_SIZE / T::SIZE)
        };
        out.try_reserve_exact(initial).map_err(alloc_error)?;
        self.stream_data(npixels * T::SIZE, |chunk| {
            out.try_reserve(chunk.len() / T::SIZE)
                .map_err(alloc_error)?;
            out.extend(chunk.chunks_exact(T::SIZE).map(T::from_be));
            Ok(())
        })?;
        Ok(out)
    }

    /// Reads `buf.len()` big-endian values of `T` into `buf`, converted.
    fn fill_pixels<T: Pixel, O: Output>(&mut self, buf: &mut [O]) -> Result<()> {
        let nbytes = buf.len() * T::SIZE;
        let mut filled = 0;
        self.stream_data(nbytes, |chunk| {
            let n = chunk.len() / T::SIZE;
            for (slot, bytes) in buf[filled..filled + n]
                .iter_mut()
                .zip(chunk.chunks_exact(T::SIZE))
            {
                *slot = O::from_pixel(T::from_be(bytes));
            }
            filled += n;
            Ok(())
        })
    }

    /// Reads `nbytes` of the current data unit in chunks of at most
    /// [`CHUNK_SIZE`], passing each to `f`.
    fn stream_data(&mut self, nbytes: usize, mut f: impl FnMut(&[u8]) -> Result<()>) -> Result<()> {
        if nbytes as u64 > self.data_left {
            return Err(Error::UnexpectedEof);
        }
        let mut buf = Vec::new();
        buf.try_reserve_exact(nbytes.min(CHUNK_SIZE))
            .map_err(alloc_error)?;
        buf.resize(nbytes.min(CHUNK_SIZE), 0u8);
        let mut left = nbytes;
        while left > 0 {
            let chunk = &mut buf[..left.min(CHUNK_SIZE)];
            let n = self.read_full(chunk)?;
            self.data_left -= n as u64;
            if n < chunk.len() {
                return Err(Error::UnexpectedEof);
            }
            f(chunk)?;
            left -= n;
        }
        Ok(())
    }

    /// Reads header blocks until one holds END. `None` if the input ends
    /// first.
    fn read_header_bytes(&mut self) -> Result<Option<Vec<u8>>> {
        let mut raw = Vec::new();
        loop {
            let start = raw.len();
            raw.resize(start + BLOCK_SIZE, 0u8);
            if self.read_full(&mut raw[start..])? < BLOCK_SIZE {
                return Ok(None);
            }
            if header_byte_len(&raw[start..]).is_ok() {
                return Ok(Some(raw));
            }
        }
    }

    /// Fills as much of `buf` as the input allows, returning the count read.
    fn read_full(&mut self, buf: &mut [u8]) -> Result<usize> {
        let mut filled = 0;
        while filled < buf.len() {
            match self.inner.read(&mut buf[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e.into()),
            }
        }
        self.pos += filled as u64;
        Ok(filled)
    }

    /// Moves forward up to `n` bytes, stopping at the end of the input.
    /// Returns how far it moved.
    fn discard(&mut self, n: u64) -> Result<u64> {
        if n == 0 {
            return Ok(0);
        }
        let moved = match (self.skip, self.len) {
            (Some(skip), Some(len)) => {
                let k = n.min(len.saturating_sub(self.pos));
                skip(&mut self.inner, k)?;
                k
            }
            _ => io::copy(&mut (&mut self.inner).take(n), &mut io::sink())?,
        };
        self.pos += moved;
        Ok(moved)
    }
}

impl<R: Read + Seek> FitsReader<R> {
    /// Reads FITS from a seekable `inner`, starting at its current position.
    ///
    /// The length is taken from the stream, so declared data sizes are
    /// checked before allocating, and data units are skipped by seeking.
    pub fn seekable(mut inner: R) -> Result<Self> {
        let start = inner.stream_position()?;
        let end = inner.seek(SeekFrom::End(0))?;
        inner.seek(SeekFrom::Start(start))?;
        let mut reader = Self::with_len(inner, end.saturating_sub(start));
        reader.skip = Some(seek_forward::<R>);
        Ok(reader)
    }
}

impl FitsReader<File> {
    /// Opens the FITS file at `path` for streaming reads.
    ///
    /// Gzip-compressed files are not decompressed; open them with
    /// `compat::FitsFile` or decompress them first.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::seekable(File::open(path)?)
    }
}

impl<'a> FitsReader<io::Cursor<&'a [u8]>> {
    /// Reads FITS from bytes already in memory, with the same bounds checks as
    /// a file. The caller still holds `data`, so this saves no memory over
    /// [`parse_fits`](crate::hdu::parse_fits); it is for code that serves
    /// both files and buffers through one path.
    pub fn from_slice(data: &'a [u8]) -> Self {
        let mut reader = Self::with_len(io::Cursor::new(data), data.len() as u64);
        reader.skip = Some(seek_forward);
        reader
    }
}

/// Reads only the primary header from `reader`: its cards, [`HduInfo`] and
/// data size, without reading the data unit.
///
/// The returned [`Hdu`]'s offsets are counted from where `reader` started.
pub fn read_primary_header<R: Read>(reader: R) -> Result<Hdu> {
    let mut reader = FitsReader::new(reader);
    reader.next_hdu()?;
    reader.current.take().ok_or(Error::UnexpectedEof)
}

/// The keyword cards of the primary header, read without the data unit.
pub fn read_header<R: Read>(reader: R) -> Result<Vec<Card>> {
    read_primary_header(reader).map(|hdu| hdu.cards)
}

enum Layout {
    Plain { bitpix: i64, npixels: usize },
    Compressed,
}

fn compressed_pixel_count(hdu: &Hdu) -> usize {
    match &hdu.info {
        HduInfo::CompressedImage { znaxes, .. } => znaxes.iter().product(),
        _ => 0,
    }
}

fn fill_from<T: Pixel, O: Output>(buf: &mut [O], src: &[T]) -> Result<()> {
    if buf.len() != src.len() {
        return Err(Error::InvalidValue);
    }
    for (slot, &p) in buf.iter_mut().zip(src) {
        *slot = O::from_pixel(p);
    }
    Ok(())
}

/// A FITS pixel type stored big-endian on disk.
trait Pixel: Copy {
    const SIZE: usize;
    fn from_be(bytes: &[u8]) -> Self;
    fn to_f32(self) -> f32;
    fn to_f64(self) -> f64;
}

/// An element type of a caller-provided output buffer.
trait Output: Copy {
    fn from_pixel<T: Pixel>(p: T) -> Self;
}

impl Output for f32 {
    #[inline]
    fn from_pixel<T: Pixel>(p: T) -> Self {
        p.to_f32()
    }
}

impl Output for f64 {
    #[inline]
    fn from_pixel<T: Pixel>(p: T) -> Self {
        p.to_f64()
    }
}

macro_rules! pixel {
    ($($t:ty),*) => {$(
        impl Pixel for $t {
            const SIZE: usize = core::mem::size_of::<$t>();
            #[inline]
            fn from_be(bytes: &[u8]) -> Self {
                let mut b = [0u8; core::mem::size_of::<$t>()];
                b.copy_from_slice(bytes);
                <$t>::from_be_bytes(b)
            }
            #[inline]
            fn to_f32(self) -> f32 {
                self as f32
            }
            #[inline]
            fn to_f64(self) -> f64 {
                self as f64
            }
        }
    )*};
}

pixel!(u8, i16, i32, i64, f32, f64);
