//! Primary HDU header parsing and construction.

use alloc::string::String;
use alloc::vec::Vec;

use crate::block::padded_byte_len;
#[cfg(test)]
use crate::block::BLOCK_SIZE;
use crate::error::{Error, Result};
use crate::header::{validate_required_keywords, Card, HduType};
use crate::value::Value;

const VALID_BITPIX: [i64; 6] = [8, 16, 32, 64, -32, -64];

/// Pad a short keyword name to 8 bytes with trailing ASCII spaces.
const fn kw(name: &[u8]) -> [u8; 8] {
    let mut buf = [b' '; 8];
    let mut i = 0;
    while i < name.len() && i < 8 {
        buf[i] = name[i];
        i += 1;
    }
    buf
}

const KW_BITPIX: [u8; 8] = kw(b"BITPIX");
const KW_NAXIS: [u8; 8] = kw(b"NAXIS");

fn find_keyword<'a>(cards: &'a [Card], name: &[u8; 8]) -> Option<&'a Card> {
    cards.iter().find(|c| &c.keyword == name)
}

fn naxis_keyword(n: usize) -> [u8; 8] {
    let s = alloc::format!("NAXIS{}", n);
    let mut buf = [b' '; 8];
    let bytes = s.as_bytes();
    let len = bytes.len().min(8);
    buf[..len].copy_from_slice(&bytes[..len]);
    buf
}

fn extract_integer(card: &Card, name: &'static str) -> Result<i64> {
    match &card.value {
        Some(Value::Integer(n)) => Ok(*n),
        _ => Err(Error::MissingKeyword(name)),
    }
}

/// Parsed primary HDU header.
#[derive(Debug, Clone, PartialEq)]
pub struct PrimaryHeader {
    /// BITPIX value (8, 16, 32, 64, -32, -64).
    pub bitpix: i64,
    /// Number of axes.
    pub naxis: usize,
    /// Dimensions (NAXIS1, NAXIS2, ..., NAXISn).
    pub naxes: Vec<usize>,
    /// All header cards for reference.
    pub cards: Vec<Card>,
}

impl PrimaryHeader {
    /// Compute the number of data bytes: `|BITPIX/8| * NAXIS1 * ... * NAXISn`.
    ///
    /// Returns 0 if NAXIS is 0 or any axis has length 0.
    pub fn data_byte_count(&self) -> usize {
        if self.naxis == 0 || self.naxes.is_empty() {
            return 0;
        }
        let bytes_per_value = (self.bitpix.unsigned_abs() / 8) as usize;
        self.naxes
            .iter()
            .copied()
            .fold(bytes_per_value, |acc, dim| acc * dim)
    }

    /// Compute the padded data size (always a multiple of 2880).
    pub fn data_padded_byte_count(&self) -> usize {
        padded_byte_len(self.data_byte_count())
    }
}

/// Parse and validate a primary HDU header from a list of cards.
pub fn parse_primary_header(cards: &[Card]) -> Result<PrimaryHeader> {
    validate_required_keywords(HduType::Primary, cards)?;

    let bitpix_card = find_keyword(cards, &KW_BITPIX).ok_or(Error::MissingKeyword("BITPIX"))?;
    let bitpix = extract_integer(bitpix_card, "BITPIX")?;
    if !VALID_BITPIX.contains(&bitpix) {
        return Err(Error::InvalidBitpix(bitpix));
    }

    let naxis_card = find_keyword(cards, &KW_NAXIS).ok_or(Error::MissingKeyword("NAXIS"))?;
    let naxis_val = extract_integer(naxis_card, "NAXIS")?;
    if naxis_val < 0 {
        return Err(Error::InvalidHeader("negative NAXIS"));
    }
    let naxis = naxis_val as usize;

    let mut naxes = Vec::with_capacity(naxis);
    for i in 1..=naxis {
        let kw_name = naxis_keyword(i);
        let card = find_keyword(cards, &kw_name).ok_or(Error::MissingKeyword("NAXISn"))?;
        let val = extract_integer(card, "NAXISn")?;
        if val < 0 {
            return Err(Error::InvalidHeader("negative NAXISn"));
        }
        naxes.push(val as usize);
    }

    Ok(PrimaryHeader {
        bitpix,
        naxis,
        naxes,
        cards: cards.to_vec(),
    })
}

/// Build the minimal set of cards for a primary HDU header.
pub fn build_primary_header(bitpix: i64, naxes: &[usize]) -> Result<Vec<Card>> {
    if !VALID_BITPIX.contains(&bitpix) {
        return Err(Error::InvalidBitpix(bitpix));
    }

    let mut cards = Vec::new();

    cards.push(Card {
        keyword: kw(b"SIMPLE"),
        value: Some(Value::Logical(true)),
        comment: Some(String::from("conforms to FITS standard")),
    });

    cards.push(Card {
        keyword: kw(b"BITPIX"),
        value: Some(Value::Integer(bitpix)),
        comment: Some(String::from("bits per data value")),
    });

    cards.push(Card {
        keyword: kw(b"NAXIS"),
        value: Some(Value::Integer(naxes.len() as i64)),
        comment: Some(String::from("number of axes")),
    });

    for (i, &dim) in naxes.iter().enumerate() {
        cards.push(Card {
            keyword: naxis_keyword(i + 1),
            value: Some(Value::Integer(dim as i64)),
            comment: None,
        });
    }

    Ok(cards)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn card(keyword: &[u8], value: Option<Value>) -> Card {
        Card {
            keyword: kw(keyword),
            value,
            comment: None,
        }
    }

    #[test]
    fn parse_valid_2d_image() {
        let cards = [
            card(b"SIMPLE", Some(Value::Logical(true))),
            card(b"BITPIX", Some(Value::Integer(16))),
            card(b"NAXIS", Some(Value::Integer(2))),
            card(b"NAXIS1", Some(Value::Integer(100))),
            card(b"NAXIS2", Some(Value::Integer(200))),
        ];
        let hdr = parse_primary_header(&cards).unwrap();
        assert_eq!(hdr.bitpix, 16);
        assert_eq!(hdr.naxis, 2);
        assert_eq!(hdr.naxes, vec![100, 200]);
    }

    #[test]
    fn parse_zero_axis() {
        let cards = [
            card(b"SIMPLE", Some(Value::Logical(true))),
            card(b"BITPIX", Some(Value::Integer(8))),
            card(b"NAXIS", Some(Value::Integer(0))),
        ];
        let hdr = parse_primary_header(&cards).unwrap();
        assert_eq!(hdr.bitpix, 8);
        assert_eq!(hdr.naxis, 0);
        assert!(hdr.naxes.is_empty());
        assert_eq!(hdr.data_byte_count(), 0);
    }

    #[test]
    fn data_byte_count_16bit_2d() {
        let cards = [
            card(b"SIMPLE", Some(Value::Logical(true))),
            card(b"BITPIX", Some(Value::Integer(16))),
            card(b"NAXIS", Some(Value::Integer(2))),
            card(b"NAXIS1", Some(Value::Integer(100))),
            card(b"NAXIS2", Some(Value::Integer(200))),
        ];
        let hdr = parse_primary_header(&cards).unwrap();
        assert_eq!(hdr.data_byte_count(), 2 * 100 * 200);
    }

    #[test]
    fn data_byte_count_8bit_1d() {
        let cards = [
            card(b"SIMPLE", Some(Value::Logical(true))),
            card(b"BITPIX", Some(Value::Integer(8))),
            card(b"NAXIS", Some(Value::Integer(1))),
            card(b"NAXIS1", Some(Value::Integer(1000))),
        ];
        let hdr = parse_primary_header(&cards).unwrap();
        assert_eq!(hdr.data_byte_count(), 1000);
    }

    #[test]
    fn data_byte_count_32bit_3d() {
        let cards = [
            card(b"SIMPLE", Some(Value::Logical(true))),
            card(b"BITPIX", Some(Value::Integer(32))),
            card(b"NAXIS", Some(Value::Integer(3))),
            card(b"NAXIS1", Some(Value::Integer(10))),
            card(b"NAXIS2", Some(Value::Integer(20))),
            card(b"NAXIS3", Some(Value::Integer(30))),
        ];
        let hdr = parse_primary_header(&cards).unwrap();
        assert_eq!(hdr.data_byte_count(), 4 * 10 * 20 * 30);
    }

    #[test]
    fn data_byte_count_64bit() {
        let cards = [
            card(b"SIMPLE", Some(Value::Logical(true))),
            card(b"BITPIX", Some(Value::Integer(64))),
            card(b"NAXIS", Some(Value::Integer(1))),
            card(b"NAXIS1", Some(Value::Integer(50))),
        ];
        let hdr = parse_primary_header(&cards).unwrap();
        assert_eq!(hdr.data_byte_count(), 8 * 50);
    }

    #[test]
    fn data_byte_count_float32() {
        let cards = [
            card(b"SIMPLE", Some(Value::Logical(true))),
            card(b"BITPIX", Some(Value::Integer(-32))),
            card(b"NAXIS", Some(Value::Integer(2))),
            card(b"NAXIS1", Some(Value::Integer(64))),
            card(b"NAXIS2", Some(Value::Integer(64))),
        ];
        let hdr = parse_primary_header(&cards).unwrap();
        assert_eq!(hdr.data_byte_count(), 4 * 64 * 64);
    }

    #[test]
    fn data_byte_count_float64() {
        let cards = [
            card(b"SIMPLE", Some(Value::Logical(true))),
            card(b"BITPIX", Some(Value::Integer(-64))),
            card(b"NAXIS", Some(Value::Integer(1))),
            card(b"NAXIS1", Some(Value::Integer(128))),
        ];
        let hdr = parse_primary_header(&cards).unwrap();
        assert_eq!(hdr.data_byte_count(), 8 * 128);
    }

    #[test]
    fn padded_byte_count_is_multiple_of_block_size() {
        let test_cases: &[(i64, &[usize])] = &[
            (8, &[1]),
            (16, &[100, 200]),
            (32, &[10, 20, 30]),
            (-32, &[64, 64]),
            (-64, &[128]),
            (8, &[]),
        ];
        for &(bitpix, naxes) in test_cases {
            let cards_result = build_primary_header(bitpix, naxes);
            let cards = cards_result.unwrap();
            let hdr = parse_primary_header(&cards).unwrap();
            let padded = hdr.data_padded_byte_count();
            if padded > 0 {
                assert_eq!(
                    padded % BLOCK_SIZE,
                    0,
                    "padded byte count {} not a multiple of {} for BITPIX={} NAXES={:?}",
                    padded,
                    BLOCK_SIZE,
                    bitpix,
                    naxes
                );
            }
        }
    }

    #[test]
    fn padded_byte_count_zero_for_no_data() {
        let cards = [
            card(b"SIMPLE", Some(Value::Logical(true))),
            card(b"BITPIX", Some(Value::Integer(8))),
            card(b"NAXIS", Some(Value::Integer(0))),
        ];
        let hdr = parse_primary_header(&cards).unwrap();
        assert_eq!(hdr.data_padded_byte_count(), 0);
    }

    #[test]
    fn padded_byte_count_rounds_up() {
        let cards = [
            card(b"SIMPLE", Some(Value::Logical(true))),
            card(b"BITPIX", Some(Value::Integer(8))),
            card(b"NAXIS", Some(Value::Integer(1))),
            card(b"NAXIS1", Some(Value::Integer(1))),
        ];
        let hdr = parse_primary_header(&cards).unwrap();
        assert_eq!(hdr.data_byte_count(), 1);
        assert_eq!(hdr.data_padded_byte_count(), BLOCK_SIZE);
    }

    #[test]
    fn build_then_parse_roundtrip() {
        let naxes = &[512, 256];
        let cards = build_primary_header(-32, naxes).unwrap();
        let hdr = parse_primary_header(&cards).unwrap();
        assert_eq!(hdr.bitpix, -32);
        assert_eq!(hdr.naxis, 2);
        assert_eq!(hdr.naxes, vec![512, 256]);
        assert_eq!(hdr.data_byte_count(), 4 * 512 * 256);
    }

    #[test]
    fn build_then_parse_zero_axes() {
        let cards = build_primary_header(8, &[]).unwrap();
        let hdr = parse_primary_header(&cards).unwrap();
        assert_eq!(hdr.bitpix, 8);
        assert_eq!(hdr.naxis, 0);
        assert!(hdr.naxes.is_empty());
    }

    #[test]
    fn build_then_parse_all_bitpix_values() {
        for &bp in &VALID_BITPIX {
            let cards = build_primary_header(bp, &[10]).unwrap();
            let hdr = parse_primary_header(&cards).unwrap();
            assert_eq!(hdr.bitpix, bp);
        }
    }

    #[test]
    fn error_invalid_bitpix_parse() {
        let cards = [
            card(b"SIMPLE", Some(Value::Logical(true))),
            card(b"BITPIX", Some(Value::Integer(7))),
            card(b"NAXIS", Some(Value::Integer(0))),
        ];
        let err = parse_primary_header(&cards).unwrap_err();
        assert!(matches!(err, Error::InvalidBitpix(7)));
    }

    #[test]
    fn error_invalid_bitpix_build() {
        let err = build_primary_header(12, &[10]).unwrap_err();
        assert!(matches!(err, Error::InvalidBitpix(12)));
    }

    #[test]
    fn error_missing_simple() {
        let cards = [
            card(b"BITPIX", Some(Value::Integer(16))),
            card(b"NAXIS", Some(Value::Integer(0))),
        ];
        assert!(parse_primary_header(&cards).is_err());
    }

    #[test]
    fn error_missing_bitpix() {
        let cards = [
            card(b"SIMPLE", Some(Value::Logical(true))),
            card(b"NAXIS", Some(Value::Integer(0))),
        ];
        assert!(parse_primary_header(&cards).is_err());
    }

    #[test]
    fn error_missing_naxis() {
        let cards = [
            card(b"SIMPLE", Some(Value::Logical(true))),
            card(b"BITPIX", Some(Value::Integer(16))),
        ];
        assert!(parse_primary_header(&cards).is_err());
    }

    #[test]
    fn error_missing_naxis_n() {
        let cards = [
            card(b"SIMPLE", Some(Value::Logical(true))),
            card(b"BITPIX", Some(Value::Integer(16))),
            card(b"NAXIS", Some(Value::Integer(2))),
            card(b"NAXIS1", Some(Value::Integer(100))),
        ];
        assert!(parse_primary_header(&cards).is_err());
    }

    #[test]
    fn cards_are_stored() {
        let cards = [
            card(b"SIMPLE", Some(Value::Logical(true))),
            card(b"BITPIX", Some(Value::Integer(8))),
            card(b"NAXIS", Some(Value::Integer(0))),
        ];
        let hdr = parse_primary_header(&cards).unwrap();
        assert_eq!(hdr.cards.len(), 3);
        assert_eq!(hdr.cards[0].keyword_str(), "SIMPLE");
    }

    #[test]
    fn build_produces_correct_card_count() {
        let cards = build_primary_header(16, &[100, 200]).unwrap();
        assert_eq!(cards.len(), 5);
        assert_eq!(cards[0].keyword_str(), "SIMPLE");
        assert_eq!(cards[1].keyword_str(), "BITPIX");
        assert_eq!(cards[2].keyword_str(), "NAXIS");
        assert_eq!(cards[3].keyword_str(), "NAXIS1");
        assert_eq!(cards[4].keyword_str(), "NAXIS2");
    }

    #[test]
    fn build_with_no_axes_produces_three_cards() {
        let cards = build_primary_header(8, &[]).unwrap();
        assert_eq!(cards.len(), 3);
    }
}
