//! Streaming image HDU writer.
//!
//! [`ImageWriter`](crate::image_writer::ImageWriter) writes an image HDU to
//! any [`Write`](crate::io::Write) sink without holding
//! the whole data unit in memory: the header goes out first, then samples are
//! converted to big-endian through a fixed-size buffer as they arrive, and
//! [`ImageWriter::finish`](crate::image_writer::ImageWriter::finish) checks the sample count against `NAXISn` and writes
//! the zero padding to the 2880-byte block boundary.
//!
//! The bytes are identical to those of [`crate::image::build_image_hdu`] (for
//! a primary HDU) or [`crate::image::build_image_hdu_with_scaling`] (via
//! [`ImageWriter::write_physical`](crate::image_writer::ImageWriter::write_physical))
//! for the same header and pixels.
//!
//! ```
//! use fitsio_pure::image_writer::ImageWriter;
//!
//! let mut out = Vec::new();
//! let mut writer = ImageWriter::primary(&mut out, -32, &[3, 2], &[]).unwrap();
//! writer.write_samples(&[0.0f32, 1.0, 2.0]).unwrap();
//! writer.write_samples(&[3.0f32, 4.0, 5.0]).unwrap();
//! writer.finish().unwrap();
//! assert_eq!(out.len(), 2 * 2880);
//! ```
//!
//! Several HDUs can be streamed into one file by handing the sink returned by
//! `finish` to [`ImageWriter::image_extension`](crate::image_writer::ImageWriter::image_extension).
//!
//! With the `std` feature, wrap an [`AtomicFile`](crate::io::AtomicFile) to
//! write a file that only replaces its target once it is complete:
//!
//! ```no_run
//! # #[cfg(feature = "std")]
//! # fn main() -> fitsio_pure::Result<()> {
//! use fitsio_pure::image_writer::ImageWriter;
//! use fitsio_pure::io::AtomicFile;
//!
//! let (width, height) = (6000, 4000);
//! let file = AtomicFile::new("stack.fits")?;
//! let mut writer = ImageWriter::primary(file, -32, &[width, height], &[])?;
//! for _row in 0..height {
//!     let row = vec![0.0f32; width];
//!     writer.write_samples(&row)?;
//! }
//! writer.finish()?.commit()?;
//! # Ok(())
//! # }
//! # #[cfg(not(feature = "std"))]
//! # fn main() {}
//! ```

use alloc::vec;
use alloc::vec::Vec;

use crate::block::{padded_byte_len, BLOCK_SIZE, DATA_PAD_BYTE};
use crate::error::{Error, Result};
use crate::extension::{build_extension_header, parse_extension_header, ExtensionType};
use crate::header::{serialize_header, Card};
use crate::image::{bytes_per_pixel, extract_bscale_bzero};
use crate::io::Write;
use crate::primary::{build_primary_header, parse_primary_header};

/// Default size of the big-endian conversion buffer: 1 MiB.
pub const DEFAULT_CHUNK_BYTES: usize = 1024 * 1024;

/// Smallest buffer size accepted: room for one 8-byte sample.
const MIN_CHUNK_BYTES: usize = 8;

const ZERO_PADDING: [u8; BLOCK_SIZE] = [DATA_PAD_BYTE; BLOCK_SIZE];

const TOO_MANY: Error = Error::InvalidHeader("more samples than NAXISn declares");

mod sealed {
    pub trait Sealed {}
}

/// A FITS image sample type: `u8`, `i16`, `i32`, `i64`, `f32` or `f64`.
///
/// Sealed; each type corresponds to exactly one `BITPIX` value.
pub trait Sample: sealed::Sealed + Copy {
    /// The `BITPIX` value this type is stored as.
    const BITPIX: i64;
    /// Size of one encoded sample in bytes.
    const SIZE: usize;
    /// Write the big-endian encoding of `self` into `dst` (`SIZE` bytes long).
    fn write_be(self, dst: &mut [u8]);
}

macro_rules! impl_sample {
    ($t:ty, $bitpix:expr) => {
        impl sealed::Sealed for $t {}
        impl Sample for $t {
            const BITPIX: i64 = $bitpix;
            const SIZE: usize = core::mem::size_of::<$t>();
            fn write_be(self, dst: &mut [u8]) {
                dst.copy_from_slice(&self.to_be_bytes());
            }
        }
    };
}

impl_sample!(u8, 8);
impl_sample!(i16, 16);
impl_sample!(i32, 32);
impl_sample!(i64, 64);
impl_sample!(f32, -32);
impl_sample!(f64, -64);

/// Streams one image HDU (header, samples, padding) to a [`Write`] sink.
///
/// The header is written by the constructor. Samples are then appended in
/// FITS order (`NAXIS1` fastest) with [`write_samples`](Self::write_samples),
/// [`write_iter`](Self::write_iter) or [`write_physical`](Self::write_physical),
/// in as many calls as convenient. Extra memory is one buffer of the chunk
/// size, fixed at construction.
///
/// Dropping the writer without calling [`finish`](Self::finish) discards any
/// buffered samples and leaves the HDU incomplete.
pub struct ImageWriter<W: Write> {
    inner: W,
    bitpix: i64,
    sample_size: usize,
    expected: u64,
    written: u64,
    bscale: f64,
    bzero: f64,
    buf: Vec<u8>,
    filled: usize,
}

impl<W: Write> ImageWriter<W> {
    /// Start an image HDU described by `cards` and write its header.
    ///
    /// `cards` must form a primary header (`SIMPLE` first) or an image
    /// extension header (`XTENSION = 'IMAGE'`), as built by
    /// [`build_primary_header`] or [`build_extension_header`], optionally
    /// followed by further cards. The `END` card and header padding are added.
    /// Uses a [`DEFAULT_CHUNK_BYTES`] buffer.
    pub fn new(inner: W, cards: &[Card]) -> Result<Self> {
        Self::with_chunk_size(inner, cards, DEFAULT_CHUNK_BYTES)
    }

    /// Like [`new`](Self::new), with a conversion buffer of `chunk_bytes`
    /// (at least 8). Samples reach `inner` in writes of at most this size.
    pub fn with_chunk_size(mut inner: W, cards: &[Card], chunk_bytes: usize) -> Result<Self> {
        let (bitpix, naxes) = image_shape(cards)?;
        let sample_size = bytes_per_pixel(bitpix)?;
        let expected = if naxes.is_empty() {
            0
        } else {
            naxes
                .iter()
                .try_fold(1u64, |acc, &n| acc.checked_mul(n as u64))
                .ok_or(Error::InvalidHeader("NAXISn product overflows"))?
        };
        expected
            .checked_mul(sample_size as u64)
            .and_then(|bytes| usize::try_from(bytes).ok())
            .ok_or(Error::InvalidHeader("image data size overflows"))?;
        let (bscale, bzero) = extract_bscale_bzero(cards);

        let header = serialize_header(cards)?;
        inner.write_all(&header)?;

        Ok(ImageWriter {
            inner,
            bitpix,
            sample_size,
            expected,
            written: 0,
            bscale,
            bzero,
            buf: vec![0u8; chunk_bytes.max(MIN_CHUNK_BYTES)],
            filled: 0,
        })
    }

    /// Start a primary image HDU with the cards of
    /// [`build_primary_header`]`(bitpix, naxes)` followed by `extra`.
    ///
    /// `naxes` is in FITS order (`NAXIS1` first). `extra` must not repeat
    /// mandatory keywords.
    pub fn primary(inner: W, bitpix: i64, naxes: &[usize], extra: &[Card]) -> Result<Self> {
        let mut cards = build_primary_header(bitpix, naxes)?;
        cards.extend_from_slice(extra);
        Self::new(inner, &cards)
    }

    /// Start an image extension HDU (`XTENSION = 'IMAGE'`) with the cards of
    /// [`build_extension_header`] followed by `extra`.
    ///
    /// Use this after a previous HDU has been written to `inner` (for
    /// example the sink returned by [`finish`](Self::finish)) to stream a
    /// multi-HDU file. `naxes` is in FITS order (`NAXIS1` first).
    pub fn image_extension(inner: W, bitpix: i64, naxes: &[usize], extra: &[Card]) -> Result<Self> {
        let mut cards = build_extension_header(ExtensionType::Image, bitpix, naxes, 0, 1)?;
        cards.extend_from_slice(extra);
        Self::new(inner, &cards)
    }

    /// The `BITPIX` of this HDU.
    pub fn bitpix(&self) -> i64 {
        self.bitpix
    }

    /// Number of samples written so far, including any still buffered.
    pub fn samples_written(&self) -> u64 {
        self.written
    }

    /// Number of samples the header declares (the product of `NAXISn`).
    pub fn samples_expected(&self) -> u64 {
        self.expected
    }

    /// Size in bytes of the conversion buffer, fixed at construction.
    pub fn buffer_capacity(&self) -> usize {
        self.buf.len()
    }

    /// Borrow the underlying sink.
    pub fn get_ref(&self) -> &W {
        &self.inner
    }

    /// Append samples. `T` must match the header's `BITPIX`.
    ///
    /// Writing more samples than the header declares is an error, and
    /// nothing from that call is written.
    pub fn write_samples<T: Sample>(&mut self, samples: &[T]) -> Result<()> {
        self.write_iter(samples.iter().copied())
    }

    /// Append samples from an iterator, such as one that de-interleaves RGB
    /// pixels into planes. `T` must match the header's `BITPIX`.
    ///
    /// Writing more samples than the header declares is an error, and
    /// nothing from that call is written.
    pub fn write_iter<T, I>(&mut self, samples: I) -> Result<()>
    where
        T: Sample,
        I: IntoIterator<Item = T>,
        I::IntoIter: ExactSizeIterator,
    {
        if T::BITPIX != self.bitpix {
            return Err(Error::InvalidHeader("sample type does not match BITPIX"));
        }
        let mut samples = samples.into_iter();
        let mut remaining = samples.len();
        if remaining as u64 > self.expected - self.written {
            return Err(TOO_MANY);
        }
        while remaining > 0 {
            let mut room = (self.buf.len() - self.filled) / T::SIZE;
            if room == 0 {
                self.drain()?;
                room = self.buf.len() / T::SIZE;
            }
            let end = self.filled + room.min(remaining) * T::SIZE;
            let mut taken = 0;
            for (dst, sample) in self.buf[self.filled..end]
                .chunks_exact_mut(T::SIZE)
                .zip(&mut samples)
            {
                sample.write_be(dst);
                taken += 1;
            }
            self.filled += taken * T::SIZE;
            self.written += taken as u64;
            remaining -= taken;
            if taken == 0 {
                break;
            }
        }
        if samples.next().is_some() {
            return Err(TOO_MANY);
        }
        Ok(())
    }

    /// Append physical values, storing `(value - BZERO) / BSCALE` at the
    /// header's `BITPIX` (rounded and clamped for integer types), exactly as
    /// [`crate::image::build_image_hdu_with_scaling`] does. `BSCALE` and
    /// `BZERO` are read from the header, defaulting to 1 and 0.
    pub fn write_physical(&mut self, values: &[f64]) -> Result<()> {
        let (bscale, bzero) = (self.bscale, self.bzero);
        let raw = move |v: f64| (v - bzero) / bscale;
        let int = move |v: f64, lo: f64, hi: f64| libm::round(raw(v)).clamp(lo, hi);
        let values = values.iter().copied();
        match self.bitpix {
            8 => self.write_iter(values.map(|v| int(v, 0.0, 255.0) as u8)),
            16 => self.write_iter(values.map(|v| int(v, i16::MIN as f64, i16::MAX as f64) as i16)),
            32 => self.write_iter(values.map(|v| int(v, i32::MIN as f64, i32::MAX as f64) as i32)),
            64 => self.write_iter(values.map(|v| int(v, i64::MIN as f64, i64::MAX as f64) as i64)),
            -32 => self.write_iter(values.map(|v| raw(v) as f32)),
            _ => self.write_iter(values.map(raw)),
        }
    }

    /// Write any buffered samples, check that the header's sample count was
    /// reached, write the zero padding to the block boundary, flush, and
    /// return the sink.
    pub fn finish(mut self) -> Result<W> {
        if self.written != self.expected {
            return Err(Error::InvalidHeader("fewer samples than NAXISn declares"));
        }
        self.drain()?;
        let data_bytes = self.expected as usize * self.sample_size;
        let padding = padded_byte_len(data_bytes) - data_bytes;
        self.inner.write_all(&ZERO_PADDING[..padding])?;
        self.inner.flush()?;
        Ok(self.inner)
    }

    fn drain(&mut self) -> Result<()> {
        if self.filled > 0 {
            self.inner.write_all(&self.buf[..self.filled])?;
            self.filled = 0;
        }
        Ok(())
    }
}

/// Read `BITPIX` and `NAXISn` from a primary or image-extension header.
fn image_shape(cards: &[Card]) -> Result<(i64, Vec<usize>)> {
    match cards.first().map(Card::keyword_str) {
        Some("SIMPLE") => {
            let header = parse_primary_header(cards)?;
            Ok((header.bitpix, header.naxes))
        }
        Some("XTENSION") => {
            let header = parse_extension_header(cards)?;
            if header.xtension != ExtensionType::Image {
                return Err(Error::UnsupportedExtension("not an IMAGE extension"));
            }
            if header.pcount != 0 || header.gcount != 1 {
                return Err(Error::InvalidHeader(
                    "image extension needs PCOUNT=0, GCOUNT=1",
                ));
            }
            Ok((header.bitpix, header.naxes))
        }
        _ => Err(Error::MissingKeyword("SIMPLE")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hdu::parse_fits;
    use crate::image::{
        build_image_hdu, build_image_hdu_with_scaling, read_image_data, serialize_image, ImageData,
    };
    use crate::value::Value;

    const SHAPES: [&[usize]; 4] = [&[7, 5], &[5, 7], &[5, 3, 4], &[2881]];
    const CHUNKS: [usize; 4] = [8, 13, 2880, DEFAULT_CHUNK_BYTES];

    fn card(keyword: &str, value: Value) -> Card {
        let mut kw = [b' '; 8];
        kw[..keyword.len()].copy_from_slice(keyword.as_bytes());
        Card {
            keyword: kw,
            value: Some(value),
            comment: None,
        }
    }

    fn sample_data(bitpix: i64, n: usize) -> ImageData {
        match bitpix {
            8 => ImageData::U8((0..n).map(|i| (i * 7) as u8).collect()),
            16 => ImageData::I16((0..n).map(|i| (i as i16).wrapping_mul(257)).collect()),
            32 => ImageData::I32((0..n).map(|i| (i as i32).wrapping_mul(65_537)).collect()),
            64 => ImageData::I64((0..n).map(|i| (i as i64) << 40 | i as i64).collect()),
            -32 => ImageData::F32((0..n).map(|i| i as f32 * -0.37 + 1e-3).collect()),
            _ => ImageData::F64((0..n).map(|i| i as f64 * 1.000_001e7).collect()),
        }
    }

    /// Write `data` in uneven pieces (1, 2, 3, ... samples).
    fn stream<W: Write>(writer: &mut ImageWriter<W>, data: &ImageData) {
        fn pieces<T: Sample>(writer: &mut ImageWriter<impl Write>, mut s: &[T]) {
            let mut step = 1;
            while !s.is_empty() {
                let n = step.min(s.len());
                writer.write_samples(&s[..n]).unwrap();
                s = &s[n..];
                step += 1;
            }
        }
        match data {
            ImageData::U8(v) => pieces(writer, v),
            ImageData::I16(v) => pieces(writer, v),
            ImageData::I32(v) => pieces(writer, v),
            ImageData::I64(v) => pieces(writer, v),
            ImageData::F32(v) => pieces(writer, v),
            ImageData::F64(v) => pieces(writer, v),
        }
    }

    #[test]
    fn primary_bytes_match_build_image_hdu() {
        for bitpix in [8, 16, 32, 64, -32, -64] {
            for naxes in SHAPES {
                let data = sample_data(bitpix, naxes.iter().product());
                let expected = build_image_hdu(bitpix, naxes, &data).unwrap();
                for chunk in CHUNKS {
                    let cards = build_primary_header(bitpix, naxes).unwrap();
                    let mut writer =
                        ImageWriter::with_chunk_size(Vec::new(), &cards, chunk).unwrap();
                    stream(&mut writer, &data);
                    let out = writer.finish().unwrap();
                    assert_eq!(out, expected, "BITPIX {bitpix}, {naxes:?}, chunk {chunk}");
                }
            }
        }
    }

    #[test]
    fn extension_bytes_match_built_extension() {
        for bitpix in [8, 16, 32, 64, -32, -64] {
            for naxes in SHAPES {
                let data = sample_data(bitpix, naxes.iter().product());
                let cards =
                    build_extension_header(ExtensionType::Image, bitpix, naxes, 0, 1).unwrap();
                let mut expected = serialize_header(&cards).unwrap();
                expected.extend_from_slice(&serialize_image(&data));

                let mut writer =
                    ImageWriter::image_extension(Vec::new(), bitpix, naxes, &[]).unwrap();
                stream(&mut writer, &data);
                assert_eq!(
                    writer.finish().unwrap(),
                    expected,
                    "BITPIX {bitpix}, {naxes:?}"
                );
            }
        }
    }

    #[test]
    fn physical_values_match_build_image_hdu_with_scaling() {
        let physical: Vec<f64> = (0..35)
            .map(|i| i as f64 * 913.25 - 7000.0)
            .chain([f64::NAN, 1e30, -1e30])
            .collect();
        let naxes = [19, 2];
        let (bscale, bzero) = (0.5, 32768.0);
        for bitpix in [8, 16, 32, 64, -32, -64] {
            let expected =
                build_image_hdu_with_scaling(bitpix, &naxes, &physical, bscale, bzero).unwrap();
            let extra = [
                card("BSCALE", Value::Float(bscale)),
                card("BZERO", Value::Float(bzero)),
            ];
            let mut writer = ImageWriter::primary(Vec::new(), bitpix, &naxes, &extra).unwrap();
            writer.write_physical(&physical[..10]).unwrap();
            writer.write_physical(&physical[10..]).unwrap();
            assert_eq!(writer.finish().unwrap(), expected, "BITPIX {bitpix}");
        }
    }

    #[test]
    fn multi_hdu_stream_parses() {
        let primary = sample_data(16, 12);
        let ext1 = sample_data(-32, 30);
        let ext2 = sample_data(8, 60);

        let mut w = ImageWriter::primary(Vec::new(), 16, &[4, 3], &[]).unwrap();
        stream(&mut w, &primary);
        let out = w.finish().unwrap();
        let mut w = ImageWriter::image_extension(out, -32, &[5, 6], &[]).unwrap();
        stream(&mut w, &ext1);
        let out = w.finish().unwrap();
        let extname = card("EXTNAME", Value::String("MASK".into()));
        let mut w = ImageWriter::image_extension(out, 8, &[3, 4, 5], &[extname]).unwrap();
        stream(&mut w, &ext2);
        let out = w.finish().unwrap();

        let fits = parse_fits(&out).unwrap();
        assert_eq!(fits.hdus.len(), 3);
        for (hdu, data) in fits.hdus.iter().zip([&primary, &ext1, &ext2]) {
            assert_eq!(&read_image_data(&out, hdu).unwrap(), data);
        }
    }

    #[test]
    fn caller_cards_serialize_like_serialize_header() {
        let extra = [
            card("FILTER", Value::Undefined),
            card("OBJECT", Value::String("M 31".into())),
            card("EXPTIME", Value::Float(30.0)),
        ];
        let mut cards = build_primary_header(16, &[2, 2]).unwrap();
        cards.extend_from_slice(&extra);
        let mut expected = serialize_header(&cards).unwrap();
        expected.extend_from_slice(&serialize_image(&ImageData::I16(vec![1, 2, 3, 4])));

        let mut w = ImageWriter::primary(Vec::new(), 16, &[2, 2], &extra).unwrap();
        w.write_samples(&[1i16, 2, 3, 4]).unwrap();
        assert_eq!(w.finish().unwrap(), expected);
    }

    #[test]
    fn empty_image_writes_header_only() {
        let w = ImageWriter::primary(Vec::new(), 8, &[], &[]).unwrap();
        assert_eq!(w.samples_expected(), 0);
        let out = w.finish().unwrap();
        assert_eq!(
            out,
            serialize_header(&build_primary_header(8, &[]).unwrap()).unwrap()
        );

        let w = ImageWriter::primary(Vec::new(), 16, &[0, 5], &[]).unwrap();
        assert_eq!(w.samples_expected(), 0);
        assert_eq!(w.finish().unwrap().len(), BLOCK_SIZE);
    }

    #[test]
    fn too_many_samples_is_an_error_and_writes_nothing() {
        let mut w = ImageWriter::primary(Vec::new(), -32, &[2, 2], &[]).unwrap();
        w.write_samples(&[1.0f32, 2.0, 3.0]).unwrap();
        assert!(w.write_samples(&[4.0f32, 5.0]).is_err());
        assert_eq!(w.samples_written(), 3);
        w.write_samples(&[4.0f32]).unwrap();
        assert!(w.write_samples(&[5.0f32]).is_err());
        let out = w.finish().unwrap();
        let data = ImageData::F32(vec![1.0, 2.0, 3.0, 4.0]);
        assert_eq!(out, build_image_hdu(-32, &[2, 2], &data).unwrap());
    }

    #[test]
    fn too_few_samples_is_an_error() {
        let mut w = ImageWriter::primary(Vec::new(), 16, &[3, 3], &[]).unwrap();
        w.write_samples(&[1i16; 8]).unwrap();
        assert!(matches!(w.finish(), Err(Error::InvalidHeader(_))));
    }

    #[test]
    fn sample_type_must_match_bitpix() {
        let mut w = ImageWriter::primary(Vec::new(), 16, &[3], &[]).unwrap();
        assert!(w.write_samples(&[1i32, 2, 3]).is_err());
        assert!(w.write_samples(&[1.0f32, 2.0, 3.0]).is_err());
        assert_eq!(w.samples_written(), 0);
        w.write_samples(&[1i16, 2, 3]).unwrap();
        w.finish().unwrap();
    }

    #[test]
    fn write_iter_deinterleaves_rgb() {
        let (width, height) = (3usize, 2usize);
        let interleaved: Vec<f32> = (0..width * height * 3).map(|i| i as f32).collect();
        let mut w = ImageWriter::primary(Vec::new(), -32, &[width, height, 3], &[]).unwrap();
        for channel in 0..3 {
            w.write_iter((0..width * height).map(|p| interleaved[p * 3 + channel]))
                .unwrap();
        }
        let out = w.finish().unwrap();
        let planar: Vec<f32> = (0..3)
            .flat_map(|c| (0..width * height).map(move |p| (p * 3 + c) as f32))
            .collect();
        let expected = build_image_hdu(-32, &[width, height, 3], &ImageData::F32(planar));
        assert_eq!(out, expected.unwrap());
    }

    #[test]
    fn non_image_headers_are_rejected() {
        let mut table =
            build_extension_header(ExtensionType::BinaryTable, 8, &[8, 10], 0, 1).unwrap();
        table.push(card("TFIELDS", Value::Integer(0)));
        assert!(matches!(
            ImageWriter::new(Vec::new(), &table),
            Err(Error::UnsupportedExtension(_))
        ));
        let grouped = build_extension_header(ExtensionType::Image, 16, &[4], 2, 1).unwrap();
        assert!(ImageWriter::new(Vec::new(), &grouped).is_err());
        assert!(ImageWriter::new(Vec::new(), &[]).is_err());
        assert!(ImageWriter::primary(Vec::new(), 12, &[4], &[]).is_err());
    }

    /// Counts bytes and the largest single write, keeping none of them.
    #[derive(Default)]
    struct CountingSink {
        total: u64,
        largest_write: usize,
        writes: u64,
    }

    impl Write for CountingSink {
        fn write(&mut self, buf: &[u8]) -> crate::io::Result<usize> {
            self.total += buf.len() as u64;
            self.largest_write = self.largest_write.max(buf.len());
            self.writes += 1;
            Ok(buf.len())
        }

        fn flush(&mut self) -> crate::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn hundred_megapixel_image_streams_in_bounded_memory() {
        let (width, height) = (10_000usize, 10_000usize);
        let mut w =
            ImageWriter::primary(CountingSink::default(), -32, &[width, height], &[]).unwrap();
        let capacity = w.buffer_capacity();
        assert_eq!(capacity, DEFAULT_CHUNK_BYTES);

        let mut row = vec![0.0f32; width];
        for y in 0..height {
            row.fill(y as f32);
            w.write_samples(&row).unwrap();
            assert_eq!(w.buffer_capacity(), capacity);
        }
        let sink = w.finish().unwrap();

        let data_bytes = (width * height * 4) as u64;
        assert_eq!(
            sink.total,
            2880 + padded_byte_len(data_bytes as usize) as u64
        );
        assert!(sink.largest_write <= capacity);
        // 400 MB of samples reach the sink in about 400 one-MiB writes.
        assert!(sink.writes <= data_bytes / capacity as u64 + 3);
    }
}
