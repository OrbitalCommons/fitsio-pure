use super::errors::{Error, Result};
use super::fitsfile::FitsFile;
use super::hdu::FitsHdu;

/// Describes the shape and type of an image HDU.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageDescription {
    pub data_type: ImageType,
    pub dimensions: Vec<usize>,
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
    if hdu.hdu_index >= fits_data.len() {
        return Err(Error::Message(format!(
            "HDU index {} out of range",
            hdu.hdu_index
        )));
    }
    Ok(hdu.hdu_index)
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

/// Trait for types that support zero-allocation image reads into a caller buffer.
pub trait ReadImageIntoBuffer: Sized {
    fn read_image_into_buffer(file: &FitsFile, hdu: &FitsHdu, buf: &mut [Self]) -> Result<()>;
}

impl ReadImageIntoBuffer for f32 {
    fn read_image_into_buffer(file: &FitsFile, hdu: &FitsHdu, buf: &mut [Self]) -> Result<()> {
        let idx = validate_hdu_index(file, hdu)?;
        let parsed = file.parsed()?;
        let core_hdu = &parsed.hdus[idx];
        crate::image::read_image_data_into_f32(file.data(), core_hdu, buf)?;
        Ok(())
    }
}

impl ReadImageIntoBuffer for f64 {
    fn read_image_into_buffer(file: &FitsFile, hdu: &FitsHdu, buf: &mut [Self]) -> Result<()> {
        let idx = validate_hdu_index(file, hdu)?;
        let parsed = file.parsed()?;
        let core_hdu = &parsed.hdus[idx];
        crate::image::read_image_data_into_f64(file.data(), core_hdu, buf)?;
        Ok(())
    }
}

/// Trait for types that can write image pixel data to a FITS file.
pub trait WriteImage {
    fn write_image(file: &mut FitsFile, hdu: &FitsHdu, data: &[Self]) -> Result<()>
    where
        Self: Sized;
}

fn ranges_to_tuples(ranges: &[std::ops::Range<usize>]) -> Vec<(usize, usize)> {
    ranges.iter().map(|r| (r.start, r.end)).collect()
}

/// Pixel values with `BSCALE`/`BZERO` applied, as cfitsio returns them.
///
/// Integer data under unit scale and an integer offset stays exact in `i128`,
/// which covers the `u64` convention (`BZERO = 2^63`); everything else is
/// scaled in `f64`, as cfitsio does.
enum Physical {
    Exact(Vec<i128>),
    Scaled(Vec<f64>),
}

/// Read stored pixels (via `read`) and apply the HDU's `BSCALE`/`BZERO`.
fn read_physical<F>(file: &FitsFile, hdu: &FitsHdu, read: F) -> Result<Physical>
where
    F: FnOnce(&[u8], &crate::hdu::Hdu) -> crate::error::Result<crate::image::ImageData>,
{
    use crate::image::ImageData;
    let idx = validate_hdu_index(file, hdu)?;
    let parsed = file.parsed()?;
    let core_hdu = &parsed.hdus[idx];
    let (bscale, bzero) = crate::image::extract_bscale_bzero(&core_hdu.cards);
    let stored: Vec<i128> = match read(file.data(), core_hdu)? {
        ImageData::U8(v) => v.into_iter().map(i128::from).collect(),
        ImageData::I16(v) => v.into_iter().map(i128::from).collect(),
        ImageData::I32(v) => v.into_iter().map(i128::from).collect(),
        ImageData::I64(v) => v.into_iter().map(i128::from).collect(),
        ImageData::F32(v) => {
            let scaled = v.into_iter().map(|x| f64::from(x) * bscale + bzero);
            return Ok(Physical::Scaled(scaled.collect()));
        }
        ImageData::F64(v) => {
            let scaled = v.into_iter().map(|x| x * bscale + bzero);
            return Ok(Physical::Scaled(scaled.collect()));
        }
    };
    if bscale == 1.0 && bzero.fract() == 0.0 && bzero.abs() < 1e20 {
        let offset = bzero as i128;
        Ok(Physical::Exact(
            stored.into_iter().map(|x| x + offset).collect(),
        ))
    } else {
        let scaled = stored.into_iter().map(|x| x as f64 * bscale + bzero);
        Ok(Physical::Scaled(scaled.collect()))
    }
}

/// Conversion from a physical pixel value to the requested element type.
/// `None` means the value does not fit, where cfitsio reports `NUM_OVERFLOW`.
trait FromPhysical: Sized {
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

fn narrow<T: FromPhysical>(physical: Physical) -> Result<Vec<T>> {
    let overflow = || {
        Error::Message(format!(
            "pixel value out of range for {}",
            std::any::type_name::<T>()
        ))
    };
    match physical {
        Physical::Exact(v) => v
            .into_iter()
            .map(|x| T::from_exact(x).ok_or_else(overflow))
            .collect(),
        Physical::Scaled(v) => v
            .into_iter()
            .map(|x| T::from_scaled(x).ok_or_else(overflow))
            .collect(),
    }
}

/// Implement `ReadImage` for a type: every read returns physical values
/// (`BSCALE`/`BZERO` applied) and fails where a value does not fit, matching
/// cfitsio rather than wrapping or returning stored values.
macro_rules! impl_read_image {
    ($($t:ty),*) => {$(
        impl ReadImage for $t {
            fn read_image(file: &FitsFile, hdu: &FitsHdu) -> Result<Vec<Self>> {
                narrow(read_physical(file, hdu, |d, h| {
                    crate::image::read_image_data(d, h)
                })?)
            }

            fn read_section(
                file: &FitsFile,
                hdu: &FitsHdu,
                range: std::ops::Range<usize>,
            ) -> Result<Vec<Self>> {
                let count = range.end.saturating_sub(range.start);
                narrow(read_physical(file, hdu, |d, h| {
                    crate::image::read_image_section(d, h, range.start, count)
                })?)
            }

            fn read_rows(
                file: &FitsFile,
                hdu: &FitsHdu,
                start_row: usize,
                num_rows: usize,
            ) -> Result<Vec<Self>> {
                narrow(read_physical(file, hdu, |d, h| {
                    crate::image::read_image_rows(d, h, start_row, num_rows)
                })?)
            }

            fn read_region(
                file: &FitsFile,
                hdu: &FitsHdu,
                ranges: &[std::ops::Range<usize>],
            ) -> Result<Vec<Self>> {
                let tuples = ranges_to_tuples(ranges);
                narrow(read_physical(file, hdu, |d, h| {
                    crate::image::read_image_region(d, h, &tuples)
                })?)
            }
        }
    )*};
}

impl_read_image!(u8, i8, i16, u16, i32, u32, i64, u64, f32, f64);

/// Splice already-serialized pixel bytes into the HDU's data region, preserving
/// any subsequent HDUs.
fn write_serialized(file: &mut FitsFile, hdu: &FitsHdu, serialized: &[u8]) -> Result<()> {
    // Read HDU metadata from cache before mutating.
    let (data_start, padded_data_len, file_len) = {
        let parsed = file.parsed()?;
        let core_hdu = parsed
            .hdus
            .get(hdu.hdu_index)
            .ok_or(Error::Message(format!(
                "HDU index {} out of range",
                hdu.hdu_index
            )))?;
        let padded = crate::block::padded_byte_len(core_hdu.data_len);
        (core_hdu.data_start, padded, file.data().len())
    };

    let next_hdu_start = data_start + padded_data_len;
    let tail_len = file_len.saturating_sub(next_hdu_start);

    let mut new_data = Vec::with_capacity(data_start + serialized.len() + tail_len);
    new_data.extend_from_slice(&file.data()[..data_start]);
    new_data.extend_from_slice(serialized);
    if tail_len > 0 {
        new_data.extend_from_slice(&file.data()[next_hdu_start..]);
    }

    file.set_data(new_data);
    Ok(())
}

macro_rules! impl_write_image {
    ($t:ty, $serialize_fn:path) => {
        impl WriteImage for $t {
            fn write_image(file: &mut FitsFile, hdu: &FitsHdu, data: &[Self]) -> Result<()> {
                write_serialized(file, hdu, &$serialize_fn(data))
            }
        }
    };
}

impl_write_image!(u8, crate::image::serialize_image_u8);
impl_write_image!(i16, crate::image::serialize_image_i16);
impl_write_image!(i32, crate::image::serialize_image_i32);
impl_write_image!(i64, crate::image::serialize_image_i64);
impl_write_image!(f32, crate::image::serialize_image_f32);
impl_write_image!(f64, crate::image::serialize_image_f64);

/// Write an unsigned image using the cfitsio storage convention: each value is
/// offset by `-BZERO` (a sign-bit flip) into the signed storage type before
/// serialization. The matching `BZERO`/`BSCALE` keywords are written by
/// `create_image`.
macro_rules! impl_write_image_unsigned {
    ($t:ty, $signed:ty, $sign_bit:expr, $serialize_fn:path) => {
        impl WriteImage for $t {
            fn write_image(file: &mut FitsFile, hdu: &FitsHdu, data: &[Self]) -> Result<()> {
                let storage: Vec<$signed> =
                    data.iter().map(|&u| (u ^ $sign_bit) as $signed).collect();
                write_serialized(file, hdu, &$serialize_fn(&storage))
            }
        }
    };
}

impl_write_image_unsigned!(u16, i16, 0x8000, crate::image::serialize_image_i16);
impl_write_image_unsigned!(u32, i32, 0x8000_0000, crate::image::serialize_image_i32);
impl_write_image_unsigned!(
    u64,
    i64,
    0x8000_0000_0000_0000,
    crate::image::serialize_image_i64
);

/// Write an `i8` image as `BITPIX = 8` storage offset by `BZERO = -128`.
impl WriteImage for i8 {
    fn write_image(file: &mut FitsFile, hdu: &FitsHdu, data: &[Self]) -> Result<()> {
        let storage: Vec<u8> = data.iter().map(|&v| (v as u8) ^ 0x80).collect();
        write_serialized(file, hdu, &crate::image::serialize_image_u8(&storage))
    }
}

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
            dimensions: vec![4],
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
            dimensions: vec![3],
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
            dimensions: vec![4],
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
            dimensions: vec![pixels.len()],
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
            dimensions: vec![3],
        };
        let hdu = f.create_image("SCI", &desc).unwrap();
        u16::write_image(&mut f, &hdu, &[0u16, 32768, 65535]).unwrap();

        // BZERO keyword present and correct.
        use crate::compat::headers::ReadsKey;
        let bzero: i64 = i64::read_key(&f, &hdu, "BZERO").unwrap();
        assert_eq!(bzero, 32768);

        // Raw signed storage is value - 32768.
        let parsed = f.parsed().unwrap();
        let raw = crate::image::read_image_data(f.data(), &parsed.hdus[hdu.hdu_index]).unwrap();
        assert_eq!(raw, crate::image::ImageData::I16(vec![-32768, 0, 32767]));
        drop(parsed);

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
            dimensions: vec![3],
        };
        let hdu = f.create_image("SCI", &desc).unwrap();
        i16::write_image(&mut f, &hdu, &[0, 1, 2]).unwrap();
        hdu.write_key(&mut f, "BSCALE", &2i64).unwrap();
        hdu.write_key(&mut f, "BZERO", &100i64).unwrap();

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
            dimensions: vec![4],
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
            dimensions: vec![3],
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
            dimensions: vec![4],
        };
        let hdu = f.create_image("SCI", &desc).unwrap();
        let pixels: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0];
        f32::write_image(&mut f, &hdu, &pixels).unwrap();

        let mut buf = vec![0.0f32; 3]; // wrong size
        assert!(f32::read_image_into_buffer(&f, &hdu, &mut buf).is_err());
    }

    #[test]
    fn read_image_into_buffer_cross_type() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("img.fits");
        let mut f = FitsFile::create(&path).open().unwrap();

        let desc = ImageDescription {
            data_type: ImageType::Short,
            dimensions: vec![3],
        };
        let hdu = f.create_image("SCI", &desc).unwrap();
        let pixels: Vec<i16> = vec![100, 200, 300];
        i16::write_image(&mut f, &hdu, &pixels).unwrap();

        let mut buf = vec![0.0f32; 3];
        f32::read_image_into_buffer(&f, &hdu, &mut buf).unwrap();
        assert_eq!(buf, vec![100.0, 200.0, 300.0]);
    }
}
