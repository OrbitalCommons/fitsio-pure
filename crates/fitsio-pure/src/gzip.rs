//! Gzip decompression (RFC 1952).
//!
//! Archives commonly distribute whole FITS files gzip-compressed (`.fits.gz`,
//! `.fit.gz`), and cfitsio opens them transparently. The same framing carries
//! `GZIP_1`/`GZIP_2` tile data. Every member's CRC-32 and length are checked,
//! so a corrupt or truncated stream is an error rather than wrong bytes.

use alloc::vec::Vec;

use miniz_oxide::inflate::core::{decompress as inflate, inflate_flags, DecompressorOxide};
use miniz_oxide::inflate::TINFLStatus;

use crate::error::{Error, Result};

/// True if `data` starts with the gzip magic bytes.
pub fn is_gzip(data: &[u8]) -> bool {
    data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b
}

/// Decompress gzip data, checking each member's CRC-32 and length.
///
/// Concatenated members (RFC 1952 §2.2) decompress to the concatenation of
/// their contents. Zero padding after the last member, which some archives
/// add to fill a block, is ignored.
pub fn decompress(data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut rest = data;
    loop {
        let consumed = decompress_member(rest, &mut out)?;
        rest = &rest[consumed..];
        if rest.iter().all(|&b| b == 0) {
            return Ok(out);
        }
    }
}

/// Decompress one member onto the end of `out`, returning the bytes it spans.
fn decompress_member(data: &[u8], out: &mut Vec<u8>) -> Result<usize> {
    let header = header_len(data)?;
    let start = out.len();
    let mut state = DecompressorOxide::new();
    let flags = inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF;
    let mut in_pos = header;
    let mut out_pos = start;
    out.resize(start + (data.len() - header).saturating_mul(2).max(64), 0);
    loop {
        let (status, read, written) = inflate(&mut state, &data[in_pos..], out, out_pos, flags);
        in_pos += read;
        out_pos += written;
        match status {
            TINFLStatus::Done => break,
            TINFLStatus::HasMoreOutput => {
                let grown = out.len() * 2;
                out.resize(grown, 0);
            }
            _ => return Err(Error::DecompressionError("gzip inflate failed")),
        }
    }
    out.truncate(out_pos);

    let trailer = data
        .get(in_pos..in_pos + 8)
        .ok_or(Error::DecompressionError("truncated gzip trailer"))?;
    let crc = u32::from_le_bytes([trailer[0], trailer[1], trailer[2], trailer[3]]);
    let size = u32::from_le_bytes([trailer[4], trailer[5], trailer[6], trailer[7]]);
    let member = &out[start..];
    if crc32(member) != crc {
        return Err(Error::DecompressionError("gzip CRC-32 mismatch"));
    }
    // ISIZE is the uncompressed length modulo 2^32.
    if member.len() as u32 != size {
        return Err(Error::DecompressionError("gzip length mismatch"));
    }
    Ok(in_pos + 8)
}

/// Length of the gzip member header at the start of `data`.
fn header_len(data: &[u8]) -> Result<usize> {
    if data.len() < 18 || !is_gzip(data) || data[2] != 0x08 {
        return Err(Error::DecompressionError("invalid gzip header"));
    }
    let flg = data[3];
    let mut pos = 10usize;
    if flg & 0x04 != 0 {
        // FEXTRA
        let xlen = data
            .get(pos..pos + 2)
            .ok_or(Error::DecompressionError("truncated gzip FEXTRA"))?;
        pos += 2 + u16::from_le_bytes([xlen[0], xlen[1]]) as usize;
    }
    for flag in [0x08, 0x10] {
        // FNAME, FCOMMENT: null-terminated strings
        if flg & flag != 0 {
            let len = data
                .get(pos..)
                .and_then(|s| s.iter().position(|&b| b == 0))
                .ok_or(Error::DecompressionError("truncated gzip header"))?;
            pos += len + 1;
        }
    }
    if flg & 0x02 != 0 {
        // FHCRC
        pos += 2;
    }
    if pos >= data.len() {
        return Err(Error::DecompressionError("truncated gzip data"));
    }
    Ok(pos)
}

/// CRC-32 (IEEE 802.3, the gzip checksum), slice-by-8.
fn crc32(data: &[u8]) -> u32 {
    let t = &CRC_TABLES;
    let mut crc = !0u32;
    let mut chunks = data.chunks_exact(8);
    for c in &mut chunks {
        let lo = crc ^ u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
        let hi = u32::from_le_bytes([c[4], c[5], c[6], c[7]]);
        crc = t[7][(lo & 0xff) as usize]
            ^ t[6][((lo >> 8) & 0xff) as usize]
            ^ t[5][((lo >> 16) & 0xff) as usize]
            ^ t[4][(lo >> 24) as usize]
            ^ t[3][(hi & 0xff) as usize]
            ^ t[2][((hi >> 8) & 0xff) as usize]
            ^ t[1][((hi >> 16) & 0xff) as usize]
            ^ t[0][(hi >> 24) as usize];
    }
    for &b in chunks.remainder() {
        crc = t[0][((crc ^ b as u32) & 0xff) as usize] ^ (crc >> 8);
    }
    !crc
}

static CRC_TABLES: [[u32; 256]; 8] = crc_tables();

const fn crc_tables() -> [[u32; 256]; 8] {
    let mut t = [[0u32; 256]; 8];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        t[0][i] = c;
        i += 1;
    }
    let mut i = 0;
    while i < 256 {
        let mut s = 1;
        while s < 8 {
            t[s][i] = (t[s - 1][i] >> 8) ^ t[0][(t[s - 1][i] & 0xff) as usize];
            s += 1;
        }
        i += 1;
    }
    t
}

/// Gzip `data` as one member with no optional header fields, for tests.
#[cfg(test)]
pub(crate) fn encode_for_tests(data: &[u8]) -> Vec<u8> {
    let mut out = alloc::vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff];
    out.extend(miniz_oxide::deflate::compress_to_vec(data, 6));
    out.extend(crc32(data).to_le_bytes());
    out.extend((data.len() as u32).to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_check_value() {
        // The standard CRC-32 check value.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn crc32_matches_bytewise_on_every_length() {
        let data: Vec<u8> = (0..300u32).map(|i| (i * 31 + 7) as u8).collect();
        for n in 0..data.len() {
            let mut crc = !0u32;
            for &b in &data[..n] {
                crc = CRC_TABLES[0][((crc ^ b as u32) & 0xff) as usize] ^ (crc >> 8);
            }
            assert_eq!(crc32(&data[..n]), !crc, "length {n}");
        }
    }

    #[test]
    fn round_trip() {
        let data: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
        let gz = encode_for_tests(&data);
        assert!(is_gzip(&gz));
        assert_eq!(decompress(&gz).unwrap(), data);
    }

    #[test]
    fn empty_payload() {
        assert_eq!(decompress(&encode_for_tests(b"")).unwrap(), b"");
    }

    #[test]
    fn optional_header_fields_are_skipped() {
        let plain = encode_for_tests(b"fits bytes");
        // FEXTRA (2 bytes), FNAME and FCOMMENT set.
        let mut gz = alloc::vec![0x1f, 0x8b, 8, 0x04 | 0x08 | 0x10, 0, 0, 0, 0, 0, 0xff];
        gz.extend([2, 0, b'x', b'y']);
        gz.extend(b"file.fits\0");
        gz.extend(b"a comment\0");
        gz.extend(&plain[10..]);
        assert_eq!(decompress(&gz).unwrap(), b"fits bytes");
    }

    #[test]
    fn concatenated_members_and_padding() {
        let mut gz = encode_for_tests(b"first ");
        gz.extend(encode_for_tests(b"second"));
        gz.extend([0u8; 512]);
        assert_eq!(decompress(&gz).unwrap(), b"first second");
    }

    #[test]
    fn corrupt_crc_is_an_error() {
        let mut gz = encode_for_tests(b"some FITS data");
        let n = gz.len();
        gz[n - 8] ^= 0xff;
        assert!(decompress(&gz).is_err());
    }

    #[test]
    fn truncated_stream_is_an_error() {
        let gz = encode_for_tests(&[7u8; 10_000]);
        assert!(decompress(&gz[..gz.len() - 12]).is_err());
        assert!(decompress(&gz[..gz.len() - 4]).is_err());
    }

    #[test]
    fn trailing_garbage_is_an_error() {
        let mut gz = encode_for_tests(b"data");
        gz.extend(b"junk");
        assert!(decompress(&gz).is_err());
    }

    #[test]
    fn non_gzip_is_rejected() {
        assert!(!is_gzip(b"SIMPLE  =                    T"));
        assert!(decompress(b"SIMPLE  =                    T").is_err());
    }
}
