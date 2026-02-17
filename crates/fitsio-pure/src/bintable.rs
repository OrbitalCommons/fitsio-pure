//! FITS binary table extension reading and writing.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::block::padded_byte_len;
use crate::endian::{
    read_f32_be, read_f64_be, read_i16_be, read_i32_be, read_i64_be, write_f32_be, write_f64_be,
    write_i16_be, write_i32_be, write_i64_be,
};
use crate::error::{Error, Result};
use crate::hdu::{Hdu, HduInfo};
use crate::header::{serialize_header, Card};
use crate::value::Value;

/// The data type of a column in a FITS binary table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryColumnType {
    /// L -- logical, stored as a single byte (T/F/0).
    Logical,
    /// X -- bit array.
    Bit,
    /// B -- unsigned byte.
    Byte,
    /// I -- 16-bit signed integer.
    Short,
    /// J -- 32-bit signed integer.
    Int,
    /// K -- 64-bit signed integer.
    Long,
    /// E -- 32-bit IEEE float.
    Float,
    /// D -- 64-bit IEEE float.
    Double,
    /// C -- complex: pair of 32-bit IEEE floats.
    ComplexFloat,
    /// M -- complex: pair of 64-bit IEEE floats.
    ComplexDouble,
    /// A -- ASCII character.
    Ascii,
    /// P -- 32-bit variable-length array descriptor (8 bytes: count + heap offset).
    VarArrayP,
    /// Q -- 64-bit variable-length array descriptor (16 bytes: count + heap offset).
    VarArrayQ,
}

/// Describes one column in a binary table.
#[derive(Debug, Clone, PartialEq)]
pub struct BinaryColumnDescriptor {
    /// Column name (from TTYPEn), if present.
    pub name: Option<String>,
    /// Repeat count from TFORMn.
    pub repeat: usize,
    /// The element data type.
    pub col_type: BinaryColumnType,
    /// Total bytes this column occupies per row.
    pub byte_width: usize,
}

/// Column data extracted from a binary table.
#[derive(Debug, Clone, PartialEq)]
pub enum BinaryColumnData {
    Logical(Vec<bool>),
    Byte(Vec<u8>),
    Short(Vec<i16>),
    Int(Vec<i32>),
    Long(Vec<i64>),
    Float(Vec<f32>),
    Double(Vec<f64>),
    ComplexFloat(Vec<(f32, f32)>),
    ComplexDouble(Vec<(f64, f64)>),
    Ascii(Vec<String>),
    Bit(Vec<Vec<u8>>),
}

/// Return the number of bytes per single element for a column type.
///
/// For `Bit`, this returns 0 because bit columns use a special formula
/// based on repeat count (ceil(repeat / 8)).
pub fn binary_type_byte_size(col_type: &BinaryColumnType) -> usize {
    match col_type {
        BinaryColumnType::Logical => 1,
        BinaryColumnType::Bit => 0,
        BinaryColumnType::Byte => 1,
        BinaryColumnType::Short => 2,
        BinaryColumnType::Int => 4,
        BinaryColumnType::Long => 8,
        BinaryColumnType::Float => 4,
        BinaryColumnType::Double => 8,
        BinaryColumnType::ComplexFloat => 8,
        BinaryColumnType::ComplexDouble => 16,
        BinaryColumnType::Ascii => 1,
        BinaryColumnType::VarArrayP => 8,
        BinaryColumnType::VarArrayQ => 16,
    }
}

/// Parse a TFORMn value like "1J", "10E", "20A", "1024X", "1PB(200)", "1QJ".
///
/// Returns the repeat count and the column type.
pub fn parse_tform_binary(s: &str) -> Result<(usize, BinaryColumnType)> {
    let s = s.trim();
    if s.is_empty() {
        return Err(Error::InvalidValue);
    }

    // Strip optional (maxlen) suffix for variable-length arrays.
    let s = if let Some(paren) = s.find('(') {
        &s[..paren]
    } else {
        s
    };

    // Check for P/Q variable-length array descriptors: rPt or rQt
    // where r is repeat count, P/Q is the descriptor type, t is element type code.
    if s.len() >= 2 {
        let bytes = s.as_bytes();
        let last = bytes[s.len() - 1];
        let second_last = bytes[s.len() - 2];
        if second_last == b'P' || second_last == b'Q' {
            // Validate the element type code (last char) is a known type
            match last {
                b'L' | b'X' | b'B' | b'I' | b'J' | b'K' | b'E' | b'D' | b'C' | b'M' | b'A' => {}
                _ => return Err(Error::InvalidValue),
            }
            let repeat_str = &s[..s.len() - 2];
            let repeat = if repeat_str.is_empty() {
                1
            } else {
                repeat_str
                    .parse::<usize>()
                    .map_err(|_| Error::InvalidValue)?
            };
            let col_type = if second_last == b'P' {
                BinaryColumnType::VarArrayP
            } else {
                BinaryColumnType::VarArrayQ
            };
            return Ok((repeat, col_type));
        }
    }

    // Find the last character, which is the type code.
    let type_char = s.as_bytes()[s.len() - 1];
    let repeat_str = &s[..s.len() - 1];

    let repeat = if repeat_str.is_empty() {
        1
    } else {
        repeat_str
            .parse::<usize>()
            .map_err(|_| Error::InvalidValue)?
    };

    let col_type = match type_char {
        b'L' => BinaryColumnType::Logical,
        b'X' => BinaryColumnType::Bit,
        b'B' => BinaryColumnType::Byte,
        b'I' => BinaryColumnType::Short,
        b'J' => BinaryColumnType::Int,
        b'K' => BinaryColumnType::Long,
        b'E' => BinaryColumnType::Float,
        b'D' => BinaryColumnType::Double,
        b'C' => BinaryColumnType::ComplexFloat,
        b'M' => BinaryColumnType::ComplexDouble,
        b'A' => BinaryColumnType::Ascii,
        _ => return Err(Error::InvalidValue),
    };

    Ok((repeat, col_type))
}

/// Compute the byte width of a column given its repeat count and type.
fn compute_byte_width(repeat: usize, col_type: &BinaryColumnType) -> usize {
    match col_type {
        BinaryColumnType::Bit => repeat.div_ceil(8),
        // VarArray descriptors have a fixed size regardless of repeat count
        BinaryColumnType::VarArrayP => 8 * repeat,
        BinaryColumnType::VarArrayQ => 16 * repeat,
        _ => repeat * binary_type_byte_size(col_type),
    }
}

fn make_keyword(name: &str) -> [u8; 8] {
    let mut k = [b' '; 8];
    let bytes = name.as_bytes();
    let len = bytes.len().min(8);
    k[..len].copy_from_slice(&bytes[..len]);
    k
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

/// Extract binary table column descriptors from header cards.
pub fn parse_binary_table_columns(
    cards: &[Card],
    tfields: usize,
) -> Result<Vec<BinaryColumnDescriptor>> {
    let mut columns = Vec::with_capacity(tfields);

    for i in 1..=tfields {
        let tform_key = alloc::format!("TFORM{}", i);
        let tform_str =
            card_string_value(cards, &tform_key).ok_or(Error::MissingKeyword("TFORMn"))?;
        let (repeat, col_type) = parse_tform_binary(&tform_str)?;

        let ttype_key = alloc::format!("TTYPE{}", i);
        let name = card_string_value(cards, &ttype_key);

        let byte_width = compute_byte_width(repeat, &col_type);

        columns.push(BinaryColumnDescriptor {
            name,
            repeat,
            col_type,
            byte_width,
        });
    }

    Ok(columns)
}

/// Extract the binary table metadata from an HDU, returning (naxis1, naxis2, tfields, columns, data_start).
fn extract_table_info(
    fits_data: &[u8],
    hdu: &Hdu,
) -> Result<(usize, usize, Vec<BinaryColumnDescriptor>)> {
    let (naxis1, naxis2, tfields) = match &hdu.info {
        HduInfo::BinaryTable {
            naxis1,
            naxis2,
            tfields,
            ..
        } => (*naxis1, *naxis2, *tfields),
        _ => return Err(Error::InvalidHeader),
    };

    if hdu.data_start + naxis1 * naxis2 > fits_data.len() {
        return Err(Error::UnexpectedEof);
    }

    let columns = parse_binary_table_columns(&hdu.cards, tfields)?;
    Ok((naxis1, naxis2, columns))
}

/// Compute byte offsets for each column within a row.
fn column_offsets(columns: &[BinaryColumnDescriptor]) -> Vec<usize> {
    let mut offsets = Vec::with_capacity(columns.len());
    let mut offset = 0usize;
    for col in columns {
        offsets.push(offset);
        offset += col.byte_width;
    }
    offsets
}

/// Read a single column from all rows of a binary table HDU.
pub fn read_binary_column(
    fits_data: &[u8],
    hdu: &Hdu,
    col_index: usize,
) -> Result<BinaryColumnData> {
    let (naxis1, naxis2, columns) = extract_table_info(fits_data, hdu)?;

    if col_index >= columns.len() {
        return Err(Error::InvalidValue);
    }

    let offsets = column_offsets(&columns);
    let col = &columns[col_index];
    let col_offset = offsets[col_index];
    let data_start = hdu.data_start;

    read_column_cells(fits_data, data_start, naxis1, naxis2, col, col_offset)
}

fn read_column_cells(
    fits_data: &[u8],
    data_start: usize,
    naxis1: usize,
    naxis2: usize,
    col: &BinaryColumnDescriptor,
    col_offset: usize,
) -> Result<BinaryColumnData> {
    match col.col_type {
        BinaryColumnType::VarArrayP | BinaryColumnType::VarArrayQ => Err(Error::InvalidValue),
        BinaryColumnType::Logical => {
            let mut values = Vec::with_capacity(naxis2 * col.repeat);
            for row in 0..naxis2 {
                let base = data_start + row * naxis1 + col_offset;
                for r in 0..col.repeat {
                    let b = fits_data[base + r];
                    values.push(b == b'T');
                }
            }
            Ok(BinaryColumnData::Logical(values))
        }
        BinaryColumnType::Byte => {
            let mut values = Vec::with_capacity(naxis2 * col.repeat);
            for row in 0..naxis2 {
                let base = data_start + row * naxis1 + col_offset;
                for r in 0..col.repeat {
                    values.push(fits_data[base + r]);
                }
            }
            Ok(BinaryColumnData::Byte(values))
        }
        BinaryColumnType::Short => {
            let mut values = Vec::with_capacity(naxis2 * col.repeat);
            for row in 0..naxis2 {
                let base = data_start + row * naxis1 + col_offset;
                for r in 0..col.repeat {
                    let off = base + r * 2;
                    values.push(read_i16_be(&fits_data[off..]));
                }
            }
            Ok(BinaryColumnData::Short(values))
        }
        BinaryColumnType::Int => {
            let mut values = Vec::with_capacity(naxis2 * col.repeat);
            for row in 0..naxis2 {
                let base = data_start + row * naxis1 + col_offset;
                for r in 0..col.repeat {
                    let off = base + r * 4;
                    values.push(read_i32_be(&fits_data[off..]));
                }
            }
            Ok(BinaryColumnData::Int(values))
        }
        BinaryColumnType::Long => {
            let mut values = Vec::with_capacity(naxis2 * col.repeat);
            for row in 0..naxis2 {
                let base = data_start + row * naxis1 + col_offset;
                for r in 0..col.repeat {
                    let off = base + r * 8;
                    values.push(read_i64_be(&fits_data[off..]));
                }
            }
            Ok(BinaryColumnData::Long(values))
        }
        BinaryColumnType::Float => {
            let mut values = Vec::with_capacity(naxis2 * col.repeat);
            for row in 0..naxis2 {
                let base = data_start + row * naxis1 + col_offset;
                for r in 0..col.repeat {
                    let off = base + r * 4;
                    values.push(read_f32_be(&fits_data[off..]));
                }
            }
            Ok(BinaryColumnData::Float(values))
        }
        BinaryColumnType::Double => {
            let mut values = Vec::with_capacity(naxis2 * col.repeat);
            for row in 0..naxis2 {
                let base = data_start + row * naxis1 + col_offset;
                for r in 0..col.repeat {
                    let off = base + r * 8;
                    values.push(read_f64_be(&fits_data[off..]));
                }
            }
            Ok(BinaryColumnData::Double(values))
        }
        BinaryColumnType::ComplexFloat => {
            let mut values = Vec::with_capacity(naxis2 * col.repeat);
            for row in 0..naxis2 {
                let base = data_start + row * naxis1 + col_offset;
                for r in 0..col.repeat {
                    let off = base + r * 8;
                    let re = read_f32_be(&fits_data[off..]);
                    let im = read_f32_be(&fits_data[off + 4..]);
                    values.push((re, im));
                }
            }
            Ok(BinaryColumnData::ComplexFloat(values))
        }
        BinaryColumnType::ComplexDouble => {
            let mut values = Vec::with_capacity(naxis2 * col.repeat);
            for row in 0..naxis2 {
                let base = data_start + row * naxis1 + col_offset;
                for r in 0..col.repeat {
                    let off = base + r * 16;
                    let re = read_f64_be(&fits_data[off..]);
                    let im = read_f64_be(&fits_data[off + 8..]);
                    values.push((re, im));
                }
            }
            Ok(BinaryColumnData::ComplexDouble(values))
        }
        BinaryColumnType::Ascii => {
            let mut values = Vec::with_capacity(naxis2);
            for row in 0..naxis2 {
                let base = data_start + row * naxis1 + col_offset;
                let bytes = &fits_data[base..base + col.repeat];
                let s = core::str::from_utf8(bytes)
                    .map_err(|_| Error::InvalidValue)?
                    .trim_end()
                    .into();
                values.push(s);
            }
            Ok(BinaryColumnData::Ascii(values))
        }
        BinaryColumnType::Bit => {
            let bytes_per_row = col.repeat.div_ceil(8);
            let mut values = Vec::with_capacity(naxis2);
            for row in 0..naxis2 {
                let base = data_start + row * naxis1 + col_offset;
                let bytes = fits_data[base..base + bytes_per_row].to_vec();
                values.push(bytes);
            }
            Ok(BinaryColumnData::Bit(values))
        }
    }
}

/// Read all columns for a single row of a binary table.
pub fn read_binary_row(
    fits_data: &[u8],
    hdu: &Hdu,
    row_index: usize,
) -> Result<Vec<BinaryColumnData>> {
    let (naxis1, naxis2, columns) = extract_table_info(fits_data, hdu)?;

    if row_index >= naxis2 {
        return Err(Error::InvalidValue);
    }

    let offsets = column_offsets(&columns);
    let data_start = hdu.data_start;
    let mut result = Vec::with_capacity(columns.len());

    for (i, col) in columns.iter().enumerate() {
        let col_offset = offsets[i];
        let cell = read_column_cells(
            fits_data,
            data_start,
            naxis1,
            1,
            col,
            col_offset + row_index * naxis1,
        )?;
        result.push(cell);
    }

    Ok(result)
}

/// Serialize a single cell (one column, one row) to big-endian bytes.
pub fn serialize_binary_column_value(
    col_type: &BinaryColumnType,
    repeat: usize,
    data: &BinaryColumnData,
    row_index: usize,
) -> Result<Vec<u8>> {
    match (col_type, data) {
        (BinaryColumnType::Logical, BinaryColumnData::Logical(vals)) => {
            let start = row_index * repeat;
            let mut out = vec![0u8; repeat];
            for i in 0..repeat {
                out[i] = if vals[start + i] { b'T' } else { b'F' };
            }
            Ok(out)
        }
        (BinaryColumnType::Byte, BinaryColumnData::Byte(vals)) => {
            let start = row_index * repeat;
            Ok(vals[start..start + repeat].to_vec())
        }
        (BinaryColumnType::Short, BinaryColumnData::Short(vals)) => {
            let start = row_index * repeat;
            let mut out = vec![0u8; repeat * 2];
            for i in 0..repeat {
                write_i16_be(&mut out[i * 2..], vals[start + i]);
            }
            Ok(out)
        }
        (BinaryColumnType::Int, BinaryColumnData::Int(vals)) => {
            let start = row_index * repeat;
            let mut out = vec![0u8; repeat * 4];
            for i in 0..repeat {
                write_i32_be(&mut out[i * 4..], vals[start + i]);
            }
            Ok(out)
        }
        (BinaryColumnType::Long, BinaryColumnData::Long(vals)) => {
            let start = row_index * repeat;
            let mut out = vec![0u8; repeat * 8];
            for i in 0..repeat {
                write_i64_be(&mut out[i * 8..], vals[start + i]);
            }
            Ok(out)
        }
        (BinaryColumnType::Float, BinaryColumnData::Float(vals)) => {
            let start = row_index * repeat;
            let mut out = vec![0u8; repeat * 4];
            for i in 0..repeat {
                write_f32_be(&mut out[i * 4..], vals[start + i]);
            }
            Ok(out)
        }
        (BinaryColumnType::Double, BinaryColumnData::Double(vals)) => {
            let start = row_index * repeat;
            let mut out = vec![0u8; repeat * 8];
            for i in 0..repeat {
                write_f64_be(&mut out[i * 8..], vals[start + i]);
            }
            Ok(out)
        }
        (BinaryColumnType::ComplexFloat, BinaryColumnData::ComplexFloat(vals)) => {
            let start = row_index * repeat;
            let mut out = vec![0u8; repeat * 8];
            for i in 0..repeat {
                let (re, im) = vals[start + i];
                write_f32_be(&mut out[i * 8..], re);
                write_f32_be(&mut out[i * 8 + 4..], im);
            }
            Ok(out)
        }
        (BinaryColumnType::ComplexDouble, BinaryColumnData::ComplexDouble(vals)) => {
            let start = row_index * repeat;
            let mut out = vec![0u8; repeat * 16];
            for i in 0..repeat {
                let (re, im) = vals[start + i];
                write_f64_be(&mut out[i * 16..], re);
                write_f64_be(&mut out[i * 16 + 8..], im);
            }
            Ok(out)
        }
        (BinaryColumnType::Ascii, BinaryColumnData::Ascii(vals)) => {
            let mut out = vec![b' '; repeat];
            let s = vals[row_index].as_bytes();
            let len = s.len().min(repeat);
            out[..len].copy_from_slice(&s[..len]);
            Ok(out)
        }
        (BinaryColumnType::Bit, BinaryColumnData::Bit(vals)) => Ok(vals[row_index].clone()),
        _ => Err(Error::InvalidValue),
    }
}

fn tform_string(repeat: usize, col_type: &BinaryColumnType) -> String {
    let ch = match col_type {
        BinaryColumnType::Logical => 'L',
        BinaryColumnType::Bit => 'X',
        BinaryColumnType::Byte => 'B',
        BinaryColumnType::Short => 'I',
        BinaryColumnType::Int => 'J',
        BinaryColumnType::Long => 'K',
        BinaryColumnType::Float => 'E',
        BinaryColumnType::Double => 'D',
        BinaryColumnType::ComplexFloat => 'C',
        BinaryColumnType::ComplexDouble => 'M',
        BinaryColumnType::Ascii => 'A',
        BinaryColumnType::VarArrayP => 'P',
        BinaryColumnType::VarArrayQ => 'Q',
    };
    alloc::format!("{}{}", repeat, ch)
}

fn make_card(keyword: &str, value: Value) -> Card {
    Card {
        keyword: make_keyword(keyword),
        value: Some(value),
        comment: None,
    }
}

/// Build the full set of header cards for a binary table extension.
pub fn build_binary_table_cards(
    columns: &[BinaryColumnDescriptor],
    naxis2: usize,
    pcount: usize,
) -> Result<Vec<Card>> {
    let naxis1: usize = columns.iter().map(|c| c.byte_width).sum();
    let tfields = columns.len();

    let mut cards = vec![
        make_card("XTENSION", Value::String(String::from("BINTABLE"))),
        make_card("BITPIX", Value::Integer(8)),
        make_card("NAXIS", Value::Integer(2)),
        make_card("NAXIS1", Value::Integer(naxis1 as i64)),
        make_card("NAXIS2", Value::Integer(naxis2 as i64)),
        make_card("PCOUNT", Value::Integer(pcount as i64)),
        make_card("GCOUNT", Value::Integer(1)),
        make_card("TFIELDS", Value::Integer(tfields as i64)),
    ];

    for (i, col) in columns.iter().enumerate() {
        let n = i + 1;
        let tform = tform_string(col.repeat, &col.col_type);
        let tform_kw = alloc::format!("TFORM{}", n);
        cards.push(make_card(&tform_kw, Value::String(tform)));

        if let Some(ref name) = col.name {
            let ttype_kw = alloc::format!("TTYPE{}", n);
            cards.push(make_card(&ttype_kw, Value::String(name.clone())));
        }
    }

    Ok(cards)
}

/// Serialize all rows of a binary table into padded FITS data bytes.
///
/// The returned buffer is padded to a multiple of 2880 bytes.
pub fn serialize_binary_table(
    columns: &[BinaryColumnDescriptor],
    col_data: &[BinaryColumnData],
    naxis2: usize,
) -> Result<Vec<u8>> {
    if columns.len() != col_data.len() {
        return Err(Error::InvalidValue);
    }

    let naxis1: usize = columns.iter().map(|c| c.byte_width).sum();
    let raw_len = naxis1 * naxis2;
    let padded_len = padded_byte_len(raw_len);
    let mut buf = vec![0u8; padded_len];

    for row in 0..naxis2 {
        let mut col_offset = 0usize;
        for (col_idx, col) in columns.iter().enumerate() {
            let cell_bytes =
                serialize_binary_column_value(&col.col_type, col.repeat, &col_data[col_idx], row)?;
            let dest_start = row * naxis1 + col_offset;
            buf[dest_start..dest_start + cell_bytes.len()].copy_from_slice(&cell_bytes);
            col_offset += col.byte_width;
        }
    }

    Ok(buf)
}

/// Build and serialize a complete binary table HDU (header + data).
///
/// Returns the combined header and data bytes, each padded to block boundaries.
pub fn serialize_binary_table_hdu(
    columns: &[BinaryColumnDescriptor],
    col_data: &[BinaryColumnData],
    naxis2: usize,
) -> Result<Vec<u8>> {
    let cards = build_binary_table_cards(columns, naxis2, 0)?;
    let header_bytes = serialize_header(&cards)?;
    let data_bytes = serialize_binary_table(columns, col_data, naxis2)?;

    let mut result = Vec::with_capacity(header_bytes.len() + data_bytes.len());
    result.extend_from_slice(&header_bytes);
    result.extend_from_slice(&data_bytes);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{padded_byte_len, BLOCK_SIZE};
    use crate::header::serialize_header;
    use alloc::string::String;
    use alloc::vec;

    // --- TFORM parsing ---

    #[test]
    fn parse_tform_single_int() {
        let (repeat, col_type) = parse_tform_binary("1J").unwrap();
        assert_eq!(repeat, 1);
        assert_eq!(col_type, BinaryColumnType::Int);
    }

    #[test]
    fn parse_tform_no_repeat_prefix() {
        let (repeat, col_type) = parse_tform_binary("J").unwrap();
        assert_eq!(repeat, 1);
        assert_eq!(col_type, BinaryColumnType::Int);
    }

    #[test]
    fn parse_tform_ten_floats() {
        let (repeat, col_type) = parse_tform_binary("10E").unwrap();
        assert_eq!(repeat, 10);
        assert_eq!(col_type, BinaryColumnType::Float);
    }

    #[test]
    fn parse_tform_ascii() {
        let (repeat, col_type) = parse_tform_binary("20A").unwrap();
        assert_eq!(repeat, 20);
        assert_eq!(col_type, BinaryColumnType::Ascii);
    }

    #[test]
    fn parse_tform_logical() {
        let (repeat, col_type) = parse_tform_binary("1L").unwrap();
        assert_eq!(repeat, 1);
        assert_eq!(col_type, BinaryColumnType::Logical);
    }

    #[test]
    fn parse_tform_bit() {
        let (repeat, col_type) = parse_tform_binary("1024X").unwrap();
        assert_eq!(repeat, 1024);
        assert_eq!(col_type, BinaryColumnType::Bit);
    }

    #[test]
    fn parse_tform_double() {
        let (repeat, col_type) = parse_tform_binary("1D").unwrap();
        assert_eq!(repeat, 1);
        assert_eq!(col_type, BinaryColumnType::Double);
    }

    #[test]
    fn parse_tform_short() {
        let (repeat, col_type) = parse_tform_binary("3I").unwrap();
        assert_eq!(repeat, 3);
        assert_eq!(col_type, BinaryColumnType::Short);
    }

    #[test]
    fn parse_tform_long() {
        let (repeat, col_type) = parse_tform_binary("1K").unwrap();
        assert_eq!(repeat, 1);
        assert_eq!(col_type, BinaryColumnType::Long);
    }

    #[test]
    fn parse_tform_byte() {
        let (repeat, col_type) = parse_tform_binary("5B").unwrap();
        assert_eq!(repeat, 5);
        assert_eq!(col_type, BinaryColumnType::Byte);
    }

    #[test]
    fn parse_tform_complex_float() {
        let (repeat, col_type) = parse_tform_binary("2C").unwrap();
        assert_eq!(repeat, 2);
        assert_eq!(col_type, BinaryColumnType::ComplexFloat);
    }

    #[test]
    fn parse_tform_complex_double() {
        let (repeat, col_type) = parse_tform_binary("1M").unwrap();
        assert_eq!(repeat, 1);
        assert_eq!(col_type, BinaryColumnType::ComplexDouble);
    }

    #[test]
    fn parse_tform_invalid_type() {
        assert!(parse_tform_binary("1Z").is_err());
    }

    #[test]
    fn parse_tform_empty() {
        assert!(parse_tform_binary("").is_err());
    }

    #[test]
    fn parse_tform_whitespace_trimmed() {
        let (repeat, col_type) = parse_tform_binary("  1J  ").unwrap();
        assert_eq!(repeat, 1);
        assert_eq!(col_type, BinaryColumnType::Int);
    }

    // --- binary_type_byte_size ---

    #[test]
    fn byte_sizes() {
        assert_eq!(binary_type_byte_size(&BinaryColumnType::Logical), 1);
        assert_eq!(binary_type_byte_size(&BinaryColumnType::Byte), 1);
        assert_eq!(binary_type_byte_size(&BinaryColumnType::Short), 2);
        assert_eq!(binary_type_byte_size(&BinaryColumnType::Int), 4);
        assert_eq!(binary_type_byte_size(&BinaryColumnType::Long), 8);
        assert_eq!(binary_type_byte_size(&BinaryColumnType::Float), 4);
        assert_eq!(binary_type_byte_size(&BinaryColumnType::Double), 8);
        assert_eq!(binary_type_byte_size(&BinaryColumnType::ComplexFloat), 8);
        assert_eq!(binary_type_byte_size(&BinaryColumnType::ComplexDouble), 16);
        assert_eq!(binary_type_byte_size(&BinaryColumnType::Ascii), 1);
        assert_eq!(binary_type_byte_size(&BinaryColumnType::Bit), 0);
    }

    // --- compute_byte_width ---

    #[test]
    fn byte_width_int() {
        assert_eq!(compute_byte_width(1, &BinaryColumnType::Int), 4);
        assert_eq!(compute_byte_width(3, &BinaryColumnType::Int), 12);
    }

    #[test]
    fn byte_width_bit() {
        assert_eq!(compute_byte_width(1, &BinaryColumnType::Bit), 1);
        assert_eq!(compute_byte_width(8, &BinaryColumnType::Bit), 1);
        assert_eq!(compute_byte_width(9, &BinaryColumnType::Bit), 2);
        assert_eq!(compute_byte_width(1024, &BinaryColumnType::Bit), 128);
    }

    #[test]
    fn byte_width_ten_floats() {
        assert_eq!(compute_byte_width(10, &BinaryColumnType::Float), 40);
    }

    // --- Helper to build test FITS data ---

    fn card_val(keyword: &str, value: Value) -> Card {
        Card {
            keyword: make_keyword(keyword),
            value: Some(value),
            comment: None,
        }
    }

    fn build_bintable_hdu(header_cards: &[Card], raw_data: &[u8]) -> Vec<u8> {
        let header = serialize_header(header_cards).unwrap();
        let padded_data = padded_byte_len(raw_data.len());
        let mut result = Vec::with_capacity(header.len() + padded_data);
        result.extend_from_slice(&header);
        let data_offset = result.len();
        result.resize(data_offset + padded_data, 0u8);
        result[data_offset..data_offset + raw_data.len()].copy_from_slice(raw_data);
        result
    }

    fn make_bintable_header(
        naxis1: usize,
        naxis2: usize,
        tfields: usize,
        tforms: &[&str],
        ttype_names: &[Option<&str>],
    ) -> Vec<Card> {
        let mut cards = vec![
            card_val("XTENSION", Value::String(String::from("BINTABLE"))),
            card_val("BITPIX", Value::Integer(8)),
            card_val("NAXIS", Value::Integer(2)),
            card_val("NAXIS1", Value::Integer(naxis1 as i64)),
            card_val("NAXIS2", Value::Integer(naxis2 as i64)),
            card_val("PCOUNT", Value::Integer(0)),
            card_val("GCOUNT", Value::Integer(1)),
            card_val("TFIELDS", Value::Integer(tfields as i64)),
        ];

        for (i, tform) in tforms.iter().enumerate() {
            let kw = alloc::format!("TFORM{}", i + 1);
            cards.push(card_val(&kw, Value::String(String::from(*tform))));
        }
        for (i, name) in ttype_names.iter().enumerate() {
            if let Some(n) = name {
                let kw = alloc::format!("TTYPE{}", i + 1);
                cards.push(card_val(&kw, Value::String(String::from(*n))));
            }
        }
        cards
    }

    fn parse_test_hdu(fits_data: &[u8]) -> (Vec<u8>, Hdu) {
        use crate::hdu::parse_fits;

        let primary_cards = vec![
            card_val("SIMPLE", Value::Logical(true)),
            card_val("BITPIX", Value::Integer(8)),
            card_val("NAXIS", Value::Integer(0)),
        ];
        let primary_header = serialize_header(&primary_cards).unwrap();

        let mut full = Vec::new();
        full.extend_from_slice(&primary_header);
        full.extend_from_slice(fits_data);

        let fits = parse_fits(&full).unwrap();
        let hdu = fits.hdus.into_iter().nth(1).unwrap();
        (full, hdu)
    }

    // --- parse_binary_table_columns ---

    #[test]
    fn parse_columns_basic() {
        let cards = make_bintable_header(12, 10, 2, &["1J", "1D"], &[Some("X"), Some("Y")]);
        let columns = parse_binary_table_columns(&cards, 2).unwrap();
        assert_eq!(columns.len(), 2);

        assert_eq!(columns[0].name, Some(String::from("X")));
        assert_eq!(columns[0].repeat, 1);
        assert_eq!(columns[0].col_type, BinaryColumnType::Int);
        assert_eq!(columns[0].byte_width, 4);

        assert_eq!(columns[1].name, Some(String::from("Y")));
        assert_eq!(columns[1].repeat, 1);
        assert_eq!(columns[1].col_type, BinaryColumnType::Double);
        assert_eq!(columns[1].byte_width, 8);
    }

    #[test]
    fn parse_columns_no_names() {
        let cards = make_bintable_header(8, 5, 2, &["1J", "1E"], &[None, None]);
        let columns = parse_binary_table_columns(&cards, 2).unwrap();
        assert!(columns[0].name.is_none());
        assert!(columns[1].name.is_none());
    }

    // --- Read/write Int column ---

    #[test]
    fn read_int_column() {
        let naxis1 = 4;
        let naxis2 = 3;
        let header = make_bintable_header(naxis1, naxis2, 1, &["1J"], &[Some("VAL")]);

        let mut raw_data = vec![0u8; naxis1 * naxis2];
        write_i32_be(&mut raw_data[0..], 100);
        write_i32_be(&mut raw_data[4..], 200);
        write_i32_be(&mut raw_data[8..], -300);

        let fits_data = build_bintable_hdu(&header, &raw_data);
        let (full_fits, hdu) = parse_test_hdu(&fits_data);

        let col = read_binary_column(&full_fits, &hdu, 0).unwrap();
        match col {
            BinaryColumnData::Int(vals) => {
                assert_eq!(vals, vec![100, 200, -300]);
            }
            other => panic!("Expected Int, got {:?}", other),
        }
    }

    // --- Read/write Float column ---

    #[test]
    fn read_float_column() {
        let naxis1 = 4;
        let naxis2 = 2;
        let header = make_bintable_header(naxis1, naxis2, 1, &["1E"], &[None]);

        let mut raw_data = vec![0u8; naxis1 * naxis2];
        write_f32_be(&mut raw_data[0..], 1.5);
        write_f32_be(&mut raw_data[4..], -2.5);

        let fits_data = build_bintable_hdu(&header, &raw_data);
        let (full_fits, hdu) = parse_test_hdu(&fits_data);

        let col = read_binary_column(&full_fits, &hdu, 0).unwrap();
        match col {
            BinaryColumnData::Float(vals) => {
                assert_eq!(vals.len(), 2);
                assert!((vals[0] - 1.5).abs() < 1e-6);
                assert!((vals[1] - (-2.5)).abs() < 1e-6);
            }
            other => panic!("Expected Float, got {:?}", other),
        }
    }

    // --- Read/write Double column ---

    #[test]
    fn read_double_column() {
        let naxis1 = 8;
        let naxis2 = 2;
        let header = make_bintable_header(naxis1, naxis2, 1, &["1D"], &[None]);

        let mut raw_data = vec![0u8; naxis1 * naxis2];
        write_f64_be(&mut raw_data[0..], 3.125);
        write_f64_be(&mut raw_data[8..], -2.625);

        let fits_data = build_bintable_hdu(&header, &raw_data);
        let (full_fits, hdu) = parse_test_hdu(&fits_data);

        let col = read_binary_column(&full_fits, &hdu, 0).unwrap();
        match col {
            BinaryColumnData::Double(vals) => {
                assert_eq!(vals.len(), 2);
                assert!((vals[0] - 3.125).abs() < 1e-10);
                assert!((vals[1] - (-2.625)).abs() < 1e-10);
            }
            other => panic!("Expected Double, got {:?}", other),
        }
    }

    // --- Read/write Short column ---

    #[test]
    fn read_short_column() {
        let naxis1 = 2;
        let naxis2 = 2;
        let header = make_bintable_header(naxis1, naxis2, 1, &["1I"], &[None]);

        let mut raw_data = vec![0u8; naxis1 * naxis2];
        write_i16_be(&mut raw_data[0..], 1000);
        write_i16_be(&mut raw_data[2..], -2000);

        let fits_data = build_bintable_hdu(&header, &raw_data);
        let (full_fits, hdu) = parse_test_hdu(&fits_data);

        let col = read_binary_column(&full_fits, &hdu, 0).unwrap();
        match col {
            BinaryColumnData::Short(vals) => {
                assert_eq!(vals, vec![1000, -2000]);
            }
            other => panic!("Expected Short, got {:?}", other),
        }
    }

    // --- Read/write Long column ---

    #[test]
    fn read_long_column() {
        let naxis1 = 8;
        let naxis2 = 2;
        let header = make_bintable_header(naxis1, naxis2, 1, &["1K"], &[None]);

        let mut raw_data = vec![0u8; naxis1 * naxis2];
        write_i64_be(&mut raw_data[0..], i64::MAX);
        write_i64_be(&mut raw_data[8..], i64::MIN);

        let fits_data = build_bintable_hdu(&header, &raw_data);
        let (full_fits, hdu) = parse_test_hdu(&fits_data);

        let col = read_binary_column(&full_fits, &hdu, 0).unwrap();
        match col {
            BinaryColumnData::Long(vals) => {
                assert_eq!(vals, vec![i64::MAX, i64::MIN]);
            }
            other => panic!("Expected Long, got {:?}", other),
        }
    }

    // --- Read/write Logical column ---

    #[test]
    fn read_logical_column() {
        let naxis1 = 1;
        let naxis2 = 3;
        let header = make_bintable_header(naxis1, naxis2, 1, &["1L"], &[None]);

        let raw_data = vec![b'T', b'F', b'T'];

        let fits_data = build_bintable_hdu(&header, &raw_data);
        let (full_fits, hdu) = parse_test_hdu(&fits_data);

        let col = read_binary_column(&full_fits, &hdu, 0).unwrap();
        match col {
            BinaryColumnData::Logical(vals) => {
                assert_eq!(vals, vec![true, false, true]);
            }
            other => panic!("Expected Logical, got {:?}", other),
        }
    }

    // --- Read/write Byte column ---

    #[test]
    fn read_byte_column() {
        let naxis1 = 3;
        let naxis2 = 2;
        let header = make_bintable_header(naxis1, naxis2, 1, &["3B"], &[None]);

        let raw_data = vec![10, 20, 30, 40, 50, 60];

        let fits_data = build_bintable_hdu(&header, &raw_data);
        let (full_fits, hdu) = parse_test_hdu(&fits_data);

        let col = read_binary_column(&full_fits, &hdu, 0).unwrap();
        match col {
            BinaryColumnData::Byte(vals) => {
                assert_eq!(vals, vec![10, 20, 30, 40, 50, 60]);
            }
            other => panic!("Expected Byte, got {:?}", other),
        }
    }

    // --- Read/write Ascii column ---

    #[test]
    fn read_ascii_column() {
        let naxis1 = 8;
        let naxis2 = 2;
        let header = make_bintable_header(naxis1, naxis2, 1, &["8A"], &[Some("NAME")]);

        let mut raw_data = vec![b' '; naxis1 * naxis2];
        raw_data[..5].copy_from_slice(b"Hello");
        raw_data[8..13].copy_from_slice(b"World");

        let fits_data = build_bintable_hdu(&header, &raw_data);
        let (full_fits, hdu) = parse_test_hdu(&fits_data);

        let col = read_binary_column(&full_fits, &hdu, 0).unwrap();
        match col {
            BinaryColumnData::Ascii(vals) => {
                assert_eq!(vals[0], "Hello");
                assert_eq!(vals[1], "World");
            }
            other => panic!("Expected Ascii, got {:?}", other),
        }
    }

    // --- Read/write Bit column ---

    #[test]
    fn read_bit_column() {
        let naxis1 = 2;
        let naxis2 = 2;
        let header = make_bintable_header(naxis1, naxis2, 1, &["16X"], &[None]);

        let raw_data = vec![0xFF, 0x00, 0xAA, 0x55];

        let fits_data = build_bintable_hdu(&header, &raw_data);
        let (full_fits, hdu) = parse_test_hdu(&fits_data);

        let col = read_binary_column(&full_fits, &hdu, 0).unwrap();
        match col {
            BinaryColumnData::Bit(vals) => {
                assert_eq!(vals.len(), 2);
                assert_eq!(vals[0], vec![0xFF, 0x00]);
                assert_eq!(vals[1], vec![0xAA, 0x55]);
            }
            other => panic!("Expected Bit, got {:?}", other),
        }
    }

    // --- Read/write ComplexFloat column ---

    #[test]
    fn read_complex_float_column() {
        let naxis1 = 8;
        let naxis2 = 2;
        let header = make_bintable_header(naxis1, naxis2, 1, &["1C"], &[None]);

        let mut raw_data = vec![0u8; naxis1 * naxis2];
        write_f32_be(&mut raw_data[0..], 1.0);
        write_f32_be(&mut raw_data[4..], 2.0);
        write_f32_be(&mut raw_data[8..], -3.0);
        write_f32_be(&mut raw_data[12..], 4.0);

        let fits_data = build_bintable_hdu(&header, &raw_data);
        let (full_fits, hdu) = parse_test_hdu(&fits_data);

        let col = read_binary_column(&full_fits, &hdu, 0).unwrap();
        match col {
            BinaryColumnData::ComplexFloat(vals) => {
                assert_eq!(vals.len(), 2);
                assert!((vals[0].0 - 1.0).abs() < 1e-6);
                assert!((vals[0].1 - 2.0).abs() < 1e-6);
                assert!((vals[1].0 - (-3.0)).abs() < 1e-6);
                assert!((vals[1].1 - 4.0).abs() < 1e-6);
            }
            other => panic!("Expected ComplexFloat, got {:?}", other),
        }
    }

    // --- Read/write ComplexDouble column ---

    #[test]
    fn read_complex_double_column() {
        let naxis1 = 16;
        let naxis2 = 1;
        let header = make_bintable_header(naxis1, naxis2, 1, &["1M"], &[None]);

        let mut raw_data = vec![0u8; naxis1 * naxis2];
        write_f64_be(&mut raw_data[0..], 1.5);
        write_f64_be(&mut raw_data[8..], -2.5);

        let fits_data = build_bintable_hdu(&header, &raw_data);
        let (full_fits, hdu) = parse_test_hdu(&fits_data);

        let col = read_binary_column(&full_fits, &hdu, 0).unwrap();
        match col {
            BinaryColumnData::ComplexDouble(vals) => {
                assert_eq!(vals.len(), 1);
                assert!((vals[0].0 - 1.5).abs() < 1e-10);
                assert!((vals[0].1 - (-2.5)).abs() < 1e-10);
            }
            other => panic!("Expected ComplexDouble, got {:?}", other),
        }
    }

    // --- Multi-column table ---

    #[test]
    fn read_multi_column_table() {
        // 2 columns: 1J (4 bytes) + 1E (4 bytes) = 8 bytes per row
        let naxis1 = 8;
        let naxis2 = 2;
        let header =
            make_bintable_header(naxis1, naxis2, 2, &["1J", "1E"], &[Some("ID"), Some("VAL")]);

        let mut raw_data = vec![0u8; naxis1 * naxis2];
        // Row 0: ID=42, VAL=1.5
        write_i32_be(&mut raw_data[0..], 42);
        write_f32_be(&mut raw_data[4..], 1.5);
        // Row 1: ID=99, VAL=-3.0
        write_i32_be(&mut raw_data[8..], 99);
        write_f32_be(&mut raw_data[12..], -3.0);

        let fits_data = build_bintable_hdu(&header, &raw_data);
        let (full_fits, hdu) = parse_test_hdu(&fits_data);

        let col0 = read_binary_column(&full_fits, &hdu, 0).unwrap();
        match col0 {
            BinaryColumnData::Int(vals) => assert_eq!(vals, vec![42, 99]),
            other => panic!("Expected Int, got {:?}", other),
        }

        let col1 = read_binary_column(&full_fits, &hdu, 1).unwrap();
        match col1 {
            BinaryColumnData::Float(vals) => {
                assert!((vals[0] - 1.5).abs() < 1e-6);
                assert!((vals[1] - (-3.0)).abs() < 1e-6);
            }
            other => panic!("Expected Float, got {:?}", other),
        }
    }

    // --- Repeat count handling ---

    #[test]
    fn read_repeat_count_floats() {
        // Column with 3 floats per cell: "3E" = 12 bytes
        let naxis1 = 12;
        let naxis2 = 2;
        let header = make_bintable_header(naxis1, naxis2, 1, &["3E"], &[None]);

        let mut raw_data = vec![0u8; naxis1 * naxis2];
        // Row 0: [1.0, 2.0, 3.0]
        write_f32_be(&mut raw_data[0..], 1.0);
        write_f32_be(&mut raw_data[4..], 2.0);
        write_f32_be(&mut raw_data[8..], 3.0);
        // Row 1: [4.0, 5.0, 6.0]
        write_f32_be(&mut raw_data[12..], 4.0);
        write_f32_be(&mut raw_data[16..], 5.0);
        write_f32_be(&mut raw_data[20..], 6.0);

        let fits_data = build_bintable_hdu(&header, &raw_data);
        let (full_fits, hdu) = parse_test_hdu(&fits_data);

        let col = read_binary_column(&full_fits, &hdu, 0).unwrap();
        match col {
            BinaryColumnData::Float(vals) => {
                assert_eq!(vals.len(), 6);
                assert!((vals[0] - 1.0).abs() < 1e-6);
                assert!((vals[1] - 2.0).abs() < 1e-6);
                assert!((vals[2] - 3.0).abs() < 1e-6);
                assert!((vals[3] - 4.0).abs() < 1e-6);
                assert!((vals[4] - 5.0).abs() < 1e-6);
                assert!((vals[5] - 6.0).abs() < 1e-6);
            }
            other => panic!("Expected Float, got {:?}", other),
        }
    }

    // --- read_binary_row ---

    #[test]
    fn read_row_basic() {
        let naxis1 = 12;
        let naxis2 = 2;
        let header =
            make_bintable_header(naxis1, naxis2, 2, &["1J", "1D"], &[Some("ID"), Some("VAL")]);

        let mut raw_data = vec![0u8; naxis1 * naxis2];
        // Row 0: ID=10, VAL=1.5
        write_i32_be(&mut raw_data[0..], 10);
        write_f64_be(&mut raw_data[4..], 1.5);
        // Row 1: ID=20, VAL=-2.5
        write_i32_be(&mut raw_data[12..], 20);
        write_f64_be(&mut raw_data[16..], -2.5);

        let fits_data = build_bintable_hdu(&header, &raw_data);
        let (full_fits, hdu) = parse_test_hdu(&fits_data);

        let row0 = read_binary_row(&full_fits, &hdu, 0).unwrap();
        assert_eq!(row0.len(), 2);
        match &row0[0] {
            BinaryColumnData::Int(vals) => assert_eq!(vals, &[10]),
            other => panic!("Expected Int, got {:?}", other),
        }
        match &row0[1] {
            BinaryColumnData::Double(vals) => {
                assert!((vals[0] - 1.5).abs() < 1e-10);
            }
            other => panic!("Expected Double, got {:?}", other),
        }

        let row1 = read_binary_row(&full_fits, &hdu, 1).unwrap();
        match &row1[0] {
            BinaryColumnData::Int(vals) => assert_eq!(vals, &[20]),
            other => panic!("Expected Int, got {:?}", other),
        }
        match &row1[1] {
            BinaryColumnData::Double(vals) => {
                assert!((vals[0] - (-2.5)).abs() < 1e-10);
            }
            other => panic!("Expected Double, got {:?}", other),
        }
    }

    #[test]
    fn read_row_out_of_bounds() {
        let naxis1 = 4;
        let naxis2 = 1;
        let header = make_bintable_header(naxis1, naxis2, 1, &["1J"], &[None]);

        let mut raw_data = vec![0u8; naxis1 * naxis2];
        write_i32_be(&mut raw_data[0..], 42);

        let fits_data = build_bintable_hdu(&header, &raw_data);
        let (full_fits, hdu) = parse_test_hdu(&fits_data);

        assert!(read_binary_row(&full_fits, &hdu, 1).is_err());
    }

    // --- serialize_binary_column_value ---

    #[test]
    fn serialize_int_value() {
        let data = BinaryColumnData::Int(vec![42, -99]);
        let bytes = serialize_binary_column_value(&BinaryColumnType::Int, 1, &data, 0).unwrap();
        assert_eq!(bytes.len(), 4);
        assert_eq!(read_i32_be(&bytes), 42);

        let bytes = serialize_binary_column_value(&BinaryColumnType::Int, 1, &data, 1).unwrap();
        assert_eq!(read_i32_be(&bytes), -99);
    }

    #[test]
    fn serialize_float_value() {
        let data = BinaryColumnData::Float(vec![1.5, -2.5]);
        let bytes = serialize_binary_column_value(&BinaryColumnType::Float, 1, &data, 0).unwrap();
        assert_eq!(bytes.len(), 4);
        assert!((read_f32_be(&bytes) - 1.5).abs() < 1e-6);
    }

    #[test]
    fn serialize_double_value() {
        let data = BinaryColumnData::Double(vec![3.125]);
        let bytes = serialize_binary_column_value(&BinaryColumnType::Double, 1, &data, 0).unwrap();
        assert_eq!(bytes.len(), 8);
        assert!((read_f64_be(&bytes) - 3.125).abs() < 1e-10);
    }

    #[test]
    fn serialize_logical_value() {
        let data = BinaryColumnData::Logical(vec![true, false]);
        let bytes = serialize_binary_column_value(&BinaryColumnType::Logical, 1, &data, 0).unwrap();
        assert_eq!(bytes, vec![b'T']);
        let bytes = serialize_binary_column_value(&BinaryColumnType::Logical, 1, &data, 1).unwrap();
        assert_eq!(bytes, vec![b'F']);
    }

    #[test]
    fn serialize_ascii_value() {
        let data = BinaryColumnData::Ascii(vec![String::from("Hi")]);
        let bytes = serialize_binary_column_value(&BinaryColumnType::Ascii, 5, &data, 0).unwrap();
        assert_eq!(bytes.len(), 5);
        assert_eq!(&bytes[..2], b"Hi");
        assert_eq!(&bytes[2..], b"   ");
    }

    #[test]
    fn serialize_type_mismatch() {
        let data = BinaryColumnData::Int(vec![42]);
        let result = serialize_binary_column_value(&BinaryColumnType::Float, 1, &data, 0);
        assert!(result.is_err());
    }

    // --- build_binary_table_cards ---

    #[test]
    fn build_cards_basic() {
        let columns = vec![
            BinaryColumnDescriptor {
                name: Some(String::from("ID")),
                repeat: 1,
                col_type: BinaryColumnType::Int,
                byte_width: 4,
            },
            BinaryColumnDescriptor {
                name: Some(String::from("VAL")),
                repeat: 1,
                col_type: BinaryColumnType::Double,
                byte_width: 8,
            },
        ];

        let cards = build_binary_table_cards(&columns, 100, 0).unwrap();

        let xtension = cards
            .iter()
            .find(|c| c.keyword_str() == "XTENSION")
            .unwrap();
        assert_eq!(
            xtension.value,
            Some(Value::String(String::from("BINTABLE")))
        );

        let naxis1 = cards.iter().find(|c| c.keyword_str() == "NAXIS1").unwrap();
        assert_eq!(naxis1.value, Some(Value::Integer(12)));

        let naxis2 = cards.iter().find(|c| c.keyword_str() == "NAXIS2").unwrap();
        assert_eq!(naxis2.value, Some(Value::Integer(100)));

        let tfields = cards.iter().find(|c| c.keyword_str() == "TFIELDS").unwrap();
        assert_eq!(tfields.value, Some(Value::Integer(2)));

        let tform1 = cards.iter().find(|c| c.keyword_str() == "TFORM1").unwrap();
        assert_eq!(tform1.value, Some(Value::String(String::from("1J"))));

        let ttype1 = cards.iter().find(|c| c.keyword_str() == "TTYPE1").unwrap();
        assert_eq!(ttype1.value, Some(Value::String(String::from("ID"))));

        let tform2 = cards.iter().find(|c| c.keyword_str() == "TFORM2").unwrap();
        assert_eq!(tform2.value, Some(Value::String(String::from("1D"))));
    }

    // --- serialize_binary_table ---

    #[test]
    fn serialize_table_padded() {
        let columns = vec![BinaryColumnDescriptor {
            name: None,
            repeat: 1,
            col_type: BinaryColumnType::Int,
            byte_width: 4,
        }];
        let col_data = vec![BinaryColumnData::Int(vec![1, 2, 3])];

        let buf = serialize_binary_table(&columns, &col_data, 3).unwrap();
        assert_eq!(buf.len() % BLOCK_SIZE, 0);
        assert_eq!(buf.len(), BLOCK_SIZE);

        assert_eq!(read_i32_be(&buf[0..]), 1);
        assert_eq!(read_i32_be(&buf[4..]), 2);
        assert_eq!(read_i32_be(&buf[8..]), 3);

        // Padding should be zeros
        for &b in &buf[12..] {
            assert_eq!(b, 0);
        }
    }

    #[test]
    fn serialize_table_mismatched_columns() {
        let columns = vec![BinaryColumnDescriptor {
            name: None,
            repeat: 1,
            col_type: BinaryColumnType::Int,
            byte_width: 4,
        }];
        let col_data: Vec<BinaryColumnData> = vec![];
        assert!(serialize_binary_table(&columns, &col_data, 1).is_err());
    }

    // --- Round-trip tests ---

    #[test]
    fn roundtrip_int_column() {
        let columns = vec![BinaryColumnDescriptor {
            name: Some(String::from("X")),
            repeat: 1,
            col_type: BinaryColumnType::Int,
            byte_width: 4,
        }];
        let original = vec![BinaryColumnData::Int(vec![10, 20, 30])];
        let naxis2 = 3;

        let data_bytes = serialize_binary_table(&columns, &original, naxis2).unwrap();
        let cards = build_binary_table_cards(&columns, naxis2, 0).unwrap();
        let header_bytes = serialize_header(&cards).unwrap();

        let mut fits = Vec::new();
        // Prepend a primary HDU
        let primary_cards = vec![
            card_val("SIMPLE", Value::Logical(true)),
            card_val("BITPIX", Value::Integer(8)),
            card_val("NAXIS", Value::Integer(0)),
        ];
        let primary_header = serialize_header(&primary_cards).unwrap();
        fits.extend_from_slice(&primary_header);
        fits.extend_from_slice(&header_bytes);
        fits.extend_from_slice(&data_bytes);

        let parsed = crate::hdu::parse_fits(&fits).unwrap();
        let hdu = parsed.get(1).unwrap();

        let col = read_binary_column(&fits, hdu, 0).unwrap();
        assert_eq!(col, BinaryColumnData::Int(vec![10, 20, 30]));
    }

    #[test]
    fn roundtrip_float_column() {
        let columns = vec![BinaryColumnDescriptor {
            name: None,
            repeat: 1,
            col_type: BinaryColumnType::Float,
            byte_width: 4,
        }];
        let original = vec![BinaryColumnData::Float(vec![1.5, -2.5, 0.0])];
        let naxis2 = 3;

        let data_bytes = serialize_binary_table(&columns, &original, naxis2).unwrap();
        let cards = build_binary_table_cards(&columns, naxis2, 0).unwrap();
        let header_bytes = serialize_header(&cards).unwrap();

        let mut fits = Vec::new();
        let primary_cards = vec![
            card_val("SIMPLE", Value::Logical(true)),
            card_val("BITPIX", Value::Integer(8)),
            card_val("NAXIS", Value::Integer(0)),
        ];
        fits.extend_from_slice(&serialize_header(&primary_cards).unwrap());
        fits.extend_from_slice(&header_bytes);
        fits.extend_from_slice(&data_bytes);

        let parsed = crate::hdu::parse_fits(&fits).unwrap();
        let hdu = parsed.get(1).unwrap();
        let col = read_binary_column(&fits, hdu, 0).unwrap();
        assert_eq!(col, BinaryColumnData::Float(vec![1.5, -2.5, 0.0]));
    }

    #[test]
    fn roundtrip_double_column() {
        let columns = vec![BinaryColumnDescriptor {
            name: None,
            repeat: 1,
            col_type: BinaryColumnType::Double,
            byte_width: 8,
        }];
        let original = vec![BinaryColumnData::Double(vec![3.125, -2.625])];
        let naxis2 = 2;

        let data_bytes = serialize_binary_table(&columns, &original, naxis2).unwrap();
        let cards = build_binary_table_cards(&columns, naxis2, 0).unwrap();
        let header_bytes = serialize_header(&cards).unwrap();

        let mut fits = Vec::new();
        let primary_cards = vec![
            card_val("SIMPLE", Value::Logical(true)),
            card_val("BITPIX", Value::Integer(8)),
            card_val("NAXIS", Value::Integer(0)),
        ];
        fits.extend_from_slice(&serialize_header(&primary_cards).unwrap());
        fits.extend_from_slice(&header_bytes);
        fits.extend_from_slice(&data_bytes);

        let parsed = crate::hdu::parse_fits(&fits).unwrap();
        let hdu = parsed.get(1).unwrap();
        let col = read_binary_column(&fits, hdu, 0).unwrap();
        match col {
            BinaryColumnData::Double(vals) => {
                assert!((vals[0] - 3.125).abs() < 1e-10);
                assert!((vals[1] - (-2.625)).abs() < 1e-10);
            }
            other => panic!("Expected Double, got {:?}", other),
        }
    }

    #[test]
    fn roundtrip_multi_column() {
        let columns = vec![
            BinaryColumnDescriptor {
                name: Some(String::from("ID")),
                repeat: 1,
                col_type: BinaryColumnType::Int,
                byte_width: 4,
            },
            BinaryColumnDescriptor {
                name: Some(String::from("NAME")),
                repeat: 10,
                col_type: BinaryColumnType::Ascii,
                byte_width: 10,
            },
            BinaryColumnDescriptor {
                name: Some(String::from("VALUE")),
                repeat: 1,
                col_type: BinaryColumnType::Double,
                byte_width: 8,
            },
        ];
        let col_data = vec![
            BinaryColumnData::Int(vec![1, 2]),
            BinaryColumnData::Ascii(vec![String::from("alpha"), String::from("beta")]),
            BinaryColumnData::Double(vec![1.5, 2.5]),
        ];
        let naxis2 = 2;

        let data_bytes = serialize_binary_table(&columns, &col_data, naxis2).unwrap();
        let cards = build_binary_table_cards(&columns, naxis2, 0).unwrap();
        let header_bytes = serialize_header(&cards).unwrap();

        let mut fits = Vec::new();
        let primary_cards = vec![
            card_val("SIMPLE", Value::Logical(true)),
            card_val("BITPIX", Value::Integer(8)),
            card_val("NAXIS", Value::Integer(0)),
        ];
        fits.extend_from_slice(&serialize_header(&primary_cards).unwrap());
        fits.extend_from_slice(&header_bytes);
        fits.extend_from_slice(&data_bytes);

        let parsed = crate::hdu::parse_fits(&fits).unwrap();
        let hdu = parsed.get(1).unwrap();

        let id_col = read_binary_column(&fits, hdu, 0).unwrap();
        assert_eq!(id_col, BinaryColumnData::Int(vec![1, 2]));

        let name_col = read_binary_column(&fits, hdu, 1).unwrap();
        match name_col {
            BinaryColumnData::Ascii(vals) => {
                assert_eq!(vals[0], "alpha");
                assert_eq!(vals[1], "beta");
            }
            other => panic!("Expected Ascii, got {:?}", other),
        }

        let val_col = read_binary_column(&fits, hdu, 2).unwrap();
        match val_col {
            BinaryColumnData::Double(vals) => {
                assert!((vals[0] - 1.5).abs() < 1e-10);
                assert!((vals[1] - 2.5).abs() < 1e-10);
            }
            other => panic!("Expected Double, got {:?}", other),
        }
    }

    #[test]
    fn roundtrip_logical_column() {
        let columns = vec![BinaryColumnDescriptor {
            name: None,
            repeat: 1,
            col_type: BinaryColumnType::Logical,
            byte_width: 1,
        }];
        let original = vec![BinaryColumnData::Logical(vec![true, false, true])];
        let naxis2 = 3;

        let data_bytes = serialize_binary_table(&columns, &original, naxis2).unwrap();
        let cards = build_binary_table_cards(&columns, naxis2, 0).unwrap();
        let header_bytes = serialize_header(&cards).unwrap();

        let mut fits = Vec::new();
        let primary_cards = vec![
            card_val("SIMPLE", Value::Logical(true)),
            card_val("BITPIX", Value::Integer(8)),
            card_val("NAXIS", Value::Integer(0)),
        ];
        fits.extend_from_slice(&serialize_header(&primary_cards).unwrap());
        fits.extend_from_slice(&header_bytes);
        fits.extend_from_slice(&data_bytes);

        let parsed = crate::hdu::parse_fits(&fits).unwrap();
        let hdu = parsed.get(1).unwrap();
        let col = read_binary_column(&fits, hdu, 0).unwrap();
        assert_eq!(col, BinaryColumnData::Logical(vec![true, false, true]));
    }

    #[test]
    fn roundtrip_short_column() {
        let columns = vec![BinaryColumnDescriptor {
            name: None,
            repeat: 1,
            col_type: BinaryColumnType::Short,
            byte_width: 2,
        }];
        let original = vec![BinaryColumnData::Short(vec![100, -200])];
        let naxis2 = 2;

        let data_bytes = serialize_binary_table(&columns, &original, naxis2).unwrap();
        let cards = build_binary_table_cards(&columns, naxis2, 0).unwrap();
        let header_bytes = serialize_header(&cards).unwrap();

        let mut fits = Vec::new();
        let primary_cards = vec![
            card_val("SIMPLE", Value::Logical(true)),
            card_val("BITPIX", Value::Integer(8)),
            card_val("NAXIS", Value::Integer(0)),
        ];
        fits.extend_from_slice(&serialize_header(&primary_cards).unwrap());
        fits.extend_from_slice(&header_bytes);
        fits.extend_from_slice(&data_bytes);

        let parsed = crate::hdu::parse_fits(&fits).unwrap();
        let hdu = parsed.get(1).unwrap();
        let col = read_binary_column(&fits, hdu, 0).unwrap();
        assert_eq!(col, BinaryColumnData::Short(vec![100, -200]));
    }

    #[test]
    fn roundtrip_long_column() {
        let columns = vec![BinaryColumnDescriptor {
            name: None,
            repeat: 1,
            col_type: BinaryColumnType::Long,
            byte_width: 8,
        }];
        let original = vec![BinaryColumnData::Long(vec![i64::MAX, i64::MIN])];
        let naxis2 = 2;

        let data_bytes = serialize_binary_table(&columns, &original, naxis2).unwrap();
        let cards = build_binary_table_cards(&columns, naxis2, 0).unwrap();
        let header_bytes = serialize_header(&cards).unwrap();

        let mut fits = Vec::new();
        let primary_cards = vec![
            card_val("SIMPLE", Value::Logical(true)),
            card_val("BITPIX", Value::Integer(8)),
            card_val("NAXIS", Value::Integer(0)),
        ];
        fits.extend_from_slice(&serialize_header(&primary_cards).unwrap());
        fits.extend_from_slice(&header_bytes);
        fits.extend_from_slice(&data_bytes);

        let parsed = crate::hdu::parse_fits(&fits).unwrap();
        let hdu = parsed.get(1).unwrap();
        let col = read_binary_column(&fits, hdu, 0).unwrap();
        assert_eq!(col, BinaryColumnData::Long(vec![i64::MAX, i64::MIN]));
    }

    #[test]
    fn roundtrip_byte_column() {
        let columns = vec![BinaryColumnDescriptor {
            name: None,
            repeat: 3,
            col_type: BinaryColumnType::Byte,
            byte_width: 3,
        }];
        let original = vec![BinaryColumnData::Byte(vec![10, 20, 30, 40, 50, 60])];
        let naxis2 = 2;

        let data_bytes = serialize_binary_table(&columns, &original, naxis2).unwrap();
        let cards = build_binary_table_cards(&columns, naxis2, 0).unwrap();
        let header_bytes = serialize_header(&cards).unwrap();

        let mut fits = Vec::new();
        let primary_cards = vec![
            card_val("SIMPLE", Value::Logical(true)),
            card_val("BITPIX", Value::Integer(8)),
            card_val("NAXIS", Value::Integer(0)),
        ];
        fits.extend_from_slice(&serialize_header(&primary_cards).unwrap());
        fits.extend_from_slice(&header_bytes);
        fits.extend_from_slice(&data_bytes);

        let parsed = crate::hdu::parse_fits(&fits).unwrap();
        let hdu = parsed.get(1).unwrap();
        let col = read_binary_column(&fits, hdu, 0).unwrap();
        assert_eq!(col, BinaryColumnData::Byte(vec![10, 20, 30, 40, 50, 60]));
    }

    #[test]
    fn roundtrip_complex_float_column() {
        let columns = vec![BinaryColumnDescriptor {
            name: None,
            repeat: 1,
            col_type: BinaryColumnType::ComplexFloat,
            byte_width: 8,
        }];
        let original = vec![BinaryColumnData::ComplexFloat(vec![
            (1.0, 2.0),
            (-3.0, 4.0),
        ])];
        let naxis2 = 2;

        let data_bytes = serialize_binary_table(&columns, &original, naxis2).unwrap();
        let cards = build_binary_table_cards(&columns, naxis2, 0).unwrap();
        let header_bytes = serialize_header(&cards).unwrap();

        let mut fits = Vec::new();
        let primary_cards = vec![
            card_val("SIMPLE", Value::Logical(true)),
            card_val("BITPIX", Value::Integer(8)),
            card_val("NAXIS", Value::Integer(0)),
        ];
        fits.extend_from_slice(&serialize_header(&primary_cards).unwrap());
        fits.extend_from_slice(&header_bytes);
        fits.extend_from_slice(&data_bytes);

        let parsed = crate::hdu::parse_fits(&fits).unwrap();
        let hdu = parsed.get(1).unwrap();
        let col = read_binary_column(&fits, hdu, 0).unwrap();
        assert_eq!(
            col,
            BinaryColumnData::ComplexFloat(vec![(1.0, 2.0), (-3.0, 4.0)])
        );
    }

    #[test]
    fn roundtrip_complex_double_column() {
        let columns = vec![BinaryColumnDescriptor {
            name: None,
            repeat: 1,
            col_type: BinaryColumnType::ComplexDouble,
            byte_width: 16,
        }];
        let original = vec![BinaryColumnData::ComplexDouble(vec![(1.5, -2.5)])];
        let naxis2 = 1;

        let data_bytes = serialize_binary_table(&columns, &original, naxis2).unwrap();
        let cards = build_binary_table_cards(&columns, naxis2, 0).unwrap();
        let header_bytes = serialize_header(&cards).unwrap();

        let mut fits = Vec::new();
        let primary_cards = vec![
            card_val("SIMPLE", Value::Logical(true)),
            card_val("BITPIX", Value::Integer(8)),
            card_val("NAXIS", Value::Integer(0)),
        ];
        fits.extend_from_slice(&serialize_header(&primary_cards).unwrap());
        fits.extend_from_slice(&header_bytes);
        fits.extend_from_slice(&data_bytes);

        let parsed = crate::hdu::parse_fits(&fits).unwrap();
        let hdu = parsed.get(1).unwrap();
        let col = read_binary_column(&fits, hdu, 0).unwrap();
        assert_eq!(col, BinaryColumnData::ComplexDouble(vec![(1.5, -2.5)]));
    }

    #[test]
    fn col_index_out_of_bounds() {
        let naxis1 = 4;
        let naxis2 = 1;
        let header = make_bintable_header(naxis1, naxis2, 1, &["1J"], &[None]);

        let mut raw_data = vec![0u8; naxis1 * naxis2];
        write_i32_be(&mut raw_data[0..], 42);

        let fits_data = build_bintable_hdu(&header, &raw_data);
        let (full_fits, hdu) = parse_test_hdu(&fits_data);

        assert!(read_binary_column(&full_fits, &hdu, 1).is_err());
    }

    #[test]
    fn serialize_binary_table_hdu_produces_valid_fits() {
        let columns = vec![
            BinaryColumnDescriptor {
                name: Some(String::from("X")),
                repeat: 1,
                col_type: BinaryColumnType::Int,
                byte_width: 4,
            },
            BinaryColumnDescriptor {
                name: Some(String::from("Y")),
                repeat: 1,
                col_type: BinaryColumnType::Double,
                byte_width: 8,
            },
        ];
        let col_data = vec![
            BinaryColumnData::Int(vec![1, 2]),
            BinaryColumnData::Double(vec![1.5, 2.5]),
        ];

        let hdu_bytes = serialize_binary_table_hdu(&columns, &col_data, 2).unwrap();

        // Should be block-aligned
        assert_eq!(hdu_bytes.len() % BLOCK_SIZE, 0);

        // Build a full FITS and parse it
        let mut fits = Vec::new();
        let primary_cards = vec![
            card_val("SIMPLE", Value::Logical(true)),
            card_val("BITPIX", Value::Integer(8)),
            card_val("NAXIS", Value::Integer(0)),
        ];
        fits.extend_from_slice(&serialize_header(&primary_cards).unwrap());
        fits.extend_from_slice(&hdu_bytes);

        let parsed = crate::hdu::parse_fits(&fits).unwrap();
        assert_eq!(parsed.len(), 2);

        let hdu = parsed.get(1).unwrap();
        let x_col = read_binary_column(&fits, hdu, 0).unwrap();
        assert_eq!(x_col, BinaryColumnData::Int(vec![1, 2]));

        let y_col = read_binary_column(&fits, hdu, 1).unwrap();
        match y_col {
            BinaryColumnData::Double(vals) => {
                assert!((vals[0] - 1.5).abs() < 1e-10);
                assert!((vals[1] - 2.5).abs() < 1e-10);
            }
            other => panic!("Expected Double, got {:?}", other),
        }
    }
}
