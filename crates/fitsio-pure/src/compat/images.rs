use super::errors::{Error, Result};
use super::fitsfile::FitsFile;
use super::hdu::FitsHdu;
use super::sys;

/// Describes the shape and type of an image HDU.
///
/// Borrows its dimensions, as `fitsio`'s does, so upstream code such as
/// `ImageDescription { data_type: ImageType::Float, dimensions: &[100, 200] }`
/// or `dimensions: &dims` compiles unchanged.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageDescription<'a> {
    pub data_type: ImageType,
    /// Axis lengths in row-major order, slowest axis first, as in `fitsio`:
    /// `[rows, columns]` for a 2-D image. FITS stores them reversed, so the
    /// last entry becomes `NAXIS1`.
    pub dimensions: &'a [usize],
}

/// The pixel data type for an image HDU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageType {
    UnsignedByte,
    /// Signed 8-bit (`i8`): stored as `BITPIX = 8` with `BZERO = -128`.
    Byte,
    Short,
    /// Unsigned 16-bit (`u16`): stored as `BITPIX = 16` with `BZERO = 32768`.
    UnsignedShort,
    Long,
    /// Unsigned 32-bit (`u32`): stored as `BITPIX = 32` with `BZERO = 2^31`.
    UnsignedLong,
    LongLong,
    /// Unsigned 64-bit (`u64`): stored as `BITPIX = 64` with `BZERO = 2^63`.
    UnsignedLongLong,
    Float,
    Double,
}

impl ImageType {
    /// Convert to the FITS BITPIX value.
    ///
    /// Unsigned types map to their signed storage BITPIX; the unsigned
    /// interpretation is carried by the `BZERO` keyword (cfitsio convention).
    pub fn to_bitpix(self) -> i64 {
        match self {
            ImageType::UnsignedByte | ImageType::Byte => 8,
            ImageType::Short | ImageType::UnsignedShort => 16,
            ImageType::Long | ImageType::UnsignedLong => 32,
            ImageType::LongLong | ImageType::UnsignedLongLong => 64,
            ImageType::Float => -32,
            ImageType::Double => -64,
        }
    }

    /// The `BZERO` offset for the cfitsio offset storage convention (`i8` and
    /// the unsigned integers), or `None` for the natively stored types.
    ///
    /// `i8`/`u16`/`u32` use an integer `BZERO`; `u64` uses `2^63`, which exceeds
    /// `i64::MAX`, so it is stored as a (exactly representable) float.
    pub(crate) fn unsigned_bzero(self) -> Option<crate::value::Value> {
        use crate::value::Value;
        match self {
            ImageType::Byte => Some(Value::Integer(-128)),
            ImageType::UnsignedShort => Some(Value::Integer(32_768)),
            ImageType::UnsignedLong => Some(Value::Integer(2_147_483_648)),
            ImageType::UnsignedLongLong => Some(Value::Float(9_223_372_036_854_775_808.0)),
            _ => None,
        }
    }

    /// Convert from FITS BITPIX value.
    pub fn from_bitpix(bitpix: i64) -> Result<Self> {
        match bitpix {
            8 => Ok(ImageType::UnsignedByte),
            16 => Ok(ImageType::Short),
            32 => Ok(ImageType::Long),
            64 => Ok(ImageType::LongLong),
            -32 => Ok(ImageType::Float),
            -64 => Ok(ImageType::Double),
            _ => Err(Error::Message(format!("unsupported BITPIX: {bitpix}"))),
        }
    }

    /// The type of the physical values once `BSCALE`/`BZERO` are applied, as
    /// cfitsio's `fits_get_img_equivtype` reports it.
    ///
    /// A `BITPIX = 16` camera frame with `BZERO = 32768` is `UnsignedShort`, not
    /// `Short`. Integer scaling picks the narrowest type holding the physical
    /// range; non-integer scaling gives `Float` (8/16-bit storage) or `Double`.
    pub fn equivalent(bitpix: i64, bscale: f64, bzero: f64) -> Result<Self> {
        let stored = Self::from_bitpix(bitpix)?;
        if bscale == 1.0 && bzero == 0.0 {
            return Ok(stored);
        }
        let (lo, hi) = match stored {
            ImageType::UnsignedByte => (0.0, 255.0),
            ImageType::Short => (-32_768.0, 32_767.0),
            ImageType::Long => (-2_147_483_648.0, 2_147_483_647.0),
            _ => return Ok(stored),
        };
        let (min, max) = if bscale >= 0.0 {
            (bzero + bscale * lo, bzero + bscale * hi)
        } else {
            (bzero + bscale * hi, bzero + bscale * lo)
        };
        let int_zero = if bzero < 2_147_483_648.0 {
            bzero.trunc()
        } else {
            0.0
        };
        let integer_scaling =
            bzero == 2_147_483_648.0 || (int_zero == bzero && bscale.trunc() == bscale);

        Ok(if !integer_scaling {
            match stored {
                ImageType::UnsignedByte | ImageType::Short => ImageType::Float,
                _ => ImageType::Double,
            }
        } else if min == -128.0 && max == 127.0 {
            ImageType::Byte
        } else if min >= -32_768.0 && max <= 32_767.0 {
            ImageType::Short
        } else if min >= 0.0 && max <= 65_535.0 {
            ImageType::UnsignedShort
        } else if min >= -2_147_483_648.0 && max <= 2_147_483_647.0 {
            ImageType::Long
        } else if min >= 0.0 && max < 4_294_967_296.0 {
            ImageType::UnsignedLong
        } else {
            ImageType::Double
        })
    }
}

fn validate_hdu_index(file: &FitsFile, hdu: &FitsHdu) -> Result<usize> {
    let fits_data = file.parsed()?;
    if hdu.number >= fits_data.len() {
        return Err(Error::status(sys::END_OF_FILE));
    }
    Ok(hdu.number)
}

/// Trait for types that can read image pixel data from a FITS file.
pub trait ReadImage: Sized {
    fn read_image(file: &FitsFile, hdu: &FitsHdu) -> Result<Vec<Self>>;
    fn read_section(
        file: &FitsFile,
        hdu: &FitsHdu,
        range: std::ops::Range<usize>,
    ) -> Result<Vec<Self>>;
    fn read_rows(
        file: &FitsFile,
        hdu: &FitsHdu,
        start_row: usize,
        num_rows: usize,
    ) -> Result<Vec<Self>>;
    fn read_region(
        file: &FitsFile,
        hdu: &FitsHdu,
        ranges: &[std::ops::Range<usize>],
    ) -> Result<Vec<Self>>;
}

/// The output of an image read through [`FitsHdu`]'s methods: `Vec<T>` or,
/// with the `array` feature, `ndarray::ArrayD<T>`, as in `fitsio`. This is
/// what lets `let pixels: Vec<f32> = hdu.read_image(&mut f)?` pick its type
/// from the binding.
///
/// Its methods are named apart from [`ReadImage`]'s so the two traits can be
/// in scope together without making `ArrayD::<f32>::read_image(…)` ambiguous.
pub trait ReadsImage: Sized {
    fn image(file: &FitsFile, hdu: &FitsHdu) -> Result<Self>;
    fn section(file: &FitsFile, hdu: &FitsHdu, range: std::ops::Range<usize>) -> Result<Self>;
    fn rows(file: &FitsFile, hdu: &FitsHdu, start_row: usize, num_rows: usize) -> Result<Self>;
    fn region(file: &FitsFile, hdu: &FitsHdu, ranges: &[std::ops::Range<usize>]) -> Result<Self>;
}

impl<T: ReadImage> ReadsImage for Vec<T> {
    fn image(file: &FitsFile, hdu: &FitsHdu) -> Result<Self> {
        T::read_image(file, hdu)
    }

    fn section(file: &FitsFile, hdu: &FitsHdu, range: std::ops::Range<usize>) -> Result<Self> {
        T::read_section(file, hdu, range)
    }

    fn rows(file: &FitsFile, hdu: &FitsHdu, start_row: usize, num_rows: usize) -> Result<Self> {
        T::read_rows(file, hdu, start_row, num_rows)
    }

    fn region(file: &FitsFile, hdu: &FitsHdu, ranges: &[std::ops::Range<usize>]) -> Result<Self> {
        T::read_region(file, hdu, ranges)
    }
}

/// Image reads into a caller-owned buffer, as cfitsio's `ffgpv` reads them.
///
/// Values are physical, with `BSCALE`/`BZERO` applied, exactly as
/// [`ReadImage::read_image`] returns them. An uncompressed image is read from
/// the file a block at a time straight into the buffer, without allocating;
/// a tile-compressed one is decompressed from its own data unit.
pub trait ReadImageIntoBuffer: Sized {
    /// Fill `buf` with the image's first `buf.len()` pixels, in the flat,
    /// `NAXIS1`-fastest order. As with `ffgpv`, the buffer may be shorter than
    /// the image; a longer one is an error (`BAD_ROW_NUM`).
    fn read_image_into_buffer(file: &FitsFile, hdu: &FitsHdu, buf: &mut [Self]) -> Result<()>;

    /// Fill `buf` with `buf.len()` pixels from pixel `first` (zero-based) on,
    /// as `ffgpv` with `firstelem = first + 1`. A `first` past the last pixel
    /// is `BAD_ELEM_NUM`, and a read running past the end `BAD_ROW_NUM`.
    fn read_section_into_buffer(
        file: &FitsFile,
        hdu: &FitsHdu,
        first: usize,
        buf: &mut [Self],
    ) -> Result<()>
    where
        Self: ReadImage + Copy,
    {
        let pixels = Self::read_section(file, hdu, first..first + buf.len())?;
        buf.copy_from_slice(&pixels);
        Ok(())
    }
}

macro_rules! impl_read_image_into_buffer {
    ($($t:ty),*) => {$(
        impl ReadImageIntoBuffer for $t {
            fn read_image_into_buffer(
                file: &FitsFile,
                hdu: &FitsHdu,
                buf: &mut [Self],
            ) -> Result<()> {
                read_into(file, hdu, 0, buf)
            }

            fn read_section_into_buffer(
                file: &FitsFile,
                hdu: &FitsHdu,
                first: usize,
                buf: &mut [Self],
            ) -> Result<()> {
                read_into(file, hdu, first, buf)
            }
        }
    )*};
}

impl_read_image_into_buffer!(u8, i8, i16, u16, i32, u32, i64, u64, f32, f64);

/// Fill `buf` with the physical values of `buf.len()` pixels from pixel
/// `first` on, as `ffgpv` does.
fn read_into<T: FromPhysical>(
    file: &FitsFile,
    hdu: &FitsHdu,
    first: usize,
    buf: &mut [T],
) -> Result<()> {
    if buf.is_empty() {
        return Ok(());
    }
    let idx = validate_hdu_index(file, hdu)?;
    let core = &file.parsed()?.hdus[idx];
    let naxes = match &core.info {
        crate::hdu::HduInfo::Primary { naxes, .. } | crate::hdu::HduInfo::Image { naxes, .. } => {
            naxes
        }
        crate::hdu::HduInfo::CompressedImage { znaxes, .. } => znaxes,
        _ => return Err(Error::status(sys::NOT_IMAGE)),
    };
    let pixels = pixel_count(naxes);
    if first >= pixels {
        return Err(Error::status(sys::BAD_ELEM_NUM));
    }
    if buf.len() > pixels - first {
        return Err(Error::status(sys::BAD_ROW_NUM));
    }
    let (bscale, bzero) = crate::image::extract_bscale_bzero(&core.cards);
    let scale = Scale::new(bscale, bzero);
    match &core.info {
        crate::hdu::HduInfo::Primary { bitpix, .. } | crate::hdu::HduInfo::Image { bitpix, .. } => {
            let bytes_per_pixel = crate::image::bytes_per_pixel(*bitpix)?;
            let mut raw = [0u8; 16 * 1024];
            let block = raw.len() / bytes_per_pixel;
            for (i, out) in buf.chunks_mut(block).enumerate() {
                let raw = &mut raw[..out.len() * bytes_per_pixel];
                let pixel = first + i * block;
                file.read_at(core.data_start + pixel * bytes_per_pixel, raw)?;
                scale.apply_raw(raw, *bitpix, out)?;
            }
            Ok(())
        }
        _ => {
            let (bytes, local) = file.hdu_data(core)?;
            let data = crate::image::read_image_data(&bytes, &local)?;
            scale.apply_data(&data, first, buf)
        }
    }
}

/// Trait for types that can write image pixel data to a FITS file.
///
/// As in cfitsio, the values are physical: they are converted to the image's
/// `BITPIX`, undoing its `BSCALE`/`BZERO`, so an `f64` slice can be written to
/// a `Float` image or a `u16` slice to a `BZERO = 32768` one. A value that
/// doesn't fit the stored type is an error, and the values before it have
/// been written. Pixels are written in place, and no write can change the
/// image's size.
pub trait WriteImage: Sized {
    /// Write `data` from the first pixel on. More values than the image has
    /// pixels is an error; fewer leave the rest of the image unchanged.
    fn write_image(file: &mut FitsFile, hdu: &FitsHdu, data: &[Self]) -> Result<()>;

    /// Write `data` to the pixels `range` covers in the flat, `NAXIS1`-fastest
    /// pixel order. As in `fitsio`, only the first `range.len()` values are
    /// written; fewer than that is an error.
    fn write_section(
        file: &mut FitsFile,
        hdu: &FitsHdu,
        range: std::ops::Range<usize>,
        data: &[Self],
    ) -> Result<()>;

    /// Write `data` to a rectangular region. `ranges` has one range per axis,
    /// `NAXIS1` first, as `read_region` takes them; `data` is `NAXIS1`-fastest.
    /// Trailing `0..1` ranges for axes the image doesn't have are ignored.
    /// As in `fitsio`, values past the region's size are ignored; fewer than
    /// it holds is an error.
    fn write_region(
        file: &mut FitsFile,
        hdu: &FitsHdu,
        ranges: &[std::ops::Range<usize>],
        data: &[Self],
    ) -> Result<()>;
}

/// Region ranges for an image with `naxis` axes. cfitsio reads only the
/// first `NAXIS` ranges, so `fitsio` callers can pass ranges for axes the
/// image doesn't have, such as a `0..1` per degenerate Stokes and frequency
/// axis on a 2-D radio image. Those are accepted when they are `0..1`, the
/// only index of an absent axis; any other trailing range stays an error.
fn trim_absent_axes(naxis: usize, ranges: &[std::ops::Range<usize>]) -> &[std::ops::Range<usize>] {
    if ranges.len() > naxis && ranges[naxis..].iter().all(|r| *r == (0..1)) {
        &ranges[..naxis]
    } else {
        ranges
    }
}

/// The image's axis lengths, `NAXIS1` first, or `None` for a table.
fn image_naxes(hdu: &FitsHdu) -> Option<Vec<usize>> {
    match &hdu.info {
        crate::compat::hdu::HduInfo::ImageInfo { shape, .. } => {
            Some(shape.iter().rev().copied().collect())
        }
        _ => None,
    }
}

/// Read pixels and apply the HDU's `BSCALE`/`BZERO`.
///
/// `runs` gives the pixels to read, each `(first pixel, count)` in the flat
/// `NAXIS1`-fastest order, from the image's axis lengths (`NAXIS1` first).
/// For an uncompressed image only those pixels are read from the file. A
/// tile-compressed image is decompressed from its own data unit, then the
/// runs are taken from it, so they address the image, not its table.
fn read_physical<T, R>(file: &FitsFile, hdu: &FitsHdu, runs: R) -> Result<Vec<T>>
where
    T: FromPhysical,
    R: FnOnce(&[usize]) -> Result<Vec<(usize, usize)>>,
{
    use crate::hdu::HduInfo;
    let idx = validate_hdu_index(file, hdu)?;
    let parsed = file.parsed()?;
    let core_hdu = &parsed.hdus[idx];
    let naxes = match &core_hdu.info {
        HduInfo::Primary { naxes, .. } | HduInfo::Image { naxes, .. } => naxes,
        HduInfo::CompressedImage { znaxes, .. } => znaxes,
        _ => return Err(Error::status(sys::NOT_IMAGE)),
    };
    let runs = runs(naxes)?;
    let pixels = pixel_count(naxes);
    if runs.iter().any(|&(first, count)| first + count > pixels) {
        return Err(Error::status(sys::BAD_ROW_NUM));
    }
    let (bscale, bzero) = crate::image::extract_bscale_bzero(&core_hdu.cards);
    let scale = Scale::new(bscale, bzero);
    let mut out = vec![T::default(); runs.iter().map(|&(_, count)| count).sum()];
    match &core_hdu.info {
        HduInfo::Primary { bitpix, .. } | HduInfo::Image { bitpix, .. } => {
            let data = read_runs(file, core_hdu.data_start, *bitpix, &runs)?;
            scale.apply_data(&data, 0, &mut out)?;
        }
        _ => {
            let (bytes, local) = file.hdu_data(core_hdu)?;
            let data = crate::tiled::read_tiled_image(&bytes, &local)?;
            let mut rest = &mut out[..];
            for &(first, count) in &runs {
                let (run, tail) = rest.split_at_mut(count);
                scale.apply_data(&data, first, run)?;
                rest = tail;
            }
        }
    }
    Ok(out)
}

/// A stored pixel type.
trait Stored: Copy {
    /// The value as an integer, or `None` for a float.
    fn exact(self) -> Option<i128>;
    fn float(self) -> f64;
}

macro_rules! impl_stored {
    (int: $($i:ty),*; float: $($f:ty),*) => {
        $(impl Stored for $i {
            fn exact(self) -> Option<i128> {
                Some(i128::from(self))
            }

            fn float(self) -> f64 {
                self as f64
            }
        })*
        $(impl Stored for $f {
            fn exact(self) -> Option<i128> {
                None
            }

            fn float(self) -> f64 {
                f64::from(self)
            }
        })*
    };
}

impl_stored!(int: u8, i16, i32, i64; float: f32, f64);

/// An image's `BSCALE`/`BZERO`, applied to stored pixels as cfitsio applies
/// them.
#[derive(Clone, Copy)]
struct Scale {
    bscale: f64,
    bzero: f64,
    /// `BZERO` when integers stay exact: unit scale and an integer offset,
    /// which covers the `u64` convention (`BZERO = 2^63`). Anything else is
    /// scaled in `f64`.
    exact_offset: Option<i128>,
}

impl Scale {
    fn new(bscale: f64, bzero: f64) -> Self {
        let exact = bscale == 1.0 && bzero.fract() == 0.0 && bzero.abs() < 1e20;
        Scale {
            bscale,
            bzero,
            exact_offset: exact.then_some(bzero as i128),
        }
    }

    /// Write the physical value of each stored pixel to `out`, stopping at
    /// whichever runs out first. A value that doesn't fit `T` is cfitsio's
    /// `NUM_OVERFLOW`; integer types truncate toward zero, as cfitsio does.
    fn apply<S: Stored, T: FromPhysical>(
        self,
        stored: impl IntoIterator<Item = S>,
        out: &mut [T],
    ) -> Result<()> {
        for (out, s) in out.iter_mut().zip(stored) {
            let value = match (s.exact(), self.exact_offset) {
                (Some(x), Some(offset)) => T::from_exact(x + offset),
                _ => T::from_scaled(s.float() * self.bscale + self.bzero),
            };
            *out = value.ok_or_else(|| Error::status(sys::NUM_OVERFLOW))?;
        }
        Ok(())
    }

    /// [`Scale::apply`] to big-endian stored pixels of type `bitpix`.
    fn apply_raw<T: FromPhysical>(self, raw: &[u8], bitpix: i64, out: &mut [T]) -> Result<()> {
        macro_rules! decode {
            ($t:ty) => {
                self.apply(
                    raw.chunks_exact(core::mem::size_of::<$t>())
                        .map(|b| <$t>::from_be_bytes(b.try_into().expect("chunk is one pixel"))),
                    out,
                )
            };
        }
        match bitpix {
            8 => decode!(u8),
            16 => decode!(i16),
            32 => decode!(i32),
            64 => decode!(i64),
            -32 => decode!(f32),
            -64 => decode!(f64),
            other => Err(Error::Message(format!("unsupported BITPIX: {other}"))),
        }
    }

    /// [`Scale::apply`] to decoded pixels from index `first` on.
    fn apply_data<T: FromPhysical>(
        self,
        data: &crate::image::ImageData,
        first: usize,
        out: &mut [T],
    ) -> Result<()> {
        use crate::image::ImageData;
        match data {
            ImageData::U8(v) => self.apply(v[first..].iter().copied(), out),
            ImageData::I16(v) => self.apply(v[first..].iter().copied(), out),
            ImageData::I32(v) => self.apply(v[first..].iter().copied(), out),
            ImageData::I64(v) => self.apply(v[first..].iter().copied(), out),
            ImageData::F32(v) => self.apply(v[first..].iter().copied(), out),
            ImageData::F64(v) => self.apply(v[first..].iter().copied(), out),
        }
    }
}

/// Read the stored pixels in `runs`, each `(first pixel, count)` in the flat
/// `NAXIS1`-fastest order, of an uncompressed image whose data unit starts at
/// `data_start`. Only those bytes are read from the file.
fn read_runs(
    file: &FitsFile,
    data_start: usize,
    bitpix: i64,
    runs: &[(usize, usize)],
) -> Result<crate::image::ImageData> {
    let bytes_per_pixel = crate::image::bytes_per_pixel(bitpix)?;
    let total: usize = runs.iter().map(|&(_, count)| count).sum();
    let mut raw = vec![0u8; total * bytes_per_pixel];
    let mut out = &mut raw[..];
    let mut runs = runs.iter().copied().peekable();
    while let Some((first, mut count)) = runs.next() {
        // Read runs that follow on from each other in one go.
        while let Some(&(next, more)) = runs.peek() {
            if next != first + count {
                break;
            }
            count += more;
            runs.next();
        }
        let (run, rest) = out.split_at_mut(count * bytes_per_pixel);
        file.read_at(data_start + first * bytes_per_pixel, run)?;
        out = rest;
    }
    Ok(crate::image::decode_pixels(&raw, bitpix)?)
}

/// The number of pixels in an image with axis lengths `naxes`.
fn pixel_count(naxes: &[usize]) -> usize {
    match naxes {
        [] => 0,
        naxes => naxes.iter().product(),
    }
}

/// Conversion from a physical pixel value to the requested element type.
/// `None` means the value does not fit, where cfitsio reports `NUM_OVERFLOW`.
trait FromPhysical: Copy + Default {
    fn from_exact(v: i128) -> Option<Self>;
    fn from_scaled(v: f64) -> Option<Self>;
}

macro_rules! impl_from_physical_int {
    ($($t:ty),*) => {$(
        impl FromPhysical for $t {
            fn from_exact(v: i128) -> Option<Self> {
                <$t>::try_from(v).ok()
            }

            fn from_scaled(v: f64) -> Option<Self> {
                (v >= <$t>::MIN as f64 && v <= <$t>::MAX as f64).then(|| v as $t)
            }
        }
    )*};
}

impl_from_physical_int!(u8, i8, i16, u16, i32, u32, i64, u64);

macro_rules! impl_from_physical_float {
    ($($t:ty),*) => {$(
        impl FromPhysical for $t {
            fn from_exact(v: i128) -> Option<Self> {
                Some(v as $t)
            }

            fn from_scaled(v: f64) -> Option<Self> {
                Some(v as $t)
            }
        }
    )*};
}

impl_from_physical_float!(f32, f64);

/// Implement `ReadImage` for a type: every read returns physical values
/// (`BSCALE`/`BZERO` applied) and fails where a value does not fit, matching
/// cfitsio rather than wrapping or returning stored values.
macro_rules! impl_read_image {
    ($($t:ty),*) => {$(
        impl ReadImage for $t {
            fn read_image(file: &FitsFile, hdu: &FitsHdu) -> Result<Vec<Self>> {
                read_physical(
                    file,
                    hdu,
                    |naxes| Ok(vec![(0, pixel_count(naxes))]),
                )
            }

            fn read_section(
                file: &FitsFile,
                hdu: &FitsHdu,
                range: std::ops::Range<usize>,
            ) -> Result<Vec<Self>> {
                if let Some(naxes) = image_naxes(hdu) {
                    if range.end > naxes.iter().product() {
                        return Err(Error::status(sys::BAD_ROW_NUM));
                    }
                }
                let count = range.end.saturating_sub(range.start);
                read_physical(
                    file,
                    hdu,
                    |_| Ok(vec![(range.start, count)]),
                )
            }

            fn read_rows(
                file: &FitsFile,
                hdu: &FitsHdu,
                start_row: usize,
                num_rows: usize,
            ) -> Result<Vec<Self>> {
                if let Some(naxes) = image_naxes(hdu) {
                    let rows: usize = naxes.iter().skip(1).product();
                    if naxes.len() >= 2 && start_row + num_rows > rows {
                        return Err(Error::status(sys::BAD_ROW_NUM));
                    }
                }
                read_physical(
                    file,
                    hdu,
                    |naxes| {
                        if naxes.len() < 2 {
                            return Err(Error::Message(
                                "image needs at least 2 axes for row slicing".to_string(),
                            ));
                        }
                        Ok(vec![(start_row * naxes[0], num_rows * naxes[0])])
                    },
                )
            }

            fn read_region(
                file: &FitsFile,
                hdu: &FitsHdu,
                ranges: &[std::ops::Range<usize>],
            ) -> Result<Vec<Self>> {
                let ranges = match &hdu.info {
                    crate::compat::hdu::HduInfo::ImageInfo { shape, .. } => {
                        trim_absent_axes(shape.len(), ranges)
                    }
                    _ => ranges,
                };
                if let Some(naxes) = image_naxes(hdu) {
                    if ranges.len() == naxes.len()
                        && ranges.iter().zip(&naxes).any(|(r, &n)| r.end > n)
                    {
                        return Err(Error::status(sys::BAD_ROW_NUM));
                    }
                }
                read_physical(
                    file,
                    hdu,
                    |naxes| region_runs(naxes, ranges),
                )
            }
        }
    )*};
}

impl_read_image!(u8, i8, i16, u16, i32, u32, i64, u64, f32, f64);

/// A pixel value to write: exact for integers, `f64` for floats.
#[derive(Clone, Copy)]
enum PhysicalValue {
    Exact(i128),
    Scaled(f64),
}

/// Conversion of an element type to the physical value it writes.
trait ToPhysical: Copy {
    fn to_physical(self) -> PhysicalValue;
}

macro_rules! impl_to_physical {
    (exact: $($i:ty),*; scaled: $($f:ty),*) => {
        $(impl ToPhysical for $i {
            fn to_physical(self) -> PhysicalValue {
                PhysicalValue::Exact(i128::from(self))
            }
        })*
        $(impl ToPhysical for $f {
            fn to_physical(self) -> PhysicalValue {
                PhysicalValue::Scaled(f64::from(self))
            }
        })*
    };
}

impl_to_physical!(exact: u8, i8, i16, u16, i32, u32, i64, u64; scaled: f32, f64);

/// An image HDU's storage: where its data unit is and how pixels are stored.
struct ImageStorage {
    data_start: usize,
    pixel_count: usize,
    bitpix: i64,
    bscale: f64,
    bzero: f64,
}

fn image_storage(file: &FitsFile, hdu: &FitsHdu) -> Result<ImageStorage> {
    let idx = validate_hdu_index(file, hdu)?;
    let parsed = file.parsed()?;
    let core = &parsed.hdus[idx];
    let bitpix = match &core.info {
        crate::hdu::HduInfo::Primary { bitpix, .. } | crate::hdu::HduInfo::Image { bitpix, .. } => {
            *bitpix
        }
        _ => {
            return Err(Error::Message(format!(
                "HDU {idx} is not an uncompressed image"
            )))
        }
    };
    let bytes_per_pixel = crate::image::bytes_per_pixel(bitpix)?;
    if core.data_start + core.data_len > file.data()?.len() {
        return Err(Error::Message(format!(
            "HDU {idx} data unit runs past the end of the file"
        )));
    }
    let (bscale, bzero) = crate::image::extract_bscale_bzero(&core.cards);
    Ok(ImageStorage {
        data_start: core.data_start,
        pixel_count: core.data_len / bytes_per_pixel,
        bitpix,
        bscale,
        bzero,
    })
}

/// Encode physical values as the image stores them into `out`, which holds
/// exactly one stored pixel per value: undo `BZERO`/`BSCALE` and narrow to
/// `BITPIX`, rounding to the nearest integer for integer images as cfitsio
/// does. A value that doesn't fit is an error (cfitsio's `NUM_OVERFLOW`); the
/// values before it are already encoded.
fn encode_into<T: ToPhysical>(data: &[T], storage: &ImageStorage, out: &mut [u8]) -> Result<()> {
    let ImageStorage {
        bitpix,
        bscale,
        bzero,
        ..
    } = *storage;
    let exact_offset =
        (bscale == 1.0 && bzero.fract() == 0.0 && bzero.abs() < 1e20).then_some(bzero as i128);
    let overflow = || Error::status(sys::NUM_OVERFLOW);
    let stored_float = |value: T| {
        let physical = match value.to_physical() {
            PhysicalValue::Exact(v) => v as f64,
            PhysicalValue::Scaled(v) => v,
        };
        (physical - bzero) / bscale
    };
    let stored_int = |value: T| match (value.to_physical(), exact_offset) {
        (PhysicalValue::Exact(v), Some(offset)) => Ok(v - offset),
        (value, _) => {
            let physical = match value {
                PhysicalValue::Exact(v) => v as f64,
                PhysicalValue::Scaled(v) => v,
            };
            let stored = ((physical - bzero) / bscale).round();
            if stored.is_finite() {
                Ok(stored as i128)
            } else {
                Err(overflow())
            }
        }
    };
    // One loop per BITPIX, so the per-pixel work is just the conversion.
    macro_rules! encode {
        ($size:literal, $stored:expr) => {
            for (&value, out) in data.iter().zip(out.chunks_exact_mut($size)) {
                out.copy_from_slice(&$stored(value)?.to_be_bytes());
            }
        };
    }
    match bitpix {
        -32 => encode!(4, |v| Ok::<_, Error>(stored_float(v) as f32)),
        -64 => encode!(8, |v| Ok::<_, Error>(stored_float(v))),
        8 => encode!(1, |v| u8::try_from(stored_int(v)?).map_err(|_| overflow())),
        16 => encode!(2, |v| i16::try_from(stored_int(v)?).map_err(|_| overflow())),
        32 => encode!(4, |v| i32::try_from(stored_int(v)?).map_err(|_| overflow())),
        _ => encode!(8, |v| i64::try_from(stored_int(v)?).map_err(|_| overflow())),
    }
    Ok(())
}

/// Write `data` in place over runs of pixels, encoded as the image stores
/// them. Each run is `(first pixel, pixel count)` in the flat `NAXIS1`-fastest
/// order, and `data` holds the runs' values back to back.
fn write_pixels<T: ToPhysical>(
    file: &mut FitsFile,
    storage: &ImageStorage,
    runs: &[(usize, usize)],
    data: &[T],
) -> Result<()> {
    let bytes_per_pixel = crate::image::bytes_per_pixel(storage.bitpix)?;
    for &(first, count) in runs {
        if first + count > storage.pixel_count {
            return Err(Error::Message(format!(
                "pixels {first}..{} are outside an image of {} pixels",
                first + count,
                storage.pixel_count
            )));
        }
    }
    let bytes = file.data_mut()?;
    let mut source = data;
    for &(first, count) in runs {
        let start = storage.data_start + first * bytes_per_pixel;
        let out = &mut bytes[start..start + count * bytes_per_pixel];
        encode_into(&source[..count], storage, out)?;
        source = &source[count..];
    }
    Ok(())
}

/// The contiguous runs covering a region of an image with axis lengths
/// `naxes` (`NAXIS1` first), in `NAXIS1`-fastest order.
fn region_runs(naxes: &[usize], ranges: &[std::ops::Range<usize>]) -> Result<Vec<(usize, usize)>> {
    if ranges.len() != naxes.len() {
        return Err(Error::Message(format!(
            "{} ranges given for a {}-dimensional image",
            ranges.len(),
            naxes.len()
        )));
    }
    for (axis, (range, &len)) in ranges.iter().zip(naxes).enumerate() {
        if range.start > range.end || range.end > len {
            return Err(Error::Message(format!(
                "range {range:?} is outside axis {} of length {len}",
                axis + 1
            )));
        }
    }
    if ranges.iter().any(|r| r.is_empty()) {
        return Ok(Vec::new());
    }
    let strides: Vec<usize> = naxes
        .iter()
        .scan(1usize, |stride, &len| {
            let this = *stride;
            *stride *= len;
            Some(this)
        })
        .collect();
    let run_len = ranges[0].end - ranges[0].start;
    let mut runs = Vec::new();
    let mut index: Vec<usize> = ranges.iter().map(|r| r.start).collect();
    loop {
        let first = index.iter().zip(&strides).map(|(i, s)| i * s).sum();
        runs.push((first, run_len));
        // Advance the outer axes like an odometer; axis 1 is the run itself.
        let mut axis = 1;
        loop {
            if axis == ranges.len() {
                return Ok(runs);
            }
            index[axis] += 1;
            if index[axis] < ranges[axis].end {
                break;
            }
            index[axis] = ranges[axis].start;
            axis += 1;
        }
    }
}

/// The image's axis lengths, `NAXIS1` first.
fn naxes(file: &FitsFile, hdu: &FitsHdu) -> Result<Vec<usize>> {
    let idx = validate_hdu_index(file, hdu)?;
    match &file.parsed()?.hdus[idx].info {
        crate::hdu::HduInfo::Primary { naxes, .. } | crate::hdu::HduInfo::Image { naxes, .. } => {
            Ok(naxes.clone())
        }
        _ => Err(Error::Message(format!(
            "HDU {idx} is not an uncompressed image"
        ))),
    }
}

macro_rules! impl_write_image {
    ($($t:ty),*) => {$(
        impl WriteImage for $t {
            fn write_image(file: &mut FitsFile, hdu: &FitsHdu, data: &[Self]) -> Result<()> {
                file.check_writable()?;
                let storage = image_storage(file, hdu)?;
                if data.len() > storage.pixel_count {
                    return Err(Error::Message(format!(
                        "{} values given for an image of {} pixels",
                        data.len(),
                        storage.pixel_count
                    )));
                }
                write_pixels(file, &storage, &[(0, data.len())], data)
            }

            fn write_section(
                file: &mut FitsFile,
                hdu: &FitsHdu,
                range: std::ops::Range<usize>,
                data: &[Self],
            ) -> Result<()> {
                file.check_writable()?;
                let count = range.end.saturating_sub(range.start);
                if data.len() < count {
                    return Err(Error::Message(format!(
                        "{} values given for a section of {count} pixels",
                        data.len()
                    )));
                }
                let storage = image_storage(file, hdu)?;
                write_pixels(file, &storage, &[(range.start, count)], &data[..count])
            }

            fn write_region(
                file: &mut FitsFile,
                hdu: &FitsHdu,
                ranges: &[std::ops::Range<usize>],
                data: &[Self],
            ) -> Result<()> {
                file.check_writable()?;
                let naxes = naxes(file, hdu)?;
                let runs = region_runs(&naxes, trim_absent_axes(naxes.len(), ranges))?;
                let count: usize = runs.iter().map(|&(_, n)| n).sum();
                if data.len() < count {
                    return Err(Error::Message(format!(
                        "{} values given for a region of {count} pixels",
                        data.len()
                    )));
                }
                let storage = image_storage(file, hdu)?;
                write_pixels(file, &storage, &runs, &data[..count])
            }
        }
    )*};
}

impl_write_image!(u8, i8, i16, u16, i32, u32, i64, u64, f32, f64);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compat::fitsfile::FitsFile;

    #[test]
    fn image_type_bitpix_roundtrip() {
        for &(it, bp) in &[
            (ImageType::UnsignedByte, 8),
            (ImageType::Short, 16),
            (ImageType::Long, 32),
            (ImageType::LongLong, 64),
            (ImageType::Float, -32),
            (ImageType::Double, -64),
        ] {
            assert_eq!(it.to_bitpix(), bp);
            assert_eq!(ImageType::from_bitpix(bp).unwrap(), it);
        }
    }

    #[test]
    fn invalid_bitpix() {
        assert!(ImageType::from_bitpix(7).is_err());
    }

    #[test]
    fn create_and_read_image_f32() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("img.fits");
        let mut f = FitsFile::create(&path).open().unwrap();

        let desc = ImageDescription {
            data_type: ImageType::Float,
            dimensions: &[4],
        };
        let hdu = f.create_image("SCI", &desc).unwrap();
        let pixels: Vec<f32> = vec![1.0, 2.5, 3.125, 4.75];
        f32::write_image(&mut f, &hdu, &pixels).unwrap();

        let read_back = f32::read_image(&f, &hdu).unwrap();
        assert_eq!(read_back, pixels);
    }

    #[test]
    fn create_and_read_image_f64() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("img.fits");
        let mut f = FitsFile::create(&path).open().unwrap();

        let desc = ImageDescription {
            data_type: ImageType::Double,
            dimensions: &[3],
        };
        let hdu = f.create_image("DATA", &desc).unwrap();
        let pixels: Vec<f64> = vec![1.5, -2.625, 0.0];
        f64::write_image(&mut f, &hdu, &pixels).unwrap();

        let read_back = f64::read_image(&f, &hdu).unwrap();
        assert_eq!(read_back, pixels);
    }

    #[test]
    fn create_and_read_image_u8() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("img.fits");
        let mut f = FitsFile::create(&path).open().unwrap();

        let desc = ImageDescription {
            data_type: ImageType::UnsignedByte,
            dimensions: &[4],
        };
        let hdu = f.create_image("RAW", &desc).unwrap();
        let pixels: Vec<u8> = vec![0, 127, 200, 255];
        u8::write_image(&mut f, &hdu, &pixels).unwrap();

        let read_back = u8::read_image(&f, &hdu).unwrap();
        assert_eq!(read_back, pixels);
    }

    fn roundtrip_unsigned<T>(data_type: ImageType, pixels: Vec<T>)
    where
        T: ReadImage + WriteImage + Clone + PartialEq + std::fmt::Debug,
    {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("img.fits");
        let mut f = FitsFile::create(&path).open().unwrap();

        let desc = ImageDescription {
            data_type,
            dimensions: &[pixels.len()],
        };
        let hdu = f.create_image("SCI", &desc).unwrap();
        <T as WriteImage>::write_image(&mut f, &hdu, &pixels).unwrap();

        let read_back = <T as ReadImage>::read_image(&f, &hdu).unwrap();
        assert_eq!(read_back, pixels);
    }

    #[test]
    fn create_and_read_image_u16() {
        // Includes 0, the BZERO midpoint, and the max value.
        roundtrip_unsigned::<u16>(
            ImageType::UnsignedShort,
            vec![0, 1, 32767, 32768, 40000, 65535],
        );
    }

    #[test]
    fn create_and_read_image_u32() {
        roundtrip_unsigned::<u32>(
            ImageType::UnsignedLong,
            vec![0, 1, 2_147_483_647, 2_147_483_648, u32::MAX],
        );
    }

    #[test]
    fn create_and_read_image_u64() {
        roundtrip_unsigned::<u64>(
            ImageType::UnsignedLongLong,
            vec![
                0,
                1,
                9_223_372_036_854_775_807,
                9_223_372_036_854_775_808,
                u64::MAX,
            ],
        );
    }

    #[test]
    fn u16_uses_cfitsio_storage_convention() {
        // A u16 image must be stored as BITPIX=16 + BZERO=32768, with the raw
        // signed pixels offset by -32768, so cfitsio (and other readers) recover
        // the unsigned values.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("u16.fits");
        let mut f = FitsFile::create(&path).open().unwrap();
        let desc = ImageDescription {
            data_type: ImageType::UnsignedShort,
            dimensions: &[3],
        };
        let hdu = f.create_image("SCI", &desc).unwrap();
        u16::write_image(&mut f, &hdu, &[0u16, 32768, 65535]).unwrap();

        // BZERO keyword present and correct.
        use crate::compat::headers::ReadsKey;
        let bzero: i64 = i64::read_key(&f, &hdu, "BZERO").unwrap();
        assert_eq!(bzero, 32768);

        // Raw signed storage is value - 32768.
        let parsed = f.parsed().unwrap();
        let raw =
            crate::image::read_image_data(f.data().unwrap(), &parsed.hdus[hdu.number]).unwrap();
        assert_eq!(raw, crate::image::ImageData::I16(vec![-32768, 0, 32767]));

        // Like cfitsio, an i16 read applies BZERO, so 32768 and 65535 overflow.
        assert!(i16::read_image(&f, &hdu).is_err());
    }

    #[test]
    fn equivalent_type_matches_cfitsio() {
        use ImageType::*;
        for &(bitpix, bscale, bzero, want) in &[
            (16, 1.0, 0.0, Short),
            (16, 1.0, 32_768.0, UnsignedShort),
            (16, 2.0, 32_768.0, Long),
            (16, 0.5, 0.0, Float),
            (16, 1.0, 100.0, Long),
            (8, 1.0, -128.0, Byte),
            (32, 1.0, 2_147_483_648.0, UnsignedLong),
            (32, 1.5, 0.0, Double),
            (-32, 2.0, 1.0, Float),
        ] {
            assert_eq!(
                ImageType::equivalent(bitpix, bscale, bzero).unwrap(),
                want,
                "BITPIX={bitpix} BSCALE={bscale} BZERO={bzero}"
            );
        }
    }

    #[test]
    fn read_applies_bscale_and_bzero() {
        // BSCALE = 2 and BZERO = 100 are non-degenerate: skipping either one
        // changes every value.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scaled.fits");
        let mut f = FitsFile::create(&path).open().unwrap();
        let desc = ImageDescription {
            data_type: ImageType::Short,
            dimensions: &[3],
        };
        let hdu = f.create_image("SCI", &desc).unwrap();
        i16::write_image(&mut f, &hdu, &[0, 1, 2]).unwrap();
        hdu.write_key(&mut f, "BSCALE", 2i64).unwrap();
        hdu.write_key(&mut f, "BZERO", 100i64).unwrap();

        assert_eq!(i32::read_image(&f, &hdu).unwrap(), vec![100, 102, 104]);
        assert_eq!(
            f32::read_image(&f, &hdu).unwrap(),
            vec![100.0, 102.0, 104.0]
        );
        match hdu.info(&f).unwrap() {
            crate::compat::hdu::HduInfo::ImageInfo { image_type, .. } => {
                assert_eq!(image_type, ImageType::Long)
            }
            other => panic!("expected ImageInfo, got {other:?}"),
        }
    }

    #[test]
    fn create_and_read_image_i8() {
        roundtrip_unsigned::<i8>(ImageType::Byte, vec![-128, -1, 0, 1, 127]);
    }

    #[test]
    fn read_image_into_buffer_f32() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("img.fits");
        let mut f = FitsFile::create(&path).open().unwrap();

        let desc = ImageDescription {
            data_type: ImageType::Float,
            dimensions: &[4],
        };
        let hdu = f.create_image("SCI", &desc).unwrap();
        let pixels: Vec<f32> = vec![1.0, 2.5, 3.125, 4.75];
        f32::write_image(&mut f, &hdu, &pixels).unwrap();

        let mut buf = vec![0.0f32; 4];
        f32::read_image_into_buffer(&f, &hdu, &mut buf).unwrap();
        assert_eq!(buf, pixels);
    }

    #[test]
    fn read_image_into_buffer_f64() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("img.fits");
        let mut f = FitsFile::create(&path).open().unwrap();

        let desc = ImageDescription {
            data_type: ImageType::Double,
            dimensions: &[3],
        };
        let hdu = f.create_image("DATA", &desc).unwrap();
        let pixels: Vec<f64> = vec![1.5, -2.625, 0.0];
        f64::write_image(&mut f, &hdu, &pixels).unwrap();

        let mut buf = vec![0.0f64; 3];
        f64::read_image_into_buffer(&f, &hdu, &mut buf).unwrap();
        assert_eq!(buf, pixels);
    }

    #[test]
    fn read_image_into_buffer_wrong_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("img.fits");
        let mut f = FitsFile::create(&path).open().unwrap();

        let desc = ImageDescription {
            data_type: ImageType::Float,
            dimensions: &[4],
        };
        let hdu = f.create_image("SCI", &desc).unwrap();
        let pixels: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0];
        f32::write_image(&mut f, &hdu, &pixels).unwrap();

        // As with ffgpv, a shorter buffer reads the leading pixels.
        let mut buf = vec![0.0f32; 3];
        f32::read_image_into_buffer(&f, &hdu, &mut buf).unwrap();
        assert_eq!(buf, vec![1.0, 2.0, 3.0]);

        let mut buf = vec![0.0f32; 5];
        let err = f32::read_image_into_buffer(&f, &hdu, &mut buf).unwrap_err();
        assert_eq!(status(err), sys::BAD_ROW_NUM);
    }

    #[test]
    fn read_image_into_buffer_cross_type() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("img.fits");
        let mut f = FitsFile::create(&path).open().unwrap();

        let desc = ImageDescription {
            data_type: ImageType::Short,
            dimensions: &[3],
        };
        let hdu = f.create_image("SCI", &desc).unwrap();
        let pixels: Vec<i16> = vec![100, 200, 300];
        i16::write_image(&mut f, &hdu, &pixels).unwrap();

        let mut buf = vec![0.0f32; 3];
        f32::read_image_into_buffer(&f, &hdu, &mut buf).unwrap();
        assert_eq!(buf, vec![100.0, 200.0, 300.0]);
    }

    fn status(err: Error) -> u32 {
        match err {
            Error::Fits(e) => e.status as u32,
            other => panic!("expected a cfitsio status, got {other:?}"),
        }
    }

    /// Save a 4x3 `BITPIX = 16` image of 0..11 with `BSCALE = 0.5` and
    /// `BZERO = 3` to `path`, tile-compressed if `compress`, and open it
    /// read-only from disk.
    fn scaled_image(dir: &std::path::Path, compress: bool) -> (FitsFile, FitsHdu) {
        let path = dir.join("scaled.fits");
        let spec = if compress { "[compress]" } else { "" };
        let mut f = FitsFile::create(format!("{}{spec}", path.display()))
            .open()
            .unwrap();
        let desc = ImageDescription {
            data_type: ImageType::Short,
            dimensions: &[3, 4],
        };
        let hdu = f.create_image("SCI", &desc).unwrap();
        let stored: Vec<i16> = (0..12).collect();
        i16::write_image(&mut f, &hdu, &stored).unwrap();
        hdu.write_key(&mut f, "BSCALE", 0.5f64).unwrap();
        hdu.write_key(&mut f, "BZERO", 3i64).unwrap();
        drop(f);
        let f = FitsFile::open(&path).unwrap();
        let info = &f.parsed().unwrap().hdus[1].info;
        assert_eq!(
            matches!(info, crate::hdu::HduInfo::CompressedImage { .. }),
            compress
        );
        let hdu = f.hdu(1).unwrap();
        (f, hdu)
    }

    /// Into-buffer reads of every pixel type match `read_image`, whole and
    /// in part, on a scaled image.
    fn check_scaled_into_buffer<T>(f: &FitsFile, hdu: &FitsHdu)
    where
        T: ReadImage + ReadImageIntoBuffer + Copy + Default + PartialEq + std::fmt::Debug,
    {
        let want = T::read_image(f, hdu).unwrap();
        assert_eq!(want.len(), 12);
        let mut buf = vec![T::default(); 12];
        T::read_image_into_buffer(f, hdu, &mut buf).unwrap();
        assert_eq!(buf, want);

        let mut part = vec![T::default(); 5];
        T::read_section_into_buffer(f, hdu, 6, &mut part).unwrap();
        assert_eq!(part, want[6..11]);
    }

    fn check_scaled_image(compress: bool) {
        let dir = tempfile::tempdir().unwrap();
        let (f, hdu) = scaled_image(dir.path(), compress);
        let physical: Vec<f64> = (0..12).map(|v| f64::from(v) * 0.5 + 3.0).collect();

        let mut buf = vec![0.0f64; 12];
        f64::read_image_into_buffer(&f, &hdu, &mut buf).unwrap();
        assert_eq!(buf, physical);
        let mut buf = vec![0.0f32; 12];
        f32::read_image_into_buffer(&f, &hdu, &mut buf).unwrap();
        assert_eq!(buf, physical.iter().map(|&v| v as f32).collect::<Vec<_>>());
        // Integer reads truncate the physical value, as cfitsio's do.
        let mut buf = vec![0i32; 4];
        i32::read_image_into_buffer(&f, &hdu, &mut buf).unwrap();
        assert_eq!(buf, vec![3, 3, 4, 4]);

        check_scaled_into_buffer::<u8>(&f, &hdu);
        check_scaled_into_buffer::<i8>(&f, &hdu);
        check_scaled_into_buffer::<i16>(&f, &hdu);
        check_scaled_into_buffer::<u16>(&f, &hdu);
        check_scaled_into_buffer::<i32>(&f, &hdu);
        check_scaled_into_buffer::<u32>(&f, &hdu);
        check_scaled_into_buffer::<i64>(&f, &hdu);
        check_scaled_into_buffer::<u64>(&f, &hdu);
        check_scaled_into_buffer::<f32>(&f, &hdu);
        check_scaled_into_buffer::<f64>(&f, &hdu);
    }

    #[test]
    fn read_image_into_buffer_applies_bscale_and_bzero() {
        check_scaled_image(false);
    }

    #[test]
    fn read_image_into_buffer_applies_bscale_and_bzero_when_compressed() {
        check_scaled_image(true);
    }

    #[test]
    fn read_section_into_buffer_checks_bounds_as_ffgpv_does() {
        let dir = tempfile::tempdir().unwrap();
        let (f, hdu) = scaled_image(dir.path(), false);
        let mut buf = [0.0f32; 1];
        f32::read_section_into_buffer(&f, &hdu, 11, &mut buf).unwrap();
        assert_eq!(buf, [8.5]);
        let err = f32::read_section_into_buffer(&f, &hdu, 12, &mut buf).unwrap_err();
        assert_eq!(status(err), sys::BAD_ELEM_NUM);
        let mut buf = [0.0f32; 2];
        let err = f32::read_section_into_buffer(&f, &hdu, 11, &mut buf).unwrap_err();
        assert_eq!(status(err), sys::BAD_ROW_NUM);
        f32::read_section_into_buffer(&f, &hdu, 12, &mut []).unwrap();
    }

    #[test]
    fn read_image_into_buffer_reads_only_the_hdu() {
        for compress in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let (f, hdu) = scaled_image(dir.path(), compress);
            let mut buf = vec![0.0f32; 12];
            f32::read_image_into_buffer(&f, &hdu, &mut buf).unwrap();
            assert!(!f.is_loaded(), "compress = {compress}");
        }
    }

    #[test]
    fn read_image_into_buffer_spans_read_blocks() {
        // More pixels than one 16 KiB read block holds.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.fits");
        let mut f = FitsFile::create(&path).open().unwrap();
        let desc = ImageDescription {
            data_type: ImageType::Long,
            dimensions: &[10_000],
        };
        let hdu = f.create_image("SCI", &desc).unwrap();
        let pixels: Vec<i32> = (0..10_000).collect();
        i32::write_image(&mut f, &hdu, &pixels).unwrap();
        hdu.write_key(&mut f, "BZERO", -5i64).unwrap();
        drop(f);

        let f = FitsFile::open(&path).unwrap();
        let hdu = f.hdu(1).unwrap();
        let mut buf = vec![0i64; 9_000];
        i64::read_section_into_buffer(&f, &hdu, 1_000, &mut buf).unwrap();
        let want: Vec<i64> = (995..9_995).collect();
        assert_eq!(buf, want);
    }

    /// A 7x10 `i32` image of 0..70 with `BSCALE = 0.5`, tile-compressed in
    /// 3x4 tiles, so the image ends in partial tiles along both axes.
    fn compressed_7x10(compression: &crate::compress::TileCompression) -> FitsFile {
        use crate::header::{serialize_header, Card};
        use crate::value::Value;
        let pixels: Vec<i32> = (0..70).collect();
        let mut bytes =
            serialize_header(&crate::primary::build_primary_header(8, &[]).unwrap()).unwrap();
        let bscale = Card::new("BSCALE", Value::Float(0.5)).unwrap();
        let compression = compression.clone().tile_dims(&[3, 4]);
        bytes.extend(
            crate::compress::compress_image_hdu(&[7, 10], &pixels, &compression, &[bscale])
                .unwrap(),
        );
        FitsFile::from_bytes(bytes).unwrap()
    }

    fn check_compressed_windows(compression: crate::compress::TileCompression) {
        let f = compressed_7x10(&compression);
        let hdu = f.hdu(1).unwrap();
        let physical: Vec<f64> = (0..70).map(|v| f64::from(v) * 0.5).collect();
        assert_eq!(f64::read_image(&f, &hdu).unwrap(), physical);
        for start in 0..70 {
            for end in start..=70 {
                assert_eq!(
                    f64::read_section(&f, &hdu, start..end).unwrap(),
                    physical[start..end],
                    "section {start}..{end}"
                );
            }
        }
        for start in 0..10 {
            for rows in 0..=10 - start {
                assert_eq!(
                    f64::read_rows(&f, &hdu, start, rows).unwrap(),
                    physical[start * 7..(start + rows) * 7],
                    "rows {start}+{rows}"
                );
            }
        }
        let region = f64::read_region(&f, &hdu, &[2..6, 3..10]).unwrap();
        let want: Vec<f64> = (3..10)
            .flat_map(|y| (2..6).map(move |x| f64::from(y * 7 + x) * 0.5))
            .collect();
        assert_eq!(region, want);
        assert_eq!(
            status(f64::read_section(&f, &hdu, 60..71).unwrap_err()),
            sys::BAD_ROW_NUM
        );
        assert_eq!(
            status(f64::read_rows(&f, &hdu, 9, 2).unwrap_err()),
            sys::BAD_ROW_NUM
        );
    }

    #[test]
    fn sections_and_rows_of_rice_compressed_images() {
        check_compressed_windows(crate::compress::TileCompression::rice());
    }

    #[test]
    fn sections_and_rows_of_gzip2_compressed_images() {
        check_compressed_windows(crate::compress::TileCompression::gzip2());
    }
}
