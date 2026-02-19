use alloc::string::String;
use alloc::vec::Vec;

use crate::block::padded_byte_len;
#[cfg(test)]
use crate::block::BLOCK_SIZE;
use crate::error::{Error, Result};
use crate::header::{validate_required_keywords, Card, HduType};
use crate::value::Value;

/// The type of FITS extension, determined by the XTENSION keyword value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionType {
    /// XTENSION = 'IMAGE'.
    Image,
    /// XTENSION = 'TABLE'.
    AsciiTable,
    /// XTENSION = 'BINTABLE'.
    BinaryTable,
}

impl ExtensionType {
    fn as_str(&self) -> &'static str {
        match self {
            ExtensionType::Image => "IMAGE",
            ExtensionType::AsciiTable => "TABLE",
            ExtensionType::BinaryTable => "BINTABLE",
        }
    }

    fn hdu_type(&self) -> HduType {
        match self {
            ExtensionType::Image => HduType::Image,
            ExtensionType::AsciiTable => HduType::AsciiTable,
            ExtensionType::BinaryTable => HduType::BinaryTable,
        }
    }
}

/// A parsed FITS extension header with all mandatory keyword values extracted.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtensionHeader {
    /// The extension type (IMAGE, TABLE, or BINTABLE).
    pub xtension: ExtensionType,
    /// Bits per pixel / data value.
    pub bitpix: i64,
    /// Number of axes.
    pub naxis: usize,
    /// Axis dimensions (NAXIS1, NAXIS2, ...).
    pub naxes: Vec<usize>,
    /// Parameter count (heap size for binary tables).
    pub pcount: usize,
    /// Group count (always 1 for standard extensions).
    pub gcount: usize,
    /// All header cards from this extension.
    pub cards: Vec<Card>,
}

impl ExtensionHeader {
    /// Compute the number of data bytes described by this extension header.
    ///
    /// Per the FITS standard the data size is:
    ///   `|BITPIX|/8 * NAXIS1 * NAXIS2 * ... * NAXISn + PCOUNT`
    /// for binary tables (where BITPIX=8 so it simplifies to the product of
    /// axes plus PCOUNT for the heap). For images, PCOUNT is typically 0.
    /// If NAXIS is 0, the data size is 0 (no data follows).
    pub fn data_byte_count(&self) -> usize {
        if self.naxis == 0 || self.naxes.is_empty() {
            return 0;
        }
        let bytes_per_value = (self.bitpix.unsigned_abs() as usize) / 8;
        let product: usize = self.naxes.iter().product();
        bytes_per_value * product + self.pcount
    }

    /// Compute the padded data byte count, rounded up to the next multiple
    /// of the FITS block size (2880 bytes).
    pub fn data_padded_byte_count(&self) -> usize {
        padded_byte_len(self.data_byte_count())
    }
}

/// Pad a keyword name to 8 bytes with trailing ASCII spaces.
fn kw(name: &[u8]) -> [u8; 8] {
    let mut buf = [b' '; 8];
    let len = name.len().min(8);
    buf[..len].copy_from_slice(&name[..len]);
    buf
}

fn find_keyword<'a>(cards: &'a [Card], name: &[u8; 8]) -> Option<&'a Card> {
    cards.iter().find(|c| &c.keyword == name)
}

fn extract_integer(card: &Card, keyword_name: &'static str) -> Result<i64> {
    match &card.value {
        Some(Value::Integer(n)) => Ok(*n),
        _ => Err(Error::MissingKeyword(keyword_name)),
    }
}

fn extract_usize(card: &Card, keyword_name: &'static str) -> Result<usize> {
    let n = extract_integer(card, keyword_name)?;
    if n < 0 {
        return Err(Error::InvalidValue);
    }
    Ok(n as usize)
}

/// Parse an XTENSION keyword value and determine the extension type.
fn parse_extension_type(cards: &[Card]) -> Result<ExtensionType> {
    let xtension_card = cards.first().ok_or(Error::MissingKeyword("XTENSION"))?;
    if xtension_card.keyword != kw(b"XTENSION") {
        return Err(Error::MissingKeyword("XTENSION"));
    }

    match &xtension_card.value {
        Some(Value::String(s)) => match s.trim() {
            "IMAGE" => Ok(ExtensionType::Image),
            "TABLE" => Ok(ExtensionType::AsciiTable),
            "BINTABLE" => Ok(ExtensionType::BinaryTable),
            other => Err(Error::UnsupportedExtension(if other.starts_with("A3D") {
                "A3DTABLE"
            } else {
                "unknown XTENSION"
            })),
        },
        _ => Err(Error::UnsupportedExtension("XTENSION not a string")),
    }
}

/// Parse an extension header from a slice of cards.
///
/// Reads the XTENSION keyword to determine the extension type, then extracts
/// all shared mandatory keywords (BITPIX, NAXIS, NAXISn, PCOUNT, GCOUNT) and
/// validates the header against the FITS standard.
pub fn parse_extension_header(cards: &[Card]) -> Result<ExtensionHeader> {
    let ext_type = parse_extension_type(cards)?;

    validate_required_keywords(ext_type.hdu_type(), cards)?;

    let bitpix_card = find_keyword(cards, &kw(b"BITPIX")).ok_or(Error::MissingKeyword("BITPIX"))?;
    let bitpix = extract_integer(bitpix_card, "BITPIX")?;

    let naxis_card = find_keyword(cards, &kw(b"NAXIS")).ok_or(Error::MissingKeyword("NAXIS"))?;
    let naxis = extract_usize(naxis_card, "NAXIS")?;

    let mut naxes = Vec::with_capacity(naxis);
    for i in 1..=naxis {
        let kw_name = alloc::format!("NAXIS{}", i);
        let mut kw_buf = [b' '; 8];
        let len = kw_name.len().min(8);
        kw_buf[..len].copy_from_slice(&kw_name.as_bytes()[..len]);
        let card = find_keyword(cards, &kw_buf).ok_or(Error::InvalidHeader("missing NAXISn"))?;
        let val = extract_usize(card, "NAXISn")?;
        naxes.push(val);
    }

    let pcount_card = find_keyword(cards, &kw(b"PCOUNT")).ok_or(Error::MissingKeyword("PCOUNT"))?;
    let pcount = extract_usize(pcount_card, "PCOUNT")?;

    let gcount_card = find_keyword(cards, &kw(b"GCOUNT")).ok_or(Error::MissingKeyword("GCOUNT"))?;
    let gcount = extract_usize(gcount_card, "GCOUNT")?;

    Ok(ExtensionHeader {
        xtension: ext_type,
        bitpix,
        naxis,
        naxes,
        pcount,
        gcount,
        cards: cards.to_vec(),
    })
}

/// Build a sequence of cards for an extension header.
///
/// Creates the mandatory keywords in the order required by the FITS standard:
/// XTENSION, BITPIX, NAXIS, NAXIS1..NAXISn, PCOUNT, GCOUNT.
pub fn build_extension_header(
    ext_type: ExtensionType,
    bitpix: i64,
    naxes: &[usize],
    pcount: usize,
    gcount: usize,
) -> Result<Vec<Card>> {
    let naxis = naxes.len();
    let mut cards = Vec::with_capacity(6 + naxis);

    cards.push(Card {
        keyword: kw(b"XTENSION"),
        value: Some(Value::String(String::from(ext_type.as_str()))),
        comment: None,
    });

    cards.push(Card {
        keyword: kw(b"BITPIX"),
        value: Some(Value::Integer(bitpix)),
        comment: None,
    });

    cards.push(Card {
        keyword: kw(b"NAXIS"),
        value: Some(Value::Integer(naxis as i64)),
        comment: None,
    });

    for (i, &dim) in naxes.iter().enumerate() {
        let kw_name = alloc::format!("NAXIS{}", i + 1);
        let mut kw_buf = [b' '; 8];
        let len = kw_name.len().min(8);
        kw_buf[..len].copy_from_slice(&kw_name.as_bytes()[..len]);
        cards.push(Card {
            keyword: kw_buf,
            value: Some(Value::Integer(dim as i64)),
            comment: None,
        });
    }

    cards.push(Card {
        keyword: kw(b"PCOUNT"),
        value: Some(Value::Integer(pcount as i64)),
        comment: None,
    });

    cards.push(Card {
        keyword: kw(b"GCOUNT"),
        value: Some(Value::Integer(gcount as i64)),
        comment: None,
    });

    Ok(cards)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;

    fn card(keyword: &[u8], value: Option<Value>) -> Card {
        Card {
            keyword: kw(keyword),
            value,
            comment: None,
        }
    }

    fn make_image_cards() -> Vec<Card> {
        vec![
            card(b"XTENSION", Some(Value::String(String::from("IMAGE")))),
            card(b"BITPIX", Some(Value::Integer(-32))),
            card(b"NAXIS", Some(Value::Integer(2))),
            card(b"NAXIS1", Some(Value::Integer(512))),
            card(b"NAXIS2", Some(Value::Integer(256))),
            card(b"PCOUNT", Some(Value::Integer(0))),
            card(b"GCOUNT", Some(Value::Integer(1))),
        ]
    }

    fn make_ascii_table_cards() -> Vec<Card> {
        vec![
            card(b"XTENSION", Some(Value::String(String::from("TABLE")))),
            card(b"BITPIX", Some(Value::Integer(8))),
            card(b"NAXIS", Some(Value::Integer(2))),
            card(b"NAXIS1", Some(Value::Integer(100))),
            card(b"NAXIS2", Some(Value::Integer(50))),
            card(b"PCOUNT", Some(Value::Integer(0))),
            card(b"GCOUNT", Some(Value::Integer(1))),
            card(b"TFIELDS", Some(Value::Integer(5))),
        ]
    }

    fn make_bintable_cards() -> Vec<Card> {
        vec![
            card(b"XTENSION", Some(Value::String(String::from("BINTABLE")))),
            card(b"BITPIX", Some(Value::Integer(8))),
            card(b"NAXIS", Some(Value::Integer(2))),
            card(b"NAXIS1", Some(Value::Integer(32))),
            card(b"NAXIS2", Some(Value::Integer(1000))),
            card(b"PCOUNT", Some(Value::Integer(0))),
            card(b"GCOUNT", Some(Value::Integer(1))),
            card(b"TFIELDS", Some(Value::Integer(3))),
        ]
    }

    #[test]
    fn parse_image_extension() {
        let cards = make_image_cards();
        let hdr = parse_extension_header(&cards).unwrap();
        assert_eq!(hdr.xtension, ExtensionType::Image);
        assert_eq!(hdr.bitpix, -32);
        assert_eq!(hdr.naxis, 2);
        assert_eq!(hdr.naxes, vec![512, 256]);
        assert_eq!(hdr.pcount, 0);
        assert_eq!(hdr.gcount, 1);
    }

    #[test]
    fn parse_ascii_table_extension() {
        let cards = make_ascii_table_cards();
        let hdr = parse_extension_header(&cards).unwrap();
        assert_eq!(hdr.xtension, ExtensionType::AsciiTable);
        assert_eq!(hdr.bitpix, 8);
        assert_eq!(hdr.naxis, 2);
        assert_eq!(hdr.naxes, vec![100, 50]);
        assert_eq!(hdr.pcount, 0);
        assert_eq!(hdr.gcount, 1);
    }

    #[test]
    fn parse_bintable_extension() {
        let cards = make_bintable_cards();
        let hdr = parse_extension_header(&cards).unwrap();
        assert_eq!(hdr.xtension, ExtensionType::BinaryTable);
        assert_eq!(hdr.bitpix, 8);
        assert_eq!(hdr.naxis, 2);
        assert_eq!(hdr.naxes, vec![32, 1000]);
        assert_eq!(hdr.pcount, 0);
        assert_eq!(hdr.gcount, 1);
    }

    #[test]
    fn data_byte_count_image() {
        let cards = make_image_cards();
        let hdr = parse_extension_header(&cards).unwrap();
        // |BITPIX|/8 * 512 * 256 = 4 * 512 * 256 = 524288
        assert_eq!(hdr.data_byte_count(), 4 * 512 * 256);
    }

    #[test]
    fn data_byte_count_ascii_table() {
        let cards = make_ascii_table_cards();
        let hdr = parse_extension_header(&cards).unwrap();
        // 1 * 100 * 50 + 0 = 5000
        assert_eq!(hdr.data_byte_count(), 100 * 50);
    }

    #[test]
    fn data_byte_count_bintable() {
        let cards = make_bintable_cards();
        let hdr = parse_extension_header(&cards).unwrap();
        // 1 * 32 * 1000 + 0 = 32000
        assert_eq!(hdr.data_byte_count(), 32 * 1000);
    }

    #[test]
    fn data_byte_count_bintable_with_heap() {
        let mut cards = make_bintable_cards();
        // Set PCOUNT to 4096 to simulate a heap
        for c in cards.iter_mut() {
            if c.keyword_str() == "PCOUNT" {
                c.value = Some(Value::Integer(4096));
            }
        }
        let hdr = parse_extension_header(&cards).unwrap();
        assert_eq!(hdr.data_byte_count(), 32 * 1000 + 4096);
    }

    #[test]
    fn data_byte_count_zero_naxis() {
        let cards = vec![
            card(b"XTENSION", Some(Value::String(String::from("IMAGE")))),
            card(b"BITPIX", Some(Value::Integer(-64))),
            card(b"NAXIS", Some(Value::Integer(0))),
            card(b"PCOUNT", Some(Value::Integer(0))),
            card(b"GCOUNT", Some(Value::Integer(1))),
        ];
        let hdr = parse_extension_header(&cards).unwrap();
        assert_eq!(hdr.data_byte_count(), 0);
    }

    #[test]
    fn padded_byte_count_image() {
        let cards = make_image_cards();
        let hdr = parse_extension_header(&cards).unwrap();
        let raw = hdr.data_byte_count();
        let padded = hdr.data_padded_byte_count();
        assert_eq!(padded % BLOCK_SIZE, 0);
        assert!(padded >= raw);
        // 524288 / 2880 = 182.0..., so 183 blocks = 527040
        assert_eq!(padded, 183 * BLOCK_SIZE);
    }

    #[test]
    fn padded_byte_count_ascii_table() {
        let cards = make_ascii_table_cards();
        let hdr = parse_extension_header(&cards).unwrap();
        let raw = hdr.data_byte_count();
        let padded = hdr.data_padded_byte_count();
        assert_eq!(padded % BLOCK_SIZE, 0);
        assert!(padded >= raw);
        // 5000 / 2880 = 1.74, so 2 blocks = 5760
        assert_eq!(padded, 2 * BLOCK_SIZE);
    }

    #[test]
    fn padded_byte_count_bintable() {
        let cards = make_bintable_cards();
        let hdr = parse_extension_header(&cards).unwrap();
        let raw = hdr.data_byte_count();
        let padded = hdr.data_padded_byte_count();
        assert_eq!(padded % BLOCK_SIZE, 0);
        assert!(padded >= raw);
        // 32000 / 2880 = 11.11, so 12 blocks = 34560
        assert_eq!(padded, 12 * BLOCK_SIZE);
    }

    #[test]
    fn padded_byte_count_zero_data() {
        let cards = vec![
            card(b"XTENSION", Some(Value::String(String::from("IMAGE")))),
            card(b"BITPIX", Some(Value::Integer(8))),
            card(b"NAXIS", Some(Value::Integer(0))),
            card(b"PCOUNT", Some(Value::Integer(0))),
            card(b"GCOUNT", Some(Value::Integer(1))),
        ];
        let hdr = parse_extension_header(&cards).unwrap();
        assert_eq!(hdr.data_padded_byte_count(), 0);
    }

    #[test]
    fn build_then_parse_image_roundtrip() {
        let cards = build_extension_header(ExtensionType::Image, -32, &[512, 256], 0, 1).unwrap();
        let hdr = parse_extension_header(&cards).unwrap();
        assert_eq!(hdr.xtension, ExtensionType::Image);
        assert_eq!(hdr.bitpix, -32);
        assert_eq!(hdr.naxis, 2);
        assert_eq!(hdr.naxes, vec![512, 256]);
        assert_eq!(hdr.pcount, 0);
        assert_eq!(hdr.gcount, 1);
    }

    #[test]
    fn build_then_parse_ascii_table_roundtrip() {
        let mut cards =
            build_extension_header(ExtensionType::AsciiTable, 8, &[100, 50], 0, 1).unwrap();
        cards.push(card(b"TFIELDS", Some(Value::Integer(5))));
        let hdr = parse_extension_header(&cards).unwrap();
        assert_eq!(hdr.xtension, ExtensionType::AsciiTable);
        assert_eq!(hdr.bitpix, 8);
        assert_eq!(hdr.naxes, vec![100, 50]);
    }

    #[test]
    fn build_then_parse_bintable_roundtrip() {
        let mut cards =
            build_extension_header(ExtensionType::BinaryTable, 8, &[32, 1000], 512, 1).unwrap();
        cards.push(card(b"TFIELDS", Some(Value::Integer(3))));
        let hdr = parse_extension_header(&cards).unwrap();
        assert_eq!(hdr.xtension, ExtensionType::BinaryTable);
        assert_eq!(hdr.bitpix, 8);
        assert_eq!(hdr.naxes, vec![32, 1000]);
        assert_eq!(hdr.pcount, 512);
        assert_eq!(hdr.gcount, 1);
    }

    #[test]
    fn error_on_unknown_xtension() {
        let cards = vec![
            card(b"XTENSION", Some(Value::String(String::from("UNKNOWN")))),
            card(b"BITPIX", Some(Value::Integer(8))),
            card(b"NAXIS", Some(Value::Integer(0))),
            card(b"PCOUNT", Some(Value::Integer(0))),
            card(b"GCOUNT", Some(Value::Integer(1))),
        ];
        assert!(matches!(
            parse_extension_header(&cards),
            Err(Error::UnsupportedExtension(_))
        ));
    }

    #[test]
    fn error_on_missing_xtension() {
        let cards = vec![
            card(b"BITPIX", Some(Value::Integer(8))),
            card(b"NAXIS", Some(Value::Integer(0))),
        ];
        assert!(matches!(
            parse_extension_header(&cards),
            Err(Error::MissingKeyword("XTENSION"))
        ));
    }

    #[test]
    fn error_on_empty_cards() {
        let cards: Vec<Card> = vec![];
        assert!(parse_extension_header(&cards).is_err());
    }

    #[test]
    fn extension_type_as_str() {
        assert_eq!(ExtensionType::Image.as_str(), "IMAGE");
        assert_eq!(ExtensionType::AsciiTable.as_str(), "TABLE");
        assert_eq!(ExtensionType::BinaryTable.as_str(), "BINTABLE");
    }

    #[test]
    fn extension_type_hdu_type() {
        assert_eq!(ExtensionType::Image.hdu_type(), HduType::Image);
        assert_eq!(ExtensionType::AsciiTable.hdu_type(), HduType::AsciiTable);
        assert_eq!(ExtensionType::BinaryTable.hdu_type(), HduType::BinaryTable);
    }

    #[test]
    fn extension_type_clone_copy_eq() {
        let a = ExtensionType::Image;
        let b = a;
        assert_eq!(a, b);
    }

    #[test]
    fn data_byte_count_image_3d() {
        let cards = vec![
            card(b"XTENSION", Some(Value::String(String::from("IMAGE")))),
            card(b"BITPIX", Some(Value::Integer(16))),
            card(b"NAXIS", Some(Value::Integer(3))),
            card(b"NAXIS1", Some(Value::Integer(100))),
            card(b"NAXIS2", Some(Value::Integer(200))),
            card(b"NAXIS3", Some(Value::Integer(10))),
            card(b"PCOUNT", Some(Value::Integer(0))),
            card(b"GCOUNT", Some(Value::Integer(1))),
        ];
        let hdr = parse_extension_header(&cards).unwrap();
        // |16|/8 * 100 * 200 * 10 = 2 * 200000 = 400000
        assert_eq!(hdr.data_byte_count(), 2 * 100 * 200 * 10);
    }

    #[test]
    fn padded_byte_count_exact_block_multiple() {
        // 2880 bytes of data should need exactly 1 block
        let cards = vec![
            card(b"XTENSION", Some(Value::String(String::from("IMAGE")))),
            card(b"BITPIX", Some(Value::Integer(8))),
            card(b"NAXIS", Some(Value::Integer(2))),
            card(b"NAXIS1", Some(Value::Integer(2880))),
            card(b"NAXIS2", Some(Value::Integer(1))),
            card(b"PCOUNT", Some(Value::Integer(0))),
            card(b"GCOUNT", Some(Value::Integer(1))),
        ];
        let hdr = parse_extension_header(&cards).unwrap();
        assert_eq!(hdr.data_byte_count(), BLOCK_SIZE);
        assert_eq!(hdr.data_padded_byte_count(), BLOCK_SIZE);
    }

    #[test]
    fn build_zero_naxis_image() {
        let cards = build_extension_header(ExtensionType::Image, -64, &[], 0, 1).unwrap();
        let hdr = parse_extension_header(&cards).unwrap();
        assert_eq!(hdr.naxis, 0);
        assert!(hdr.naxes.is_empty());
        assert_eq!(hdr.data_byte_count(), 0);
    }

    #[test]
    fn xtension_with_trailing_spaces() {
        let cards = vec![
            card(b"XTENSION", Some(Value::String(String::from("IMAGE   ")))),
            card(b"BITPIX", Some(Value::Integer(8))),
            card(b"NAXIS", Some(Value::Integer(0))),
            card(b"PCOUNT", Some(Value::Integer(0))),
            card(b"GCOUNT", Some(Value::Integer(1))),
        ];
        let hdr = parse_extension_header(&cards).unwrap();
        assert_eq!(hdr.xtension, ExtensionType::Image);
    }

    #[test]
    fn cards_preserved_in_header() {
        let cards = make_image_cards();
        let hdr = parse_extension_header(&cards).unwrap();
        assert_eq!(hdr.cards.len(), cards.len());
        assert_eq!(hdr.cards[0].keyword_str(), "XTENSION");
    }

    #[test]
    fn build_extension_header_card_count() {
        let cards = build_extension_header(ExtensionType::Image, -32, &[100, 200], 0, 1).unwrap();
        // XTENSION + BITPIX + NAXIS + NAXIS1 + NAXIS2 + PCOUNT + GCOUNT = 7
        assert_eq!(cards.len(), 7);
    }

    #[test]
    fn build_extension_header_card_order() {
        let cards =
            build_extension_header(ExtensionType::BinaryTable, 8, &[32, 500], 128, 1).unwrap();
        assert_eq!(cards[0].keyword_str(), "XTENSION");
        assert_eq!(cards[1].keyword_str(), "BITPIX");
        assert_eq!(cards[2].keyword_str(), "NAXIS");
        assert_eq!(cards[3].keyword_str(), "NAXIS1");
        assert_eq!(cards[4].keyword_str(), "NAXIS2");
        assert_eq!(cards[5].keyword_str(), "PCOUNT");
        assert_eq!(cards[6].keyword_str(), "GCOUNT");
    }
}
