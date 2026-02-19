//! Image data reading for FITS HDUs.
//!
//! Provides functions to extract image pixel data from FITS byte streams,
//! with support for all standard BITPIX types and BSCALE/BZERO calibration.

use alloc::vec;
use alloc::vec::Vec;

use bytemuck::pod_collect_to_vec;

use crate::block::padded_byte_len;
use crate::endian::{
    buf_f32_native_to_be, buf_f64_native_to_be, buf_i16_native_to_be, buf_i32_native_to_be,
    buf_i64_native_to_be,
};
use crate::error::{Error, Result};
use crate::hdu::{Hdu, HduInfo};
use crate::header::{serialize_header, Card};
use crate::primary::build_primary_header;
use crate::value::Value;

/// Image pixel data extracted from a FITS HDU, typed by BITPIX.
#[derive(Debug, Clone, PartialEq)]
pub enum ImageData {
    U8(Vec<u8>),
    I16(Vec<i16>),
    I32(Vec<i32>),
    I64(Vec<i64>),
    F32(Vec<f32>),
    F64(Vec<f64>),
}

/// Returns the image dimensions (NAXESn values) from an HDU.
///
/// Returns an error if the HDU is not a Primary or Image HDU.
pub fn image_dimensions(hdu: &Hdu) -> Result<Vec<usize>> {
    match &hdu.info {
        HduInfo::Primary { naxes, .. } => Ok(naxes.clone()),
        HduInfo::Image { naxes, .. } => Ok(naxes.clone()),
        HduInfo::CompressedImage { znaxes, .. } => Ok(znaxes.clone()),
        _ => Err(Error::InvalidHeader("not an image HDU")),
    }
}

/// Extracts the BITPIX value from an HDU info, returning an error for
/// non-image HDU types.
fn hdu_bitpix(hdu: &Hdu) -> Result<i64> {
    match &hdu.info {
        HduInfo::Primary { bitpix, .. } | HduInfo::Image { bitpix, .. } => Ok(*bitpix),
        HduInfo::CompressedImage { zbitpix, .. } => Ok(*zbitpix),
        _ => Err(Error::InvalidHeader("not an image HDU")),
    }
}

/// Read raw image pixel data from a FITS byte stream for the given HDU.
///
/// Converts big-endian on-disk bytes to native-endian typed arrays.
/// Returns an `ImageData` enum variant matching the BITPIX type.
pub fn read_image_data(fits_data: &[u8], hdu: &Hdu) -> Result<ImageData> {
    if matches!(&hdu.info, HduInfo::CompressedImage { .. }) {
        return crate::tiled::read_tiled_image(fits_data, hdu);
    }
    let bitpix = hdu_bitpix(hdu)?;
    let data_len = hdu.data_len;

    if data_len == 0 {
        return match bitpix {
            8 => Ok(ImageData::U8(Vec::new())),
            16 => Ok(ImageData::I16(Vec::new())),
            32 => Ok(ImageData::I32(Vec::new())),
            64 => Ok(ImageData::I64(Vec::new())),
            -32 => Ok(ImageData::F32(Vec::new())),
            -64 => Ok(ImageData::F64(Vec::new())),
            other => Err(Error::InvalidBitpix(other)),
        };
    }

    let end = hdu.data_start + data_len;
    if end > fits_data.len() {
        return Err(Error::UnexpectedEof);
    }

    let raw = &fits_data[hdu.data_start..end];

    match bitpix {
        8 => Ok(ImageData::U8(raw.to_vec())),
        16 => {
            // Interpret big-endian bytes as i16, collect into properly-aligned Vec<i16>,
            // then swap each element to native endianness in place.
            let mut pixels: Vec<i16> = pod_collect_to_vec(raw);
            for v in &mut pixels {
                *v = i16::from_be(*v);
            }
            Ok(ImageData::I16(pixels))
        }
        32 => {
            let mut pixels: Vec<i32> = pod_collect_to_vec(raw);
            for v in &mut pixels {
                *v = i32::from_be(*v);
            }
            Ok(ImageData::I32(pixels))
        }
        64 => {
            let mut pixels: Vec<i64> = pod_collect_to_vec(raw);
            for v in &mut pixels {
                *v = i64::from_be(*v);
            }
            Ok(ImageData::I64(pixels))
        }
        -32 => {
            let mut pixels: Vec<f32> = pod_collect_to_vec(raw);
            for v in &mut pixels {
                *v = f32::from_bits(u32::from_be(v.to_bits()));
            }
            Ok(ImageData::F32(pixels))
        }
        -64 => {
            let mut pixels: Vec<f64> = pod_collect_to_vec(raw);
            for v in &mut pixels {
                *v = f64::from_bits(u64::from_be(v.to_bits()));
            }
            Ok(ImageData::F64(pixels))
        }
        other => Err(Error::InvalidBitpix(other)),
    }
}

/// Read image pixel data into a pre-allocated `f32` buffer.
///
/// The buffer must have exactly the right number of elements for the image.
/// Data is converted from the on-disk BITPIX type to f32.
pub fn read_image_data_into_f32(fits_data: &[u8], hdu: &Hdu, buf: &mut [f32]) -> Result<()> {
    let bitpix = hdu_bitpix(hdu)?;
    let bpp = bytes_per_pixel(bitpix)?;
    let data_len = hdu.data_len;
    let npixels = if bpp > 0 { data_len / bpp } else { 0 };

    if buf.len() != npixels {
        return Err(Error::InvalidValue);
    }

    if npixels == 0 {
        return Ok(());
    }

    let end = hdu.data_start + data_len;
    if end > fits_data.len() {
        return Err(Error::UnexpectedEof);
    }
    let raw = &fits_data[hdu.data_start..end];

    match bitpix {
        8 => {
            for (i, &b) in raw.iter().enumerate() {
                buf[i] = b as f32;
            }
        }
        16 => {
            for i in 0..npixels {
                buf[i] = crate::endian::read_i16_be(&raw[i * 2..]) as f32;
            }
        }
        32 => {
            for i in 0..npixels {
                buf[i] = crate::endian::read_i32_be(&raw[i * 4..]) as f32;
            }
        }
        64 => {
            for i in 0..npixels {
                buf[i] = crate::endian::read_i64_be(&raw[i * 8..]) as f32;
            }
        }
        -32 => {
            for i in 0..npixels {
                buf[i] = crate::endian::read_f32_be(&raw[i * 4..]);
            }
        }
        -64 => {
            for i in 0..npixels {
                buf[i] = crate::endian::read_f64_be(&raw[i * 8..]) as f32;
            }
        }
        other => return Err(Error::InvalidBitpix(other)),
    }
    Ok(())
}

/// Read image pixel data into a pre-allocated `f64` buffer.
///
/// The buffer must have exactly the right number of elements for the image.
/// Data is converted from the on-disk BITPIX type to f64.
pub fn read_image_data_into_f64(fits_data: &[u8], hdu: &Hdu, buf: &mut [f64]) -> Result<()> {
    let bitpix = hdu_bitpix(hdu)?;
    let bpp = bytes_per_pixel(bitpix)?;
    let data_len = hdu.data_len;
    let npixels = if bpp > 0 { data_len / bpp } else { 0 };

    if buf.len() != npixels {
        return Err(Error::InvalidValue);
    }

    if npixels == 0 {
        return Ok(());
    }

    let end = hdu.data_start + data_len;
    if end > fits_data.len() {
        return Err(Error::UnexpectedEof);
    }
    let raw = &fits_data[hdu.data_start..end];

    match bitpix {
        8 => {
            for (i, &b) in raw.iter().enumerate() {
                buf[i] = b as f64;
            }
        }
        16 => {
            for i in 0..npixels {
                buf[i] = crate::endian::read_i16_be(&raw[i * 2..]) as f64;
            }
        }
        32 => {
            for i in 0..npixels {
                buf[i] = crate::endian::read_i32_be(&raw[i * 4..]) as f64;
            }
        }
        64 => {
            for i in 0..npixels {
                buf[i] = crate::endian::read_i64_be(&raw[i * 8..]) as f64;
            }
        }
        -32 => {
            for i in 0..npixels {
                buf[i] = crate::endian::read_f32_be(&raw[i * 4..]) as f64;
            }
        }
        -64 => {
            for i in 0..npixels {
                buf[i] = crate::endian::read_f64_be(&raw[i * 8..]);
            }
        }
        other => return Err(Error::InvalidBitpix(other)),
    }
    Ok(())
}

/// Apply BSCALE/BZERO calibration to raw image data.
///
/// Computes `physical = bzero + bscale * pixel` for every pixel and returns
/// the results as `Vec<f64>`.
pub fn apply_bscale_bzero(data: &ImageData, bscale: f64, bzero: f64) -> Vec<f64> {
    match data {
        ImageData::U8(v) => v.iter().map(|&p| bzero + bscale * (p as f64)).collect(),
        ImageData::I16(v) => v.iter().map(|&p| bzero + bscale * (p as f64)).collect(),
        ImageData::I32(v) => v.iter().map(|&p| bzero + bscale * (p as f64)).collect(),
        ImageData::I64(v) => v.iter().map(|&p| bzero + bscale * (p as f64)).collect(),
        ImageData::F32(v) => v.iter().map(|&p| bzero + bscale * (p as f64)).collect(),
        ImageData::F64(v) => v.iter().map(|&p| bzero + bscale * p).collect(),
    }
}

/// Extract BSCALE and BZERO values from a slice of header cards.
///
/// Returns `(bscale, bzero)`. Defaults to `(1.0, 0.0)` if the keywords
/// are not present.
pub fn extract_bscale_bzero(cards: &[Card]) -> (f64, f64) {
    let bscale = find_float_keyword(cards, "BSCALE").unwrap_or(1.0);
    let bzero = find_float_keyword(cards, "BZERO").unwrap_or(0.0);
    (bscale, bzero)
}

/// Extract the BLANK keyword value from header cards.
///
/// The BLANK keyword defines the integer value used to represent undefined
/// pixels in images with positive BITPIX (8, 16, 32, 64). Returns `None`
/// if the keyword is not present.
pub fn extract_blank(cards: &[Card]) -> Option<i64> {
    find_integer_keyword(cards, "BLANK")
}

/// Find an integer-valued keyword in the card list.
fn find_integer_keyword(cards: &[Card], keyword: &str) -> Option<i64> {
    cards.iter().find_map(|c| {
        if c.keyword_str() == keyword {
            match &c.value {
                Some(Value::Integer(n)) => Some(*n),
                _ => None,
            }
        } else {
            None
        }
    })
}

/// Create a boolean mask indicating which pixels are undefined (BLANK).
///
/// Returns a `Vec<bool>` with the same length as the pixel array, where
/// `true` means the pixel is undefined. For floating-point types, NaN
/// pixels are marked as undefined regardless of the BLANK keyword.
///
/// Returns `None` if the image data is empty or no BLANK/NaN values exist.
pub fn blank_mask(data: &ImageData, blank: Option<i64>) -> Option<Vec<bool>> {
    let mask: Vec<bool> = match data {
        ImageData::U8(v) => match blank {
            Some(b) => {
                let bv = b as u8;
                v.iter().map(|&p| p == bv).collect()
            }
            None => return None,
        },
        ImageData::I16(v) => match blank {
            Some(b) => {
                let bv = b as i16;
                v.iter().map(|&p| p == bv).collect()
            }
            None => return None,
        },
        ImageData::I32(v) => match blank {
            Some(b) => {
                let bv = b as i32;
                v.iter().map(|&p| p == bv).collect()
            }
            None => return None,
        },
        ImageData::I64(v) => match blank {
            Some(b) => v.iter().map(|&p| p == b).collect(),
            None => return None,
        },
        ImageData::F32(v) => v.iter().map(|p| p.is_nan()).collect(),
        ImageData::F64(v) => v.iter().map(|p| p.is_nan()).collect(),
    };
    if mask.iter().any(|&b| b) {
        Some(mask)
    } else {
        None
    }
}

/// Find a float-valued keyword in the card list, accepting both Float and
/// Integer values (integers are promoted to f64).
fn find_float_keyword(cards: &[Card], keyword: &str) -> Option<f64> {
    cards.iter().find_map(|c| {
        if c.keyword_str() == keyword {
            match &c.value {
                Some(Value::Float(f)) => Some(*f),
                Some(Value::Integer(n)) => Some(*n as f64),
                _ => None,
            }
        } else {
            None
        }
    })
}

/// Read image data with BSCALE/BZERO calibration applied.
///
/// Reads raw pixel data from the HDU, extracts BSCALE and BZERO from the
/// header cards, and returns calibrated physical values as `Vec<f64>`.
/// Pixels matching the BLANK keyword value are set to NaN.
pub fn read_image_physical(fits_data: &[u8], hdu: &Hdu) -> Result<Vec<f64>> {
    let raw = read_image_data(fits_data, hdu)?;
    let (bscale, bzero) = extract_bscale_bzero(&hdu.cards);
    let blank = extract_blank(&hdu.cards);
    let mut physical = apply_bscale_bzero(&raw, bscale, bzero);
    if let Some(mask) = blank_mask(&raw, blank) {
        for (val, is_blank) in physical.iter_mut().zip(mask.iter()) {
            if *is_blank {
                *val = f64::NAN;
            }
        }
    }
    Ok(physical)
}

// ---- Image write functions ----

/// Serialize a slice of `u8` pixel values into a block-padded FITS data segment.
pub fn serialize_image_u8(pixels: &[u8]) -> Vec<u8> {
    let raw_len = pixels.len();
    let padded_len = padded_byte_len(raw_len);
    let mut buf = vec![0u8; padded_len];
    buf[..raw_len].copy_from_slice(pixels);
    buf
}

/// Serialize a slice of `i16` pixel values into big-endian, block-padded FITS data.
pub fn serialize_image_i16(pixels: &[i16]) -> Vec<u8> {
    let raw_len = pixels.len() * 2;
    let padded_len = padded_byte_len(raw_len);
    // Copy pixel bytes via bytemuck, then swap endianness in-place
    let mut buf: Vec<u8> = pod_collect_to_vec(pixels);
    buf_i16_native_to_be(&mut buf);
    buf.resize(padded_len, 0);
    buf
}

/// Serialize a slice of `i32` pixel values into big-endian, block-padded FITS data.
pub fn serialize_image_i32(pixels: &[i32]) -> Vec<u8> {
    let raw_len = pixels.len() * 4;
    let padded_len = padded_byte_len(raw_len);
    let mut buf: Vec<u8> = pod_collect_to_vec(pixels);
    buf_i32_native_to_be(&mut buf);
    buf.resize(padded_len, 0);
    buf
}

/// Serialize a slice of `i64` pixel values into big-endian, block-padded FITS data.
pub fn serialize_image_i64(pixels: &[i64]) -> Vec<u8> {
    let raw_len = pixels.len() * 8;
    let padded_len = padded_byte_len(raw_len);
    let mut buf: Vec<u8> = pod_collect_to_vec(pixels);
    buf_i64_native_to_be(&mut buf);
    buf.resize(padded_len, 0);
    buf
}

/// Serialize a slice of `f32` pixel values into big-endian, block-padded FITS data.
pub fn serialize_image_f32(pixels: &[f32]) -> Vec<u8> {
    let raw_len = pixels.len() * 4;
    let padded_len = padded_byte_len(raw_len);
    let mut buf: Vec<u8> = pod_collect_to_vec(pixels);
    buf_f32_native_to_be(&mut buf);
    buf.resize(padded_len, 0);
    buf
}

/// Serialize a slice of `f64` pixel values into big-endian, block-padded FITS data.
pub fn serialize_image_f64(pixels: &[f64]) -> Vec<u8> {
    let raw_len = pixels.len() * 8;
    let padded_len = padded_byte_len(raw_len);
    let mut buf: Vec<u8> = pod_collect_to_vec(pixels);
    buf_f64_native_to_be(&mut buf);
    buf.resize(padded_len, 0);
    buf
}

/// Serialize an `ImageData` variant into block-padded FITS data bytes.
pub fn serialize_image(data: &ImageData) -> Vec<u8> {
    match data {
        ImageData::U8(v) => serialize_image_u8(v),
        ImageData::I16(v) => serialize_image_i16(v),
        ImageData::I32(v) => serialize_image_i32(v),
        ImageData::I64(v) => serialize_image_i64(v),
        ImageData::F32(v) => serialize_image_f32(v),
        ImageData::F64(v) => serialize_image_f64(v),
    }
}

/// Build a complete image HDU (header + data) as a byte vector.
pub fn build_image_hdu(bitpix: i64, naxes: &[usize], data: &ImageData) -> Result<Vec<u8>> {
    let cards = build_primary_header(bitpix, naxes)?;
    let header_bytes = serialize_header(&cards)?;
    let data_bytes = serialize_image(data);

    let mut hdu = Vec::with_capacity(header_bytes.len() + data_bytes.len());
    hdu.extend_from_slice(&header_bytes);
    hdu.extend_from_slice(&data_bytes);
    Ok(hdu)
}

/// Reverse BSCALE/BZERO calibration: convert physical f64 values to raw
/// integer values using `raw = (physical - bzero) / bscale`.
///
/// The target BITPIX determines the output type. Values are rounded to the
/// nearest integer and clamped to the valid range for the target type.
pub fn reverse_bscale_bzero(
    physical: &[f64],
    bscale: f64,
    bzero: f64,
    bitpix: i64,
) -> Result<ImageData> {
    let inv = |v: f64| (v - bzero) / bscale;
    let round_clamp = |v: f64, lo: f64, hi: f64| -> f64 {
        let r = libm::round(v);
        if r < lo {
            lo
        } else if r > hi {
            hi
        } else {
            r
        }
    };
    match bitpix {
        8 => {
            let pixels: Vec<u8> = physical
                .iter()
                .map(|&v| round_clamp(inv(v), 0.0, 255.0) as u8)
                .collect();
            Ok(ImageData::U8(pixels))
        }
        16 => {
            let pixels: Vec<i16> = physical
                .iter()
                .map(|&v| round_clamp(inv(v), i16::MIN as f64, i16::MAX as f64) as i16)
                .collect();
            Ok(ImageData::I16(pixels))
        }
        32 => {
            let pixels: Vec<i32> = physical
                .iter()
                .map(|&v| round_clamp(inv(v), i32::MIN as f64, i32::MAX as f64) as i32)
                .collect();
            Ok(ImageData::I32(pixels))
        }
        64 => {
            let pixels: Vec<i64> = physical
                .iter()
                .map(|&v| round_clamp(inv(v), i64::MIN as f64, i64::MAX as f64) as i64)
                .collect();
            Ok(ImageData::I64(pixels))
        }
        -32 => {
            let pixels: Vec<f32> = physical.iter().map(|&v| inv(v) as f32).collect();
            Ok(ImageData::F32(pixels))
        }
        -64 => {
            let pixels: Vec<f64> = physical.iter().map(|&v| inv(v)).collect();
            Ok(ImageData::F64(pixels))
        }
        other => Err(Error::InvalidBitpix(other)),
    }
}

/// Build a complete image HDU with BSCALE/BZERO keywords.
///
/// Takes physical `f64` values, reverse-applies BSCALE/BZERO to produce raw
/// pixel data at the specified BITPIX, and includes the calibration keywords
/// in the header so that readers can recover the physical values.
pub fn build_image_hdu_with_scaling(
    bitpix: i64,
    naxes: &[usize],
    physical: &[f64],
    bscale: f64,
    bzero: f64,
) -> Result<Vec<u8>> {
    let raw = reverse_bscale_bzero(physical, bscale, bzero, bitpix)?;
    let mut cards = build_primary_header(bitpix, naxes)?;

    let is_non_default = bscale != 1.0 || bzero != 0.0;
    if is_non_default {
        fn make_card(keyword: &str, value: Value) -> Card {
            let mut kw = [b' '; 8];
            let bytes = keyword.as_bytes();
            let len = bytes.len().min(8);
            kw[..len].copy_from_slice(&bytes[..len]);
            Card {
                keyword: kw,
                value: Some(value),
                comment: None,
            }
        }
        cards.push(make_card("BSCALE", Value::Float(bscale)));
        cards.push(make_card("BZERO", Value::Float(bzero)));
    }

    let header_bytes = serialize_header(&cards)?;
    let data_bytes = serialize_image(&raw);

    let mut hdu = Vec::with_capacity(header_bytes.len() + data_bytes.len());
    hdu.extend_from_slice(&header_bytes);
    hdu.extend_from_slice(&data_bytes);
    Ok(hdu)
}

// ---- Image region/section/row functions ----

/// Returns the number of bytes per pixel for a given BITPIX value.
pub fn bytes_per_pixel(bitpix: i64) -> Result<usize> {
    match bitpix {
        8 | 16 | 32 | 64 | -32 | -64 => Ok((bitpix.unsigned_abs() / 8) as usize),
        _ => Err(Error::InvalidBitpix(bitpix)),
    }
}

/// Extract the BITPIX value and axis dimensions from an HDU.
fn hdu_bitpix_naxes(hdu: &Hdu) -> Result<(i64, &[usize])> {
    match &hdu.info {
        HduInfo::Primary { bitpix, naxes } | HduInfo::Image { bitpix, naxes } => {
            Ok((*bitpix, naxes))
        }
        HduInfo::CompressedImage {
            zbitpix, znaxes, ..
        } => Ok((*zbitpix, znaxes)),
        _ => Err(Error::InvalidHeader("not an image HDU")),
    }
}

/// Decode a contiguous byte slice into an `ImageData` variant based on BITPIX.
fn decode_pixels(raw: &[u8], bitpix: i64) -> Result<ImageData> {
    bytes_per_pixel(bitpix)?; // validate
    match bitpix {
        8 => Ok(ImageData::U8(raw.to_vec())),
        16 => {
            let mut pixels: Vec<i16> = pod_collect_to_vec(raw);
            for v in &mut pixels {
                *v = i16::from_be(*v);
            }
            Ok(ImageData::I16(pixels))
        }
        32 => {
            let mut pixels: Vec<i32> = pod_collect_to_vec(raw);
            for v in &mut pixels {
                *v = i32::from_be(*v);
            }
            Ok(ImageData::I32(pixels))
        }
        64 => {
            let mut pixels: Vec<i64> = pod_collect_to_vec(raw);
            for v in &mut pixels {
                *v = i64::from_be(*v);
            }
            Ok(ImageData::I64(pixels))
        }
        -32 => {
            let mut pixels: Vec<f32> = pod_collect_to_vec(raw);
            for v in &mut pixels {
                *v = f32::from_bits(u32::from_be(v.to_bits()));
            }
            Ok(ImageData::F32(pixels))
        }
        -64 => {
            let mut pixels: Vec<f64> = pod_collect_to_vec(raw);
            for v in &mut pixels {
                *v = f64::from_bits(u64::from_be(v.to_bits()));
            }
            Ok(ImageData::F64(pixels))
        }
        _ => Err(Error::InvalidBitpix(bitpix)),
    }
}

/// Read a flat range of pixels `[start_pixel..start_pixel+count)` from the image data.
pub fn read_image_section(
    fits_data: &[u8],
    hdu: &Hdu,
    start_pixel: usize,
    count: usize,
) -> Result<ImageData> {
    let (bitpix, naxes) = hdu_bitpix_naxes(hdu)?;
    let bpp = bytes_per_pixel(bitpix)?;

    let total_pixels: usize = if naxes.is_empty() {
        0
    } else {
        naxes.iter().copied().product()
    };

    let end_pixel = start_pixel.checked_add(count).ok_or(Error::InvalidValue)?;
    if end_pixel > total_pixels {
        return Err(Error::UnexpectedEof);
    }

    let byte_offset = hdu.data_start + start_pixel * bpp;
    let byte_end = byte_offset + count * bpp;
    if byte_end > fits_data.len() {
        return Err(Error::UnexpectedEof);
    }

    decode_pixels(&fits_data[byte_offset..byte_end], bitpix)
}

/// Read complete rows `[start_row..start_row+num_rows)` from a 2D+ image.
///
/// Row length is NAXIS1 pixels. Requires `naxis >= 2`.
pub fn read_image_rows(
    fits_data: &[u8],
    hdu: &Hdu,
    start_row: usize,
    num_rows: usize,
) -> Result<ImageData> {
    let (_, naxes) = hdu_bitpix_naxes(hdu)?;
    if naxes.len() < 2 {
        return Err(Error::InvalidHeader(
            "image needs at least 2 axes for row slicing",
        ));
    }

    let row_len = naxes[0];
    let total_rows: usize = naxes[1..].iter().copied().product();

    let end_row = start_row.checked_add(num_rows).ok_or(Error::InvalidValue)?;
    if end_row > total_rows {
        return Err(Error::UnexpectedEof);
    }

    let start_pixel = start_row * row_len;
    let pixel_count = num_rows * row_len;
    read_image_section(fits_data, hdu, start_pixel, pixel_count)
}

/// Read a rectangular sub-region from an image of arbitrary dimensionality.
///
/// `ranges` contains one `(start, end)` pair per axis (end exclusive).
/// Returns pixels in row-major (C) order for the sub-region.
pub fn read_image_region(
    fits_data: &[u8],
    hdu: &Hdu,
    ranges: &[(usize, usize)],
) -> Result<ImageData> {
    let (bitpix, naxes) = hdu_bitpix_naxes(hdu)?;
    let bpp = bytes_per_pixel(bitpix)?;
    let ndim = naxes.len();

    if ranges.len() != ndim {
        return Err(Error::InvalidValue);
    }

    let mut sub_dims = Vec::with_capacity(ndim);
    for (i, &(start, end)) in ranges.iter().enumerate() {
        if start > end || end > naxes[i] {
            return Err(Error::InvalidValue);
        }
        sub_dims.push(end - start);
    }

    let total_out: usize = if sub_dims.is_empty() {
        0
    } else {
        sub_dims.iter().copied().product()
    };

    if total_out == 0 {
        return decode_pixels(&[], bitpix);
    }

    // Axis strides: stride[0]=1, stride[1]=naxes[0], stride[2]=naxes[0]*naxes[1], ...
    let mut strides = Vec::with_capacity(ndim);
    let mut s: usize = 1;
    for &dim in naxes.iter() {
        strides.push(s);
        s *= dim;
    }

    let mut raw = Vec::with_capacity(total_out * bpp);
    let mut idx: Vec<usize> = vec![0; ndim];

    for _ in 0..total_out {
        let mut flat = 0;
        for d in 0..ndim {
            flat += (ranges[d].0 + idx[d]) * strides[d];
        }

        let byte_offset = hdu.data_start + flat * bpp;
        let byte_end = byte_offset + bpp;
        if byte_end > fits_data.len() {
            return Err(Error::UnexpectedEof);
        }
        raw.extend_from_slice(&fits_data[byte_offset..byte_end]);

        // Increment multi-index with first axis varying fastest (Fortran/FITS order).
        let mut carry = true;
        for d in 0..ndim {
            if carry {
                idx[d] += 1;
                if idx[d] < sub_dims[d] {
                    carry = false;
                } else {
                    idx[d] = 0;
                }
            }
        }
    }

    decode_pixels(&raw, bitpix)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::endian::{
        read_f32_be, read_f64_be, read_i16_be, read_i32_be, write_f32_be, write_f64_be,
        write_i16_be, write_i32_be, write_i64_be,
    };
    use crate::header::Card;
    use crate::value::Value;

    /// Pad a keyword name to 8 bytes with trailing spaces.
    fn kw(name: &[u8]) -> [u8; 8] {
        let mut buf = [b' '; 8];
        let len = name.len().min(8);
        buf[..len].copy_from_slice(&name[..len]);
        buf
    }

    fn card(keyword: &str, value: Value) -> Card {
        Card {
            keyword: kw(keyword.as_bytes()),
            value: Some(value),
            comment: None,
        }
    }

    fn primary_header_image(bitpix: i64, dims: &[usize]) -> Vec<Card> {
        let mut cards = vec![
            card("SIMPLE", Value::Logical(true)),
            card("BITPIX", Value::Integer(bitpix)),
            card("NAXIS", Value::Integer(dims.len() as i64)),
        ];
        for (i, &d) in dims.iter().enumerate() {
            let name = alloc::format!("NAXIS{}", i + 1);
            cards.push(card(&name, Value::Integer(d as i64)));
        }
        cards
    }

    fn primary_header_with_bscale(
        bitpix: i64,
        dims: &[usize],
        bscale: f64,
        bzero: f64,
    ) -> Vec<Card> {
        let mut cards = primary_header_image(bitpix, dims);
        cards.push(card("BSCALE", Value::Float(bscale)));
        cards.push(card("BZERO", Value::Float(bzero)));
        cards
    }

    /// Build a FITS byte buffer from header cards and raw data bytes.
    fn build_fits(header_cards: &[Card], data: &[u8]) -> Vec<u8> {
        let header = serialize_header(header_cards).unwrap();
        let padded_data_len = padded_byte_len(data.len());
        let mut result = Vec::with_capacity(header.len() + padded_data_len);
        result.extend_from_slice(&header);
        result.resize(header.len() + padded_data_len, 0u8);
        result[header.len()..header.len() + data.len()].copy_from_slice(data);
        result
    }

    /// Parse primary HDU from a FITS byte buffer.
    fn parse_primary(data: &[u8]) -> Hdu {
        crate::hdu::parse_fits(data)
            .unwrap()
            .hdus
            .into_iter()
            .next()
            .unwrap()
    }

    // ---- BITPIX 8 (u8) ----

    #[test]
    fn read_u8_image() {
        let pixels: Vec<u8> = vec![0, 1, 127, 255];
        let cards = primary_header_image(8, &[4]);
        let fits = build_fits(&cards, &pixels);
        let hdu = parse_primary(&fits);

        let data = read_image_data(&fits, &hdu).unwrap();
        assert_eq!(data, ImageData::U8(vec![0, 1, 127, 255]));
    }

    // ---- BITPIX 16 (i16) ----

    #[test]
    fn read_i16_image() {
        let values: [i16; 4] = [0, 1, -1, i16::MAX];
        let mut raw = vec![0u8; 8];
        for (i, &v) in values.iter().enumerate() {
            write_i16_be(&mut raw[i * 2..], v);
        }

        let cards = primary_header_image(16, &[4]);
        let fits = build_fits(&cards, &raw);
        let hdu = parse_primary(&fits);

        let data = read_image_data(&fits, &hdu).unwrap();
        assert_eq!(data, ImageData::I16(vec![0, 1, -1, i16::MAX]));
    }

    // ---- BITPIX 32 (i32) ----

    #[test]
    fn read_i32_image() {
        let values: [i32; 3] = [0, -42, i32::MAX];
        let mut raw = vec![0u8; 12];
        for (i, &v) in values.iter().enumerate() {
            write_i32_be(&mut raw[i * 4..], v);
        }

        let cards = primary_header_image(32, &[3]);
        let fits = build_fits(&cards, &raw);
        let hdu = parse_primary(&fits);

        let data = read_image_data(&fits, &hdu).unwrap();
        assert_eq!(data, ImageData::I32(vec![0, -42, i32::MAX]));
    }

    // ---- BITPIX 64 (i64) ----

    #[test]
    fn read_i64_image() {
        let values: [i64; 2] = [i64::MIN, i64::MAX];
        let mut raw = vec![0u8; 16];
        for (i, &v) in values.iter().enumerate() {
            write_i64_be(&mut raw[i * 8..], v);
        }

        let cards = primary_header_image(64, &[2]);
        let fits = build_fits(&cards, &raw);
        let hdu = parse_primary(&fits);

        let data = read_image_data(&fits, &hdu).unwrap();
        assert_eq!(data, ImageData::I64(vec![i64::MIN, i64::MAX]));
    }

    // ---- BITPIX -32 (f32) ----

    #[test]
    fn read_f32_image() {
        let values: [f32; 3] = [0.0, 1.5, -42.25];
        let mut raw = vec![0u8; 12];
        for (i, &v) in values.iter().enumerate() {
            write_f32_be(&mut raw[i * 4..], v);
        }

        let cards = primary_header_image(-32, &[3]);
        let fits = build_fits(&cards, &raw);
        let hdu = parse_primary(&fits);

        let data = read_image_data(&fits, &hdu).unwrap();
        assert_eq!(data, ImageData::F32(vec![0.0, 1.5, -42.25]));
    }

    // ---- BITPIX -64 (f64) ----

    #[test]
    fn read_f64_image() {
        let values: [f64; 2] = [core::f64::consts::FRAC_1_SQRT_2, -1e100];
        let mut raw = vec![0u8; 16];
        for (i, &v) in values.iter().enumerate() {
            write_f64_be(&mut raw[i * 8..], v);
        }

        let cards = primary_header_image(-64, &[2]);
        let fits = build_fits(&cards, &raw);
        let hdu = parse_primary(&fits);

        let data = read_image_data(&fits, &hdu).unwrap();
        assert_eq!(
            data,
            ImageData::F64(vec![core::f64::consts::FRAC_1_SQRT_2, -1e100])
        );
    }

    // ---- Zero-size image ----

    #[test]
    fn read_zero_size_image() {
        let cards = primary_header_image(16, &[]);
        let fits = build_fits(&cards, &[]);
        let hdu = parse_primary(&fits);

        let data = read_image_data(&fits, &hdu).unwrap();
        assert_eq!(data, ImageData::I16(Vec::new()));
    }

    // ---- Single pixel ----

    #[test]
    fn read_single_pixel_u8() {
        let cards = primary_header_image(8, &[1]);
        let fits = build_fits(&cards, &[42]);
        let hdu = parse_primary(&fits);

        let data = read_image_data(&fits, &hdu).unwrap();
        assert_eq!(data, ImageData::U8(vec![42]));
    }

    #[test]
    fn read_single_pixel_f32() {
        let mut raw = vec![0u8; 4];
        write_f32_be(&mut raw, 2.5);

        let cards = primary_header_image(-32, &[1]);
        let fits = build_fits(&cards, &raw);
        let hdu = parse_primary(&fits);

        let data = read_image_data(&fits, &hdu).unwrap();
        assert_eq!(data, ImageData::F32(vec![2.5]));
    }

    // ---- 2D image ----

    #[test]
    fn read_2d_i16_image() {
        let width = 3;
        let height = 2;
        let pixel_count = width * height;
        let mut raw = vec![0u8; pixel_count * 2];
        for i in 0..pixel_count {
            write_i16_be(&mut raw[i * 2..], (i + 1) as i16);
        }

        let cards = primary_header_image(16, &[width, height]);
        let fits = build_fits(&cards, &raw);
        let hdu = parse_primary(&fits);

        let data = read_image_data(&fits, &hdu).unwrap();
        assert_eq!(data, ImageData::I16(vec![1, 2, 3, 4, 5, 6]));
    }

    // ---- BSCALE/BZERO ----

    #[test]
    fn apply_bscale_bzero_identity() {
        let data = ImageData::U8(vec![10, 20, 30]);
        let result = apply_bscale_bzero(&data, 1.0, 0.0);
        assert_eq!(result, vec![10.0, 20.0, 30.0]);
    }

    #[test]
    fn apply_bscale_bzero_scale_and_offset() {
        let data = ImageData::I16(vec![0, 1, 2, 3]);
        let result = apply_bscale_bzero(&data, 2.0, 100.0);
        assert_eq!(result, vec![100.0, 102.0, 104.0, 106.0]);
    }

    #[test]
    fn apply_bscale_bzero_unsigned_16bit() {
        // Common pattern: BITPIX=16 with BZERO=32768 to represent unsigned 16-bit
        let data = ImageData::I16(vec![0, -32768, 32767]);
        let result = apply_bscale_bzero(&data, 1.0, 32768.0);
        assert_eq!(result, vec![32768.0, 0.0, 65535.0]);
    }

    #[test]
    fn apply_bscale_bzero_f64() {
        let data = ImageData::F64(vec![1.0, 2.0]);
        let result = apply_bscale_bzero(&data, 0.5, 10.0);
        assert_eq!(result, vec![10.5, 11.0]);
    }

    // ---- extract_bscale_bzero ----

    #[test]
    fn extract_bscale_bzero_defaults() {
        let cards: Vec<Card> = vec![card("BITPIX", Value::Integer(16))];
        let (bscale, bzero) = extract_bscale_bzero(&cards);
        assert_eq!(bscale, 1.0);
        assert_eq!(bzero, 0.0);
    }

    #[test]
    fn extract_bscale_bzero_present() {
        let cards = vec![
            card("BSCALE", Value::Float(2.5)),
            card("BZERO", Value::Float(100.0)),
        ];
        let (bscale, bzero) = extract_bscale_bzero(&cards);
        assert_eq!(bscale, 2.5);
        assert_eq!(bzero, 100.0);
    }

    #[test]
    fn extract_bscale_bzero_integer_values() {
        let cards = vec![
            card("BSCALE", Value::Integer(3)),
            card("BZERO", Value::Integer(32768)),
        ];
        let (bscale, bzero) = extract_bscale_bzero(&cards);
        assert_eq!(bscale, 3.0);
        assert_eq!(bzero, 32768.0);
    }

    // ---- read_image_physical ----

    #[test]
    fn read_image_physical_with_calibration() {
        let values: [i16; 3] = [0, 1, 2];
        let mut raw = vec![0u8; 6];
        for (i, &v) in values.iter().enumerate() {
            write_i16_be(&mut raw[i * 2..], v);
        }

        let cards = primary_header_with_bscale(16, &[3], 2.0, 100.0);
        let fits = build_fits(&cards, &raw);
        let hdu = parse_primary(&fits);

        let physical = read_image_physical(&fits, &hdu).unwrap();
        assert_eq!(physical, vec![100.0, 102.0, 104.0]);
    }

    #[test]
    fn read_image_physical_no_calibration() {
        let pixels: Vec<u8> = vec![10, 20];
        let cards = primary_header_image(8, &[2]);
        let fits = build_fits(&cards, &pixels);
        let hdu = parse_primary(&fits);

        let physical = read_image_physical(&fits, &hdu).unwrap();
        assert_eq!(physical, vec![10.0, 20.0]);
    }

    // ---- image_dimensions ----

    #[test]
    fn image_dimensions_2d() {
        let cards = primary_header_image(16, &[100, 200]);
        let fits = build_fits(&cards, &vec![0u8; 100 * 200 * 2]);
        let hdu = parse_primary(&fits);

        let dims = image_dimensions(&hdu).unwrap();
        assert_eq!(dims, vec![100, 200]);
    }

    #[test]
    fn image_dimensions_0d() {
        let cards = primary_header_image(8, &[]);
        let fits = build_fits(&cards, &[]);
        let hdu = parse_primary(&fits);

        let dims = image_dimensions(&hdu).unwrap();
        assert!(dims.is_empty());
    }

    // ---- Invalid BITPIX ----

    #[test]
    fn read_image_data_invalid_bitpix() {
        // Build an HDU with a fake invalid bitpix by constructing one manually
        let hdu = Hdu {
            info: HduInfo::Primary {
                bitpix: 7,
                naxes: vec![10],
            },
            header_start: 0,
            data_start: 2880,
            data_len: 10,
            cards: vec![],
        };
        let fits = vec![0u8; 5760];
        let result = read_image_data(&fits, &hdu);
        assert!(result.is_err());
    }

    // ---- Truncated data ----

    #[test]
    fn read_image_data_truncated() {
        let hdu = Hdu {
            info: HduInfo::Primary {
                bitpix: 8,
                naxes: vec![100],
            },
            header_start: 0,
            data_start: 2880,
            data_len: 100,
            cards: vec![],
        };
        // Provide a buffer that is too small
        let fits = vec![0u8; 2900];
        let result = read_image_data(&fits, &hdu);
        assert!(result.is_err());
    }

    // ---- Non-image HDU ----

    #[test]
    fn image_dimensions_non_image_hdu() {
        let hdu = Hdu {
            info: HduInfo::AsciiTable {
                naxis1: 100,
                naxis2: 50,
                tfields: 5,
            },
            header_start: 0,
            data_start: 2880,
            data_len: 5000,
            cards: vec![],
        };
        assert!(image_dimensions(&hdu).is_err());
    }

    // ---- 3D cube ----

    #[test]
    fn read_3d_f32_cube() {
        let dims = [2, 2, 2];
        let pixel_count: usize = dims.iter().product();
        let mut raw = vec![0u8; pixel_count * 4];
        for i in 0..pixel_count {
            write_f32_be(&mut raw[i * 4..], (i as f32) + 0.5);
        }

        let cards = primary_header_image(-32, &dims);
        let fits = build_fits(&cards, &raw);
        let hdu = parse_primary(&fits);

        let data = read_image_data(&fits, &hdu).unwrap();
        let expected: Vec<f32> = (0..pixel_count).map(|i| (i as f32) + 0.5).collect();
        assert_eq!(data, ImageData::F32(expected));
    }

    // ---- Empty zero-length BITPIX variants ----

    #[test]
    fn zero_length_all_bitpix_types() {
        for &bitpix in &[8i64, 16, 32, 64, -32, -64] {
            let cards = primary_header_image(bitpix, &[]);
            let fits = build_fits(&cards, &[]);
            let hdu = parse_primary(&fits);

            let data = read_image_data(&fits, &hdu).unwrap();
            match (bitpix, &data) {
                (8, ImageData::U8(v)) => assert!(v.is_empty()),
                (16, ImageData::I16(v)) => assert!(v.is_empty()),
                (32, ImageData::I32(v)) => assert!(v.is_empty()),
                (64, ImageData::I64(v)) => assert!(v.is_empty()),
                (-32, ImageData::F32(v)) => assert!(v.is_empty()),
                (-64, ImageData::F64(v)) => assert!(v.is_empty()),
                _ => panic!("Unexpected variant for bitpix={}", bitpix),
            }
        }
    }

    // ---- Write tests ----

    #[test]
    fn serialize_u8_roundtrip() {
        let pixels: Vec<u8> = (0..=255).collect();
        let bytes = serialize_image_u8(&pixels);
        assert_eq!(&bytes[..256], &pixels[..]);
    }

    #[test]
    fn serialize_u8_padding() {
        let pixels = vec![42u8; 100];
        let bytes = serialize_image_u8(&pixels);
        assert_eq!(bytes.len(), crate::block::BLOCK_SIZE);
        assert_eq!(&bytes[..100], &pixels[..]);
        for &b in &bytes[100..] {
            assert_eq!(b, 0);
        }
    }

    #[test]
    fn serialize_i16_roundtrip() {
        let pixels: Vec<i16> = vec![0, 1, -1, i16::MIN, i16::MAX, 256, -256];
        let bytes = serialize_image_i16(&pixels);
        for (i, &expected) in pixels.iter().enumerate() {
            let actual = read_i16_be(&bytes[i * 2..]);
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn serialize_i32_roundtrip() {
        let pixels: Vec<i32> = vec![0, 1, -1, i32::MIN, i32::MAX];
        let bytes = serialize_image_i32(&pixels);
        for (i, &expected) in pixels.iter().enumerate() {
            let actual = read_i32_be(&bytes[i * 4..]);
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn serialize_f32_roundtrip() {
        let pixels: Vec<f32> = vec![0.0, 1.0, -1.0, f32::MAX, core::f32::consts::PI];
        let bytes = serialize_image_f32(&pixels);
        for (i, &expected) in pixels.iter().enumerate() {
            let actual = read_f32_be(&bytes[i * 4..]);
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn serialize_f64_roundtrip() {
        let pixels: Vec<f64> = vec![0.0, 1.0, -1.0, f64::MAX, core::f64::consts::PI];
        let bytes = serialize_image_f64(&pixels);
        for (i, &expected) in pixels.iter().enumerate() {
            let actual = read_f64_be(&bytes[i * 8..]);
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn serialize_empty_images() {
        assert!(serialize_image_u8(&[]).is_empty());
        assert!(serialize_image_i16(&[]).is_empty());
        assert!(serialize_image_i32(&[]).is_empty());
        assert!(serialize_image_i64(&[]).is_empty());
        assert!(serialize_image_f32(&[]).is_empty());
        assert!(serialize_image_f64(&[]).is_empty());
    }

    #[test]
    fn all_serializers_block_aligned() {
        assert_eq!(
            serialize_image_u8(&[1; 100]).len() % crate::block::BLOCK_SIZE,
            0
        );
        assert_eq!(
            serialize_image_i16(&[1; 100]).len() % crate::block::BLOCK_SIZE,
            0
        );
        assert_eq!(
            serialize_image_i32(&[1; 100]).len() % crate::block::BLOCK_SIZE,
            0
        );
        assert_eq!(
            serialize_image_i64(&[1; 100]).len() % crate::block::BLOCK_SIZE,
            0
        );
        assert_eq!(
            serialize_image_f32(&[1.0; 100]).len() % crate::block::BLOCK_SIZE,
            0
        );
        assert_eq!(
            serialize_image_f64(&[1.0; 100]).len() % crate::block::BLOCK_SIZE,
            0
        );
    }

    #[test]
    fn build_image_hdu_block_aligned() {
        let data = ImageData::U8(vec![1; 100]);
        let hdu = build_image_hdu(8, &[100], &data).unwrap();
        assert_eq!(hdu.len() % crate::block::BLOCK_SIZE, 0);
    }

    #[test]
    fn build_image_hdu_invalid_bitpix() {
        let data = ImageData::U8(vec![1]);
        assert!(build_image_hdu(12, &[1], &data).is_err());
    }

    // ---- Region/section/row tests ----

    fn build_i16_image_fits(cols: usize, rows: usize) -> (Vec<u8>, Vec<i16>) {
        let cards = crate::primary::build_primary_header(16, &[cols, rows]).unwrap();
        let n = cols * rows;
        let mut raw = vec![0u8; n * 2];
        let mut expected = Vec::with_capacity(n);
        for i in 0..n {
            let val = i as i16;
            write_i16_be(&mut raw[i * 2..], val);
            expected.push(val);
        }
        (build_fits(&cards, &raw), expected)
    }

    fn build_i16_cube_fits(nx: usize, ny: usize, nz: usize) -> (Vec<u8>, Vec<i16>) {
        let cards = crate::primary::build_primary_header(16, &[nx, ny, nz]).unwrap();
        let n = nx * ny * nz;
        let mut raw = vec![0u8; n * 2];
        let mut expected = Vec::with_capacity(n);
        for i in 0..n {
            let val = i as i16;
            write_i16_be(&mut raw[i * 2..], val);
            expected.push(val);
        }
        (build_fits(&cards, &raw), expected)
    }

    #[test]
    fn bpp_valid_values() {
        assert_eq!(bytes_per_pixel(8).unwrap(), 1);
        assert_eq!(bytes_per_pixel(16).unwrap(), 2);
        assert_eq!(bytes_per_pixel(32).unwrap(), 4);
        assert_eq!(bytes_per_pixel(64).unwrap(), 8);
        assert_eq!(bytes_per_pixel(-32).unwrap(), 4);
        assert_eq!(bytes_per_pixel(-64).unwrap(), 8);
    }

    #[test]
    fn bpp_invalid() {
        assert!(bytes_per_pixel(0).is_err());
        assert!(bytes_per_pixel(7).is_err());
    }

    #[test]
    fn section_full_image() {
        let (fits, expected) = build_i16_image_fits(10, 10);
        let parsed = crate::hdu::parse_fits(&fits).unwrap();
        let hdu = parsed.primary();
        let data = read_image_section(&fits, hdu, 0, 100).unwrap();
        match data {
            ImageData::I16(v) => assert_eq!(v, expected),
            other => panic!("Expected I16, got {:?}", other),
        }
    }

    #[test]
    fn section_partial() {
        let (fits, expected) = build_i16_image_fits(10, 10);
        let parsed = crate::hdu::parse_fits(&fits).unwrap();
        let hdu = parsed.primary();
        let data = read_image_section(&fits, hdu, 5, 10).unwrap();
        match data {
            ImageData::I16(v) => assert_eq!(v, expected[5..15]),
            other => panic!("Expected I16, got {:?}", other),
        }
    }

    #[test]
    fn section_out_of_bounds() {
        let (fits, _) = build_i16_image_fits(10, 10);
        let parsed = crate::hdu::parse_fits(&fits).unwrap();
        let hdu = parsed.primary();
        assert!(read_image_section(&fits, hdu, 95, 10).is_err());
    }

    #[test]
    fn rows_single() {
        let (fits, expected) = build_i16_image_fits(10, 5);
        let parsed = crate::hdu::parse_fits(&fits).unwrap();
        let hdu = parsed.primary();
        let data = read_image_rows(&fits, hdu, 2, 1).unwrap();
        match data {
            ImageData::I16(v) => assert_eq!(v, expected[20..30]),
            other => panic!("Expected I16, got {:?}", other),
        }
    }

    #[test]
    fn rows_1d_errors() {
        let cards = crate::primary::build_primary_header(16, &[100]).unwrap();
        let raw = vec![0u8; 200];
        let fits = build_fits(&cards, &raw);
        let parsed = crate::hdu::parse_fits(&fits).unwrap();
        let hdu = parsed.primary();
        assert!(read_image_rows(&fits, hdu, 0, 1).is_err());
    }

    #[test]
    fn region_2d_subregion() {
        let (fits, _) = build_i16_image_fits(6, 5);
        let parsed = crate::hdu::parse_fits(&fits).unwrap();
        let hdu = parsed.primary();
        let data = read_image_region(&fits, hdu, &[(1, 4), (2, 4)]).unwrap();
        match data {
            ImageData::I16(v) => {
                assert_eq!(v.len(), 6);
                assert_eq!(v, vec![13, 14, 15, 19, 20, 21]);
            }
            other => panic!("Expected I16, got {:?}", other),
        }
    }

    #[test]
    fn region_3d_subregion() {
        let (fits, _) = build_i16_cube_fits(4, 3, 2);
        let parsed = crate::hdu::parse_fits(&fits).unwrap();
        let hdu = parsed.primary();
        let data = read_image_region(&fits, hdu, &[(1, 3), (0, 2), (0, 2)]).unwrap();
        match data {
            ImageData::I16(v) => {
                assert_eq!(v.len(), 8);
                assert_eq!(v, vec![1, 2, 5, 6, 13, 14, 17, 18]);
            }
            other => panic!("Expected I16, got {:?}", other),
        }
    }

    #[test]
    fn region_wrong_dim_count() {
        let (fits, _) = build_i16_image_fits(10, 10);
        let parsed = crate::hdu::parse_fits(&fits).unwrap();
        let hdu = parsed.primary();
        assert!(read_image_region(&fits, hdu, &[(0, 10), (0, 10), (0, 1)]).is_err());
    }

    #[test]
    fn region_empty_range() {
        let (fits, _) = build_i16_image_fits(10, 10);
        let parsed = crate::hdu::parse_fits(&fits).unwrap();
        let hdu = parsed.primary();
        let data = read_image_region(&fits, hdu, &[(5, 5), (0, 10)]).unwrap();
        match data {
            ImageData::I16(v) => assert!(v.is_empty()),
            other => panic!("Expected I16, got {:?}", other),
        }
    }

    // ---- BLANK keyword ----

    #[test]
    fn extract_blank_present() {
        let cards = vec![
            card("SIMPLE", Value::Logical(true)),
            card("BITPIX", Value::Integer(16)),
            card("NAXIS", Value::Integer(1)),
            card("NAXIS1", Value::Integer(4)),
            card("BLANK", Value::Integer(-32768)),
        ];
        assert_eq!(extract_blank(&cards), Some(-32768));
    }

    #[test]
    fn extract_blank_absent() {
        let cards = vec![
            card("SIMPLE", Value::Logical(true)),
            card("BITPIX", Value::Integer(16)),
            card("NAXIS", Value::Integer(0)),
        ];
        assert_eq!(extract_blank(&cards), None);
    }

    #[test]
    fn blank_mask_i16() {
        let data = ImageData::I16(vec![1, -32768, 3, -32768]);
        let mask = blank_mask(&data, Some(-32768));
        assert_eq!(mask, Some(vec![false, true, false, true]));
    }

    #[test]
    fn blank_mask_no_blank_keyword() {
        let data = ImageData::I16(vec![1, 2, 3]);
        assert!(blank_mask(&data, None).is_none());
    }

    #[test]
    fn blank_mask_no_matches() {
        let data = ImageData::I16(vec![1, 2, 3]);
        assert!(blank_mask(&data, Some(-32768)).is_none());
    }

    #[test]
    fn blank_mask_f32_nan() {
        let data = ImageData::F32(vec![1.0, f32::NAN, 3.0]);
        let mask = blank_mask(&data, None);
        assert_eq!(mask, Some(vec![false, true, false]));
    }

    #[test]
    fn read_physical_with_blank() {
        let blank_val: i16 = -32768;
        let values: [i16; 4] = [100, blank_val, 200, blank_val];
        let mut raw = vec![0u8; 8];
        for (i, &v) in values.iter().enumerate() {
            write_i16_be(&mut raw[i * 2..], v);
        }

        let mut cards = primary_header_image(16, &[4]);
        cards.push(card("BLANK", Value::Integer(blank_val as i64)));
        let fits = build_fits(&cards, &raw);
        let hdu = parse_primary(&fits);

        let physical = read_image_physical(&fits, &hdu).unwrap();
        assert_eq!(physical[0], 100.0);
        assert!(physical[1].is_nan());
        assert_eq!(physical[2], 200.0);
        assert!(physical[3].is_nan());
    }

    // ---- BSCALE/BZERO write path ----

    #[test]
    fn reverse_bscale_bzero_i16() {
        let physical = vec![32768.0, 0.0, 65535.0];
        let raw = reverse_bscale_bzero(&physical, 1.0, 32768.0, 16).unwrap();
        assert_eq!(raw, ImageData::I16(vec![0, -32768, 32767]));
    }

    #[test]
    fn reverse_bscale_bzero_u8() {
        let physical = vec![0.0, 127.5, 255.0];
        let raw = reverse_bscale_bzero(&physical, 1.0, 0.0, 8).unwrap();
        assert_eq!(raw, ImageData::U8(vec![0, 128, 255]));
    }

    #[test]
    fn reverse_bscale_bzero_with_scale() {
        let physical = vec![100.0, 102.0, 104.0];
        let raw = reverse_bscale_bzero(&physical, 2.0, 100.0, 16).unwrap();
        assert_eq!(raw, ImageData::I16(vec![0, 1, 2]));
    }

    #[test]
    fn reverse_bscale_bzero_clamps() {
        let physical = vec![-1.0, 256.0];
        let raw = reverse_bscale_bzero(&physical, 1.0, 0.0, 8).unwrap();
        assert_eq!(raw, ImageData::U8(vec![0, 255]));
    }

    #[test]
    fn reverse_bscale_bzero_invalid_bitpix() {
        assert!(reverse_bscale_bzero(&[1.0], 1.0, 0.0, 7).is_err());
    }

    #[test]
    fn build_hdu_with_scaling_roundtrip() {
        let physical = vec![32768.0, 0.0, 65535.0];
        let hdu_bytes = build_image_hdu_with_scaling(16, &[3], &physical, 1.0, 32768.0).unwrap();

        let fits = hdu_bytes;
        let parsed = crate::hdu::parse_fits(&fits).unwrap();
        let hdu = parsed.primary();

        let (bscale, bzero) = extract_bscale_bzero(&hdu.cards);
        assert_eq!(bscale, 1.0);
        assert_eq!(bzero, 32768.0);

        let result = read_image_physical(&fits, hdu).unwrap();
        assert_eq!(result, vec![32768.0, 0.0, 65535.0]);
    }

    #[test]
    fn build_hdu_with_scaling_no_keywords_when_default() {
        let physical = vec![1.0, 2.0, 3.0];
        let hdu_bytes = build_image_hdu_with_scaling(-64, &[3], &physical, 1.0, 0.0).unwrap();

        let parsed = crate::hdu::parse_fits(&hdu_bytes).unwrap();
        let hdu = parsed.primary();

        // Should not have BSCALE/BZERO cards
        let has_bscale = hdu.cards.iter().any(|c| c.keyword_str() == "BSCALE");
        assert!(!has_bscale);
    }

    // ---- read_image_data_into ----

    #[test]
    fn read_into_f32_from_f32_image() {
        let pixels: Vec<f32> = vec![1.0, 2.5, 3.125];
        let fits = build_image_hdu(-32, &[3], &ImageData::F32(pixels.clone())).unwrap();
        let parsed = crate::hdu::parse_fits(&fits).unwrap();
        let hdu = parsed.primary();

        let mut buf = vec![0.0f32; 3];
        read_image_data_into_f32(&fits, hdu, &mut buf).unwrap();
        assert_eq!(buf, pixels);
    }

    #[test]
    fn read_into_f64_from_i16_image() {
        let pixels: Vec<i16> = vec![100, -200, 300];
        let fits = build_image_hdu(16, &[3], &ImageData::I16(pixels)).unwrap();
        let parsed = crate::hdu::parse_fits(&fits).unwrap();
        let hdu = parsed.primary();

        let mut buf = vec![0.0f64; 3];
        read_image_data_into_f64(&fits, hdu, &mut buf).unwrap();
        assert_eq!(buf, vec![100.0, -200.0, 300.0]);
    }

    #[test]
    fn read_into_wrong_size_errors() {
        let pixels: Vec<f32> = vec![1.0, 2.0, 3.0];
        let fits = build_image_hdu(-32, &[3], &ImageData::F32(pixels)).unwrap();
        let parsed = crate::hdu::parse_fits(&fits).unwrap();
        let hdu = parsed.primary();

        let mut buf = vec![0.0f32; 2]; // wrong size
        assert!(read_image_data_into_f32(&fits, hdu, &mut buf).is_err());
    }
}
