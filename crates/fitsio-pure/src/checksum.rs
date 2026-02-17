//! FITS CHECKSUM and DATASUM keyword support.
//!
//! Implements the HEASARC ones-complement checksum convention for verifying
//! and computing integrity checksums on FITS HDUs.

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

use crate::block::{padded_byte_len, BLOCK_SIZE};
use crate::hdu::Hdu;
use crate::header::{serialize_header, Card};
use crate::value::Value;

// ---------------------------------------------------------------------------
// Ones-complement 32-bit checksum (cfitsio `ffcsum`)
// ---------------------------------------------------------------------------

/// Compute the ones-complement 32-bit checksum over block-aligned data.
///
/// `data` must be a multiple of [`BLOCK_SIZE`] (2880) bytes.
/// Returns the accumulated checksum.
pub fn checksum_blocks(data: &[u8]) -> u32 {
    assert!(
        data.len().is_multiple_of(BLOCK_SIZE),
        "data length must be a multiple of 2880"
    );
    let mut sum: u32 = 0;
    for block in data.chunks_exact(BLOCK_SIZE) {
        sum = accumulate_block(sum, block);
    }
    sum
}

/// Accumulate a single 2880-byte block into an existing checksum.
fn accumulate_block(sum: u32, block: &[u8]) -> u32 {
    let mut hi: u32 = sum >> 16;
    let mut lo: u32 = sum & 0xFFFF;

    // Process 1440 big-endian u16 values, alternating into hi and lo.
    for pair in block.chunks_exact(4) {
        let w0 = u16::from_be_bytes([pair[0], pair[1]]) as u32;
        let w1 = u16::from_be_bytes([pair[2], pair[3]]) as u32;
        hi += w0;
        lo += w1;
    }

    // Fold carry bits.
    let mut hicarry = hi >> 16;
    let mut locarry = lo >> 16;
    while (hicarry | locarry) != 0 {
        hi = (hi & 0xFFFF) + locarry;
        lo = (lo & 0xFFFF) + hicarry;
        hicarry = hi >> 16;
        locarry = lo >> 16;
    }

    (hi << 16) | lo
}

// ---------------------------------------------------------------------------
// ASCII encoding/decoding (cfitsio `ffesum` / `ffdsum`)
// ---------------------------------------------------------------------------

/// ASCII characters to exclude (punctuation gaps between digits/upper/lower).
const EXCLUDE: [u8; 13] = [
    0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f, 0x40, // : ; < = > ? @
    0x5b, 0x5c, 0x5d, 0x5e, 0x5f, 0x60, // [ \ ] ^ _ `
];
const ASCII_OFFSET: i32 = 0x30; // ASCII '0'

/// Encode a 32-bit checksum into a 16-character ASCII string.
///
/// If `complement` is true, the bitwise complement of `sum` is encoded.
pub fn encode_checksum(sum: u32, complement: bool) -> [u8; 16] {
    let value = if complement {
        0xFFFFFFFFu32.wrapping_sub(sum)
    } else {
        sum
    };

    let mut asc = [0u8; 16];
    for ii in 0..4u32 {
        let byte_val = ((value >> (24 - 8 * ii)) & 0xFF) as i32;
        let quotient = byte_val / 4 + ASCII_OFFSET;
        let remainder = byte_val % 4;

        let mut ch = [quotient; 4];
        ch[0] += remainder;

        // Adjust pairs to avoid excluded ASCII characters.
        loop {
            let mut adjusted = false;
            for &ex in &EXCLUDE {
                let ex = ex as i32;
                for jj in (0..4).step_by(2) {
                    if ch[jj] == ex || ch[jj + 1] == ex {
                        ch[jj] += 1;
                        ch[jj + 1] -= 1;
                        adjusted = true;
                    }
                }
            }
            if !adjusted {
                break;
            }
        }

        // Interleave bytes.
        for jj in 0..4 {
            asc[4 * jj + ii as usize] = ch[jj] as u8;
        }
    }

    // Circular right-shift by 1 position.
    let mut result = [0u8; 16];
    for ii in 0..16 {
        result[ii] = asc[(ii + 15) % 16];
    }
    result
}

/// Decode a 16-character ASCII encoded checksum into a 32-bit value.
///
/// If `complement` is true, the complement of the decoded value is returned.
pub fn decode_checksum(ascii: &[u8; 16], complement: bool) -> u32 {
    let mut cbuf = [0i32; 16];
    for ii in 0..16 {
        cbuf[ii] = ascii[(ii + 1) % 16] as i32 - ASCII_OFFSET;
    }

    let mut hi: u32 = 0;
    let mut lo: u32 = 0;
    for ii in (0..16).step_by(4) {
        hi += ((cbuf[ii] << 8) + cbuf[ii + 1]) as u32;
        lo += ((cbuf[ii + 2] << 8) + cbuf[ii + 3]) as u32;
    }

    let mut hicarry = hi >> 16;
    let mut locarry = lo >> 16;
    while hicarry != 0 || locarry != 0 {
        hi = (hi & 0xFFFF) + locarry;
        lo = (lo & 0xFFFF) + hicarry;
        hicarry = hi >> 16;
        locarry = lo >> 16;
    }

    let mut sum = (hi << 16) + lo;
    if complement {
        sum = 0xFFFFFFFFu32.wrapping_sub(sum);
    }
    sum
}

// ---------------------------------------------------------------------------
// Verification (read path)
// ---------------------------------------------------------------------------

/// Verify the DATASUM keyword against the actual data checksum.
///
/// Returns `Ok(true)` if DATASUM matches, `Ok(false)` if it doesn't,
/// or `Ok(true)` if no DATASUM keyword is present (nothing to verify).
pub fn verify_datasum(fits_data: &[u8], hdu: &Hdu) -> bool {
    let stored = match find_string_keyword(&hdu.cards, "DATASUM") {
        Some(s) => s,
        None => return true,
    };
    let expected: u32 = match stored.trim().parse::<u64>() {
        Ok(v) => v as u32,
        Err(_) => return false,
    };
    let computed = compute_datasum(fits_data, hdu);
    computed == expected
}

/// Verify the CHECKSUM keyword for an entire HDU.
///
/// The total ones-complement checksum of a valid HDU should be -0
/// (i.e. `0x00000000` or `0xFFFFFFFF`).
pub fn verify_checksum(fits_data: &[u8], hdu: &Hdu) -> bool {
    if find_string_keyword(&hdu.cards, "CHECKSUM").is_none() {
        return true;
    }
    let header_end = hdu.data_start;
    let data_padded = padded_byte_len(hdu.data_len);
    let hdu_end = header_end + data_padded;
    if hdu_end > fits_data.len() || hdu.header_start > fits_data.len() {
        return false;
    }
    let hdu_bytes = &fits_data[hdu.header_start..hdu_end];
    if !hdu_bytes.len().is_multiple_of(BLOCK_SIZE) {
        return false;
    }
    let sum = checksum_blocks(hdu_bytes);
    sum == 0 || sum == 0xFFFFFFFF
}

// ---------------------------------------------------------------------------
// Computation (write path)
// ---------------------------------------------------------------------------

/// Compute the DATASUM for an HDU's data blocks.
pub fn compute_datasum(fits_data: &[u8], hdu: &Hdu) -> u32 {
    if hdu.data_len == 0 {
        return 0;
    }
    let data_padded = padded_byte_len(hdu.data_len);
    let data_end = hdu.data_start + data_padded;
    if data_end > fits_data.len() {
        return 0;
    }
    checksum_blocks(&fits_data[hdu.data_start..data_end])
}

/// Stamp CHECKSUM and DATASUM keywords onto a set of header cards.
///
/// Given the raw FITS data and an HDU, computes both checksums and returns
/// a new card list with CHECKSUM and DATASUM inserted (or updated).
/// The caller should then re-serialize the header with these cards.
///
/// This function performs the iterative computation: it first sets
/// `CHECKSUM = '0000000000000000'`, serializes, checksums the whole HDU,
/// then encodes the complement.
pub fn stamp_checksum(cards: &[Card], data_bytes: &[u8]) -> Vec<Card> {
    // Build card list without any existing CHECKSUM/DATASUM.
    let mut new_cards: Vec<Card> = cards
        .iter()
        .filter(|c| {
            let kw = c.keyword_str();
            kw != "CHECKSUM" && kw != "DATASUM"
        })
        .cloned()
        .collect();

    // Compute data checksum.
    let data_padded_len = padded_byte_len(data_bytes.len());
    let mut data_padded = Vec::with_capacity(data_padded_len);
    data_padded.extend_from_slice(data_bytes);
    data_padded.resize(data_padded_len, 0u8);
    let datasum = if data_bytes.is_empty() {
        0u32
    } else {
        checksum_blocks(&data_padded)
    };

    // Add DATASUM card.
    let datasum_card = Card {
        keyword: make_keyword(b"DATASUM"),
        value: Some(Value::String(datasum.to_string())),
        comment: Some(String::from("data unit checksum")),
    };
    new_cards.push(datasum_card);

    // Add CHECKSUM placeholder.
    let checksum_card = Card {
        keyword: make_keyword(b"CHECKSUM"),
        value: Some(Value::String(String::from("0000000000000000"))),
        comment: Some(String::from("HDU checksum")),
    };
    new_cards.push(checksum_card);

    // Serialize header with placeholder, compute HDU checksum.
    let header_bytes = match serialize_header(&new_cards) {
        Ok(h) => h,
        Err(_) => return new_cards,
    };
    let header_sum = checksum_blocks(&header_bytes);

    // Total HDU sum = header_sum + datasum (ones-complement addition).
    let hdu_sum = ones_complement_add(header_sum, datasum);

    // Encode the complement so the HDU sums to -0.
    let encoded = encode_checksum(hdu_sum, true);
    let checksum_str = core::str::from_utf8(&encoded).unwrap_or("0000000000000000");

    // Replace the placeholder with the actual checksum.
    if let Some(card) = new_cards.iter_mut().find(|c| c.keyword_str() == "CHECKSUM") {
        card.value = Some(Value::String(String::from(checksum_str)));
    }

    new_cards
}

/// Ones-complement addition of two 32-bit values.
fn ones_complement_add(a: u32, b: u32) -> u32 {
    let mut hi = (a >> 16) + (b >> 16);
    let mut lo = (a & 0xFFFF) + (b & 0xFFFF);
    let mut hicarry = hi >> 16;
    let mut locarry = lo >> 16;
    while (hicarry | locarry) != 0 {
        hi = (hi & 0xFFFF) + locarry;
        lo = (lo & 0xFFFF) + hicarry;
        hicarry = hi >> 16;
        locarry = lo >> 16;
    }
    (hi << 16) | lo
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_keyword(name: &[u8]) -> [u8; 8] {
    let mut buf = [b' '; 8];
    let len = name.len().min(8);
    buf[..len].copy_from_slice(&name[..len]);
    buf
}

fn find_string_keyword(cards: &[Card], keyword: &str) -> Option<String> {
    cards.iter().find_map(|c| {
        if c.keyword_str() == keyword {
            match &c.value {
                Some(Value::String(s)) => Some(s.clone()),
                _ => None,
            }
        } else {
            None
        }
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Encoding/Decoding ----

    #[test]
    fn encode_zero() {
        let encoded = encode_checksum(0, false);
        assert_eq!(&encoded, b"0000000000000000");
    }

    #[test]
    fn encode_complement_of_max() {
        // complement of 0xFFFFFFFF = 0x00000000, should encode as all zeros.
        let encoded = encode_checksum(0xFFFFFFFF, true);
        assert_eq!(&encoded, b"0000000000000000");
    }

    #[test]
    fn encode_decode_roundtrip() {
        let values: &[u32] = &[0, 1, 42, 1234567890, 0xDEADBEEF, 0xFFFFFFFF];
        for &v in values {
            let encoded = encode_checksum(v, false);
            let decoded = decode_checksum(&encoded, false);
            assert_eq!(decoded, v, "roundtrip failed for {v:#010X}");
        }
    }

    #[test]
    fn encode_decode_complement_roundtrip() {
        let values: &[u32] = &[0, 1, 42, 1234567890, 0xDEADBEEF, 0xFFFFFFFF];
        for &v in values {
            let encoded = encode_checksum(v, true);
            let decoded = decode_checksum(&encoded, true);
            assert_eq!(decoded, v, "complement roundtrip failed for {v:#010X}");
        }
    }

    #[test]
    fn encode_known_value() {
        // cfitsio test vector: encode(1234567890, false) = "dCW2fBU0dBU0dBU0"
        let encoded = encode_checksum(1234567890, false);
        assert_eq!(core::str::from_utf8(&encoded).unwrap(), "dCW2fBU0dBU0dBU0");
    }

    #[test]
    fn decode_known_value() {
        let decoded = decode_checksum(b"dCW2fBU0dBU0dBU0", false);
        assert_eq!(decoded, 1234567890);
    }

    #[test]
    fn encoded_chars_are_alphanumeric() {
        let values: &[u32] = &[0, 1, 255, 65535, 1234567890, 0xDEADBEEF, 0xFFFFFFFF];
        for &v in values {
            let encoded = encode_checksum(v, false);
            for &ch in &encoded {
                assert!(
                    ch.is_ascii_alphanumeric(),
                    "non-alphanumeric char {ch:#04x} in encoding of {v:#010X}"
                );
            }
        }
    }

    // ---- Checksum computation ----

    #[test]
    fn checksum_all_zeros() {
        let data = vec![0u8; BLOCK_SIZE];
        assert_eq!(checksum_blocks(&data), 0);
    }

    #[test]
    fn checksum_all_ff() {
        let data = vec![0xFFu8; BLOCK_SIZE];
        assert_eq!(checksum_blocks(&data), 0xFFFFFFFF);
    }

    #[test]
    fn checksum_complement_sums_to_negative_zero() {
        // A block plus its complement should sum to -0 (0xFFFFFFFF).
        let mut block = vec![0u8; BLOCK_SIZE];
        for (i, b) in block.iter_mut().enumerate() {
            *b = (i % 256) as u8;
        }
        let sum = checksum_blocks(&block);
        let complement = 0xFFFFFFFFu32.wrapping_sub(sum);
        let total = ones_complement_add(sum, complement);
        assert!(total == 0xFFFFFFFF || total == 0);
    }

    // ---- Ones-complement addition ----

    #[test]
    fn ones_complement_add_zero() {
        assert_eq!(ones_complement_add(0, 0), 0);
    }

    #[test]
    fn ones_complement_add_identity() {
        assert_eq!(ones_complement_add(0x12345678, 0), 0x12345678);
    }

    #[test]
    fn ones_complement_add_negative_zero() {
        // x + ~x = -0 in ones-complement
        let x = 0x12345678u32;
        let complement = 0xFFFFFFFFu32 - x;
        let result = ones_complement_add(x, complement);
        assert!(result == 0xFFFFFFFF || result == 0);
    }

    // ---- stamp_checksum ----

    #[test]
    fn stamp_produces_valid_checksum() {
        use crate::header::serialize_header;
        use crate::primary::build_primary_header;

        let cards = build_primary_header(8, &[10]).unwrap();
        let data = vec![42u8; 10];

        let stamped = stamp_checksum(&cards, &data);

        // Verify DATASUM is present and correct.
        let datasum_card = stamped
            .iter()
            .find(|c| c.keyword_str() == "DATASUM")
            .unwrap();
        let datasum_str = match &datasum_card.value {
            Some(Value::String(s)) => s.clone(),
            _ => panic!("DATASUM should be a string"),
        };
        let datasum_val: u64 = datasum_str.parse().unwrap();
        assert!(datasum_val <= u32::MAX as u64);

        // Verify CHECKSUM is present.
        let checksum_card = stamped
            .iter()
            .find(|c| c.keyword_str() == "CHECKSUM")
            .unwrap();
        let checksum_str = match &checksum_card.value {
            Some(Value::String(s)) => s.clone(),
            _ => panic!("CHECKSUM should be a string"),
        };
        assert_eq!(checksum_str.len(), 16);

        // Serialize and verify the total HDU sums to -0.
        let header_bytes = serialize_header(&stamped).unwrap();
        let data_padded_len = padded_byte_len(data.len());
        let mut hdu_bytes = Vec::with_capacity(header_bytes.len() + data_padded_len);
        hdu_bytes.extend_from_slice(&header_bytes);
        hdu_bytes.extend_from_slice(&data);
        hdu_bytes.resize(header_bytes.len() + data_padded_len, 0u8);

        let total = checksum_blocks(&hdu_bytes);
        assert!(
            total == 0 || total == 0xFFFFFFFF,
            "HDU checksum should be -0, got {total:#010X}"
        );
    }

    #[test]
    fn stamp_empty_data() {
        use crate::primary::build_primary_header;

        let cards = build_primary_header(8, &[]).unwrap();
        let stamped = stamp_checksum(&cards, &[]);

        let datasum_card = stamped
            .iter()
            .find(|c| c.keyword_str() == "DATASUM")
            .unwrap();
        let datasum_str = match &datasum_card.value {
            Some(Value::String(s)) => s.clone(),
            _ => panic!("DATASUM should be a string"),
        };
        assert_eq!(datasum_str, "0");

        // Verify total HDU checksum.
        let header_bytes = serialize_header(&stamped).unwrap();
        let total = checksum_blocks(&header_bytes);
        assert!(
            total == 0 || total == 0xFFFFFFFF,
            "HDU checksum should be -0, got {total:#010X}"
        );
    }

    #[test]
    fn stamp_replaces_existing_keywords() {
        use crate::primary::build_primary_header;

        let mut cards = build_primary_header(8, &[4]).unwrap();
        // Add fake existing CHECKSUM/DATASUM.
        cards.push(Card {
            keyword: make_keyword(b"DATASUM"),
            value: Some(Value::String(String::from("999"))),
            comment: None,
        });
        cards.push(Card {
            keyword: make_keyword(b"CHECKSUM"),
            value: Some(Value::String(String::from("AAAAAAAAAAAAAAAA"))),
            comment: None,
        });

        let data = vec![0u8; 4];
        let stamped = stamp_checksum(&cards, &data);

        // Should have exactly one of each.
        let checksum_count = stamped
            .iter()
            .filter(|c| c.keyword_str() == "CHECKSUM")
            .count();
        let datasum_count = stamped
            .iter()
            .filter(|c| c.keyword_str() == "DATASUM")
            .count();
        assert_eq!(checksum_count, 1);
        assert_eq!(datasum_count, 1);
    }

    #[test]
    fn stamp_then_parse_and_verify() {
        use crate::hdu::parse_fits;
        use crate::header::serialize_header;
        use crate::primary::build_primary_header;

        let cards = build_primary_header(16, &[20, 10]).unwrap();
        let data = vec![0xABu8; 400]; // 20*10*2 bytes for BITPIX=16

        let stamped = stamp_checksum(&cards, &data);
        let header_bytes = serialize_header(&stamped).unwrap();
        let data_padded_len = padded_byte_len(data.len());
        let mut fits_bytes = Vec::with_capacity(header_bytes.len() + data_padded_len);
        fits_bytes.extend_from_slice(&header_bytes);
        fits_bytes.extend_from_slice(&data);
        fits_bytes.resize(header_bytes.len() + data_padded_len, 0u8);

        let fits = parse_fits(&fits_bytes).unwrap();
        let hdu = fits.primary();

        assert!(
            verify_datasum(&fits_bytes, hdu),
            "DATASUM verification failed"
        );
        assert!(
            verify_checksum(&fits_bytes, hdu),
            "CHECKSUM verification failed"
        );
    }

    #[test]
    fn verify_fails_on_corrupted_data() {
        use crate::hdu::parse_fits;
        use crate::header::serialize_header;
        use crate::primary::build_primary_header;

        let cards = build_primary_header(8, &[100]).unwrap();
        let data = vec![0u8; 100];

        let stamped = stamp_checksum(&cards, &data);
        let header_bytes = serialize_header(&stamped).unwrap();
        let data_padded_len = padded_byte_len(data.len());
        let mut fits_bytes = Vec::with_capacity(header_bytes.len() + data_padded_len);
        fits_bytes.extend_from_slice(&header_bytes);
        fits_bytes.extend_from_slice(&data);
        fits_bytes.resize(header_bytes.len() + data_padded_len, 0u8);

        // Corrupt the data.
        fits_bytes[header_bytes.len()] = 0xFF;

        let fits = parse_fits(&fits_bytes).unwrap();
        let hdu = fits.primary();

        assert!(!verify_datasum(&fits_bytes, hdu), "DATASUM should fail");
        assert!(!verify_checksum(&fits_bytes, hdu), "CHECKSUM should fail");
    }
}
