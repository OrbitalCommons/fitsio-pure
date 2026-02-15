//! Big-endian byte conversion for FITS data.
//!
//! FITS stores all binary data in big-endian (most-significant byte first) format.
//! This module provides functions to read and write native Rust types from/to
//! big-endian byte slices, plus bulk conversion routines for data arrays.

/// Read a `u8` from the first byte of the slice.
#[inline]
pub fn read_u8(buf: &[u8]) -> u8 {
    buf[0]
}

/// Read a big-endian `i16` from the first 2 bytes of the slice.
#[inline]
pub fn read_i16_be(buf: &[u8]) -> i16 {
    i16::from_be_bytes([buf[0], buf[1]])
}

/// Read a big-endian `u16` from the first 2 bytes of the slice.
#[inline]
pub fn read_u16_be(buf: &[u8]) -> u16 {
    u16::from_be_bytes([buf[0], buf[1]])
}

/// Read a big-endian `i32` from the first 4 bytes of the slice.
#[inline]
pub fn read_i32_be(buf: &[u8]) -> i32 {
    i32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]])
}

/// Read a big-endian `u32` from the first 4 bytes of the slice.
#[inline]
pub fn read_u32_be(buf: &[u8]) -> u32 {
    u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]])
}

/// Read a big-endian `i64` from the first 8 bytes of the slice.
#[inline]
pub fn read_i64_be(buf: &[u8]) -> i64 {
    i64::from_be_bytes([
        buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
    ])
}

/// Read a big-endian `u64` from the first 8 bytes of the slice.
#[inline]
pub fn read_u64_be(buf: &[u8]) -> u64 {
    u64::from_be_bytes([
        buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
    ])
}

/// Read a big-endian `f32` (IEEE 754) from the first 4 bytes of the slice.
#[inline]
pub fn read_f32_be(buf: &[u8]) -> f32 {
    f32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]])
}

/// Read a big-endian `f64` (IEEE 754) from the first 8 bytes of the slice.
#[inline]
pub fn read_f64_be(buf: &[u8]) -> f64 {
    f64::from_be_bytes([
        buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
    ])
}

// --- Single-value writes ---

/// Write a `u8` into the first byte of the slice.
#[inline]
pub fn write_u8(buf: &mut [u8], val: u8) {
    buf[0] = val;
}

/// Write an `i16` in big-endian format into the first 2 bytes of the slice.
#[inline]
pub fn write_i16_be(buf: &mut [u8], val: i16) {
    let bytes = val.to_be_bytes();
    buf[0] = bytes[0];
    buf[1] = bytes[1];
}

/// Write a `u16` in big-endian format into the first 2 bytes of the slice.
#[inline]
pub fn write_u16_be(buf: &mut [u8], val: u16) {
    let bytes = val.to_be_bytes();
    buf[0] = bytes[0];
    buf[1] = bytes[1];
}

/// Write an `i32` in big-endian format into the first 4 bytes of the slice.
#[inline]
pub fn write_i32_be(buf: &mut [u8], val: i32) {
    let bytes = val.to_be_bytes();
    buf[..4].copy_from_slice(&bytes);
}

/// Write a `u32` in big-endian format into the first 4 bytes of the slice.
#[inline]
pub fn write_u32_be(buf: &mut [u8], val: u32) {
    let bytes = val.to_be_bytes();
    buf[..4].copy_from_slice(&bytes);
}

/// Write an `i64` in big-endian format into the first 8 bytes of the slice.
#[inline]
pub fn write_i64_be(buf: &mut [u8], val: i64) {
    let bytes = val.to_be_bytes();
    buf[..8].copy_from_slice(&bytes);
}

/// Write a `u64` in big-endian format into the first 8 bytes of the slice.
#[inline]
pub fn write_u64_be(buf: &mut [u8], val: u64) {
    let bytes = val.to_be_bytes();
    buf[..8].copy_from_slice(&bytes);
}

/// Write an `f32` in big-endian format into the first 4 bytes of the slice.
#[inline]
pub fn write_f32_be(buf: &mut [u8], val: f32) {
    let bytes = val.to_be_bytes();
    buf[..4].copy_from_slice(&bytes);
}

/// Write an `f64` in big-endian format into the first 8 bytes of the slice.
#[inline]
pub fn write_f64_be(buf: &mut [u8], val: f64) {
    let bytes = val.to_be_bytes();
    buf[..8].copy_from_slice(&bytes);
}

// --- Bulk conversions ---
//
// These convert a mutable byte buffer **in place** from big-endian to native
// endianness for each element type. On big-endian platforms these are no-ops.
// The buffer length must be a multiple of the element size.

/// Convert a byte buffer of big-endian `i16` values to native endianness in place.
///
/// # Panics
/// Panics if `buf.len()` is not a multiple of 2.
pub fn buf_i16_be_to_native(buf: &mut [u8]) {
    assert!(
        buf.len().is_multiple_of(2),
        "buffer length must be a multiple of 2"
    );
    for chunk in buf.chunks_exact_mut(2) {
        let val = i16::from_be_bytes([chunk[0], chunk[1]]);
        let native = val.to_ne_bytes();
        chunk[0] = native[0];
        chunk[1] = native[1];
    }
}

/// Convert a byte buffer of big-endian `u16` values to native endianness in place.
///
/// # Panics
/// Panics if `buf.len()` is not a multiple of 2.
pub fn buf_u16_be_to_native(buf: &mut [u8]) {
    assert!(
        buf.len().is_multiple_of(2),
        "buffer length must be a multiple of 2"
    );
    for chunk in buf.chunks_exact_mut(2) {
        let val = u16::from_be_bytes([chunk[0], chunk[1]]);
        let native = val.to_ne_bytes();
        chunk[0] = native[0];
        chunk[1] = native[1];
    }
}

/// Convert a byte buffer of big-endian `i32` values to native endianness in place.
///
/// # Panics
/// Panics if `buf.len()` is not a multiple of 4.
pub fn buf_i32_be_to_native(buf: &mut [u8]) {
    assert!(
        buf.len().is_multiple_of(4),
        "buffer length must be a multiple of 4"
    );
    for chunk in buf.chunks_exact_mut(4) {
        let val = i32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        let native = val.to_ne_bytes();
        chunk.copy_from_slice(&native);
    }
}

/// Convert a byte buffer of big-endian `u32` values to native endianness in place.
///
/// # Panics
/// Panics if `buf.len()` is not a multiple of 4.
pub fn buf_u32_be_to_native(buf: &mut [u8]) {
    assert!(
        buf.len().is_multiple_of(4),
        "buffer length must be a multiple of 4"
    );
    for chunk in buf.chunks_exact_mut(4) {
        let val = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        let native = val.to_ne_bytes();
        chunk.copy_from_slice(&native);
    }
}

/// Convert a byte buffer of big-endian `i64` values to native endianness in place.
///
/// # Panics
/// Panics if `buf.len()` is not a multiple of 8.
pub fn buf_i64_be_to_native(buf: &mut [u8]) {
    assert!(
        buf.len().is_multiple_of(8),
        "buffer length must be a multiple of 8"
    );
    for chunk in buf.chunks_exact_mut(8) {
        let val = i64::from_be_bytes([
            chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
        ]);
        let native = val.to_ne_bytes();
        chunk.copy_from_slice(&native);
    }
}

/// Convert a byte buffer of big-endian `u64` values to native endianness in place.
///
/// # Panics
/// Panics if `buf.len()` is not a multiple of 8.
pub fn buf_u64_be_to_native(buf: &mut [u8]) {
    assert!(
        buf.len().is_multiple_of(8),
        "buffer length must be a multiple of 8"
    );
    for chunk in buf.chunks_exact_mut(8) {
        let val = u64::from_be_bytes([
            chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
        ]);
        let native = val.to_ne_bytes();
        chunk.copy_from_slice(&native);
    }
}

/// Convert a byte buffer of big-endian `f32` values to native endianness in place.
///
/// # Panics
/// Panics if `buf.len()` is not a multiple of 4.
pub fn buf_f32_be_to_native(buf: &mut [u8]) {
    assert!(
        buf.len().is_multiple_of(4),
        "buffer length must be a multiple of 4"
    );
    for chunk in buf.chunks_exact_mut(4) {
        let val = f32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        let native = val.to_ne_bytes();
        chunk.copy_from_slice(&native);
    }
}

/// Convert a byte buffer of big-endian `f64` values to native endianness in place.
///
/// # Panics
/// Panics if `buf.len()` is not a multiple of 8.
pub fn buf_f64_be_to_native(buf: &mut [u8]) {
    assert!(
        buf.len().is_multiple_of(8),
        "buffer length must be a multiple of 8"
    );
    for chunk in buf.chunks_exact_mut(8) {
        let val = f64::from_be_bytes([
            chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
        ]);
        let native = val.to_ne_bytes();
        chunk.copy_from_slice(&native);
    }
}

// --- Bulk native-to-big-endian conversions ---

/// Convert a byte buffer of native-endian `i16` values to big-endian in place.
///
/// # Panics
/// Panics if `buf.len()` is not a multiple of 2.
pub fn buf_i16_native_to_be(buf: &mut [u8]) {
    assert!(
        buf.len().is_multiple_of(2),
        "buffer length must be a multiple of 2"
    );
    for chunk in buf.chunks_exact_mut(2) {
        let val = i16::from_ne_bytes([chunk[0], chunk[1]]);
        let be = val.to_be_bytes();
        chunk[0] = be[0];
        chunk[1] = be[1];
    }
}

/// Convert a byte buffer of native-endian `i32` values to big-endian in place.
///
/// # Panics
/// Panics if `buf.len()` is not a multiple of 4.
pub fn buf_i32_native_to_be(buf: &mut [u8]) {
    assert!(
        buf.len().is_multiple_of(4),
        "buffer length must be a multiple of 4"
    );
    for chunk in buf.chunks_exact_mut(4) {
        let val = i32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        let be = val.to_be_bytes();
        chunk.copy_from_slice(&be);
    }
}

/// Convert a byte buffer of native-endian `i64` values to big-endian in place.
///
/// # Panics
/// Panics if `buf.len()` is not a multiple of 8.
pub fn buf_i64_native_to_be(buf: &mut [u8]) {
    assert!(
        buf.len().is_multiple_of(8),
        "buffer length must be a multiple of 8"
    );
    for chunk in buf.chunks_exact_mut(8) {
        let val = i64::from_ne_bytes([
            chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
        ]);
        let be = val.to_be_bytes();
        chunk.copy_from_slice(&be);
    }
}

/// Convert a byte buffer of native-endian `f32` values to big-endian in place.
///
/// # Panics
/// Panics if `buf.len()` is not a multiple of 4.
pub fn buf_f32_native_to_be(buf: &mut [u8]) {
    assert!(
        buf.len().is_multiple_of(4),
        "buffer length must be a multiple of 4"
    );
    for chunk in buf.chunks_exact_mut(4) {
        let val = f32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        let be = val.to_be_bytes();
        chunk.copy_from_slice(&be);
    }
}

/// Convert a byte buffer of native-endian `f64` values to big-endian in place.
///
/// # Panics
/// Panics if `buf.len()` is not a multiple of 8.
pub fn buf_f64_native_to_be(buf: &mut [u8]) {
    assert!(
        buf.len().is_multiple_of(8),
        "buffer length must be a multiple of 8"
    );
    for chunk in buf.chunks_exact_mut(8) {
        let val = f64::from_ne_bytes([
            chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
        ]);
        let be = val.to_be_bytes();
        chunk.copy_from_slice(&be);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- read/write round-trip tests ---

    #[test]
    fn roundtrip_u8() {
        let mut buf = [0u8; 1];
        write_u8(&mut buf, 0xAB);
        assert_eq!(read_u8(&buf), 0xAB);
    }

    #[test]
    fn roundtrip_i16() {
        let mut buf = [0u8; 2];
        for val in [0_i16, 1, -1, i16::MIN, i16::MAX, 256, -256] {
            write_i16_be(&mut buf, val);
            assert_eq!(read_i16_be(&buf), val);
        }
    }

    #[test]
    fn roundtrip_u16() {
        let mut buf = [0u8; 2];
        for val in [0_u16, 1, u16::MAX, 256, 0xFF00] {
            write_u16_be(&mut buf, val);
            assert_eq!(read_u16_be(&buf), val);
        }
    }

    #[test]
    fn roundtrip_i32() {
        let mut buf = [0u8; 4];
        for val in [0_i32, 1, -1, i32::MIN, i32::MAX, 0x01020304, -0x01020304] {
            write_i32_be(&mut buf, val);
            assert_eq!(read_i32_be(&buf), val);
        }
    }

    #[test]
    fn roundtrip_u32() {
        let mut buf = [0u8; 4];
        for val in [0_u32, 1, u32::MAX, 0xDEADBEEF] {
            write_u32_be(&mut buf, val);
            assert_eq!(read_u32_be(&buf), val);
        }
    }

    #[test]
    fn roundtrip_i64() {
        let mut buf = [0u8; 8];
        for val in [0_i64, 1, -1, i64::MIN, i64::MAX] {
            write_i64_be(&mut buf, val);
            assert_eq!(read_i64_be(&buf), val);
        }
    }

    #[test]
    fn roundtrip_u64() {
        let mut buf = [0u8; 8];
        for val in [0_u64, 1, u64::MAX, 0xDEADBEEFCAFEBABE] {
            write_u64_be(&mut buf, val);
            assert_eq!(read_u64_be(&buf), val);
        }
    }

    #[test]
    fn roundtrip_f32() {
        let mut buf = [0u8; 4];
        for val in [
            0.0_f32,
            1.0,
            -1.0,
            f32::MIN,
            f32::MAX,
            f32::MIN_POSITIVE,
            f32::INFINITY,
            f32::NEG_INFINITY,
            core::f32::consts::PI,
        ] {
            write_f32_be(&mut buf, val);
            assert_eq!(read_f32_be(&buf), val);
        }
    }

    #[test]
    fn roundtrip_f32_nan() {
        let mut buf = [0u8; 4];
        write_f32_be(&mut buf, f32::NAN);
        assert!(read_f32_be(&buf).is_nan());
    }

    #[test]
    fn roundtrip_f64() {
        let mut buf = [0u8; 8];
        for val in [
            0.0_f64,
            1.0,
            -1.0,
            f64::MIN,
            f64::MAX,
            f64::MIN_POSITIVE,
            f64::INFINITY,
            f64::NEG_INFINITY,
            core::f64::consts::PI,
        ] {
            write_f64_be(&mut buf, val);
            assert_eq!(read_f64_be(&buf), val);
        }
    }

    #[test]
    fn roundtrip_f64_nan() {
        let mut buf = [0u8; 8];
        write_f64_be(&mut buf, f64::NAN);
        assert!(read_f64_be(&buf).is_nan());
    }

    // --- Known byte sequence tests ---

    #[test]
    fn known_bytes_i16() {
        assert_eq!(read_i16_be(&[0x00, 0x01]), 1_i16);
        assert_eq!(read_i16_be(&[0xFF, 0xFF]), -1_i16);
        assert_eq!(read_i16_be(&[0x80, 0x00]), i16::MIN);
        assert_eq!(read_i16_be(&[0x7F, 0xFF]), i16::MAX);
        assert_eq!(read_i16_be(&[0x01, 0x00]), 256_i16);
    }

    #[test]
    fn known_bytes_u16() {
        assert_eq!(read_u16_be(&[0x00, 0x01]), 1_u16);
        assert_eq!(read_u16_be(&[0xFF, 0xFF]), u16::MAX);
        assert_eq!(read_u16_be(&[0x00, 0x00]), 0_u16);
    }

    #[test]
    fn known_bytes_i32() {
        assert_eq!(read_i32_be(&[0x00, 0x00, 0x00, 0x01]), 1_i32);
        assert_eq!(read_i32_be(&[0xFF, 0xFF, 0xFF, 0xFF]), -1_i32);
        assert_eq!(read_i32_be(&[0x80, 0x00, 0x00, 0x00]), i32::MIN);
        assert_eq!(read_i32_be(&[0x7F, 0xFF, 0xFF, 0xFF]), i32::MAX);
    }

    #[test]
    fn known_bytes_i64() {
        assert_eq!(
            read_i64_be(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01]),
            1_i64
        );
        assert_eq!(
            read_i64_be(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]),
            -1_i64
        );
        assert_eq!(
            read_i64_be(&[0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]),
            i64::MIN
        );
    }

    #[test]
    fn known_bytes_f32() {
        // IEEE 754: 1.0f32 = 0x3F800000
        assert_eq!(read_f32_be(&[0x3F, 0x80, 0x00, 0x00]), 1.0_f32);
        // -1.0f32 = 0xBF800000
        assert_eq!(read_f32_be(&[0xBF, 0x80, 0x00, 0x00]), -1.0_f32);
        // 0.0f32 = 0x00000000
        assert_eq!(read_f32_be(&[0x00, 0x00, 0x00, 0x00]), 0.0_f32);
        // +inf = 0x7F800000
        assert_eq!(read_f32_be(&[0x7F, 0x80, 0x00, 0x00]), f32::INFINITY);
    }

    #[test]
    fn known_bytes_f64() {
        // IEEE 754: 1.0f64 = 0x3FF0000000000000
        assert_eq!(
            read_f64_be(&[0x3F, 0xF0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]),
            1.0_f64
        );
        // -1.0f64 = 0xBFF0000000000000
        assert_eq!(
            read_f64_be(&[0xBF, 0xF0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]),
            -1.0_f64
        );
    }

    #[test]
    fn write_known_bytes_i16() {
        let mut buf = [0u8; 2];
        write_i16_be(&mut buf, 1);
        assert_eq!(buf, [0x00, 0x01]);
        write_i16_be(&mut buf, -1);
        assert_eq!(buf, [0xFF, 0xFF]);
    }

    #[test]
    fn write_known_bytes_i32() {
        let mut buf = [0u8; 4];
        write_i32_be(&mut buf, 1);
        assert_eq!(buf, [0x00, 0x00, 0x00, 0x01]);
    }

    #[test]
    fn write_known_bytes_f32() {
        let mut buf = [0u8; 4];
        write_f32_be(&mut buf, 1.0);
        assert_eq!(buf, [0x3F, 0x80, 0x00, 0x00]);
    }

    // --- Bulk conversion tests ---

    #[test]
    fn bulk_i16_roundtrip() {
        let values: [i16; 4] = [1, -1, i16::MIN, i16::MAX];
        let mut buf = [0u8; 8];
        for (i, &v) in values.iter().enumerate() {
            write_i16_be(&mut buf[i * 2..], v);
        }
        let original = buf;

        buf_i16_be_to_native(&mut buf);
        buf_i16_native_to_be(&mut buf);
        assert_eq!(buf, original);
    }

    #[test]
    fn bulk_i32_roundtrip() {
        let values: [i32; 3] = [1, -1, i32::MAX];
        let mut buf = [0u8; 12];
        for (i, &v) in values.iter().enumerate() {
            write_i32_be(&mut buf[i * 4..], v);
        }
        let original = buf;

        buf_i32_be_to_native(&mut buf);
        buf_i32_native_to_be(&mut buf);
        assert_eq!(buf, original);
    }

    #[test]
    fn bulk_i64_roundtrip() {
        let values: [i64; 2] = [i64::MIN, i64::MAX];
        let mut buf = [0u8; 16];
        for (i, &v) in values.iter().enumerate() {
            write_i64_be(&mut buf[i * 8..], v);
        }
        let original = buf;

        buf_i64_be_to_native(&mut buf);
        buf_i64_native_to_be(&mut buf);
        assert_eq!(buf, original);
    }

    #[test]
    fn bulk_f32_roundtrip() {
        let values: [f32; 3] = [1.0, -1.0, core::f32::consts::PI];
        let mut buf = [0u8; 12];
        for (i, &v) in values.iter().enumerate() {
            write_f32_be(&mut buf[i * 4..], v);
        }
        let original = buf;

        buf_f32_be_to_native(&mut buf);
        buf_f32_native_to_be(&mut buf);
        assert_eq!(buf, original);
    }

    #[test]
    fn bulk_f64_roundtrip() {
        let values: [f64; 2] = [f64::MIN, f64::MAX];
        let mut buf = [0u8; 16];
        for (i, &v) in values.iter().enumerate() {
            write_f64_be(&mut buf[i * 8..], v);
        }
        let original = buf;

        buf_f64_be_to_native(&mut buf);
        buf_f64_native_to_be(&mut buf);
        assert_eq!(buf, original);
    }

    #[test]
    fn bulk_u16_roundtrip() {
        let values: [u16; 3] = [0, 1, u16::MAX];
        let mut buf = [0u8; 6];
        for (i, &v) in values.iter().enumerate() {
            write_u16_be(&mut buf[i * 2..], v);
        }
        let original = buf;

        buf_u16_be_to_native(&mut buf);
        // Convert back: read as native, write as big-endian
        for chunk in buf.chunks_exact_mut(2) {
            let val = u16::from_ne_bytes([chunk[0], chunk[1]]);
            let be = val.to_be_bytes();
            chunk[0] = be[0];
            chunk[1] = be[1];
        }
        assert_eq!(buf, original);
    }

    #[test]
    fn bulk_u32_roundtrip() {
        let values: [u32; 2] = [0, u32::MAX];
        let mut buf = [0u8; 8];
        for (i, &v) in values.iter().enumerate() {
            write_u32_be(&mut buf[i * 4..], v);
        }
        let original = buf;

        buf_u32_be_to_native(&mut buf);
        for chunk in buf.chunks_exact_mut(4) {
            let val = u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            let be = val.to_be_bytes();
            chunk.copy_from_slice(&be);
        }
        assert_eq!(buf, original);
    }

    #[test]
    fn bulk_u64_roundtrip() {
        let values: [u64; 2] = [0, u64::MAX];
        let mut buf = [0u8; 16];
        for (i, &v) in values.iter().enumerate() {
            write_u64_be(&mut buf[i * 8..], v);
        }
        let original = buf;

        buf_u64_be_to_native(&mut buf);
        for chunk in buf.chunks_exact_mut(8) {
            let val = u64::from_ne_bytes([
                chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
            ]);
            let be = val.to_be_bytes();
            chunk.copy_from_slice(&be);
        }
        assert_eq!(buf, original);
    }

    #[test]
    fn bulk_empty_buffer() {
        let mut empty: [u8; 0] = [];
        buf_i16_be_to_native(&mut empty);
        buf_i32_be_to_native(&mut empty);
        buf_i64_be_to_native(&mut empty);
        buf_f32_be_to_native(&mut empty);
        buf_f64_be_to_native(&mut empty);
    }

    #[test]
    #[should_panic(expected = "buffer length must be a multiple of 2")]
    fn bulk_i16_odd_length_panics() {
        let mut buf = [0u8; 3];
        buf_i16_be_to_native(&mut buf);
    }

    #[test]
    #[should_panic(expected = "buffer length must be a multiple of 4")]
    fn bulk_i32_bad_length_panics() {
        let mut buf = [0u8; 5];
        buf_i32_be_to_native(&mut buf);
    }

    #[test]
    #[should_panic(expected = "buffer length must be a multiple of 8")]
    fn bulk_i64_bad_length_panics() {
        let mut buf = [0u8; 7];
        buf_i64_be_to_native(&mut buf);
    }

    #[test]
    fn bulk_be_to_native_reads_correctly() {
        let mut buf = [0u8; 8];
        write_i16_be(&mut buf[0..], 100);
        write_i16_be(&mut buf[2..], -200);
        write_i16_be(&mut buf[4..], i16::MAX);
        write_i16_be(&mut buf[6..], i16::MIN);

        buf_i16_be_to_native(&mut buf);

        assert_eq!(i16::from_ne_bytes([buf[0], buf[1]]), 100);
        assert_eq!(i16::from_ne_bytes([buf[2], buf[3]]), -200);
        assert_eq!(i16::from_ne_bytes([buf[4], buf[5]]), i16::MAX);
        assert_eq!(i16::from_ne_bytes([buf[6], buf[7]]), i16::MIN);
    }

    #[test]
    fn bulk_f32_be_to_native_reads_correctly() {
        let mut buf = [0u8; 8];
        write_f32_be(&mut buf[0..], 1.0);
        write_f32_be(&mut buf[4..], -42.5);

        buf_f32_be_to_native(&mut buf);

        assert_eq!(
            f32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]),
            1.0_f32
        );
        assert_eq!(
            f32::from_ne_bytes([buf[4], buf[5], buf[6], buf[7]]),
            -42.5_f32
        );
    }

    #[test]
    fn read_at_offset() {
        let buf = [0x00, 0x00, 0x00, 0x01, 0x00, 0x02];
        assert_eq!(read_i16_be(&buf[4..]), 2_i16);
        assert_eq!(read_i32_be(&buf[0..]), 1_i32);
    }
}
