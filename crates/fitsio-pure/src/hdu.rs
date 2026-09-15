use alloc::string::String;
use alloc::vec::Vec;

use crate::block::{padded_byte_len, BLOCK_SIZE};
use crate::error::{Error, Result};
use crate::header::{header_byte_len, parse_header_blocks, Card};
use crate::value::Value;

/// Describes the kind and shape of data in a single HDU.
#[derive(Debug, Clone, PartialEq)]
pub enum HduInfo {
    /// Primary HDU containing image data.
    Primary {
        /// BITPIX value (8, 16, 32, 64, -32, -64).
        bitpix: i64,
        /// Axis dimensions (NAXIS1, NAXIS2, ...).
        naxes: Vec<usize>,
    },
    /// Image extension (XTENSION = 'IMAGE').
    Image {
        /// BITPIX value.
        bitpix: i64,
        /// Axis dimensions.
        naxes: Vec<usize>,
    },
    /// ASCII table extension (XTENSION = 'TABLE').
    AsciiTable {
        /// Row width in bytes.
        naxis1: usize,
        /// Number of rows.
        naxis2: usize,
        /// Number of columns.
        tfields: usize,
    },
    /// Binary table extension (XTENSION = 'BINTABLE').
    BinaryTable {
        /// Row width in bytes.
        naxis1: usize,
        /// Number of rows.
        naxis2: usize,
        /// Size of the variable-length array heap in bytes.
        pcount: usize,
        /// Number of columns.
        tfields: usize,
    },
    /// Random groups structure (primary HDU with GROUPS=T, NAXIS1=0).
    RandomGroups {
        /// BITPIX value.
        bitpix: i64,
        /// Group dimensions (NAXIS2..NAXISm, excluding NAXIS1=0).
        naxes: Vec<usize>,
        /// Number of parameters per group.
        pcount: usize,
        /// Number of groups.
        gcount: usize,
    },
    /// Tile-compressed image stored as a binary table (ZIMAGE=T).
    CompressedImage {
        /// Original image BITPIX before compression.
        zbitpix: i64,
        /// Original image dimensions.
        znaxes: Vec<usize>,
        /// Compression algorithm name (e.g. "RICE_1", "GZIP_1").
        zcmptype: String,
        /// Tile dimensions for compression.
        ztile: Vec<usize>,
        /// Rice compression block size (ZVAL1).
        blocksize: usize,
        /// Rice bytes per pixel (ZVAL2).
        rice_bytepix: usize,
        /// Underlying binary table row width.
        naxis1: usize,
        /// Underlying binary table row count (number of tiles).
        naxis2: usize,
        /// Heap size for variable-length compressed data.
        pcount: usize,
        /// Number of columns in the underlying binary table.
        tfields: usize,
    },
}

/// A single Header Data Unit parsed from a FITS byte stream.
#[derive(Debug, Clone)]
pub struct Hdu {
    /// Parsed metadata describing the HDU type and shape.
    pub info: HduInfo,
    /// Byte offset where the header begins in the FITS stream.
    pub header_start: usize,
    /// Byte offset where the data segment begins.
    pub data_start: usize,
    /// Length of the data segment in bytes (unpadded).
    pub data_len: usize,
    /// All header cards parsed from this HDU.
    pub cards: Vec<Card>,
}

/// A collection of HDUs parsed from a complete FITS file.
#[derive(Debug, Clone)]
pub struct FitsData {
    /// All HDUs in the file, with the primary HDU at index 0.
    pub hdus: Vec<Hdu>,
}

impl FitsData {
    /// Returns the primary (first) HDU.
    pub fn primary(&self) -> &Hdu {
        &self.hdus[0]
    }

    /// Returns the HDU at the given index, or `None` if out of bounds.
    pub fn get(&self, index: usize) -> Option<&Hdu> {
        self.hdus.get(index)
    }

    /// Finds the first HDU whose EXTNAME matches `name`.
    pub fn find_by_name(&self, name: &str) -> Option<&Hdu> {
        self.hdus.iter().find(|hdu| {
            card_string_value(&hdu.cards, "EXTNAME")
                .map(|s| s == name)
                .unwrap_or(false)
        })
    }

    /// Returns the number of HDUs.
    pub fn len(&self) -> usize {
        self.hdus.len()
    }

    /// Returns `true` if the file contains no HDUs.
    pub fn is_empty(&self) -> bool {
        self.hdus.is_empty()
    }

    /// Iterates over all HDUs in order.
    pub fn iter(&self) -> impl Iterator<Item = &Hdu> {
        self.hdus.iter()
    }
}

fn card_integer_value(cards: &[Card], keyword: &str) -> Option<i64> {
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

fn card_string_value(cards: &[Card], keyword: &str) -> Option<String> {
    cards.iter().find_map(|c| {
        if c.keyword_str() == keyword {
            match &c.value {
                Some(Value::String(s)) => Some(s.trim().into()),
                _ => None,
            }
        } else {
            None
        }
    })
}

fn card_logical_value(cards: &[Card], keyword: &str) -> Option<bool> {
    cards.iter().find_map(|c| {
        if c.keyword_str() == keyword {
            match &c.value {
                Some(Value::Logical(b)) => Some(*b),
                _ => None,
            }
        } else {
            None
        }
    })
}

fn is_primary_hdu(cards: &[Card]) -> bool {
    cards
        .first()
        .map(|c| c.keyword_str() == "SIMPLE")
        .unwrap_or(false)
}

fn compute_data_byte_len(cards: &[Card], is_primary: bool) -> Result<usize> {
    let bitpix = card_integer_value(cards, "BITPIX").ok_or(Error::MissingKeyword("BITPIX"))?;
    let naxis = card_integer_value(cards, "NAXIS").ok_or(Error::MissingKeyword("NAXIS"))?;
    let naxis = naxis as usize;

    if naxis == 0 {
        return Ok(0);
    }

    let bytes_per_value = (bitpix.unsigned_abs() as usize) / 8;

    let mut dims = Vec::with_capacity(naxis);
    for i in 1..=naxis {
        let kw = alloc::format!("NAXIS{}", i);
        let dim = card_integer_value(cards, &kw).ok_or(Error::MissingKeyword("NAXISn"))? as usize;
        dims.push(dim);
    }

    // Random groups: primary HDU with NAXIS1=0 and GROUPS=T
    if is_primary && dims[0] == 0 && card_logical_value(cards, "GROUPS") == Some(true) {
        let pcount =
            card_integer_value(cards, "PCOUNT").ok_or(Error::MissingKeyword("PCOUNT"))? as usize;
        let gcount =
            card_integer_value(cards, "GCOUNT").ok_or(Error::MissingKeyword("GCOUNT"))? as usize;

        // Product of NAXIS2 * NAXIS3 * ... * NAXISm
        let mut product: usize = 1;
        for &d in &dims[1..] {
            product = product
                .checked_mul(d)
                .ok_or(Error::InvalidHeader("NAXIS overflow"))?;
        }

        // Nbytes = bytes_per_value * GCOUNT * (PCOUNT + product)
        let group_size = pcount
            .checked_add(product)
            .ok_or(Error::InvalidHeader("random groups size overflow"))?;
        let data_bytes = bytes_per_value
            .checked_mul(gcount)
            .ok_or(Error::InvalidHeader("random groups size overflow"))?
            .checked_mul(group_size)
            .ok_or(Error::InvalidHeader("random groups size overflow"))?;
        return Ok(data_bytes);
    }

    let total_pixels: usize = dims
        .iter()
        .try_fold(1usize, |acc, &d| acc.checked_mul(d))
        .ok_or(Error::InvalidHeader("pixel count overflow"))?;

    let pcount = if is_primary {
        0
    } else {
        card_integer_value(cards, "PCOUNT").unwrap_or(0) as usize
    };

    let gcount = if is_primary {
        1
    } else {
        let g = card_integer_value(cards, "GCOUNT").unwrap_or(1) as usize;
        if g == 0 {
            1
        } else {
            g
        }
    };

    let data_bytes = gcount
        .checked_mul(
            total_pixels
                .checked_mul(bytes_per_value)
                .ok_or(Error::InvalidHeader("data size overflow"))?,
        )
        .ok_or(Error::InvalidHeader("data size overflow"))?
        .checked_add(
            gcount
                .checked_mul(pcount)
                .ok_or(Error::InvalidHeader("data size overflow"))?,
        )
        .ok_or(Error::InvalidHeader("data size overflow"))?;

    Ok(data_bytes)
}

fn parse_hdu_info(cards: &[Card], is_primary: bool) -> Result<HduInfo> {
    if is_primary {
        let bitpix = card_integer_value(cards, "BITPIX").ok_or(Error::MissingKeyword("BITPIX"))?;
        let naxis =
            card_integer_value(cards, "NAXIS").ok_or(Error::MissingKeyword("NAXIS"))? as usize;
        let mut naxes = Vec::with_capacity(naxis);
        for i in 1..=naxis {
            let kw = alloc::format!("NAXIS{}", i);
            let dim =
                card_integer_value(cards, &kw).ok_or(Error::MissingKeyword("NAXISn"))? as usize;
            naxes.push(dim);
        }

        if naxis > 0 && naxes[0] == 0 && card_logical_value(cards, "GROUPS") == Some(true) {
            let pcount = card_integer_value(cards, "PCOUNT")
                .ok_or(Error::MissingKeyword("PCOUNT"))? as usize;
            let gcount = card_integer_value(cards, "GCOUNT")
                .ok_or(Error::MissingKeyword("GCOUNT"))? as usize;
            return Ok(HduInfo::RandomGroups {
                bitpix,
                naxes: naxes[1..].to_vec(),
                pcount,
                gcount,
            });
        }

        return Ok(HduInfo::Primary { bitpix, naxes });
    }

    let xtension = card_string_value(cards, "XTENSION").ok_or(Error::MissingKeyword("XTENSION"))?;
    match xtension.as_str() {
        "IMAGE" => {
            let bitpix =
                card_integer_value(cards, "BITPIX").ok_or(Error::MissingKeyword("BITPIX"))?;
            let naxis =
                card_integer_value(cards, "NAXIS").ok_or(Error::MissingKeyword("NAXIS"))? as usize;
            let mut naxes = Vec::with_capacity(naxis);
            for i in 1..=naxis {
                let kw = alloc::format!("NAXIS{}", i);
                let dim =
                    card_integer_value(cards, &kw).ok_or(Error::MissingKeyword("NAXISn"))? as usize;
                naxes.push(dim);
            }
            Ok(HduInfo::Image { bitpix, naxes })
        }
        "TABLE" => {
            let naxis1 = card_integer_value(cards, "NAXIS1")
                .ok_or(Error::MissingKeyword("NAXIS1"))? as usize;
            let naxis2 = card_integer_value(cards, "NAXIS2")
                .ok_or(Error::MissingKeyword("NAXIS2"))? as usize;
            let tfields = card_integer_value(cards, "TFIELDS")
                .ok_or(Error::MissingKeyword("TFIELDS"))? as usize;
            Ok(HduInfo::AsciiTable {
                naxis1,
                naxis2,
                tfields,
            })
        }
        "BINTABLE" => {
            let naxis1 = card_integer_value(cards, "NAXIS1")
                .ok_or(Error::MissingKeyword("NAXIS1"))? as usize;
            let naxis2 = card_integer_value(cards, "NAXIS2")
                .ok_or(Error::MissingKeyword("NAXIS2"))? as usize;
            let pcount = card_integer_value(cards, "PCOUNT")
                .ok_or(Error::MissingKeyword("PCOUNT"))? as usize;
            let tfields = card_integer_value(cards, "TFIELDS")
                .ok_or(Error::MissingKeyword("TFIELDS"))? as usize;

            if card_logical_value(cards, "ZIMAGE") == Some(true) {
                let zbitpix =
                    card_integer_value(cards, "ZBITPIX").ok_or(Error::MissingKeyword("ZBITPIX"))?;
                let znaxis = card_integer_value(cards, "ZNAXIS")
                    .ok_or(Error::MissingKeyword("ZNAXIS"))? as usize;
                let mut znaxes = Vec::with_capacity(znaxis);
                for i in 1..=znaxis {
                    let kw = alloc::format!("ZNAXIS{}", i);
                    let dim = card_integer_value(cards, &kw)
                        .ok_or(Error::MissingKeyword("ZNAXISn"))?
                        as usize;
                    znaxes.push(dim);
                }
                let zcmptype = card_string_value(cards, "ZCMPTYPE")
                    .ok_or(Error::MissingKeyword("ZCMPTYPE"))?;
                let mut ztile = Vec::with_capacity(znaxis);
                for i in 1..=znaxis {
                    let kw = alloc::format!("ZTILE{}", i);
                    let default = if i == 1 && !znaxes.is_empty() {
                        znaxes[0]
                    } else {
                        1
                    };
                    let val = card_integer_value(cards, &kw).unwrap_or(default as i64) as usize;
                    ztile.push(val);
                }
                let mut blocksize = card_integer_value(cards, "ZVAL1").unwrap_or(32) as usize;
                let mut rice_bytepix = card_integer_value(cards, "ZVAL2").unwrap_or(4) as usize;
                // cfitsio compatibility: if blocksize < 16 and bytepix > 8, values are swapped
                if blocksize < 16 && rice_bytepix > 8 {
                    core::mem::swap(&mut blocksize, &mut rice_bytepix);
                }
                return Ok(HduInfo::CompressedImage {
                    zbitpix,
                    znaxes,
                    zcmptype,
                    ztile,
                    blocksize,
                    rice_bytepix,
                    naxis1,
                    naxis2,
                    pcount,
                    tfields,
                });
            }

            Ok(HduInfo::BinaryTable {
                naxis1,
                naxis2,
                pcount,
                tfields,
            })
        }
        other => Err(Error::UnsupportedExtension(
            if other.starts_with("A3DTABLE") {
                "A3DTABLE"
            } else if other.starts_with("FOREIGN") {
                "FOREIGN"
            } else {
                "unknown XTENSION"
            },
        )),
    }
}

/// Parse a complete FITS byte stream into a [`FitsData`] containing all HDUs.
pub fn parse_fits(data: &[u8]) -> Result<FitsData> {
    if data.is_empty() {
        return Err(Error::UnexpectedEof);
    }
    if data.len() < BLOCK_SIZE {
        return Err(Error::UnexpectedEof);
    }

    let mut hdus = Vec::new();
    let mut offset: usize = 0;

    while offset < data.len() {
        let remaining = &data[offset..];
        if remaining.len() < BLOCK_SIZE {
            break;
        }

        let header_len = match header_byte_len(remaining) {
            Ok(len) => len,
            Err(_) if !hdus.is_empty() => break,
            Err(e) => return Err(e),
        };
        let header_data = &remaining[..header_len];
        let cards = match parse_header_blocks(header_data) {
            Ok(cards) => cards,
            Err(_) if !hdus.is_empty() => break,
            Err(e) => return Err(e),
        };

        let is_primary = hdus.is_empty() && is_primary_hdu(&cards);
        if hdus.is_empty() && !is_primary {
            return Err(Error::InvalidHeader("first HDU must be primary"));
        }

        let info = match parse_hdu_info(&cards, is_primary) {
            Ok(info) => info,
            Err(_) if !hdus.is_empty() => break,
            Err(e) => return Err(e),
        };
        let data_len = match compute_data_byte_len(&cards, is_primary) {
            Ok(len) => len,
            Err(_) if !hdus.is_empty() => break,
            Err(e) => return Err(e),
        };
        let data_start = offset + header_len;

        // Require that all actual data bytes are present, but allow
        // the trailing block padding to be missing.  Many real-world
        // files (HiPS tiles from Aladin/Hipsgen) omit trailing padding.
        if data_len > 0 && data_start + data_len > data.len() {
            return Err(Error::UnexpectedEof);
        }

        hdus.push(Hdu {
            info,
            header_start: offset,
            data_start,
            data_len,
            cards,
        });

        let padded_data = padded_byte_len(data_len);
        offset = data_start + padded_data;
    }

    if hdus.is_empty() {
        return Err(Error::InvalidHeader("no valid HDUs found"));
    }

    Ok(FitsData { hdus })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::BLOCK_SIZE;
    use crate::header::{serialize_header, Card};
    use crate::value::Value;
    use alloc::string::String;
    use alloc::vec;

    fn make_keyword(name: &str) -> [u8; 8] {
        let mut k = [b' '; 8];
        let bytes = name.as_bytes();
        let len = bytes.len().min(8);
        k[..len].copy_from_slice(&bytes[..len]);
        k
    }

    fn card(keyword: &str, value: Value) -> Card {
        Card {
            keyword: make_keyword(keyword),
            value: Some(value),
            comment: None,
        }
    }

    fn primary_header_naxis0() -> Vec<Card> {
        vec![
            card("SIMPLE", Value::Logical(true)),
            card("BITPIX", Value::Integer(8)),
            card("NAXIS", Value::Integer(0)),
        ]
    }

    fn primary_header_image(bitpix: i64, dims: &[usize]) -> Vec<Card> {
        let mut cards = vec![
            card("SIMPLE", Value::Logical(true)),
            card("BITPIX", Value::Integer(bitpix)),
            card("NAXIS", Value::Integer(dims.len() as i64)),
        ];
        for (i, &d) in dims.iter().enumerate() {
            let kw = alloc::format!("NAXIS{}", i + 1);
            cards.push(card(&kw, Value::Integer(d as i64)));
        }
        cards
    }

    fn image_extension_header(bitpix: i64, dims: &[usize], extname: Option<&str>) -> Vec<Card> {
        let mut cards = vec![
            card("XTENSION", Value::String(String::from("IMAGE"))),
            card("BITPIX", Value::Integer(bitpix)),
            card("NAXIS", Value::Integer(dims.len() as i64)),
        ];
        for (i, &d) in dims.iter().enumerate() {
            let kw = alloc::format!("NAXIS{}", i + 1);
            cards.push(card(&kw, Value::Integer(d as i64)));
        }
        cards.push(card("PCOUNT", Value::Integer(0)));
        cards.push(card("GCOUNT", Value::Integer(1)));
        if let Some(name) = extname {
            cards.push(card("EXTNAME", Value::String(String::from(name))));
        }
        cards
    }

    fn bintable_extension_header(
        naxis1: usize,
        naxis2: usize,
        pcount: usize,
        tfields: usize,
        extname: Option<&str>,
    ) -> Vec<Card> {
        let mut cards = vec![
            card("XTENSION", Value::String(String::from("BINTABLE"))),
            card("BITPIX", Value::Integer(8)),
            card("NAXIS", Value::Integer(2)),
            card("NAXIS1", Value::Integer(naxis1 as i64)),
            card("NAXIS2", Value::Integer(naxis2 as i64)),
            card("PCOUNT", Value::Integer(pcount as i64)),
            card("GCOUNT", Value::Integer(1)),
            card("TFIELDS", Value::Integer(tfields as i64)),
        ];
        if let Some(name) = extname {
            cards.push(card("EXTNAME", Value::String(String::from(name))));
        }
        cards
    }

    fn build_fits_bytes(header_cards: &[Card], data_bytes: usize) -> Vec<u8> {
        let header = serialize_header(header_cards).unwrap();
        let padded_data = padded_byte_len(data_bytes);
        let mut result = Vec::with_capacity(header.len() + padded_data);
        result.extend_from_slice(&header);
        result.resize(header.len() + padded_data, 0u8);
        result
    }

    #[test]
    fn parse_minimal_primary_naxis0() {
        let cards = primary_header_naxis0();
        let data = build_fits_bytes(&cards, 0);
        let fits = parse_fits(&data).unwrap();

        assert_eq!(fits.len(), 1);
        assert!(!fits.is_empty());
        let primary = fits.primary();
        assert_eq!(primary.header_start, 0);
        assert_eq!(primary.data_start, BLOCK_SIZE);
        assert_eq!(primary.data_len, 0);
        match &primary.info {
            HduInfo::Primary { bitpix, naxes } => {
                assert_eq!(*bitpix, 8);
                assert!(naxes.is_empty());
            }
            other => panic!("Expected Primary, got {:?}", other),
        }
    }

    #[test]
    fn parse_primary_with_image_data() {
        let dims = [100, 200];
        let bitpix: i64 = 16;
        let cards = primary_header_image(bitpix, &dims);
        let data_bytes = 100 * 200 * 2; // 16 bits = 2 bytes per pixel
        let data = build_fits_bytes(&cards, data_bytes);
        let fits = parse_fits(&data).unwrap();

        assert_eq!(fits.len(), 1);
        let primary = fits.primary();
        assert_eq!(primary.data_len, data_bytes);
        assert_eq!(primary.header_start, 0);
        assert_eq!(primary.data_start, BLOCK_SIZE);
        match &primary.info {
            HduInfo::Primary { bitpix, naxes } => {
                assert_eq!(*bitpix, 16);
                assert_eq!(naxes, &[100, 200]);
            }
            other => panic!("Expected Primary, got {:?}", other),
        }
    }

    #[test]
    fn parse_primary_32bit_float_image() {
        let dims = [10, 10];
        let bitpix: i64 = -32;
        let cards = primary_header_image(bitpix, &dims);
        let data_bytes = 10 * 10 * 4;
        let data = build_fits_bytes(&cards, data_bytes);
        let fits = parse_fits(&data).unwrap();

        let primary = fits.primary();
        assert_eq!(primary.data_len, 400);
        match &primary.info {
            HduInfo::Primary { bitpix, naxes } => {
                assert_eq!(*bitpix, -32);
                assert_eq!(naxes, &[10, 10]);
            }
            other => panic!("Expected Primary, got {:?}", other),
        }
    }

    #[test]
    fn parse_multi_extension_fits() {
        let primary_cards = primary_header_naxis0();
        let ext_cards = image_extension_header(16, &[64, 64], Some("SCI"));

        let primary_header = serialize_header(&primary_cards).unwrap();
        let ext_header = serialize_header(&ext_cards).unwrap();
        let ext_data_bytes = 64 * 64 * 2;
        let ext_data_padded = padded_byte_len(ext_data_bytes);

        let mut data = Vec::new();
        data.extend_from_slice(&primary_header);
        data.extend_from_slice(&ext_header);
        data.resize(data.len() + ext_data_padded, 0u8);

        let fits = parse_fits(&data).unwrap();
        assert_eq!(fits.len(), 2);

        let primary = fits.primary();
        assert_eq!(primary.data_len, 0);
        match &primary.info {
            HduInfo::Primary { bitpix, naxes } => {
                assert_eq!(*bitpix, 8);
                assert!(naxes.is_empty());
            }
            other => panic!("Expected Primary, got {:?}", other),
        }

        let ext = fits.get(1).unwrap();
        assert_eq!(ext.data_len, ext_data_bytes);
        match &ext.info {
            HduInfo::Image { bitpix, naxes } => {
                assert_eq!(*bitpix, 16);
                assert_eq!(naxes, &[64, 64]);
            }
            other => panic!("Expected Image, got {:?}", other),
        }
    }

    #[test]
    fn find_by_name_lookup() {
        let primary_cards = primary_header_naxis0();
        let ext1_cards = image_extension_header(-32, &[32, 32], Some("SCI"));
        let ext2_cards = image_extension_header(16, &[10, 10], Some("ERR"));

        let primary_header = serialize_header(&primary_cards).unwrap();
        let ext1_header = serialize_header(&ext1_cards).unwrap();
        let ext1_data_bytes = 32 * 32 * 4;
        let ext1_data_padded = padded_byte_len(ext1_data_bytes);
        let ext2_header = serialize_header(&ext2_cards).unwrap();
        let ext2_data_bytes = 10 * 10 * 2;
        let ext2_data_padded = padded_byte_len(ext2_data_bytes);

        let mut data = Vec::new();
        data.extend_from_slice(&primary_header);
        data.extend_from_slice(&ext1_header);
        data.resize(data.len() + ext1_data_padded, 0u8);
        data.extend_from_slice(&ext2_header);
        data.resize(data.len() + ext2_data_padded, 0u8);

        let fits = parse_fits(&data).unwrap();
        assert_eq!(fits.len(), 3);

        let sci = fits.find_by_name("SCI").unwrap();
        match &sci.info {
            HduInfo::Image { bitpix, naxes } => {
                assert_eq!(*bitpix, -32);
                assert_eq!(naxes, &[32, 32]);
            }
            other => panic!("Expected Image, got {:?}", other),
        }

        let err = fits.find_by_name("ERR").unwrap();
        match &err.info {
            HduInfo::Image { bitpix, naxes } => {
                assert_eq!(*bitpix, 16);
                assert_eq!(naxes, &[10, 10]);
            }
            other => panic!("Expected Image, got {:?}", other),
        }

        assert!(fits.find_by_name("MISSING").is_none());
    }

    #[test]
    fn correct_byte_offsets() {
        let primary_cards = primary_header_image(8, &[100]);
        let primary_header = serialize_header(&primary_cards).unwrap();
        let primary_data_bytes = 100;
        let primary_data_padded = padded_byte_len(primary_data_bytes);

        let ext_cards = image_extension_header(-64, &[50], None);
        let ext_header = serialize_header(&ext_cards).unwrap();
        let ext_data_bytes = 50 * 8;
        let ext_data_padded = padded_byte_len(ext_data_bytes);

        let mut data = Vec::new();
        data.extend_from_slice(&primary_header);
        data.resize(data.len() + primary_data_padded, 0u8);
        data.extend_from_slice(&ext_header);
        data.resize(data.len() + ext_data_padded, 0u8);

        let fits = parse_fits(&data).unwrap();
        assert_eq!(fits.len(), 2);

        let p = fits.primary();
        assert_eq!(p.header_start, 0);
        assert_eq!(p.data_start, primary_header.len());
        assert_eq!(p.data_len, primary_data_bytes);

        let ext = fits.get(1).unwrap();
        let expected_ext_header_start = primary_header.len() + primary_data_padded;
        assert_eq!(ext.header_start, expected_ext_header_start);
        assert_eq!(ext.data_start, expected_ext_header_start + ext_header.len());
        assert_eq!(ext.data_len, ext_data_bytes);
    }

    #[test]
    fn data_length_calculation_naxis0() {
        let cards = primary_header_naxis0();
        let len = compute_data_byte_len(&cards, true).unwrap();
        assert_eq!(len, 0);
    }

    #[test]
    fn data_length_calculation_2d_image() {
        let cards = primary_header_image(16, &[100, 200]);
        let len = compute_data_byte_len(&cards, true).unwrap();
        assert_eq!(len, 100 * 200 * 2);
    }

    #[test]
    fn data_length_calculation_float64() {
        let cards = primary_header_image(-64, &[50, 50]);
        let len = compute_data_byte_len(&cards, true).unwrap();
        assert_eq!(len, 50 * 50 * 8);
    }

    #[test]
    fn data_length_bintable_with_pcount() {
        let cards_vec = bintable_extension_header(24, 100, 500, 3, None);
        let len = compute_data_byte_len(&cards_vec, false).unwrap();
        assert_eq!(len, 24 * 100 + 500);
    }

    #[test]
    fn error_on_empty_data() {
        assert!(parse_fits(&[]).is_err());
    }

    #[test]
    fn error_on_too_small() {
        let data = vec![0u8; 100];
        assert!(parse_fits(&data).is_err());
    }

    #[test]
    fn error_on_invalid_first_hdu() {
        let cards = vec![
            card("XTENSION", Value::String(String::from("IMAGE"))),
            card("BITPIX", Value::Integer(8)),
            card("NAXIS", Value::Integer(0)),
            card("PCOUNT", Value::Integer(0)),
            card("GCOUNT", Value::Integer(1)),
        ];
        let data = build_fits_bytes(&cards, 0);
        assert!(parse_fits(&data).is_err());
    }

    #[test]
    fn error_on_truncated_data() {
        let cards = primary_header_image(16, &[100, 200]);
        let header = serialize_header(&cards).unwrap();
        let mut data = Vec::new();
        data.extend_from_slice(&header);
        data.resize(header.len() + BLOCK_SIZE, 0u8);

        assert!(parse_fits(&data).is_err());
    }

    #[test]
    fn iter_over_hdus() {
        let primary_cards = primary_header_naxis0();
        let ext_cards = image_extension_header(8, &[10], None);

        let mut data = Vec::new();
        data.extend_from_slice(&serialize_header(&primary_cards).unwrap());
        data.extend_from_slice(&serialize_header(&ext_cards).unwrap());
        data.resize(data.len() + padded_byte_len(10), 0u8);

        let fits = parse_fits(&data).unwrap();
        let count = fits.iter().count();
        assert_eq!(count, 2);
    }

    #[test]
    fn get_out_of_bounds() {
        let cards = primary_header_naxis0();
        let data = build_fits_bytes(&cards, 0);
        let fits = parse_fits(&data).unwrap();
        assert!(fits.get(1).is_none());
    }

    #[test]
    fn parse_bintable_extension() {
        let primary_cards = primary_header_naxis0();
        let ext_cards = bintable_extension_header(24, 100, 0, 3, Some("EVENTS"));

        let primary_header = serialize_header(&primary_cards).unwrap();
        let ext_header = serialize_header(&ext_cards).unwrap();
        let ext_data_bytes = 24 * 100;
        let ext_data_padded = padded_byte_len(ext_data_bytes);

        let mut data = Vec::new();
        data.extend_from_slice(&primary_header);
        data.extend_from_slice(&ext_header);
        data.resize(data.len() + ext_data_padded, 0u8);

        let fits = parse_fits(&data).unwrap();
        assert_eq!(fits.len(), 2);

        let ext = fits.get(1).unwrap();
        assert_eq!(ext.data_len, ext_data_bytes);
        match &ext.info {
            HduInfo::BinaryTable {
                naxis1,
                naxis2,
                pcount,
                tfields,
            } => {
                assert_eq!(*naxis1, 24);
                assert_eq!(*naxis2, 100);
                assert_eq!(*pcount, 0);
                assert_eq!(*tfields, 3);
            }
            other => panic!("Expected BinaryTable, got {:?}", other),
        }

        let found = fits.find_by_name("EVENTS").unwrap();
        assert_eq!(found.header_start, ext.header_start);
    }

    #[test]
    fn three_extensions() {
        let primary_cards = primary_header_naxis0();
        let ext1 = image_extension_header(8, &[10], Some("A"));
        let ext2 = image_extension_header(16, &[20], Some("B"));
        let ext3 = image_extension_header(-32, &[30], Some("C"));

        let mut data = Vec::new();
        data.extend_from_slice(&serialize_header(&primary_cards).unwrap());

        for (ext_cards, dim, bpp) in [(&ext1, 10usize, 1usize), (&ext2, 20, 2), (&ext3, 30, 4)] {
            data.extend_from_slice(&serialize_header(ext_cards).unwrap());
            let db = dim * bpp;
            let padded = padded_byte_len(db);
            data.resize(data.len() + padded, 0u8);
        }

        let fits = parse_fits(&data).unwrap();
        assert_eq!(fits.len(), 4);

        assert!(fits.find_by_name("A").is_some());
        assert!(fits.find_by_name("B").is_some());
        assert!(fits.find_by_name("C").is_some());
    }

    fn random_groups_header(
        bitpix: i64,
        naxes: &[usize],
        pcount: usize,
        gcount: usize,
    ) -> Vec<Card> {
        // naxes should include NAXIS1=0 as the first element
        let naxis = naxes.len();
        let mut cards = vec![
            card("SIMPLE", Value::Logical(true)),
            card("BITPIX", Value::Integer(bitpix)),
            card("NAXIS", Value::Integer(naxis as i64)),
        ];
        for (i, &d) in naxes.iter().enumerate() {
            let kw = alloc::format!("NAXIS{}", i + 1);
            cards.push(card(&kw, Value::Integer(d as i64)));
        }
        cards.push(card("GROUPS", Value::Logical(true)));
        cards.push(card("PCOUNT", Value::Integer(pcount as i64)));
        cards.push(card("GCOUNT", Value::Integer(gcount as i64)));
        cards
    }

    #[test]
    fn parse_random_groups_synthetic() {
        // BITPIX=-32, NAXIS=6, NAXIS1=0, NAXIS2=3, NAXIS3=4, NAXIS4-6=1
        // GROUPS=T, PCOUNT=6, GCOUNT=2
        // data_bytes = 4 * 2 * (6 + 3*4*1*1*1) = 4 * 2 * 18 = 144
        let cards = random_groups_header(-32, &[0, 3, 4, 1, 1, 1], 6, 2);
        let data_bytes = 144; // 4 * 2 * (6 + 3*4*1*1*1)
        let data = build_fits_bytes(&cards, data_bytes);
        let fits = parse_fits(&data).unwrap();

        assert_eq!(fits.len(), 1);
        let primary = fits.primary();
        assert_eq!(primary.data_len, 144);
        match &primary.info {
            HduInfo::RandomGroups {
                bitpix,
                naxes,
                pcount,
                gcount,
            } => {
                assert_eq!(*bitpix, -32);
                assert_eq!(naxes, &[3, 4, 1, 1, 1]);
                assert_eq!(*pcount, 6);
                assert_eq!(*gcount, 2);
            }
            other => panic!("Expected RandomGroups, got {:?}", other),
        }
    }

    #[test]
    fn random_groups_data_length() {
        let cards = random_groups_header(-32, &[0, 3, 4, 1, 1, 1], 6, 2);
        let len = compute_data_byte_len(&cards, true).unwrap();
        assert_eq!(len, 144);
    }

    #[test]
    fn parse_random_groups_real_file() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../reference/fitsrs-samples/samples/fits.gsfc.nasa.gov/Random_Groups.fits");
        if !path.exists() {
            eprintln!("Skipping: {:?} not found", path);
            return;
        }
        let data = std::fs::read(&path).unwrap();
        let fits = parse_fits(&data).unwrap();

        let primary = fits.primary();
        match &primary.info {
            HduInfo::RandomGroups {
                bitpix,
                naxes,
                pcount,
                gcount,
            } => {
                assert_eq!(*bitpix, -32);
                assert_eq!(naxes, &[3, 4, 1, 1, 1]);
                assert_eq!(*pcount, 6);
                assert_eq!(*gcount, 7956);
            }
            other => panic!("Expected RandomGroups, got {:?}", other),
        }

        // data_bytes = 4 * 7956 * (6 + 3*4*1*1*1) = 4 * 7956 * 18 = 572832
        assert_eq!(primary.data_len, 572832);
    }

    #[test]
    fn primary_3d_cube() {
        let dims = [10, 20, 30];
        let bitpix: i64 = -32;
        let cards = primary_header_image(bitpix, &dims);
        let data_bytes = 10 * 20 * 30 * 4;
        let data = build_fits_bytes(&cards, data_bytes);
        let fits = parse_fits(&data).unwrap();

        let primary = fits.primary();
        assert_eq!(primary.data_len, data_bytes);
        match &primary.info {
            HduInfo::Primary { bitpix, naxes } => {
                assert_eq!(*bitpix, -32);
                assert_eq!(naxes, &[10, 20, 30]);
            }
            other => panic!("Expected Primary, got {:?}", other),
        }
    }
}
