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
    Long,
    LongLong,
    Float,
    Double,
}

impl ImageType {
    /// Convert to the FITS BITPIX value.
    pub fn to_bitpix(self) -> i64 {
        match self {
            ImageType::UnsignedByte => 8,
            ImageType::Short => 16,
            ImageType::Long => 32,
            ImageType::LongLong => 64,
            ImageType::Float => -32,
            ImageType::Double => -64,
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

macro_rules! impl_write_image {
    ($t:ty, $bitpix:expr, $serialize_fn:path) => {
        impl WriteImage for $t {
            fn write_image(file: &mut FitsFile, hdu: &FitsHdu, data: &[Self]) -> Result<()> {
                // Read HDU metadata from cache before mutating
                let (header_end, data_start, padded_data_len, file_len) = {
                    let parsed = file.parsed()?;
                    let core_hdu =
                        parsed
                            .hdus
                            .get(hdu.hdu_index)
                            .ok_or(Error::Message(format!(
                                "HDU index {} out of range",
                                hdu.hdu_index
                            )))?;
                    let padded = crate::block::padded_byte_len(core_hdu.data_len);
                    (
                        core_hdu.data_start,
                        core_hdu.data_start,
                        padded,
                        file.data().len(),
                    )
                };

                let serialized = $serialize_fn(data);

                let next_hdu_start = data_start + padded_data_len;
                let tail_len = if next_hdu_start < file_len {
                    file_len - next_hdu_start
                } else {
                    0
                };

                let mut new_data = Vec::with_capacity(header_end + serialized.len() + tail_len);
                new_data.extend_from_slice(&file.data()[..header_end]);
                new_data.extend_from_slice(&serialized);
                if tail_len > 0 {
                    new_data.extend_from_slice(&file.data()[next_hdu_start..]);
                }

                file.set_data(new_data);
                Ok(())
            }
        }
    };
}

impl_write_image!(u8, 8, crate::image::serialize_image_u8);
impl_write_image!(i16, 16, crate::image::serialize_image_i16);
impl_write_image!(i32, 32, crate::image::serialize_image_i32);
impl_write_image!(i64, 64, crate::image::serialize_image_i64);
impl_write_image!(f32, -32, crate::image::serialize_image_f32);
impl_write_image!(f64, -64, crate::image::serialize_image_f64);

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
}
