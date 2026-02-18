//! ASCII table HDU reading and writing for FITS files.

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::block::padded_byte_len;
use crate::error::{Error, Result};
use crate::hdu::{Hdu, HduInfo};
use crate::header::Card;
use crate::value::Value;

// ── Column Format ──

/// The format code for an ASCII table column, parsed from a TFORMn keyword.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsciiColumnFormat {
    /// `Aw` -- character string, `w` characters wide.
    Character(usize),
    /// `Iw` -- integer, `w` characters wide.
    Integer(usize),
    /// `Fw.d` -- fixed-point decimal, `w` wide with `d` decimal places.
    FloatF(usize, usize),
    /// `Ew.d` -- single-precision exponential, `w` wide with `d` decimal places.
    FloatE(usize, usize),
    /// `Dw.d` -- double-precision exponential, `w` wide with `d` decimal places.
    DoubleE(usize, usize),
}

impl AsciiColumnFormat {
    /// Return the total width in bytes of a field with this format.
    pub fn width(&self) -> usize {
        match self {
            AsciiColumnFormat::Character(w)
            | AsciiColumnFormat::Integer(w)
            | AsciiColumnFormat::FloatF(w, _)
            | AsciiColumnFormat::FloatE(w, _)
            | AsciiColumnFormat::DoubleE(w, _) => *w,
        }
    }
}

// ── Column Descriptor ──

/// Describes one column in an ASCII table extension.
#[derive(Debug, Clone, PartialEq)]
pub struct AsciiColumnDescriptor {
    /// Column name from TTYPEn (may be absent).
    pub name: Option<String>,
    /// The format code from TFORMn.
    pub format: AsciiColumnFormat,
    /// 0-indexed byte position within the row (converted from 1-indexed TBCOLn).
    pub tbcol: usize,
}

// ── Column Data ──

/// Holds the data for one column across all rows (or a single row).
#[derive(Debug, Clone, PartialEq)]
pub enum AsciiColumnData {
    /// Character/string column.
    Character(Vec<String>),
    /// Integer column.
    Integer(Vec<i64>),
    /// Float column (covers Fw.d, Ew.d, and Dw.d).
    Float(Vec<f64>),
}

// ── TFORM Parsing ──

/// Parse a FITS ASCII-table TFORM string such as `"A20"`, `"I10"`, `"F12.4"`,
/// `"E15.7"`, or `"D25.17"`.
pub fn parse_tform_ascii(s: &str) -> Result<AsciiColumnFormat> {
    let s = s.trim();
    if s.is_empty() {
        return Err(Error::InvalidValue);
    }

    let code = s.as_bytes()[0];
    let rest = &s[1..];

    match code {
        b'A' => {
            let w = parse_usize(rest)?;
            Ok(AsciiColumnFormat::Character(w))
        }
        b'I' => {
            let w = parse_usize(rest)?;
            Ok(AsciiColumnFormat::Integer(w))
        }
        b'F' => {
            let (w, d) = parse_width_decimal(rest)?;
            Ok(AsciiColumnFormat::FloatF(w, d))
        }
        b'E' => {
            let (w, d) = parse_width_decimal(rest)?;
            Ok(AsciiColumnFormat::FloatE(w, d))
        }
        b'D' => {
            let (w, d) = parse_width_decimal(rest)?;
            Ok(AsciiColumnFormat::DoubleE(w, d))
        }
        _ => Err(Error::InvalidValue),
    }
}

fn parse_usize(s: &str) -> Result<usize> {
    s.parse::<usize>().map_err(|_| Error::InvalidValue)
}

fn parse_width_decimal(s: &str) -> Result<(usize, usize)> {
    let dot = s.find('.').ok_or(Error::InvalidValue)?;
    let w = parse_usize(&s[..dot])?;
    let d = parse_usize(&s[dot + 1..])?;
    Ok((w, d))
}

// ── Column Descriptor Parsing ──

/// Extract column descriptors from the header cards of an ASCII table extension.
///
/// Reads `TFORMn`, `TBCOLn`, and optionally `TTYPEn` for each column
/// `n` in `1..=tfields`.
pub fn parse_ascii_table_columns(
    cards: &[Card],
    tfields: usize,
) -> Result<Vec<AsciiColumnDescriptor>> {
    let mut columns = Vec::with_capacity(tfields);

    for i in 1..=tfields {
        let tform_kw = format!("TFORM{}", i);
        let tform_str =
            find_card_string(cards, &tform_kw).ok_or(Error::MissingKeyword("TFORMn"))?;
        let fmt = parse_tform_ascii(&tform_str)?;

        let tbcol_kw = format!("TBCOL{}", i);
        let tbcol_val =
            find_card_integer(cards, &tbcol_kw).ok_or(Error::MissingKeyword("TBCOLn"))?;
        if tbcol_val < 1 {
            return Err(Error::InvalidValue);
        }
        let tbcol = (tbcol_val - 1) as usize; // convert to 0-indexed

        let ttype_kw = format!("TTYPE{}", i);
        let name = find_card_string(cards, &ttype_kw);

        columns.push(AsciiColumnDescriptor {
            name,
            format: fmt,
            tbcol,
        });
    }

    Ok(columns)
}

// ── Reading ──

/// Read a single column from all rows of an ASCII table HDU.
///
/// `fits_data` is the entire FITS byte stream. The HDU must describe an
/// `AsciiTable`.  `col_index` is 0-based.
pub fn read_ascii_column(fits_data: &[u8], hdu: &Hdu, col_index: usize) -> Result<AsciiColumnData> {
    let (naxis1, naxis2, tfields) = ascii_table_dims(hdu)?;
    if col_index >= tfields {
        return Err(Error::InvalidValue);
    }

    let columns = parse_ascii_table_columns(&hdu.cards, tfields)?;
    let col = &columns[col_index];

    let data_start = hdu.data_start;
    if data_start + naxis1 * naxis2 > fits_data.len() {
        return Err(Error::UnexpectedEof);
    }

    parse_column_values(fits_data, data_start, naxis1, naxis2, col)
}

/// Read all columns for a single row of an ASCII table HDU.
///
/// `fits_data` is the entire FITS byte stream. `row_index` is 0-based.
pub fn read_ascii_row(
    fits_data: &[u8],
    hdu: &Hdu,
    row_index: usize,
) -> Result<Vec<AsciiColumnData>> {
    let (naxis1, naxis2, tfields) = ascii_table_dims(hdu)?;
    if row_index >= naxis2 {
        return Err(Error::InvalidValue);
    }

    let columns = parse_ascii_table_columns(&hdu.cards, tfields)?;
    let data_start = hdu.data_start;
    if data_start + naxis1 * naxis2 > fits_data.len() {
        return Err(Error::UnexpectedEof);
    }

    let row_offset = data_start + row_index * naxis1;
    let mut result = Vec::with_capacity(tfields);

    for col in &columns {
        let field_start = row_offset + col.tbcol;
        let field_end = field_start + col.format.width();
        if field_end > fits_data.len() {
            return Err(Error::UnexpectedEof);
        }
        let field_bytes = &fits_data[field_start..field_end];
        let field_str = core::str::from_utf8(field_bytes).map_err(|_| Error::InvalidValue)?;

        let data = parse_single_field(field_str, &col.format)?;
        result.push(data);
    }

    Ok(result)
}

// ── Writing ──

/// Format a single value from a column data vector according to its column format.
///
/// `index` selects which element of the data vector to format.
pub fn format_ascii_field(
    value: &AsciiColumnData,
    fmt: &AsciiColumnFormat,
    index: usize,
) -> Result<String> {
    let w = fmt.width();
    match (value, fmt) {
        (AsciiColumnData::Character(vals), AsciiColumnFormat::Character(_)) => {
            let s = vals.get(index).ok_or(Error::InvalidValue)?;
            Ok(pad_or_truncate_left(s, w))
        }
        (AsciiColumnData::Integer(vals), AsciiColumnFormat::Integer(_)) => {
            let n = vals.get(index).ok_or(Error::InvalidValue)?;
            let s = format!("{}", n);
            Ok(right_justify(&s, w))
        }
        (AsciiColumnData::Float(vals), AsciiColumnFormat::FloatF(_, d)) => {
            let f = vals.get(index).ok_or(Error::InvalidValue)?;
            let s = format!("{:.*}", *d, f);
            Ok(right_justify(&s, w))
        }
        (AsciiColumnData::Float(vals), AsciiColumnFormat::FloatE(_, d)) => {
            let f = vals.get(index).ok_or(Error::InvalidValue)?;
            let s = format_exponential(*f, *d);
            Ok(right_justify(&s, w))
        }
        (AsciiColumnData::Float(vals), AsciiColumnFormat::DoubleE(_, d)) => {
            let f = vals.get(index).ok_or(Error::InvalidValue)?;
            let s = format_exponential_d(*f, *d);
            Ok(right_justify(&s, w))
        }
        _ => Err(Error::InvalidValue),
    }
}

/// Build the header cards for an ASCII table extension.
///
/// Creates XTENSION, BITPIX, NAXIS, NAXIS1, NAXIS2, PCOUNT, GCOUNT, TFIELDS,
/// and per-column TFORMn, TBCOLn, and TTYPEn cards.
pub fn build_ascii_table_cards(
    columns: &[AsciiColumnDescriptor],
    naxis2: usize,
) -> Result<Vec<Card>> {
    let naxis1: usize = columns
        .iter()
        .map(|c| c.tbcol + c.format.width())
        .max()
        .unwrap_or(0);

    let tfields = columns.len();
    let mut cards = Vec::with_capacity(8 + tfields * 3);

    cards.push(make_card("XTENSION", Value::String(String::from("TABLE"))));
    cards.push(make_card("BITPIX", Value::Integer(8)));
    cards.push(make_card("NAXIS", Value::Integer(2)));
    cards.push(make_card("NAXIS1", Value::Integer(naxis1 as i64)));
    cards.push(make_card("NAXIS2", Value::Integer(naxis2 as i64)));
    cards.push(make_card("PCOUNT", Value::Integer(0)));
    cards.push(make_card("GCOUNT", Value::Integer(1)));
    cards.push(make_card("TFIELDS", Value::Integer(tfields as i64)));

    for (i, col) in columns.iter().enumerate() {
        let n = i + 1;

        let tform_str = format_tform(&col.format);
        cards.push(make_card(&format!("TFORM{}", n), Value::String(tform_str)));

        cards.push(make_card(
            &format!("TBCOL{}", n),
            Value::Integer((col.tbcol + 1) as i64), // back to 1-indexed
        ));

        if let Some(ref name) = col.name {
            cards.push(make_card(
                &format!("TTYPE{}", n),
                Value::String(name.clone()),
            ));
        }
    }

    Ok(cards)
}

/// Serialize column data into padded FITS data bytes for an ASCII table.
///
/// `naxis1` is the row width in bytes.  Each row is written to exactly
/// `naxis1` bytes, space-padded.  The result is padded to a FITS block
/// boundary.
pub fn serialize_ascii_table(
    columns: &[AsciiColumnDescriptor],
    col_data: &[AsciiColumnData],
    naxis1: usize,
) -> Result<Vec<u8>> {
    if columns.len() != col_data.len() {
        return Err(Error::InvalidValue);
    }

    let naxis2 = match col_data.first() {
        Some(AsciiColumnData::Character(v)) => v.len(),
        Some(AsciiColumnData::Integer(v)) => v.len(),
        Some(AsciiColumnData::Float(v)) => v.len(),
        None => 0,
    };

    let raw_len = naxis1 * naxis2;
    let padded_len = padded_byte_len(raw_len);
    let mut buf = vec![b' '; padded_len];

    for row in 0..naxis2 {
        let row_start = row * naxis1;
        for (col_idx, (col, data)) in columns.iter().zip(col_data.iter()).enumerate() {
            let field_str = format_ascii_field(data, &col.format, row)?;
            let field_bytes = field_str.as_bytes();
            let dest_start = row_start + col.tbcol;
            let copy_len = field_bytes.len().min(col.format.width());
            if dest_start + copy_len > raw_len {
                return Err(Error::InvalidValue);
            }
            buf[dest_start..dest_start + copy_len].copy_from_slice(&field_bytes[..copy_len]);
            let _ = col_idx;
        }
    }

    // Zero-pad after the raw data (FITS data blocks are zero-padded).
    for b in &mut buf[raw_len..] {
        *b = 0;
    }

    Ok(buf)
}

/// Build and serialize a complete ASCII table HDU (header + data).
///
/// Returns the combined header and data bytes, each padded to block boundaries.
pub fn serialize_ascii_table_hdu(
    columns: &[AsciiColumnDescriptor],
    col_data: &[AsciiColumnData],
) -> Result<Vec<u8>> {
    let naxis1: usize = columns
        .iter()
        .map(|c| c.tbcol + c.format.width())
        .max()
        .unwrap_or(0);

    let naxis2 = match col_data.first() {
        Some(AsciiColumnData::Character(v)) => v.len(),
        Some(AsciiColumnData::Integer(v)) => v.len(),
        Some(AsciiColumnData::Float(v)) => v.len(),
        None => 0,
    };

    let cards = build_ascii_table_cards(columns, naxis2)?;
    let header_bytes = crate::header::serialize_header(&cards)?;
    let data_bytes = serialize_ascii_table(columns, col_data, naxis1)?;

    let mut result = Vec::with_capacity(header_bytes.len() + data_bytes.len());
    result.extend_from_slice(&header_bytes);
    result.extend_from_slice(&data_bytes);
    Ok(result)
}

// ── Internal Helpers ──

fn ascii_table_dims(hdu: &Hdu) -> Result<(usize, usize, usize)> {
    match &hdu.info {
        HduInfo::AsciiTable {
            naxis1,
            naxis2,
            tfields,
        } => Ok((*naxis1, *naxis2, *tfields)),
        _ => Err(Error::InvalidHeader),
    }
}

fn find_card_string(cards: &[Card], keyword: &str) -> Option<String> {
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

fn find_card_integer(cards: &[Card], keyword: &str) -> Option<i64> {
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

fn parse_column_values(
    fits_data: &[u8],
    data_start: usize,
    naxis1: usize,
    naxis2: usize,
    col: &AsciiColumnDescriptor,
) -> Result<AsciiColumnData> {
    match &col.format {
        AsciiColumnFormat::Character(w) => {
            let mut vals = Vec::with_capacity(naxis2);
            for row in 0..naxis2 {
                let offset = data_start + row * naxis1 + col.tbcol;
                let end = offset + w;
                if end > fits_data.len() {
                    return Err(Error::UnexpectedEof);
                }
                let s = core::str::from_utf8(&fits_data[offset..end])
                    .map_err(|_| Error::InvalidValue)?;
                vals.push(String::from(s.trim_end()));
            }
            Ok(AsciiColumnData::Character(vals))
        }
        AsciiColumnFormat::Integer(w) => {
            let mut vals = Vec::with_capacity(naxis2);
            for row in 0..naxis2 {
                let offset = data_start + row * naxis1 + col.tbcol;
                let end = offset + w;
                if end > fits_data.len() {
                    return Err(Error::UnexpectedEof);
                }
                let s = core::str::from_utf8(&fits_data[offset..end])
                    .map_err(|_| Error::InvalidValue)?;
                let n: i64 = s.trim().parse().map_err(|_| Error::InvalidValue)?;
                vals.push(n);
            }
            Ok(AsciiColumnData::Integer(vals))
        }
        AsciiColumnFormat::FloatF(w, _)
        | AsciiColumnFormat::FloatE(w, _)
        | AsciiColumnFormat::DoubleE(w, _) => {
            let mut vals = Vec::with_capacity(naxis2);
            for row in 0..naxis2 {
                let offset = data_start + row * naxis1 + col.tbcol;
                let end = offset + w;
                if end > fits_data.len() {
                    return Err(Error::UnexpectedEof);
                }
                let s = core::str::from_utf8(&fits_data[offset..end])
                    .map_err(|_| Error::InvalidValue)?;
                let f = parse_fits_float(s.trim())?;
                vals.push(f);
            }
            Ok(AsciiColumnData::Float(vals))
        }
    }
}

fn parse_single_field(field_str: &str, fmt: &AsciiColumnFormat) -> Result<AsciiColumnData> {
    match fmt {
        AsciiColumnFormat::Character(_) => Ok(AsciiColumnData::Character(vec![String::from(
            field_str.trim_end(),
        )])),
        AsciiColumnFormat::Integer(_) => {
            let n: i64 = field_str.trim().parse().map_err(|_| Error::InvalidValue)?;
            Ok(AsciiColumnData::Integer(vec![n]))
        }
        AsciiColumnFormat::FloatF(_, _)
        | AsciiColumnFormat::FloatE(_, _)
        | AsciiColumnFormat::DoubleE(_, _) => {
            let f = parse_fits_float(field_str.trim())?;
            Ok(AsciiColumnData::Float(vec![f]))
        }
    }
}

/// Parse a FITS float string, handling `D` exponent notation.
fn parse_fits_float(s: &str) -> Result<f64> {
    let normalized = s.replace('D', "E").replace('d', "e");
    normalized.parse::<f64>().map_err(|_| Error::InvalidValue)
}

fn pad_or_truncate_left(s: &str, width: usize) -> String {
    if s.len() >= width {
        String::from(&s[..width])
    } else {
        let mut out = String::from(s);
        for _ in 0..(width - s.len()) {
            out.push(' ');
        }
        out
    }
}

fn right_justify(s: &str, width: usize) -> String {
    if s.len() >= width {
        String::from(&s[s.len() - width..])
    } else {
        let pad = width - s.len();
        let mut out = String::with_capacity(width);
        for _ in 0..pad {
            out.push(' ');
        }
        out.push_str(s);
        out
    }
}

fn format_exponential(f: f64, d: usize) -> String {
    format!("{:.*E}", d, f)
}

fn format_exponential_d(f: f64, d: usize) -> String {
    let s = format!("{:.*E}", d, f);
    s.replace('E', "D")
}

fn format_tform(fmt: &AsciiColumnFormat) -> String {
    match fmt {
        AsciiColumnFormat::Character(w) => format!("A{}", w),
        AsciiColumnFormat::Integer(w) => format!("I{}", w),
        AsciiColumnFormat::FloatF(w, d) => format!("F{}.{}", w, d),
        AsciiColumnFormat::FloatE(w, d) => format!("E{}.{}", w, d),
        AsciiColumnFormat::DoubleE(w, d) => format!("D{}.{}", w, d),
    }
}

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

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::padded_byte_len;
    use crate::header::serialize_header;
    use alloc::string::String;
    use alloc::vec;

    // ---- TFORM parsing ----

    #[test]
    fn parse_tform_character() {
        let fmt = parse_tform_ascii("A20").unwrap();
        assert_eq!(fmt, AsciiColumnFormat::Character(20));
        assert_eq!(fmt.width(), 20);
    }

    #[test]
    fn parse_tform_integer() {
        let fmt = parse_tform_ascii("I10").unwrap();
        assert_eq!(fmt, AsciiColumnFormat::Integer(10));
        assert_eq!(fmt.width(), 10);
    }

    #[test]
    fn parse_tform_float_f() {
        let fmt = parse_tform_ascii("F12.4").unwrap();
        assert_eq!(fmt, AsciiColumnFormat::FloatF(12, 4));
        assert_eq!(fmt.width(), 12);
    }

    #[test]
    fn parse_tform_float_e() {
        let fmt = parse_tform_ascii("E15.7").unwrap();
        assert_eq!(fmt, AsciiColumnFormat::FloatE(15, 7));
        assert_eq!(fmt.width(), 15);
    }

    #[test]
    fn parse_tform_double_d() {
        let fmt = parse_tform_ascii("D25.17").unwrap();
        assert_eq!(fmt, AsciiColumnFormat::DoubleE(25, 17));
        assert_eq!(fmt.width(), 25);
    }

    #[test]
    fn parse_tform_with_whitespace() {
        let fmt = parse_tform_ascii("  A5  ").unwrap();
        assert_eq!(fmt, AsciiColumnFormat::Character(5));
    }

    #[test]
    fn parse_tform_empty_is_error() {
        assert!(parse_tform_ascii("").is_err());
    }

    #[test]
    fn parse_tform_unknown_code_is_error() {
        assert!(parse_tform_ascii("X10").is_err());
    }

    #[test]
    fn parse_tform_missing_decimal_is_error() {
        assert!(parse_tform_ascii("F12").is_err());
    }

    // ---- Column descriptor parsing ----

    fn mk_card(keyword: &str, value: Value) -> Card {
        make_card(keyword, value)
    }

    fn build_table_cards(
        naxis1: usize,
        naxis2: usize,
        col_descs: &[(Option<&str>, &str, usize)],
    ) -> Vec<Card> {
        let tfields = col_descs.len();
        let mut cards = vec![
            mk_card("XTENSION", Value::String(String::from("TABLE"))),
            mk_card("BITPIX", Value::Integer(8)),
            mk_card("NAXIS", Value::Integer(2)),
            mk_card("NAXIS1", Value::Integer(naxis1 as i64)),
            mk_card("NAXIS2", Value::Integer(naxis2 as i64)),
            mk_card("PCOUNT", Value::Integer(0)),
            mk_card("GCOUNT", Value::Integer(1)),
            mk_card("TFIELDS", Value::Integer(tfields as i64)),
        ];
        for (i, (name, tform, tbcol)) in col_descs.iter().enumerate() {
            let n = i + 1;
            cards.push(mk_card(
                &format!("TFORM{}", n),
                Value::String(String::from(*tform)),
            ));
            cards.push(mk_card(
                &format!("TBCOL{}", n),
                Value::Integer(*tbcol as i64),
            ));
            if let Some(nm) = name {
                cards.push(mk_card(
                    &format!("TTYPE{}", n),
                    Value::String(String::from(*nm)),
                ));
            }
        }
        cards
    }

    fn build_hdu(cards: Vec<Card>, data: &[u8]) -> (Vec<u8>, Hdu) {
        let header_bytes = serialize_header(&cards).unwrap();
        let data_padded_len = padded_byte_len(data.len());
        let mut fits_data = Vec::with_capacity(header_bytes.len() + data_padded_len);
        fits_data.extend_from_slice(&header_bytes);
        fits_data.resize(header_bytes.len() + data_padded_len, 0u8);
        fits_data[header_bytes.len()..header_bytes.len() + data.len()].copy_from_slice(data);

        let naxis1 = find_card_integer(&cards, "NAXIS1").unwrap_or(0) as usize;
        let naxis2 = find_card_integer(&cards, "NAXIS2").unwrap_or(0) as usize;
        let tfields = find_card_integer(&cards, "TFIELDS").unwrap_or(0) as usize;

        let hdu = Hdu {
            info: HduInfo::AsciiTable {
                naxis1,
                naxis2,
                tfields,
            },
            header_start: 0,
            data_start: header_bytes.len(),
            data_len: data.len(),
            cards,
        };

        (fits_data, hdu)
    }

    #[test]
    fn parse_columns_basic() {
        let cards = build_table_cards(
            30,
            2,
            &[
                (Some("NAME"), "A10", 1),
                (None, "I8", 11),
                (Some("VALUE"), "F12.4", 19),
            ],
        );
        let cols = parse_ascii_table_columns(&cards, 3).unwrap();
        assert_eq!(cols.len(), 3);

        assert_eq!(cols[0].name, Some(String::from("NAME")));
        assert_eq!(cols[0].format, AsciiColumnFormat::Character(10));
        assert_eq!(cols[0].tbcol, 0);

        assert_eq!(cols[1].name, None);
        assert_eq!(cols[1].format, AsciiColumnFormat::Integer(8));
        assert_eq!(cols[1].tbcol, 10);

        assert_eq!(cols[2].name, Some(String::from("VALUE")));
        assert_eq!(cols[2].format, AsciiColumnFormat::FloatF(12, 4));
        assert_eq!(cols[2].tbcol, 18);
    }

    // ---- Reading: character column ----

    #[test]
    fn read_column_character() {
        let naxis1 = 10;
        let naxis2 = 3;
        let cards = build_table_cards(naxis1, naxis2, &[(Some("NAME"), "A10", 1)]);

        let mut raw = vec![b' '; naxis1 * naxis2];
        raw[0..5].copy_from_slice(b"Hello");
        raw[10..15].copy_from_slice(b"World");
        raw[20..24].copy_from_slice(b"Test");

        let (fits_data, hdu) = build_hdu(cards, &raw);
        let col = read_ascii_column(&fits_data, &hdu, 0).unwrap();
        match col {
            AsciiColumnData::Character(vals) => {
                assert_eq!(vals.len(), 3);
                assert_eq!(vals[0], "Hello");
                assert_eq!(vals[1], "World");
                assert_eq!(vals[2], "Test");
            }
            other => panic!("Expected Character, got {:?}", other),
        }
    }

    // ---- Reading: integer column ----

    #[test]
    fn read_column_integer() {
        let naxis1 = 10;
        let naxis2 = 3;
        let cards = build_table_cards(naxis1, naxis2, &[(None, "I10", 1)]);

        let mut raw = vec![b' '; naxis1 * naxis2];
        // Write right-justified integers
        let vals_str = ["        42", "      -999", "   1234567"];
        for (i, s) in vals_str.iter().enumerate() {
            raw[i * naxis1..i * naxis1 + 10].copy_from_slice(s.as_bytes());
        }

        let (fits_data, hdu) = build_hdu(cards, &raw);
        let col = read_ascii_column(&fits_data, &hdu, 0).unwrap();
        match col {
            AsciiColumnData::Integer(vals) => {
                assert_eq!(vals, vec![42, -999, 1234567]);
            }
            other => panic!("Expected Integer, got {:?}", other),
        }
    }

    // ---- Reading: float F column ----

    #[test]
    fn read_column_float_f() {
        let naxis1 = 12;
        let naxis2 = 2;
        let cards = build_table_cards(naxis1, naxis2, &[(None, "F12.4", 1)]);

        let mut raw = vec![b' '; naxis1 * naxis2];
        let v1 = "    3.14160";
        let v2 = "  -99.99000";
        raw[0..11].copy_from_slice(v1.as_bytes());
        raw[12..23].copy_from_slice(v2.as_bytes());

        let (fits_data, hdu) = build_hdu(cards, &raw);
        let col = read_ascii_column(&fits_data, &hdu, 0).unwrap();
        match col {
            AsciiColumnData::Float(vals) => {
                assert_eq!(vals.len(), 2);
                assert!((vals[0] - 3.14160).abs() < 1e-4);
                assert!((vals[1] - (-99.99)).abs() < 1e-2);
            }
            other => panic!("Expected Float, got {:?}", other),
        }
    }

    // ---- Reading: float E column ----

    #[test]
    fn read_column_float_e() {
        let naxis1 = 15;
        let naxis2 = 2;
        let cards = build_table_cards(naxis1, naxis2, &[(None, "E15.7", 1)]);

        let mut raw = vec![b' '; naxis1 * naxis2];
        let v1 = "  1.2340000E+05";
        let v2 = " -6.7800000E-03";
        raw[0..15].copy_from_slice(v1.as_bytes());
        raw[15..30].copy_from_slice(v2.as_bytes());

        let (fits_data, hdu) = build_hdu(cards, &raw);
        let col = read_ascii_column(&fits_data, &hdu, 0).unwrap();
        match col {
            AsciiColumnData::Float(vals) => {
                assert_eq!(vals.len(), 2);
                assert!((vals[0] - 1.234e5).abs() < 1.0);
                assert!((vals[1] - (-6.78e-3)).abs() < 1e-6);
            }
            other => panic!("Expected Float, got {:?}", other),
        }
    }

    // ---- Reading: double D column ----

    #[test]
    fn read_column_double_d() {
        let naxis1 = 25;
        let naxis2 = 1;
        let cards = build_table_cards(naxis1, naxis2, &[(None, "D25.17", 1)]);

        let mut raw = vec![b' '; naxis1 * naxis2];
        let v1 = b" 3.14159265358979300D+00 ";
        assert_eq!(v1.len(), naxis1);
        raw[0..naxis1].copy_from_slice(v1);

        let (fits_data, hdu) = build_hdu(cards, &raw);
        let col = read_ascii_column(&fits_data, &hdu, 0).unwrap();
        match col {
            AsciiColumnData::Float(vals) => {
                assert_eq!(vals.len(), 1);
                assert!((vals[0] - core::f64::consts::PI).abs() < 1e-14);
            }
            other => panic!("Expected Float, got {:?}", other),
        }
    }

    // ---- Reading: multiple columns ----

    #[test]
    fn read_multi_column_table() {
        let naxis1 = 22;
        let naxis2 = 2;
        let cards = build_table_cards(
            naxis1,
            naxis2,
            &[
                (Some("NAME"), "A10", 1),
                (Some("ID"), "I4", 11),
                (Some("FLUX"), "E8.2", 15),
            ],
        );

        let mut raw = vec![b' '; naxis1 * naxis2];
        // Row 0: NAME="Alpha     ", ID=" 100", FLUX="1.23E+01"
        raw[0..5].copy_from_slice(b"Alpha");
        raw[10..14].copy_from_slice(b" 100");
        raw[14..22].copy_from_slice(b"1.23E+01");
        // Row 1: NAME="Beta      ", ID="  42", FLUX="-5.0E-02"
        raw[22..26].copy_from_slice(b"Beta");
        raw[32..36].copy_from_slice(b"  42");
        raw[36..44].copy_from_slice(b"-5.0E-02");

        let (fits_data, hdu) = build_hdu(cards, &raw);

        let col0 = read_ascii_column(&fits_data, &hdu, 0).unwrap();
        match col0 {
            AsciiColumnData::Character(vals) => {
                assert_eq!(vals[0], "Alpha");
                assert_eq!(vals[1], "Beta");
            }
            other => panic!("Expected Character, got {:?}", other),
        }

        let col1 = read_ascii_column(&fits_data, &hdu, 1).unwrap();
        match col1 {
            AsciiColumnData::Integer(vals) => {
                assert_eq!(vals[0], 100);
                assert_eq!(vals[1], 42);
            }
            other => panic!("Expected Integer, got {:?}", other),
        }

        let col2 = read_ascii_column(&fits_data, &hdu, 2).unwrap();
        match col2 {
            AsciiColumnData::Float(vals) => {
                assert!((vals[0] - 12.3).abs() < 0.1);
                assert!((vals[1] - (-0.05)).abs() < 0.01);
            }
            other => panic!("Expected Float, got {:?}", other),
        }
    }

    // ---- Reading: row ----

    #[test]
    fn read_row_basic() {
        let naxis1 = 18;
        let naxis2 = 2;
        let cards = build_table_cards(
            naxis1,
            naxis2,
            &[(Some("LABEL"), "A8", 1), (Some("COUNT"), "I10", 9)],
        );

        let mut raw = vec![b' '; naxis1 * naxis2];
        raw[0..4].copy_from_slice(b"Star");
        raw[8..18].copy_from_slice(b"       123");
        raw[18..24].copy_from_slice(b"Galaxy");
        raw[26..36].copy_from_slice(b"       456");

        let (fits_data, hdu) = build_hdu(cards, &raw);

        let row0 = read_ascii_row(&fits_data, &hdu, 0).unwrap();
        assert_eq!(row0.len(), 2);
        assert_eq!(
            row0[0],
            AsciiColumnData::Character(vec![String::from("Star")])
        );
        assert_eq!(row0[1], AsciiColumnData::Integer(vec![123]));

        let row1 = read_ascii_row(&fits_data, &hdu, 1).unwrap();
        assert_eq!(
            row1[0],
            AsciiColumnData::Character(vec![String::from("Galaxy")])
        );
        assert_eq!(row1[1], AsciiColumnData::Integer(vec![456]));
    }

    // ---- Reading: out of bounds ----

    #[test]
    fn read_column_out_of_bounds() {
        let naxis1 = 10;
        let naxis2 = 1;
        let cards = build_table_cards(naxis1, naxis2, &[(None, "A10", 1)]);
        let raw = vec![b' '; naxis1 * naxis2];
        let (fits_data, hdu) = build_hdu(cards, &raw);
        assert!(read_ascii_column(&fits_data, &hdu, 1).is_err());
    }

    #[test]
    fn read_row_out_of_bounds() {
        let naxis1 = 10;
        let naxis2 = 1;
        let cards = build_table_cards(naxis1, naxis2, &[(None, "A10", 1)]);
        let raw = vec![b' '; naxis1 * naxis2];
        let (fits_data, hdu) = build_hdu(cards, &raw);
        assert!(read_ascii_row(&fits_data, &hdu, 1).is_err());
    }

    // ---- Writing: format_ascii_field ----

    #[test]
    fn format_field_character() {
        let data = AsciiColumnData::Character(vec![String::from("Hi")]);
        let fmt = AsciiColumnFormat::Character(8);
        let s = format_ascii_field(&data, &fmt, 0).unwrap();
        assert_eq!(s.len(), 8);
        assert_eq!(s, "Hi      ");
    }

    #[test]
    fn format_field_integer() {
        let data = AsciiColumnData::Integer(vec![42]);
        let fmt = AsciiColumnFormat::Integer(10);
        let s = format_ascii_field(&data, &fmt, 0).unwrap();
        assert_eq!(s.len(), 10);
        assert_eq!(s.trim(), "42");
        assert!(s.ends_with("42"));
    }

    #[test]
    fn format_field_float_f() {
        let data = AsciiColumnData::Float(vec![3.125]);
        let fmt = AsciiColumnFormat::FloatF(12, 4);
        let s = format_ascii_field(&data, &fmt, 0).unwrap();
        assert_eq!(s.len(), 12);
        assert!(s.contains("3.1250"));
    }

    #[test]
    fn format_field_float_e() {
        let data = AsciiColumnData::Float(vec![1.234e5]);
        let fmt = AsciiColumnFormat::FloatE(15, 3);
        let s = format_ascii_field(&data, &fmt, 0).unwrap();
        assert_eq!(s.len(), 15);
        assert!(s.contains('E'));
    }

    #[test]
    fn format_field_double_d() {
        let data = AsciiColumnData::Float(vec![1.234e5]);
        let fmt = AsciiColumnFormat::DoubleE(25, 10);
        let s = format_ascii_field(&data, &fmt, 0).unwrap();
        assert_eq!(s.len(), 25);
        assert!(s.contains('D'));
    }

    // ---- Writing: build cards ----

    #[test]
    fn build_cards_basic() {
        let cols = vec![
            AsciiColumnDescriptor {
                name: Some(String::from("NAME")),
                format: AsciiColumnFormat::Character(10),
                tbcol: 0,
            },
            AsciiColumnDescriptor {
                name: None,
                format: AsciiColumnFormat::Integer(8),
                tbcol: 10,
            },
        ];
        let cards = build_ascii_table_cards(&cols, 5).unwrap();

        // Check mandatory keywords
        assert_eq!(cards[0].keyword_str(), "XTENSION");
        assert_eq!(cards[1].keyword_str(), "BITPIX");
        assert_eq!(cards[1].value, Some(Value::Integer(8)));
        assert_eq!(cards[2].keyword_str(), "NAXIS");
        assert_eq!(cards[2].value, Some(Value::Integer(2)));
        assert_eq!(cards[3].keyword_str(), "NAXIS1");
        assert_eq!(cards[3].value, Some(Value::Integer(18))); // 10 + 8
        assert_eq!(cards[4].keyword_str(), "NAXIS2");
        assert_eq!(cards[4].value, Some(Value::Integer(5)));
        assert_eq!(cards[7].keyword_str(), "TFIELDS");
        assert_eq!(cards[7].value, Some(Value::Integer(2)));
    }

    // ---- Write + read roundtrip ----

    #[test]
    fn roundtrip_character_column() {
        let cols = vec![AsciiColumnDescriptor {
            name: Some(String::from("LABEL")),
            format: AsciiColumnFormat::Character(8),
            tbcol: 0,
        }];
        let data = vec![AsciiColumnData::Character(vec![
            String::from("Alpha"),
            String::from("Beta"),
            String::from("Gamma"),
        ])];

        let naxis1 = 8;
        let naxis2 = 3;
        let cards = build_ascii_table_cards(&cols, naxis2).unwrap();
        let serialized = serialize_ascii_table(&cols, &data, naxis1).unwrap();

        let header_bytes = serialize_header(&cards).unwrap();
        let mut fits_data = Vec::new();
        fits_data.extend_from_slice(&header_bytes);
        fits_data.extend_from_slice(&serialized);

        let hdu = Hdu {
            info: HduInfo::AsciiTable {
                naxis1,
                naxis2,
                tfields: 1,
            },
            header_start: 0,
            data_start: header_bytes.len(),
            data_len: naxis1 * naxis2,
            cards,
        };

        let col = read_ascii_column(&fits_data, &hdu, 0).unwrap();
        match col {
            AsciiColumnData::Character(vals) => {
                assert_eq!(vals, vec!["Alpha", "Beta", "Gamma"]);
            }
            other => panic!("Expected Character, got {:?}", other),
        }
    }

    #[test]
    fn roundtrip_integer_column() {
        let cols = vec![AsciiColumnDescriptor {
            name: Some(String::from("COUNT")),
            format: AsciiColumnFormat::Integer(10),
            tbcol: 0,
        }];
        let data = vec![AsciiColumnData::Integer(vec![42, -7, 1000000])];

        let naxis1 = 10;
        let naxis2 = 3;
        let cards = build_ascii_table_cards(&cols, naxis2).unwrap();
        let serialized = serialize_ascii_table(&cols, &data, naxis1).unwrap();

        let header_bytes = serialize_header(&cards).unwrap();
        let mut fits_data = Vec::new();
        fits_data.extend_from_slice(&header_bytes);
        fits_data.extend_from_slice(&serialized);

        let hdu = Hdu {
            info: HduInfo::AsciiTable {
                naxis1,
                naxis2,
                tfields: 1,
            },
            header_start: 0,
            data_start: header_bytes.len(),
            data_len: naxis1 * naxis2,
            cards,
        };

        let col = read_ascii_column(&fits_data, &hdu, 0).unwrap();
        match col {
            AsciiColumnData::Integer(vals) => {
                assert_eq!(vals, vec![42, -7, 1000000]);
            }
            other => panic!("Expected Integer, got {:?}", other),
        }
    }

    #[test]
    fn roundtrip_float_column() {
        let cols = vec![AsciiColumnDescriptor {
            name: Some(String::from("FLUX")),
            format: AsciiColumnFormat::FloatE(15, 7),
            tbcol: 0,
        }];
        let data = vec![AsciiColumnData::Float(vec![1.234e5, -6.78e-3])];

        let naxis1 = 15;
        let naxis2 = 2;
        let cards = build_ascii_table_cards(&cols, naxis2).unwrap();
        let serialized = serialize_ascii_table(&cols, &data, naxis1).unwrap();

        let header_bytes = serialize_header(&cards).unwrap();
        let mut fits_data = Vec::new();
        fits_data.extend_from_slice(&header_bytes);
        fits_data.extend_from_slice(&serialized);

        let hdu = Hdu {
            info: HduInfo::AsciiTable {
                naxis1,
                naxis2,
                tfields: 1,
            },
            header_start: 0,
            data_start: header_bytes.len(),
            data_len: naxis1 * naxis2,
            cards,
        };

        let col = read_ascii_column(&fits_data, &hdu, 0).unwrap();
        match col {
            AsciiColumnData::Float(vals) => {
                assert_eq!(vals.len(), 2);
                assert!((vals[0] - 1.234e5).abs() / 1.234e5 < 1e-6);
                assert!((vals[1] - (-6.78e-3)).abs() / 6.78e-3 < 1e-6);
            }
            other => panic!("Expected Float, got {:?}", other),
        }
    }

    #[test]
    fn roundtrip_multi_column() {
        let cols = vec![
            AsciiColumnDescriptor {
                name: Some(String::from("NAME")),
                format: AsciiColumnFormat::Character(8),
                tbcol: 0,
            },
            AsciiColumnDescriptor {
                name: Some(String::from("COUNT")),
                format: AsciiColumnFormat::Integer(6),
                tbcol: 8,
            },
            AsciiColumnDescriptor {
                name: Some(String::from("FLUX")),
                format: AsciiColumnFormat::FloatF(10, 3),
                tbcol: 14,
            },
        ];
        let data = vec![
            AsciiColumnData::Character(vec![String::from("Vega"), String::from("Sirius")]),
            AsciiColumnData::Integer(vec![100, 200]),
            AsciiColumnData::Float(vec![3.125, -2.625]),
        ];

        let naxis1 = 24;
        let naxis2 = 2;
        let cards = build_ascii_table_cards(&cols, naxis2).unwrap();
        let serialized = serialize_ascii_table(&cols, &data, naxis1).unwrap();

        let header_bytes = serialize_header(&cards).unwrap();
        let mut fits_data = Vec::new();
        fits_data.extend_from_slice(&header_bytes);
        fits_data.extend_from_slice(&serialized);

        let hdu = Hdu {
            info: HduInfo::AsciiTable {
                naxis1,
                naxis2,
                tfields: 3,
            },
            header_start: 0,
            data_start: header_bytes.len(),
            data_len: naxis1 * naxis2,
            cards,
        };

        let col0 = read_ascii_column(&fits_data, &hdu, 0).unwrap();
        match col0 {
            AsciiColumnData::Character(vals) => {
                assert_eq!(vals[0], "Vega");
                assert_eq!(vals[1], "Sirius");
            }
            other => panic!("Expected Character, got {:?}", other),
        }

        let col1 = read_ascii_column(&fits_data, &hdu, 1).unwrap();
        match col1 {
            AsciiColumnData::Integer(vals) => {
                assert_eq!(vals, vec![100, 200]);
            }
            other => panic!("Expected Integer, got {:?}", other),
        }

        let col2 = read_ascii_column(&fits_data, &hdu, 2).unwrap();
        match col2 {
            AsciiColumnData::Float(vals) => {
                assert!((vals[0] - 3.125).abs() < 0.001);
                assert!((vals[1] - (-2.625)).abs() < 0.001);
            }
            other => panic!("Expected Float, got {:?}", other),
        }
    }

    // ---- Edge cases ----

    #[test]
    fn serialize_empty_table() {
        let cols: Vec<AsciiColumnDescriptor> = vec![];
        let data: Vec<AsciiColumnData> = vec![];
        let result = serialize_ascii_table(&cols, &data, 0).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn serialize_mismatched_columns_data_is_error() {
        let cols = vec![AsciiColumnDescriptor {
            name: None,
            format: AsciiColumnFormat::Integer(10),
            tbcol: 0,
        }];
        let data: Vec<AsciiColumnData> = vec![];
        assert!(serialize_ascii_table(&cols, &data, 10).is_err());
    }

    #[test]
    fn format_field_out_of_bounds_index() {
        let data = AsciiColumnData::Integer(vec![42]);
        let fmt = AsciiColumnFormat::Integer(10);
        assert!(format_ascii_field(&data, &fmt, 1).is_err());
    }

    #[test]
    fn format_field_type_mismatch_is_error() {
        let data = AsciiColumnData::Integer(vec![42]);
        let fmt = AsciiColumnFormat::Character(10);
        assert!(format_ascii_field(&data, &fmt, 0).is_err());
    }

    #[test]
    fn format_tform_roundtrip() {
        let cases = vec![
            AsciiColumnFormat::Character(20),
            AsciiColumnFormat::Integer(10),
            AsciiColumnFormat::FloatF(12, 4),
            AsciiColumnFormat::FloatE(15, 7),
            AsciiColumnFormat::DoubleE(25, 17),
        ];
        for fmt in &cases {
            let s = format_tform(fmt);
            let parsed = parse_tform_ascii(&s).unwrap();
            assert_eq!(&parsed, fmt);
        }
    }

    #[test]
    fn build_cards_naxis1_computed_correctly() {
        let cols = vec![
            AsciiColumnDescriptor {
                name: None,
                format: AsciiColumnFormat::Character(5),
                tbcol: 0,
            },
            AsciiColumnDescriptor {
                name: None,
                format: AsciiColumnFormat::Integer(10),
                tbcol: 5,
            },
            AsciiColumnDescriptor {
                name: None,
                format: AsciiColumnFormat::FloatE(15, 7),
                tbcol: 15,
            },
        ];
        let cards = build_ascii_table_cards(&cols, 1).unwrap();
        let naxis1 = find_card_integer(&cards, "NAXIS1").unwrap();
        assert_eq!(naxis1, 30); // 15 + 15
    }

    #[test]
    fn read_non_ascii_table_hdu_is_error() {
        let hdu = Hdu {
            info: HduInfo::Primary {
                bitpix: 8,
                naxes: vec![],
            },
            header_start: 0,
            data_start: 0,
            data_len: 0,
            cards: vec![],
        };
        assert!(read_ascii_column(&[], &hdu, 0).is_err());
    }

    // ---- serialize_ascii_table_hdu ----

    #[test]
    fn serialize_hdu_produces_valid_fits() {
        let cols = vec![
            AsciiColumnDescriptor {
                name: Some(String::from("NAME")),
                format: AsciiColumnFormat::Character(8),
                tbcol: 0,
            },
            AsciiColumnDescriptor {
                name: Some(String::from("COUNT")),
                format: AsciiColumnFormat::Integer(6),
                tbcol: 8,
            },
        ];
        let data = vec![
            AsciiColumnData::Character(vec![String::from("Vega"), String::from("Sirius")]),
            AsciiColumnData::Integer(vec![100, 200]),
        ];

        let hdu_bytes = serialize_ascii_table_hdu(&cols, &data).unwrap();
        assert_eq!(hdu_bytes.len() % crate::block::BLOCK_SIZE, 0);

        // Build a full FITS with a primary HDU and parse it
        let primary_cards = vec![
            mk_card("SIMPLE", Value::Logical(true)),
            mk_card("BITPIX", Value::Integer(8)),
            mk_card("NAXIS", Value::Integer(0)),
        ];
        let primary_header = serialize_header(&primary_cards).unwrap();

        let mut fits = Vec::new();
        fits.extend_from_slice(&primary_header);
        fits.extend_from_slice(&hdu_bytes);

        let parsed = crate::hdu::parse_fits(&fits).unwrap();
        assert_eq!(parsed.len(), 2);

        let hdu = parsed.get(1).unwrap();
        match &hdu.info {
            HduInfo::AsciiTable {
                naxis1,
                naxis2,
                tfields,
            } => {
                assert_eq!(*naxis1, 14); // 8 + 6
                assert_eq!(*naxis2, 2);
                assert_eq!(*tfields, 2);
            }
            other => panic!("Expected AsciiTable, got {:?}", other),
        }

        let col0 = read_ascii_column(&fits, hdu, 0).unwrap();
        match col0 {
            AsciiColumnData::Character(vals) => {
                assert_eq!(vals[0], "Vega");
                assert_eq!(vals[1], "Sirius");
            }
            other => panic!("Expected Character, got {:?}", other),
        }

        let col1 = read_ascii_column(&fits, hdu, 1).unwrap();
        match col1 {
            AsciiColumnData::Integer(vals) => {
                assert_eq!(vals, vec![100, 200]);
            }
            other => panic!("Expected Integer, got {:?}", other),
        }
    }
}
