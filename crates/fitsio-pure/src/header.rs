//! FITS header card parsing, writing, and validation.

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use core::str;

use crate::block::{BLOCK_SIZE, CARDS_PER_BLOCK, CARD_SIZE, HEADER_PAD_BYTE};
use crate::error::{Error, Result};
use crate::value::{format_value, parse_value, Value};

// ── Types ──

/// A parsed FITS header card (one 80-byte keyword record).
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    /// The 8-byte keyword name, ASCII, left-justified, space-padded.
    pub keyword: [u8; 8],
    /// The parsed value, if this card has a value indicator (`= ` in bytes 8..10).
    pub value: Option<Value>,
    /// An optional comment string.
    pub comment: Option<String>,
}

impl Card {
    /// Return the keyword as a trimmed UTF-8 string.
    pub fn keyword_str(&self) -> &str {
        let end = self
            .keyword
            .iter()
            .rposition(|&b| b != b' ')
            .map(|i| i + 1)
            .unwrap_or(0);
        str::from_utf8(&self.keyword[..end]).unwrap_or("")
    }

    /// Returns `true` if this card is the END keyword.
    pub fn is_end(&self) -> bool {
        &self.keyword == b"END     "
    }

    /// Returns `true` if this is a blank card (keyword is all spaces).
    pub fn is_blank(&self) -> bool {
        self.keyword.iter().all(|&b| b == b' ')
    }

    /// Returns `true` if this card carries a commentary keyword
    /// (COMMENT, HISTORY, or blank).
    pub fn is_commentary(&self) -> bool {
        let kw = self.keyword_str();
        kw == "COMMENT" || kw == "HISTORY" || self.is_blank()
    }
}

/// The type of HDU, which determines required keywords per the FITS standard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HduType {
    /// Primary HDU (starts with SIMPLE=T).
    Primary,
    /// Image extension.
    Image,
    /// ASCII table extension.
    AsciiTable,
    /// Binary table extension.
    BinaryTable,
}

// ── Parsing ──

/// Keywords that never carry a value indicator. Their bytes 8..80 are free-form text.
const COMMENTARY_KEYWORDS: [&[u8; 8]; 3] = [b"COMMENT ", b"HISTORY ", b"        "];

/// Returns `true` if `keyword` is a commentary keyword (COMMENT, HISTORY, or blank).
fn is_commentary_keyword(keyword: &[u8; 8]) -> bool {
    COMMENTARY_KEYWORDS.contains(&keyword)
}

/// Parse a single 80-byte FITS header card.
pub fn parse_card(card_bytes: &[u8; CARD_SIZE]) -> Result<Card> {
    let mut keyword = [b' '; 8];
    keyword.copy_from_slice(&card_bytes[..8]);

    for &b in &keyword {
        match b {
            b'A'..=b'Z' | b'0'..=b'9' | b' ' | b'-' | b'_' => {}
            _ => return Err(Error::InvalidKeyword),
        }
    }

    if &keyword == b"END     " {
        return Ok(Card {
            keyword,
            value: None,
            comment: None,
        });
    }

    if is_commentary_keyword(&keyword) {
        let text_bytes = &card_bytes[8..CARD_SIZE];
        let text = str::from_utf8(text_bytes)
            .map_err(|_| Error::InvalidHeader("non-UTF8 card data"))?
            .trim_end();
        let comment = if text.is_empty() {
            None
        } else {
            Some(String::from(text))
        };
        return Ok(Card {
            keyword,
            value: None,
            comment,
        });
    }

    if card_bytes[8] == b'=' && card_bytes[9] == b' ' {
        let value_field = &card_bytes[10..CARD_SIZE];
        match parse_value(value_field) {
            Some((val, comment)) => Ok(Card {
                keyword,
                value: Some(val),
                comment: comment.map(String::from),
            }),
            None => {
                let field_str = str::from_utf8(value_field)
                    .map_err(|_| Error::InvalidHeader("non-UTF8 card data"))?;
                let comment = extract_comment_from_empty_value(field_str);
                Ok(Card {
                    keyword,
                    value: None,
                    comment,
                })
            }
        }
    } else {
        let text_bytes = &card_bytes[8..CARD_SIZE];
        let text = str::from_utf8(text_bytes)
            .map_err(|_| Error::InvalidHeader("non-UTF8 card data"))?
            .trim_end();
        let comment = if text.is_empty() {
            None
        } else {
            Some(String::from(text))
        };
        Ok(Card {
            keyword,
            value: None,
            comment,
        })
    }
}

fn extract_comment_from_empty_value(field: &str) -> Option<String> {
    if let Some(idx) = field.find(" /") {
        // Skip the slash; also skip one optional space after it.
        let after_slash = idx + 2;
        let comment_start = if field.as_bytes().get(after_slash) == Some(&b' ') {
            after_slash + 1
        } else {
            after_slash
        };
        let comment = field[comment_start..].trim_end();
        if !comment.is_empty() {
            return Some(String::from(comment));
        }
    }
    None
}

const KW_CONTINUE: [u8; 8] = *b"CONTINUE";

/// Parse consecutive 2880-byte header blocks until the END card is found.
///
/// The input data does not need to be an exact multiple of [`BLOCK_SIZE`].
/// Only complete 2880-byte blocks are scanned; any trailing bytes shorter
/// than a full block are ignored.  This allows parsing headers from files
/// whose total size is not block-aligned (e.g. HiPS tiles that omit
/// trailing padding).
///
/// String values that use the CONTINUE long-string convention (HEASARC)
/// are automatically concatenated: when a string value ends with `&` and
/// is followed by one or more `CONTINUE` keyword cards, the values are
/// merged into a single `Value::String`.
pub fn parse_header_blocks(data: &[u8]) -> Result<Vec<Card>> {
    if data.len() < BLOCK_SIZE {
        return Err(Error::UnexpectedEof);
    }

    let mut cards = Vec::new();
    let num_blocks = data.len() / BLOCK_SIZE;

    for block_idx in 0..num_blocks {
        let block_start = block_idx * BLOCK_SIZE;
        for card_idx in 0..CARDS_PER_BLOCK {
            let card_start = block_start + card_idx * CARD_SIZE;
            let card_bytes: &[u8; CARD_SIZE] = data[card_start..card_start + CARD_SIZE]
                .try_into()
                .map_err(|_| Error::InvalidHeader("non-UTF8 card data"))?;

            let card = parse_card(card_bytes)?;
            let is_end = card.is_end();
            cards.push(card);

            if is_end {
                merge_continue_cards(&mut cards);
                return Ok(cards);
            }
        }
    }

    Err(Error::UnexpectedEof)
}

/// Merge CONTINUE cards into the preceding card's string value.
///
/// A string ending with `&` followed by one or more CONTINUE cards forms
/// a single long string. The `&` markers and CONTINUE cards are removed;
/// the preceding card's value receives the concatenated string.
fn merge_continue_cards(cards: &mut Vec<Card>) {
    let mut i = 0;
    while i < cards.len() {
        let has_continuation = match &cards[i].value {
            Some(Value::String(s)) => s.ends_with('&'),
            _ => false,
        };
        if !has_continuation {
            i += 1;
            continue;
        }

        // Strip trailing '&' from the base string.
        let mut combined = match &cards[i].value {
            Some(Value::String(s)) => {
                let mut s = s.clone();
                s.pop(); // remove '&'
                s
            }
            _ => unreachable!(),
        };

        // Absorb subsequent CONTINUE cards.
        let mut j = i + 1;
        while j < cards.len() && cards[j].keyword == KW_CONTINUE {
            // CONTINUE cards store their string in the comment field
            // (no `= ` indicator), or sometimes use a value field.
            let cont_str = extract_continue_string(&cards[j]);
            let ends_with_amp = cont_str.ends_with('&');
            if ends_with_amp {
                combined.push_str(&cont_str[..cont_str.len() - 1]);
            } else {
                combined.push_str(&cont_str);
            }
            j += 1;
            if !ends_with_amp {
                break;
            }
        }

        // Update the base card with the merged string.
        cards[i].value = Some(Value::String(combined));
        // Remove the consumed CONTINUE cards.
        cards.drain(i + 1..j);
        i += 1;
    }
}

/// Extract the string payload from a CONTINUE card.
///
/// CONTINUE cards may carry their value in two forms:
/// 1. As a quoted string in bytes 8..80 (no `= ` indicator but starts with
///    optional spaces then `'`).
/// 2. As a regular value field if the card was parsed with a value.
fn extract_continue_string(card: &Card) -> String {
    // If the parser found a value (some files use `= ` on CONTINUE), use it.
    if let Some(Value::String(s)) = &card.value {
        return s.clone();
    }
    // Otherwise the string lives in the comment field (raw text after keyword).
    // Try to parse it as a quoted string.
    if let Some(text) = &card.comment {
        let trimmed = text.trim_start();
        if trimmed.starts_with('\'') {
            if let Some(s) = parse_continue_quoted(trimmed) {
                return s;
            }
        }
        return trimmed.to_string();
    }
    String::new()
}

/// Parse a quoted string from CONTINUE card text (bytes 8..80 as a &str).
fn parse_continue_quoted(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes[0] != b'\'' {
        return None;
    }
    let mut value = String::new();
    let mut i = 1;
    let len = bytes.len();
    loop {
        if i >= len {
            break;
        }
        if bytes[i] == b'\'' {
            if i + 1 < len && bytes[i + 1] == b'\'' {
                value.push('\'');
                i += 2;
            } else {
                break;
            }
        } else {
            value.push(bytes[i] as char);
            i += 1;
        }
    }
    Some(value.trim_end().to_string())
}

/// Return the number of bytes consumed by the header (always a multiple of BLOCK_SIZE).
///
/// The input data does not need to be block-aligned.  Only complete
/// 2880-byte blocks are scanned for the END card.
pub fn header_byte_len(data: &[u8]) -> Result<usize> {
    if data.len() < BLOCK_SIZE {
        return Err(Error::UnexpectedEof);
    }

    let num_blocks = data.len() / BLOCK_SIZE;

    for block_idx in 0..num_blocks {
        let block_start = block_idx * BLOCK_SIZE;
        for card_idx in 0..CARDS_PER_BLOCK {
            let card_start = block_start + card_idx * CARD_SIZE;
            let keyword = &data[card_start..card_start + 8];
            if keyword == b"END     " {
                return Ok((block_idx + 1) * BLOCK_SIZE);
            }
        }
    }

    Err(Error::UnexpectedEof)
}

// ── Writing ──

/// Serialize a [`Card`] into an 80-byte FITS card image.
pub fn format_card(card: &Card) -> [u8; CARD_SIZE] {
    let mut buf = [b' '; CARD_SIZE];

    for (i, &b) in card.keyword.iter().enumerate() {
        buf[i] = b;
    }

    let is_blank = card.keyword == [b' '; 8];

    if let Some(ref value) = card.value {
        buf[8] = b'=';
        buf[9] = b' ';

        let value_field = format_value(value);

        if let Some(ref comment) = card.comment {
            let mut merged = value_field;
            insert_comment(&mut merged, comment);
            buf[10..80].copy_from_slice(&merged);
        } else {
            buf[10..80].copy_from_slice(&value_field);
        }
    } else if !is_blank {
        if let Some(ref comment) = card.comment {
            let bytes = comment.as_bytes();
            let len = bytes.len().min(72);
            buf[8..8 + len].copy_from_slice(&bytes[..len]);
        }
    }

    buf
}

/// Insert a ` / comment` string into a 70-byte value field.
fn insert_comment(field: &mut [u8; 70], comment: &str) {
    let content_end = if field[0] == b'\'' {
        let mut i = 1;
        loop {
            if i >= 70 {
                break i;
            }
            if field[i] == b'\'' {
                if i + 1 < 70 && field[i + 1] == b'\'' {
                    i += 2;
                } else {
                    break i + 1;
                }
            } else {
                i += 1;
            }
        }
    } else {
        20
    };

    let sep_start = content_end + 1;
    if sep_start + 3 >= 70 {
        return;
    }

    field[sep_start] = b'/';
    field[sep_start + 1] = b' ';

    let comment_start = sep_start + 2;
    let comment_bytes = comment.as_bytes();
    let max_len = 70 - comment_start;
    let len = comment_bytes.len().min(max_len);
    field[comment_start..comment_start + len].copy_from_slice(&comment_bytes[..len]);
}

/// Create the standard FITS END card.
pub fn format_end_card() -> [u8; CARD_SIZE] {
    let mut buf = [b' '; CARD_SIZE];
    buf[0] = b'E';
    buf[1] = b'N';
    buf[2] = b'D';
    buf
}

/// Detect the HDU type from the cards in a header.
///
/// Looks for `SIMPLE` or `XTENSION` anywhere in the card list (not just
/// position 0) so that misordered headers are still detected and rejected
/// by the subsequent validation step.
///
/// Returns `None` if neither keyword is present.
fn detect_hdu_type(cards: &[Card]) -> Option<HduType> {
    if find_keyword(cards, &KW_SIMPLE).is_some() {
        return Some(HduType::Primary);
    }
    if let Some(xt) = find_keyword(cards, &KW_XTENSION) {
        match &xt.value {
            Some(Value::String(s)) => match s.trim() {
                "IMAGE" => Some(HduType::Image),
                "TABLE" => Some(HduType::AsciiTable),
                "BINTABLE" => Some(HduType::BinaryTable),
                _ => None,
            },
            _ => None,
        }
    } else {
        None
    }
}

/// Serialize a sequence of header cards into complete FITS header blocks.
///
/// Validates that mandatory keywords are present and correctly ordered
/// per the FITS standard before serializing. Returns an error if the
/// header does not conform.
///
/// Appends the END card and pads the final block with blank cards.
/// The returned length is always a multiple of [`BLOCK_SIZE`].
pub fn serialize_header(cards: &[Card]) -> Result<Vec<u8>> {
    if let Some(hdu_type) = detect_hdu_type(cards) {
        validate_required_keywords(hdu_type, cards)?;
    }

    let total_cards = cards.len() + 1; // +1 for END
    let total_blocks = total_cards.div_ceil(CARDS_PER_BLOCK);
    let total_bytes = total_blocks * BLOCK_SIZE;

    let mut buf = vec![HEADER_PAD_BYTE; total_bytes];

    for (i, card) in cards.iter().enumerate() {
        let offset = i * CARD_SIZE;
        let formatted = format_card(card);
        buf[offset..offset + CARD_SIZE].copy_from_slice(&formatted);
    }

    let end_offset = cards.len() * CARD_SIZE;
    let end_card = format_end_card();
    buf[end_offset..end_offset + CARD_SIZE].copy_from_slice(&end_card);

    Ok(buf)
}

// ── Validation ──

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

const KW_SIMPLE: [u8; 8] = kw(b"SIMPLE");
const KW_BITPIX: [u8; 8] = kw(b"BITPIX");
const KW_NAXIS: [u8; 8] = kw(b"NAXIS");
const KW_NAXIS1: [u8; 8] = kw(b"NAXIS1");
const KW_NAXIS2: [u8; 8] = kw(b"NAXIS2");
const KW_XTENSION: [u8; 8] = kw(b"XTENSION");
const KW_PCOUNT: [u8; 8] = kw(b"PCOUNT");
const KW_GCOUNT: [u8; 8] = kw(b"GCOUNT");
const KW_TFIELDS: [u8; 8] = kw(b"TFIELDS");

fn find_keyword<'a>(cards: &'a [Card], name: &[u8; 8]) -> Option<&'a Card> {
    cards.iter().find(|c| &c.keyword == name)
}

fn require_keyword_at<'a>(
    cards: &'a [Card],
    index: usize,
    expected: &[u8; 8],
    name: &'static str,
) -> Result<&'a Card> {
    let card = cards.get(index).ok_or(Error::MissingKeyword(name))?;
    if &card.keyword != expected {
        return Err(Error::MissingKeyword(name));
    }
    Ok(card)
}

fn require_keyword_present<'a>(
    cards: &'a [Card],
    expected: &[u8; 8],
    name: &'static str,
) -> Result<&'a Card> {
    find_keyword(cards, expected).ok_or(Error::MissingKeyword(name))
}

fn require_logical(card: &Card, expected: bool) -> Result<()> {
    match &card.value {
        Some(Value::Logical(b)) if *b == expected => Ok(()),
        _ => Err(Error::InvalidHeader("unexpected logical value")),
    }
}

fn require_integer(card: &Card, expected: i64) -> Result<()> {
    match &card.value {
        Some(Value::Integer(n)) if *n == expected => Ok(()),
        _ => Err(Error::InvalidHeader("unexpected integer value")),
    }
}

fn require_string(card: &Card, expected: &str) -> Result<()> {
    match &card.value {
        Some(Value::String(s)) if s.trim() == expected => Ok(()),
        _ => Err(Error::InvalidHeader("unexpected string value")),
    }
}

/// Validate that all mandatory keywords are present and correctly ordered
/// for the given HDU type, per the FITS standard.
pub fn validate_required_keywords(hdu_type: HduType, cards: &[Card]) -> Result<()> {
    match hdu_type {
        HduType::Primary => validate_primary(cards),
        HduType::Image => validate_image_extension(cards),
        HduType::AsciiTable => validate_ascii_table(cards),
        HduType::BinaryTable => validate_binary_table(cards),
    }
}

fn validate_primary(cards: &[Card]) -> Result<()> {
    let simple = require_keyword_at(cards, 0, &KW_SIMPLE, "SIMPLE")?;
    require_logical(simple, true)?;
    require_keyword_at(cards, 1, &KW_BITPIX, "BITPIX")?;
    require_keyword_at(cards, 2, &KW_NAXIS, "NAXIS")?;
    Ok(())
}

fn validate_image_extension(cards: &[Card]) -> Result<()> {
    let xtension = require_keyword_at(cards, 0, &KW_XTENSION, "XTENSION")?;
    require_string(xtension, "IMAGE")?;
    require_keyword_at(cards, 1, &KW_BITPIX, "BITPIX")?;
    require_keyword_at(cards, 2, &KW_NAXIS, "NAXIS")?;
    require_keyword_present(cards, &KW_PCOUNT, "PCOUNT")?;
    require_keyword_present(cards, &KW_GCOUNT, "GCOUNT")?;
    Ok(())
}

fn validate_ascii_table(cards: &[Card]) -> Result<()> {
    let xtension = require_keyword_at(cards, 0, &KW_XTENSION, "XTENSION")?;
    require_string(xtension, "TABLE")?;
    let bitpix = require_keyword_at(cards, 1, &KW_BITPIX, "BITPIX")?;
    require_integer(bitpix, 8)?;
    let naxis = require_keyword_at(cards, 2, &KW_NAXIS, "NAXIS")?;
    require_integer(naxis, 2)?;
    require_keyword_present(cards, &KW_NAXIS1, "NAXIS1")?;
    require_keyword_present(cards, &KW_NAXIS2, "NAXIS2")?;
    require_keyword_present(cards, &KW_PCOUNT, "PCOUNT")?;
    require_keyword_present(cards, &KW_GCOUNT, "GCOUNT")?;
    require_keyword_present(cards, &KW_TFIELDS, "TFIELDS")?;
    Ok(())
}

fn validate_binary_table(cards: &[Card]) -> Result<()> {
    let xtension = require_keyword_at(cards, 0, &KW_XTENSION, "XTENSION")?;
    require_string(xtension, "BINTABLE")?;
    let bitpix = require_keyword_at(cards, 1, &KW_BITPIX, "BITPIX")?;
    require_integer(bitpix, 8)?;
    let naxis = require_keyword_at(cards, 2, &KW_NAXIS, "NAXIS")?;
    require_integer(naxis, 2)?;
    require_keyword_present(cards, &KW_NAXIS1, "NAXIS1")?;
    require_keyword_present(cards, &KW_NAXIS2, "NAXIS2")?;
    require_keyword_present(cards, &KW_PCOUNT, "PCOUNT")?;
    require_keyword_present(cards, &KW_GCOUNT, "GCOUNT")?;
    require_keyword_present(cards, &KW_TFIELDS, "TFIELDS")?;
    Ok(())
}

// ── Tests ──

#[cfg(test)]
mod parse_tests {
    use super::*;
    use alloc::string::String;

    fn make_card(s: &str) -> [u8; CARD_SIZE] {
        let mut buf = [b' '; CARD_SIZE];
        let bytes = s.as_bytes();
        let len = bytes.len().min(CARD_SIZE);
        buf[..len].copy_from_slice(&bytes[..len]);
        buf
    }

    fn make_header_block(cards: &[[u8; CARD_SIZE]]) -> Vec<u8> {
        assert!(cards.len() <= CARDS_PER_BLOCK);
        let mut block = vec![b' '; BLOCK_SIZE];
        for (i, card) in cards.iter().enumerate() {
            let start = i * CARD_SIZE;
            block[start..start + CARD_SIZE].copy_from_slice(card);
        }
        block
    }

    #[test]
    fn parse_card_string_value() {
        let card = make_card("TELESCOP= 'Hubble  '           / telescope name");
        let c = parse_card(&card).unwrap();
        assert_eq!(c.keyword_str(), "TELESCOP");
        assert_eq!(c.value, Some(Value::String(String::from("Hubble"))));
        assert_eq!(c.comment, Some(String::from("telescope name")));
    }

    #[test]
    fn parse_card_string_no_comment() {
        let card = make_card("OBJECT  = 'NGC 1234'");
        let c = parse_card(&card).unwrap();
        assert_eq!(c.keyword_str(), "OBJECT");
        assert_eq!(c.value, Some(Value::String(String::from("NGC 1234"))));
        assert!(c.comment.is_none());
    }

    #[test]
    fn parse_card_integer_value() {
        let card = make_card("BITPIX  =                    16 / bits per pixel");
        let c = parse_card(&card).unwrap();
        assert_eq!(c.keyword_str(), "BITPIX");
        assert_eq!(c.value, Some(Value::Integer(16)));
        assert_eq!(c.comment, Some(String::from("bits per pixel")));
    }

    #[test]
    fn parse_card_integer_negative() {
        let card = make_card("BITPIX  =                   -32 / IEEE float");
        let c = parse_card(&card).unwrap();
        assert_eq!(c.value, Some(Value::Integer(-32)));
    }

    #[test]
    fn parse_card_float_value() {
        let card = make_card("CRVAL1  =            2.7315E+02 / temperature");
        let c = parse_card(&card).unwrap();
        match c.value {
            Some(Value::Float(f)) => assert!((f - 273.15).abs() < 1e-5),
            other => panic!("Expected Float, got {:?}", other),
        }
    }

    #[test]
    fn parse_card_logical_true() {
        let card = make_card("SIMPLE  =                    T / standard FITS");
        let c = parse_card(&card).unwrap();
        assert_eq!(c.value, Some(Value::Logical(true)));
    }

    #[test]
    fn parse_card_logical_false() {
        let card = make_card("EXTEND  =                    F");
        let c = parse_card(&card).unwrap();
        assert_eq!(c.value, Some(Value::Logical(false)));
    }

    #[test]
    fn parse_card_comment_keyword() {
        let card = make_card("COMMENT This is a comment about the FITS file.");
        let c = parse_card(&card).unwrap();
        assert_eq!(c.keyword_str(), "COMMENT");
        assert!(c.value.is_none());
        assert_eq!(
            c.comment,
            Some(String::from("This is a comment about the FITS file."))
        );
        assert!(c.is_commentary());
    }

    #[test]
    fn parse_card_history_keyword() {
        let card = make_card("HISTORY Created by fitsio-pure v0.1");
        let c = parse_card(&card).unwrap();
        assert_eq!(c.keyword_str(), "HISTORY");
        assert!(c.is_commentary());
    }

    #[test]
    fn parse_card_blank_keyword() {
        let card = make_card("        some free-form text here");
        let c = parse_card(&card).unwrap();
        assert!(c.is_blank());
        assert!(c.is_commentary());
    }

    #[test]
    fn parse_card_blank_keyword_empty() {
        let card = [b' '; CARD_SIZE];
        let c = parse_card(&card).unwrap();
        assert!(c.is_blank());
        assert!(c.comment.is_none());
    }

    #[test]
    fn parse_card_end() {
        let card = make_card("END");
        let c = parse_card(&card).unwrap();
        assert!(c.is_end());
    }

    #[test]
    fn parse_card_invalid_keyword_lowercase() {
        let card = make_card("bitpix  =                    16");
        assert!(matches!(parse_card(&card), Err(Error::InvalidKeyword)));
    }

    #[test]
    fn parse_card_invalid_keyword_special_chars() {
        let card = make_card("FOO@BAR =                    16");
        assert!(parse_card(&card).is_err());
    }

    #[test]
    fn parse_card_empty_value_with_comment() {
        let card = make_card("BLANK   =                      / undefined value");
        let c = parse_card(&card).unwrap();
        assert!(c.value.is_none());
        assert_eq!(c.comment, Some(String::from("undefined value")));
    }

    #[test]
    fn parse_card_hyphen_keyword() {
        let card = make_card("DATE-OBS= '2024-01-15'");
        let c = parse_card(&card).unwrap();
        assert_eq!(c.keyword_str(), "DATE-OBS");
    }

    #[test]
    fn parse_card_string_with_embedded_quotes() {
        let card = make_card("COMMENT1= 'it''s ok '");
        let c = parse_card(&card).unwrap();
        assert_eq!(c.value, Some(Value::String(String::from("it's ok"))));
    }

    #[test]
    fn parse_header_simple() {
        let cards = [
            make_card("SIMPLE  =                    T / conforms to FITS standard"),
            make_card("BITPIX  =                   16 / 16-bit integers"),
            make_card("NAXIS   =                    2 / number of axes"),
            make_card("NAXIS1  =                  100 / width"),
            make_card("NAXIS2  =                  200 / height"),
            make_card("END"),
        ];
        let block = make_header_block(&cards);
        let parsed = parse_header_blocks(&block).unwrap();

        assert_eq!(parsed.len(), 6);
        assert_eq!(parsed[0].keyword_str(), "SIMPLE");
        assert_eq!(parsed[0].value, Some(Value::Logical(true)));
        assert!(parsed[5].is_end());
    }

    #[test]
    fn parse_header_no_end_card() {
        let cards = [
            make_card("SIMPLE  =                    T"),
            make_card("BITPIX  =                    8"),
        ];
        let block = make_header_block(&cards);
        assert!(matches!(
            parse_header_blocks(&block),
            Err(Error::UnexpectedEof)
        ));
    }

    #[test]
    fn parse_header_empty_data() {
        assert!(matches!(
            parse_header_blocks(&[]),
            Err(Error::UnexpectedEof)
        ));
    }

    #[test]
    fn parse_header_too_small() {
        let data = vec![b' '; 100];
        assert!(matches!(
            parse_header_blocks(&data),
            Err(Error::UnexpectedEof)
        ));
    }

    #[test]
    fn parse_header_spanning_two_blocks() {
        let mut data = vec![b' '; 2 * BLOCK_SIZE];
        for i in 0..CARDS_PER_BLOCK {
            let kw = alloc::format!("KEY{:<5}", i);
            let card_str = alloc::format!("{}=                     {}", kw, i);
            let card = make_card(&card_str);
            let start = i * CARD_SIZE;
            data[start..start + CARD_SIZE].copy_from_slice(&card);
        }
        let end_card = make_card("END");
        data[BLOCK_SIZE..BLOCK_SIZE + CARD_SIZE].copy_from_slice(&end_card);

        let parsed = parse_header_blocks(&data).unwrap();
        assert_eq!(parsed.len(), CARDS_PER_BLOCK + 1);
        assert!(parsed.last().unwrap().is_end());
    }

    #[test]
    fn header_byte_len_single_block() {
        let cards = [
            make_card("SIMPLE  =                    T"),
            make_card("END"),
        ];
        let block = make_header_block(&cards);
        assert_eq!(header_byte_len(&block).unwrap(), BLOCK_SIZE);
    }

    #[test]
    fn header_byte_len_no_end() {
        let cards = [make_card("SIMPLE  =                    T")];
        let block = make_header_block(&cards);
        assert!(header_byte_len(&block).is_err());
    }

    // ---- CONTINUE long-string convention ----

    #[test]
    fn continue_single_continuation() {
        let cards = [
            make_card("SIMPLE  =                    T"),
            make_card("BITPIX  =                    8"),
            make_card("NAXIS   =                    0"),
            make_card("LONGSTR = 'Hello &'"),
            make_card("CONTINUE  'World'"),
            make_card("END"),
        ];
        let block = make_header_block(&cards);
        let parsed = parse_header_blocks(&block).unwrap();
        let long = parsed
            .iter()
            .find(|c| c.keyword_str() == "LONGSTR")
            .unwrap();
        assert_eq!(long.value, Some(Value::String(String::from("Hello World"))));
        assert!(parsed.iter().all(|c| c.keyword != KW_CONTINUE));
    }

    #[test]
    fn continue_multi_chain() {
        let cards = [
            make_card("SIMPLE  =                    T"),
            make_card("BITPIX  =                    8"),
            make_card("NAXIS   =                    0"),
            make_card("LONGSTR = 'part1&'"),
            make_card("CONTINUE  'part2&'"),
            make_card("CONTINUE  'part3'"),
            make_card("END"),
        ];
        let block = make_header_block(&cards);
        let parsed = parse_header_blocks(&block).unwrap();
        let long = parsed
            .iter()
            .find(|c| c.keyword_str() == "LONGSTR")
            .unwrap();
        assert_eq!(
            long.value,
            Some(Value::String(String::from("part1part2part3")))
        );
        assert!(parsed.iter().all(|c| c.keyword != KW_CONTINUE));
    }

    #[test]
    fn continue_no_ampersand_no_merge() {
        let cards = [
            make_card("SIMPLE  =                    T"),
            make_card("BITPIX  =                    8"),
            make_card("NAXIS   =                    0"),
            make_card("MYKEY   = 'no ampersand'"),
            make_card("CONTINUE  'should stay'"),
            make_card("END"),
        ];
        let block = make_header_block(&cards);
        let parsed = parse_header_blocks(&block).unwrap();
        let mykey = parsed.iter().find(|c| c.keyword_str() == "MYKEY").unwrap();
        assert_eq!(
            mykey.value,
            Some(Value::String(String::from("no ampersand")))
        );
        // CONTINUE card remains because preceding string lacked '&'
        assert!(parsed.iter().any(|c| c.keyword == KW_CONTINUE));
    }

    #[test]
    fn continue_integer_value_not_merged() {
        let cards = [
            make_card("SIMPLE  =                    T"),
            make_card("BITPIX  =                    8"),
            make_card("NAXIS   =                    0"),
            make_card("INTKEY  =                   42"),
            make_card("CONTINUE  'ignored'"),
            make_card("END"),
        ];
        let block = make_header_block(&cards);
        let parsed = parse_header_blocks(&block).unwrap();
        let intkey = parsed.iter().find(|c| c.keyword_str() == "INTKEY").unwrap();
        assert_eq!(intkey.value, Some(Value::Integer(42)));
        assert!(parsed.iter().any(|c| c.keyword == KW_CONTINUE));
    }
}

#[cfg(test)]
mod write_tests {
    use super::*;
    use alloc::string::String;

    fn make_keyword(name: &str) -> [u8; 8] {
        let mut k = [b' '; 8];
        let bytes = name.as_bytes();
        let len = bytes.len().min(8);
        k[..len].copy_from_slice(&bytes[..len]);
        k
    }

    #[test]
    fn format_card_string_value_is_80_bytes() {
        let card = Card {
            keyword: make_keyword("TELESCOP"),
            value: Some(Value::String(String::from("Hubble"))),
            comment: None,
        };
        assert_eq!(format_card(&card).len(), 80);
    }

    #[test]
    fn format_card_value_indicator() {
        let card = Card {
            keyword: make_keyword("TELESCOP"),
            value: Some(Value::String(String::from("Hubble"))),
            comment: None,
        };
        let buf = format_card(&card);
        assert_eq!(&buf[8..10], b"= ");
    }

    #[test]
    fn format_card_integer_value() {
        let card = Card {
            keyword: make_keyword("NAXIS"),
            value: Some(Value::Integer(2)),
            comment: None,
        };
        let buf = format_card(&card);
        assert_eq!(&buf[0..8], b"NAXIS   ");
        assert_eq!(buf[29], b'2');
    }

    #[test]
    fn format_card_logical_value() {
        let card = Card {
            keyword: make_keyword("SIMPLE"),
            value: Some(Value::Logical(true)),
            comment: None,
        };
        let buf = format_card(&card);
        assert_eq!(buf[29], b'T');
    }

    #[test]
    fn format_card_with_comment() {
        let card = Card {
            keyword: make_keyword("NAXIS"),
            value: Some(Value::Integer(2)),
            comment: Some(String::from("number of axes")),
        };
        let buf = format_card(&card);
        let s = core::str::from_utf8(&buf).unwrap();
        assert!(s.contains("/ number of axes"));
    }

    #[test]
    fn end_card_format() {
        let buf = format_end_card();
        assert_eq!(buf.len(), 80);
        assert_eq!(&buf[0..3], b"END");
        for &b in &buf[3..] {
            assert_eq!(b, b' ');
        }
    }

    fn minimal_primary_cards() -> Vec<Card> {
        vec![
            Card {
                keyword: make_keyword("SIMPLE"),
                value: Some(Value::Logical(true)),
                comment: None,
            },
            Card {
                keyword: make_keyword("BITPIX"),
                value: Some(Value::Integer(8)),
                comment: None,
            },
            Card {
                keyword: make_keyword("NAXIS"),
                value: Some(Value::Integer(0)),
                comment: None,
            },
        ]
    }

    #[test]
    fn serialize_header_block_aligned() {
        let cards = minimal_primary_cards();
        let header = serialize_header(&cards).unwrap();
        assert_eq!(header.len() % BLOCK_SIZE, 0);
        assert_eq!(header.len(), BLOCK_SIZE);
    }

    #[test]
    fn serialize_header_contains_end() {
        let cards = minimal_primary_cards();
        let header = serialize_header(&cards).unwrap();
        let end_offset = cards.len() * CARD_SIZE;
        assert_eq!(&header[end_offset..end_offset + 3], b"END");
    }

    #[test]
    fn serialize_header_padding_is_spaces() {
        let cards = minimal_primary_cards();
        let header = serialize_header(&cards).unwrap();
        let after_end = (cards.len() + 1) * CARD_SIZE;
        for &b in &header[after_end..] {
            assert_eq!(b, b' ');
        }
    }

    #[test]
    fn serialize_header_empty_cards() {
        let header = serialize_header(&[]).unwrap();
        assert_eq!(header.len(), BLOCK_SIZE);
        assert_eq!(&header[0..3], b"END");
    }

    #[test]
    fn serialize_header_exactly_one_block() {
        let cards: Vec<Card> = (0..35)
            .map(|i| Card {
                keyword: make_keyword(&alloc::format!("KEY{:05}", i)),
                value: Some(Value::Integer(i as i64)),
                comment: None,
            })
            .collect();
        assert_eq!(serialize_header(&cards).unwrap().len(), BLOCK_SIZE);
    }

    #[test]
    fn serialize_header_spills_to_two_blocks() {
        let cards: Vec<Card> = (0..36)
            .map(|i| Card {
                keyword: make_keyword(&alloc::format!("KEY{:05}", i)),
                value: Some(Value::Integer(i as i64)),
                comment: None,
            })
            .collect();
        assert_eq!(serialize_header(&cards).unwrap().len(), 2 * BLOCK_SIZE);
    }

    #[test]
    fn format_commentary_card() {
        let card = Card {
            keyword: make_keyword("COMMENT"),
            value: None,
            comment: Some(String::from("This is a comment.")),
        };
        let buf = format_card(&card);
        let text = core::str::from_utf8(&buf[8..]).unwrap();
        assert!(text.starts_with("This is a comment."));
    }

    #[test]
    fn format_blank_card() {
        let card = Card {
            keyword: [b' '; 8],
            value: None,
            comment: None,
        };
        let buf = format_card(&card);
        for &b in &buf[..] {
            assert_eq!(b, b' ');
        }
    }

    #[test]
    fn roundtrip_card_logical() {
        let card = Card {
            keyword: make_keyword("SIMPLE"),
            value: Some(Value::Logical(true)),
            comment: None,
        };
        let buf = format_card(&card);
        let (val, _) = parse_value(&buf[10..80]).unwrap();
        assert_eq!(val, Value::Logical(true));
    }

    #[test]
    fn roundtrip_card_integer() {
        let card = Card {
            keyword: make_keyword("BITPIX"),
            value: Some(Value::Integer(-32)),
            comment: None,
        };
        let buf = format_card(&card);
        let (val, _) = parse_value(&buf[10..80]).unwrap();
        assert_eq!(val, Value::Integer(-32));
    }

    #[test]
    fn roundtrip_card_string() {
        let card = Card {
            keyword: make_keyword("OBJECT"),
            value: Some(Value::String(String::from("NGC 1234"))),
            comment: None,
        };
        let buf = format_card(&card);
        let (val, _) = parse_value(&buf[10..80]).unwrap();
        assert_eq!(val, Value::String(String::from("NGC 1234")));
    }

    #[test]
    fn roundtrip_card_string_with_comment() {
        let card = Card {
            keyword: make_keyword("OBJECT"),
            value: Some(Value::String(String::from("M31"))),
            comment: Some(String::from("Andromeda Galaxy")),
        };
        let buf = format_card(&card);
        let (val, comment) = parse_value(&buf[10..80]).unwrap();
        assert_eq!(val, Value::String(String::from("M31")));
        assert_eq!(comment.unwrap(), "Andromeda Galaxy");
    }

    #[test]
    fn roundtrip_serialize_then_parse() {
        let cards = vec![
            Card {
                keyword: make_keyword("SIMPLE"),
                value: Some(Value::Logical(true)),
                comment: Some(String::from("conforms to FITS")),
            },
            Card {
                keyword: make_keyword("BITPIX"),
                value: Some(Value::Integer(16)),
                comment: None,
            },
            Card {
                keyword: make_keyword("NAXIS"),
                value: Some(Value::Integer(0)),
                comment: None,
            },
        ];
        let header = serialize_header(&cards).unwrap();
        let parsed = parse_header_blocks(&header).unwrap();

        assert_eq!(parsed.len(), 4); // 3 cards + END
        assert_eq!(parsed[0].keyword_str(), "SIMPLE");
        assert_eq!(parsed[0].value, Some(Value::Logical(true)));
        assert_eq!(parsed[1].value, Some(Value::Integer(16)));
        assert_eq!(parsed[2].value, Some(Value::Integer(0)));
        assert!(parsed[3].is_end());
    }
}

#[cfg(test)]
mod validate_tests {
    use super::*;
    use alloc::string::String;

    fn card(keyword: &[u8], value: Option<Value>) -> Card {
        Card {
            keyword: kw(keyword),
            value,
            comment: None,
        }
    }

    #[test]
    fn valid_primary_header() {
        let cards = [
            card(b"SIMPLE", Some(Value::Logical(true))),
            card(b"BITPIX", Some(Value::Integer(16))),
            card(b"NAXIS", Some(Value::Integer(2))),
        ];
        assert!(validate_required_keywords(HduType::Primary, &cards).is_ok());
    }

    #[test]
    fn primary_missing_simple() {
        let cards = [
            card(b"BITPIX", Some(Value::Integer(16))),
            card(b"NAXIS", Some(Value::Integer(2))),
        ];
        assert!(matches!(
            validate_required_keywords(HduType::Primary, &cards),
            Err(Error::MissingKeyword("SIMPLE"))
        ));
    }

    #[test]
    fn primary_missing_bitpix() {
        let cards = [
            card(b"SIMPLE", Some(Value::Logical(true))),
            card(b"NAXIS", Some(Value::Integer(2))),
        ];
        assert!(matches!(
            validate_required_keywords(HduType::Primary, &cards),
            Err(Error::MissingKeyword("BITPIX"))
        ));
    }

    #[test]
    fn primary_missing_naxis() {
        let cards = [
            card(b"SIMPLE", Some(Value::Logical(true))),
            card(b"BITPIX", Some(Value::Integer(16))),
        ];
        assert!(matches!(
            validate_required_keywords(HduType::Primary, &cards),
            Err(Error::MissingKeyword("NAXIS"))
        ));
    }

    #[test]
    fn primary_simple_not_true() {
        let cards = [
            card(b"SIMPLE", Some(Value::Logical(false))),
            card(b"BITPIX", Some(Value::Integer(16))),
            card(b"NAXIS", Some(Value::Integer(0))),
        ];
        assert!(matches!(
            validate_required_keywords(HduType::Primary, &cards),
            Err(Error::InvalidHeader(_))
        ));
    }

    #[test]
    fn primary_wrong_order() {
        let cards = [
            card(b"BITPIX", Some(Value::Integer(16))),
            card(b"SIMPLE", Some(Value::Logical(true))),
            card(b"NAXIS", Some(Value::Integer(0))),
        ];
        assert!(matches!(
            validate_required_keywords(HduType::Primary, &cards),
            Err(Error::MissingKeyword("SIMPLE"))
        ));
    }

    #[test]
    fn primary_empty_cards() {
        let cards: &[Card] = &[];
        assert!(matches!(
            validate_required_keywords(HduType::Primary, cards),
            Err(Error::MissingKeyword("SIMPLE"))
        ));
    }

    #[test]
    fn valid_image_extension() {
        let cards = [
            card(b"XTENSION", Some(Value::String(String::from("IMAGE")))),
            card(b"BITPIX", Some(Value::Integer(-32))),
            card(b"NAXIS", Some(Value::Integer(2))),
            card(b"NAXIS1", Some(Value::Integer(512))),
            card(b"NAXIS2", Some(Value::Integer(512))),
            card(b"PCOUNT", Some(Value::Integer(0))),
            card(b"GCOUNT", Some(Value::Integer(1))),
        ];
        assert!(validate_required_keywords(HduType::Image, &cards).is_ok());
    }

    #[test]
    fn image_wrong_xtension() {
        let cards = [
            card(b"XTENSION", Some(Value::String(String::from("TABLE")))),
            card(b"BITPIX", Some(Value::Integer(-32))),
            card(b"NAXIS", Some(Value::Integer(2))),
            card(b"PCOUNT", Some(Value::Integer(0))),
            card(b"GCOUNT", Some(Value::Integer(1))),
        ];
        assert!(matches!(
            validate_required_keywords(HduType::Image, &cards),
            Err(Error::InvalidHeader(_))
        ));
    }

    #[test]
    fn image_missing_pcount() {
        let cards = [
            card(b"XTENSION", Some(Value::String(String::from("IMAGE")))),
            card(b"BITPIX", Some(Value::Integer(-32))),
            card(b"NAXIS", Some(Value::Integer(2))),
            card(b"GCOUNT", Some(Value::Integer(1))),
        ];
        assert!(matches!(
            validate_required_keywords(HduType::Image, &cards),
            Err(Error::MissingKeyword("PCOUNT"))
        ));
    }

    #[test]
    fn valid_ascii_table() {
        let cards = [
            card(b"XTENSION", Some(Value::String(String::from("TABLE")))),
            card(b"BITPIX", Some(Value::Integer(8))),
            card(b"NAXIS", Some(Value::Integer(2))),
            card(b"NAXIS1", Some(Value::Integer(100))),
            card(b"NAXIS2", Some(Value::Integer(50))),
            card(b"PCOUNT", Some(Value::Integer(0))),
            card(b"GCOUNT", Some(Value::Integer(1))),
            card(b"TFIELDS", Some(Value::Integer(5))),
        ];
        assert!(validate_required_keywords(HduType::AsciiTable, &cards).is_ok());
    }

    #[test]
    fn ascii_table_wrong_bitpix() {
        let cards = [
            card(b"XTENSION", Some(Value::String(String::from("TABLE")))),
            card(b"BITPIX", Some(Value::Integer(16))),
            card(b"NAXIS", Some(Value::Integer(2))),
            card(b"NAXIS1", Some(Value::Integer(100))),
            card(b"NAXIS2", Some(Value::Integer(50))),
            card(b"PCOUNT", Some(Value::Integer(0))),
            card(b"GCOUNT", Some(Value::Integer(1))),
            card(b"TFIELDS", Some(Value::Integer(5))),
        ];
        assert!(matches!(
            validate_required_keywords(HduType::AsciiTable, &cards),
            Err(Error::InvalidHeader(_))
        ));
    }

    #[test]
    fn ascii_table_missing_tfields() {
        let cards = [
            card(b"XTENSION", Some(Value::String(String::from("TABLE")))),
            card(b"BITPIX", Some(Value::Integer(8))),
            card(b"NAXIS", Some(Value::Integer(2))),
            card(b"NAXIS1", Some(Value::Integer(100))),
            card(b"NAXIS2", Some(Value::Integer(50))),
            card(b"PCOUNT", Some(Value::Integer(0))),
            card(b"GCOUNT", Some(Value::Integer(1))),
        ];
        assert!(matches!(
            validate_required_keywords(HduType::AsciiTable, &cards),
            Err(Error::MissingKeyword("TFIELDS"))
        ));
    }

    #[test]
    fn valid_binary_table() {
        let cards = [
            card(b"XTENSION", Some(Value::String(String::from("BINTABLE")))),
            card(b"BITPIX", Some(Value::Integer(8))),
            card(b"NAXIS", Some(Value::Integer(2))),
            card(b"NAXIS1", Some(Value::Integer(32))),
            card(b"NAXIS2", Some(Value::Integer(1000))),
            card(b"PCOUNT", Some(Value::Integer(0))),
            card(b"GCOUNT", Some(Value::Integer(1))),
            card(b"TFIELDS", Some(Value::Integer(3))),
        ];
        assert!(validate_required_keywords(HduType::BinaryTable, &cards).is_ok());
    }

    #[test]
    fn binary_table_wrong_xtension() {
        let cards = [
            card(b"XTENSION", Some(Value::String(String::from("IMAGE")))),
            card(b"BITPIX", Some(Value::Integer(8))),
            card(b"NAXIS", Some(Value::Integer(2))),
            card(b"NAXIS1", Some(Value::Integer(32))),
            card(b"NAXIS2", Some(Value::Integer(1000))),
            card(b"PCOUNT", Some(Value::Integer(0))),
            card(b"GCOUNT", Some(Value::Integer(1))),
            card(b"TFIELDS", Some(Value::Integer(3))),
        ];
        assert!(matches!(
            validate_required_keywords(HduType::BinaryTable, &cards),
            Err(Error::InvalidHeader(_))
        ));
    }

    #[test]
    fn binary_table_wrong_bitpix() {
        let cards = [
            card(b"XTENSION", Some(Value::String(String::from("BINTABLE")))),
            card(b"BITPIX", Some(Value::Integer(-32))),
            card(b"NAXIS", Some(Value::Integer(2))),
            card(b"NAXIS1", Some(Value::Integer(32))),
            card(b"NAXIS2", Some(Value::Integer(1000))),
            card(b"PCOUNT", Some(Value::Integer(0))),
            card(b"GCOUNT", Some(Value::Integer(1))),
            card(b"TFIELDS", Some(Value::Integer(3))),
        ];
        assert!(matches!(
            validate_required_keywords(HduType::BinaryTable, &cards),
            Err(Error::InvalidHeader(_))
        ));
    }

    #[test]
    fn binary_table_missing_naxis2() {
        let cards = [
            card(b"XTENSION", Some(Value::String(String::from("BINTABLE")))),
            card(b"BITPIX", Some(Value::Integer(8))),
            card(b"NAXIS", Some(Value::Integer(2))),
            card(b"NAXIS1", Some(Value::Integer(32))),
            card(b"PCOUNT", Some(Value::Integer(0))),
            card(b"GCOUNT", Some(Value::Integer(1))),
            card(b"TFIELDS", Some(Value::Integer(3))),
        ];
        assert!(matches!(
            validate_required_keywords(HduType::BinaryTable, &cards),
            Err(Error::MissingKeyword("NAXIS2"))
        ));
    }

    #[test]
    fn image_xtension_with_trailing_spaces() {
        let cards = [
            card(b"XTENSION", Some(Value::String(String::from("IMAGE   ")))),
            card(b"BITPIX", Some(Value::Integer(-64))),
            card(b"NAXIS", Some(Value::Integer(0))),
            card(b"PCOUNT", Some(Value::Integer(0))),
            card(b"GCOUNT", Some(Value::Integer(1))),
        ];
        assert!(validate_required_keywords(HduType::Image, &cards).is_ok());
    }

    #[test]
    fn hdu_type_clone_copy_eq() {
        let a = HduType::Primary;
        let b = a;
        assert_eq!(a, b);
    }

    // ── serialize_header validation tests ──

    #[test]
    fn serialize_rejects_misordered_primary() {
        let cards = vec![
            card(b"BITPIX", Some(Value::Integer(8))),
            card(b"SIMPLE", Some(Value::Logical(true))),
            card(b"NAXIS", Some(Value::Integer(0))),
        ];
        assert!(serialize_header(&cards).is_err());
    }

    #[test]
    fn serialize_rejects_missing_bitpix() {
        let cards = vec![
            card(b"SIMPLE", Some(Value::Logical(true))),
            card(b"NAXIS", Some(Value::Integer(0))),
        ];
        assert!(serialize_header(&cards).is_err());
    }

    #[test]
    fn serialize_accepts_valid_primary() {
        let cards = vec![
            card(b"SIMPLE", Some(Value::Logical(true))),
            card(b"BITPIX", Some(Value::Integer(8))),
            card(b"NAXIS", Some(Value::Integer(0))),
        ];
        assert!(serialize_header(&cards).is_ok());
    }

    #[test]
    fn serialize_rejects_misordered_extension() {
        let cards = vec![
            card(b"BITPIX", Some(Value::Integer(8))),
            card(b"XTENSION", Some(Value::String(String::from("IMAGE")))),
            card(b"NAXIS", Some(Value::Integer(0))),
            card(b"PCOUNT", Some(Value::Integer(0))),
            card(b"GCOUNT", Some(Value::Integer(1))),
        ];
        assert!(serialize_header(&cards).is_err());
    }

    #[test]
    fn serialize_rejects_bintable_missing_tfields() {
        let cards = vec![
            card(b"XTENSION", Some(Value::String(String::from("BINTABLE")))),
            card(b"BITPIX", Some(Value::Integer(8))),
            card(b"NAXIS", Some(Value::Integer(2))),
            card(b"NAXIS1", Some(Value::Integer(16))),
            card(b"NAXIS2", Some(Value::Integer(10))),
            card(b"PCOUNT", Some(Value::Integer(0))),
            card(b"GCOUNT", Some(Value::Integer(1))),
        ];
        assert!(serialize_header(&cards).is_err());
    }

    #[test]
    fn serialize_accepts_valid_bintable() {
        let cards = vec![
            card(b"XTENSION", Some(Value::String(String::from("BINTABLE")))),
            card(b"BITPIX", Some(Value::Integer(8))),
            card(b"NAXIS", Some(Value::Integer(2))),
            card(b"NAXIS1", Some(Value::Integer(16))),
            card(b"NAXIS2", Some(Value::Integer(10))),
            card(b"PCOUNT", Some(Value::Integer(0))),
            card(b"GCOUNT", Some(Value::Integer(1))),
            card(b"TFIELDS", Some(Value::Integer(2))),
        ];
        assert!(serialize_header(&cards).is_ok());
    }

    #[test]
    fn serialize_skips_validation_for_unknown_first_keyword() {
        let cards = vec![card(b"FOOBAR", Some(Value::Integer(42)))];
        assert!(serialize_header(&cards).is_ok());
    }
}
