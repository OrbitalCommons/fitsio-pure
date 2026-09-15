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
            ImageType::UnsignedByte => 8,
            ImageType::Short | ImageType::UnsignedShort => 16,
            ImageType::Long | ImageType::UnsignedLong => 32,
            ImageType::LongLong | ImageType::UnsignedLongLong => 64,
            ImageType::Float => -32,
            ImageType::Double => -64,
        }
    }

    /// The `BZERO` offset for the cfitsio unsigned-integer storage convention,
    /// or `None` for signed/floating types.
    ///
    /// `u16`/`u32` use an integer `BZERO`; `u64` uses `2^63`, which exceeds
    /// `i64::MAX`, so it is stored as a (exactly representable) float.
    pub(crate) fn unsigned_bzero(self) -> Option<crate::value::Value> {
        use crate::value::Value;
        match self {
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

fn extract_from_image_data<T: Clone>(
    data: &crate::image::ImageData,
    convert_u8: fn(&[u8]) -> Vec<T>,
    convert_i16: fn(&[i16]) -> Vec<T>,
    convert_i32: fn(&[i32]) -> Vec<T>,
    convert_i64: fn(&[i64]) -> Vec<T>,
    convert_f32: fn(&[f32]) -> Vec<T>,
    convert_f64: fn(&[f64]) -> Vec<T>,
) -> Vec<T> {
    match data {
        crate::image::ImageData::U8(v) => convert_u8(v),
        crate::image::ImageData::I16(v) => convert_i16(v),
        crate::image::ImageData::I32(v) => convert_i32(v),
        crate::image::ImageData::I64(v) => convert_i64(v),
        crate::image::ImageData::F32(v) => convert_f32(v),
        crate::image::ImageData::F64(v) => convert_f64(v),
    }
}

fn ranges_to_tuples(ranges: &[std::ops::Range<usize>]) -> Vec<(usize, usize)> {
    ranges.iter().map(|r| (r.start, r.end)).collect()
}

/// Collect raw stored pixel values as `i64`, regardless of the on-disk type.
fn image_data_to_i64(data: &crate::image::ImageData) -> Vec<i64> {
    extract_from_image_data(
        data,
        |v: &[u8]| v.iter().map(|&x| x as i64).collect(),
        |v: &[i16]| v.iter().map(|&x| x as i64).collect(),
        |v: &[i32]| v.iter().map(|&x| x as i64).collect(),
        |v: &[i64]| v.to_vec(),
        |v: &[f32]| v.iter().map(|&x| x as i64).collect(),
        |v: &[f64]| v.iter().map(|&x| x as i64).collect(),
    )
}

/// Read raw stored pixels (via `read`) as `i64` together with the HDU's `BZERO`
/// offset, for recovering values written with the unsigned storage convention.
fn read_raw_with_bzero<F>(file: &FitsFile, hdu: &FitsHdu, read: F) -> Result<(Vec<i64>, i128)>
where
    F: FnOnce(&[u8], &crate::hdu::Hdu) -> crate::error::Result<crate::image::ImageData>,
{
    let idx = validate_hdu_index(file, hdu)?;
    let parsed = file.parsed()?;
    let core_hdu = &parsed.hdus[idx];
    let (_bscale, bzero) = crate::image::extract_bscale_bzero(&core_hdu.cards);
    let img = read(file.data(), core_hdu)?;
    Ok((image_data_to_i64(&img), bzero as i128))
}

/// Implement `ReadImage` for an unsigned type, recovering each value as
/// `stored + BZERO` (cfitsio unsigned storage convention).
macro_rules! impl_read_image_unsigned {
    ($t:ty) => {
        impl ReadImage for $t {
            fn read_image(file: &FitsFile, hdu: &FitsHdu) -> Result<Vec<Self>> {
                let (raw, bz) =
                    read_raw_with_bzero(file, hdu, |d, h| crate::image::read_image_data(d, h))?;
                Ok(raw.into_iter().map(|p| (p as i128 + bz) as $t).collect())
            }

            fn read_section(
                file: &FitsFile,
                hdu: &FitsHdu,
                range: std::ops::Range<usize>,
            ) -> Result<Vec<Self>> {
                let count = range.end.saturating_sub(range.start);
                let (raw, bz) = read_raw_with_bzero(file, hdu, |d, h| {
                    crate::image::read_image_section(d, h, range.start, count)
                })?;
                Ok(raw.into_iter().map(|p| (p as i128 + bz) as $t).collect())
            }

            fn read_rows(
                file: &FitsFile,
                hdu: &FitsHdu,
                start_row: usize,
                num_rows: usize,
            ) -> Result<Vec<Self>> {
                let (raw, bz) = read_raw_with_bzero(file, hdu, |d, h| {
                    crate::image::read_image_rows(d, h, start_row, num_rows)
                })?;
                Ok(raw.into_iter().map(|p| (p as i128 + bz) as $t).collect())
            }

            fn read_region(
                file: &FitsFile,
                hdu: &FitsHdu,
                ranges: &[std::ops::Range<usize>],
            ) -> Result<Vec<Self>> {
                let tuples = ranges_to_tuples(ranges);
                let (raw, bz) = read_raw_with_bzero(file, hdu, |d, h| {
                    crate::image::read_image_region(d, h, &tuples)
                })?;
                Ok(raw.into_iter().map(|p| (p as i128 + bz) as $t).collect())
            }
        }
    };
}

impl_read_image_unsigned!(u16);
impl_read_image_unsigned!(u32);
impl_read_image_unsigned!(u64);

macro_rules! impl_read_image {
    ($t:ty, $u8_fn:expr, $i16_fn:expr, $i32_fn:expr, $i64_fn:expr, $f32_fn:expr, $f64_fn:expr) => {
        impl ReadImage for $t {
            fn read_image(file: &FitsFile, hdu: &FitsHdu) -> Result<Vec<Self>> {
                let idx = validate_hdu_index(file, hdu)?;
                let parsed = file.parsed()?;
                let core_hdu = &parsed.hdus[idx];
                let img = crate::image::read_image_data(file.data(), core_hdu)?;
                Ok(extract_from_image_data(
                    &img, $u8_fn, $i16_fn, $i32_fn, $i64_fn, $f32_fn, $f64_fn,
                ))
            }

            fn read_section(
                file: &FitsFile,
                hdu: &FitsHdu,
                range: std::ops::Range<usize>,
            ) -> Result<Vec<Self>> {
                let idx = validate_hdu_index(file, hdu)?;
                let parsed = file.parsed()?;
                let core_hdu = &parsed.hdus[idx];
                let count = range.end.saturating_sub(range.start);
                let img =
                    crate::image::read_image_section(file.data(), core_hdu, range.start, count)?;
                Ok(extract_from_image_data(
                    &img, $u8_fn, $i16_fn, $i32_fn, $i64_fn, $f32_fn, $f64_fn,
                ))
            }

            fn read_rows(
                file: &FitsFile,
                hdu: &FitsHdu,
                start_row: usize,
                num_rows: usize,
            ) -> Result<Vec<Self>> {
                let idx = validate_hdu_index(file, hdu)?;
                let parsed = file.parsed()?;
                let core_hdu = &parsed.hdus[idx];
                let img =
                    crate::image::read_image_rows(file.data(), core_hdu, start_row, num_rows)?;
                Ok(extract_from_image_data(
                    &img, $u8_fn, $i16_fn, $i32_fn, $i64_fn, $f32_fn, $f64_fn,
                ))
            }

            fn read_region(
                file: &FitsFile,
                hdu: &FitsHdu,
                ranges: &[std::ops::Range<usize>],
            ) -> Result<Vec<Self>> {
                let idx = validate_hdu_index(file, hdu)?;
                let parsed = file.parsed()?;
                let core_hdu = &parsed.hdus[idx];
                let tuples = ranges_to_tuples(ranges);
                let img = crate::image::read_image_region(file.data(), core_hdu, &tuples)?;
                Ok(extract_from_image_data(
                    &img, $u8_fn, $i16_fn, $i32_fn, $i64_fn, $f32_fn, $f64_fn,
                ))
            }
        }
    };
}

impl_read_image!(
    u8,
    |v: &[u8]| v.to_vec(),
    |v: &[i16]| v.iter().map(|&x| x as u8).collect(),
    |v: &[i32]| v.iter().map(|&x| x as u8).collect(),
    |v: &[i64]| v.iter().map(|&x| x as u8).collect(),
    |v: &[f32]| v.iter().map(|&x| x as u8).collect(),
    |v: &[f64]| v.iter().map(|&x| x as u8).collect()
);

impl_read_image!(
    i16,
    |v: &[u8]| v.iter().map(|&x| x as i16).collect(),
    |v: &[i16]| v.to_vec(),
    |v: &[i32]| v.iter().map(|&x| x as i16).collect(),
    |v: &[i64]| v.iter().map(|&x| x as i16).collect(),
    |v: &[f32]| v.iter().map(|&x| x as i16).collect(),
    |v: &[f64]| v.iter().map(|&x| x as i16).collect()
);

impl_read_image!(
    i32,
    |v: &[u8]| v.iter().map(|&x| x as i32).collect(),
    |v: &[i16]| v.iter().map(|&x| x as i32).collect(),
    |v: &[i32]| v.to_vec(),
    |v: &[i64]| v.iter().map(|&x| x as i32).collect(),
    |v: &[f32]| v.iter().map(|&x| x as i32).collect(),
    |v: &[f64]| v.iter().map(|&x| x as i32).collect()
);

impl_read_image!(
    i64,
    |v: &[u8]| v.iter().map(|&x| x as i64).collect(),
    |v: &[i16]| v.iter().map(|&x| x as i64).collect(),
    |v: &[i32]| v.iter().map(|&x| x as i64).collect(),
    |v: &[i64]| v.to_vec(),
    |v: &[f32]| v.iter().map(|&x| x as i64).collect(),
    |v: &[f64]| v.iter().map(|&x| x as i64).collect()
);

impl_read_image!(
    f32,
    |v: &[u8]| v.iter().map(|&x| x as f32).collect(),
    |v: &[i16]| v.iter().map(|&x| x as f32).collect(),
    |v: &[i32]| v.iter().map(|&x| x as f32).collect(),
    |v: &[i64]| v.iter().map(|&x| x as f32).collect(),
    |v: &[f32]| v.to_vec(),
    |v: &[f64]| v.iter().map(|&x| x as f32).collect()
);

impl_read_image!(
    f64,
    |v: &[u8]| v.iter().map(|&x| x as f64).collect(),
    |v: &[i16]| v.iter().map(|&x| x as f64).collect(),
    |v: &[i32]| v.iter().map(|&x| x as f64).collect(),
    |v: &[i64]| v.iter().map(|&x| x as f64).collect(),
    |v: &[f32]| v.iter().map(|&x| x as f64).collect(),
    |v: &[f64]| v.to_vec()
);

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
        let raw: Vec<i16> = i16::read_image(&f, &hdu).unwrap();
        assert_eq!(raw, vec![-32768, 0, 32767]);
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
