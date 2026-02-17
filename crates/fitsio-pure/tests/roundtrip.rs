//! Round-trip integration tests for fitsio-pure.
//!
//! All tests use in-memory byte vectors only (no std::fs). This guarantees
//! compatibility with wasm32 targets where filesystem access is unavailable.

use fitsio_pure::bintable::{
    build_binary_table_cards, read_binary_column, serialize_binary_table,
    serialize_binary_table_hdu, BinaryColumnData, BinaryColumnDescriptor, BinaryColumnType,
};
use fitsio_pure::block::BLOCK_SIZE;
use fitsio_pure::extension::{build_extension_header, ExtensionType};
use fitsio_pure::hdu::{parse_fits, HduInfo};
use fitsio_pure::header::{serialize_header, Card};
use fitsio_pure::image::{
    build_image_hdu, extract_bscale_bzero, read_image_data, read_image_physical, read_image_region,
    read_image_rows, read_image_section, serialize_image, ImageData,
};
use fitsio_pure::primary::build_primary_header;
use fitsio_pure::table::{
    build_ascii_table_cards, read_ascii_column, serialize_ascii_table, AsciiColumnData,
    AsciiColumnDescriptor, AsciiColumnFormat,
};
use fitsio_pure::value::Value;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

/// Build a minimal primary HDU with NAXIS=0 (no data).
fn empty_primary_hdu() -> Vec<u8> {
    let cards = build_primary_header(8, &[]).unwrap();
    serialize_header(&cards).unwrap()
}

/// Build an image extension HDU (header + data) with the given EXTNAME.
fn build_image_extension(
    bitpix: i64,
    naxes: &[usize],
    data: &ImageData,
    extname: Option<&str>,
) -> Vec<u8> {
    let mut cards = build_extension_header(ExtensionType::Image, bitpix, naxes, 0, 1).unwrap();
    if let Some(name) = extname {
        cards.push(card("EXTNAME", Value::String(String::from(name))));
    }
    let header_bytes = serialize_header(&cards).unwrap();
    let data_bytes = serialize_image(data);
    let mut hdu = Vec::with_capacity(header_bytes.len() + data_bytes.len());
    hdu.extend_from_slice(&header_bytes);
    hdu.extend_from_slice(&data_bytes);
    hdu
}

// ===========================================================================
// R1.1  Image HDU round-trip for each BITPIX type
// ===========================================================================

#[test]
fn roundtrip_image_u8() {
    let pixels: Vec<u8> = (0..=255).collect();
    let data = ImageData::U8(pixels.clone());
    let bytes = build_image_hdu(8, &[256], &data).unwrap();
    let fits = parse_fits(&bytes).unwrap();
    let hdu = fits.primary();
    let read_back = read_image_data(&bytes, hdu).unwrap();
    assert_eq!(read_back, ImageData::U8(pixels));
}

#[test]
fn roundtrip_image_i16() {
    let pixels: Vec<i16> = vec![0, 1, -1, i16::MIN, i16::MAX, 256, -256, 12345];
    let data = ImageData::I16(pixels.clone());
    let bytes = build_image_hdu(16, &[8], &data).unwrap();
    let fits = parse_fits(&bytes).unwrap();
    let hdu = fits.primary();
    let read_back = read_image_data(&bytes, hdu).unwrap();
    assert_eq!(read_back, ImageData::I16(pixels));
}

#[test]
fn roundtrip_image_i32() {
    let pixels: Vec<i32> = vec![0, 1, -1, i32::MIN, i32::MAX, -42, 1000000];
    let data = ImageData::I32(pixels.clone());
    let bytes = build_image_hdu(32, &[7], &data).unwrap();
    let fits = parse_fits(&bytes).unwrap();
    let hdu = fits.primary();
    let read_back = read_image_data(&bytes, hdu).unwrap();
    assert_eq!(read_back, ImageData::I32(pixels));
}

#[test]
fn roundtrip_image_i64() {
    let pixels: Vec<i64> = vec![0, i64::MIN, i64::MAX, -1, 1];
    let data = ImageData::I64(pixels.clone());
    let bytes = build_image_hdu(64, &[5], &data).unwrap();
    let fits = parse_fits(&bytes).unwrap();
    let hdu = fits.primary();
    let read_back = read_image_data(&bytes, hdu).unwrap();
    assert_eq!(read_back, ImageData::I64(pixels));
}

#[test]
fn roundtrip_image_f32() {
    let pixels: Vec<f32> = vec![0.0, 1.5, -2.5, f32::MAX, f32::MIN_POSITIVE, 1e30];
    let data = ImageData::F32(pixels.clone());
    let bytes = build_image_hdu(-32, &[6], &data).unwrap();
    let fits = parse_fits(&bytes).unwrap();
    let hdu = fits.primary();
    let read_back = read_image_data(&bytes, hdu).unwrap();
    assert_eq!(read_back, ImageData::F32(pixels));
}

#[test]
fn roundtrip_image_f64() {
    let pixels: Vec<f64> = vec![0.0, 1.5, -2.5, f64::MAX, f64::MIN_POSITIVE, 1e200];
    let data = ImageData::F64(pixels.clone());
    let bytes = build_image_hdu(-64, &[6], &data).unwrap();
    let fits = parse_fits(&bytes).unwrap();
    let hdu = fits.primary();
    let read_back = read_image_data(&bytes, hdu).unwrap();
    assert_eq!(read_back, ImageData::F64(pixels));
}

// ===========================================================================
// R1.2  2D and 3D image round-trips
// ===========================================================================

#[test]
fn roundtrip_2d_image() {
    let width = 10;
    let height = 8;
    let pixels: Vec<i16> = (0..(width * height) as i16).collect();
    let data = ImageData::I16(pixels.clone());
    let bytes = build_image_hdu(16, &[width, height], &data).unwrap();
    let fits = parse_fits(&bytes).unwrap();
    let hdu = fits.primary();

    match &hdu.info {
        HduInfo::Primary { bitpix, naxes } => {
            assert_eq!(*bitpix, 16);
            assert_eq!(naxes, &[10, 8]);
        }
        other => panic!("Expected Primary, got {:?}", other),
    }

    let read_back = read_image_data(&bytes, hdu).unwrap();
    assert_eq!(read_back, ImageData::I16(pixels));

    // Verify row reading
    let row0 = read_image_rows(&bytes, hdu, 0, 1).unwrap();
    match row0 {
        ImageData::I16(v) => assert_eq!(v, (0..10).collect::<Vec<i16>>()),
        other => panic!("Expected I16, got {:?}", other),
    }
}

#[test]
fn roundtrip_3d_image() {
    let nx = 4;
    let ny = 3;
    let nz = 2;
    let total = nx * ny * nz;
    let pixels: Vec<f32> = (0..total).map(|i| (i as f32) + 0.25).collect();
    let data = ImageData::F32(pixels.clone());
    let bytes = build_image_hdu(-32, &[nx, ny, nz], &data).unwrap();
    let fits = parse_fits(&bytes).unwrap();
    let hdu = fits.primary();

    match &hdu.info {
        HduInfo::Primary { bitpix, naxes } => {
            assert_eq!(*bitpix, -32);
            assert_eq!(naxes, &[4, 3, 2]);
        }
        other => panic!("Expected Primary, got {:?}", other),
    }

    let read_back = read_image_data(&bytes, hdu).unwrap();
    assert_eq!(read_back, ImageData::F32(pixels));

    // Verify subregion reading
    let region = read_image_region(&bytes, hdu, &[(0, 2), (0, 2), (0, 1)]).unwrap();
    match region {
        ImageData::F32(v) => {
            assert_eq!(v.len(), 4); // 2 * 2 * 1
            assert_eq!(v[0], 0.25); // (0,0,0)
            assert_eq!(v[1], 1.25); // (1,0,0)
            assert_eq!(v[2], 4.25); // (0,1,0)
            assert_eq!(v[3], 5.25); // (1,1,0)
        }
        other => panic!("Expected F32, got {:?}", other),
    }
}

// ===========================================================================
// R1.3  Multi-extension file round-trip
// ===========================================================================

#[test]
fn roundtrip_multi_extension() {
    // 1. Empty primary
    let primary = empty_primary_hdu();

    // 2. Image extension "SCI"
    let sci_pixels: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0];
    let sci_data = ImageData::F32(sci_pixels.clone());
    let sci_ext = build_image_extension(-32, &[2, 2], &sci_data, Some("SCI"));

    // 3. Binary table extension "CATALOG"
    let bt_columns = vec![
        BinaryColumnDescriptor {
            name: Some(String::from("ID")),
            repeat: 1,
            col_type: BinaryColumnType::Int,
            byte_width: 4,
        },
        BinaryColumnDescriptor {
            name: Some(String::from("FLUX")),
            repeat: 1,
            col_type: BinaryColumnType::Double,
            byte_width: 8,
        },
    ];
    let bt_col_data = vec![
        BinaryColumnData::Int(vec![1, 2, 3]),
        BinaryColumnData::Double(vec![10.5, 20.5, 30.5]),
    ];
    let bt_ext = serialize_binary_table_hdu(&bt_columns, &bt_col_data, 3).unwrap();

    // Concatenate all HDUs
    let mut fits_bytes = Vec::new();
    fits_bytes.extend_from_slice(&primary);
    fits_bytes.extend_from_slice(&sci_ext);
    fits_bytes.extend_from_slice(&bt_ext);

    // Parse and verify
    let fits = parse_fits(&fits_bytes).unwrap();
    assert_eq!(fits.len(), 3);

    // Verify primary
    match &fits.primary().info {
        HduInfo::Primary { naxes, .. } => assert!(naxes.is_empty()),
        other => panic!("Expected Primary, got {:?}", other),
    }

    // Verify image extension
    let sci_hdu = fits.find_by_name("SCI").unwrap();
    match &sci_hdu.info {
        HduInfo::Image { bitpix, naxes } => {
            assert_eq!(*bitpix, -32);
            assert_eq!(naxes, &[2, 2]);
        }
        other => panic!("Expected Image, got {:?}", other),
    }
    let sci_read = read_image_data(&fits_bytes, sci_hdu).unwrap();
    assert_eq!(sci_read, ImageData::F32(sci_pixels));

    // Verify binary table
    let bt_hdu = fits.get(2).unwrap();
    match &bt_hdu.info {
        HduInfo::BinaryTable {
            naxis2, tfields, ..
        } => {
            assert_eq!(*naxis2, 3);
            assert_eq!(*tfields, 2);
        }
        other => panic!("Expected BinaryTable, got {:?}", other),
    }
    let id_col = read_binary_column(&fits_bytes, bt_hdu, 0).unwrap();
    assert_eq!(id_col, BinaryColumnData::Int(vec![1, 2, 3]));

    let flux_col = read_binary_column(&fits_bytes, bt_hdu, 1).unwrap();
    match flux_col {
        BinaryColumnData::Double(vals) => {
            assert_eq!(vals.len(), 3);
            assert!((vals[0] - 10.5).abs() < 1e-10);
            assert!((vals[1] - 20.5).abs() < 1e-10);
            assert!((vals[2] - 30.5).abs() < 1e-10);
        }
        other => panic!("Expected Double, got {:?}", other),
    }
}

// ===========================================================================
// R1.4  BSCALE/BZERO calibration round-trip
// ===========================================================================

#[test]
fn roundtrip_bscale_bzero() {
    // Build a primary HDU header with BSCALE/BZERO, then serialize data
    let pixels: Vec<i16> = vec![0, 1, 2, 3, 4];
    let bscale = 2.0;
    let bzero = 100.0;

    let mut cards = build_primary_header(16, &[5]).unwrap();
    cards.push(card("BSCALE", Value::Float(bscale)));
    cards.push(card("BZERO", Value::Float(bzero)));

    let header_bytes = serialize_header(&cards).unwrap();
    let data_bytes = serialize_image(&ImageData::I16(pixels));

    let mut fits_bytes = Vec::new();
    fits_bytes.extend_from_slice(&header_bytes);
    fits_bytes.extend_from_slice(&data_bytes);

    let fits = parse_fits(&fits_bytes).unwrap();
    let hdu = fits.primary();

    // Verify raw values are preserved
    let raw = read_image_data(&fits_bytes, hdu).unwrap();
    assert_eq!(raw, ImageData::I16(vec![0, 1, 2, 3, 4]));

    // Verify BSCALE/BZERO extraction
    let (bs, bz) = extract_bscale_bzero(&hdu.cards);
    assert_eq!(bs, bscale);
    assert_eq!(bz, bzero);

    // Verify physical values: physical = bzero + bscale * pixel
    let physical = read_image_physical(&fits_bytes, hdu).unwrap();
    assert_eq!(physical, vec![100.0, 102.0, 104.0, 106.0, 108.0]);
}

#[test]
fn roundtrip_unsigned_16bit_via_bzero() {
    // Common pattern: BITPIX=16 with BZERO=32768 to represent unsigned 16-bit
    let pixels: Vec<i16> = vec![0, -32768, 32767];

    let mut cards = build_primary_header(16, &[3]).unwrap();
    cards.push(card("BSCALE", Value::Float(1.0)));
    cards.push(card("BZERO", Value::Float(32768.0)));

    let header_bytes = serialize_header(&cards).unwrap();
    let data_bytes = serialize_image(&ImageData::I16(pixels));

    let mut fits_bytes = Vec::new();
    fits_bytes.extend_from_slice(&header_bytes);
    fits_bytes.extend_from_slice(&data_bytes);

    let fits = parse_fits(&fits_bytes).unwrap();
    let hdu = fits.primary();
    let physical = read_image_physical(&fits_bytes, hdu).unwrap();
    assert_eq!(physical, vec![32768.0, 0.0, 65535.0]);
}

// ===========================================================================
// R1.5  Binary table round-trip
// ===========================================================================

#[test]
fn roundtrip_binary_table_multi_type() {
    let columns = vec![
        BinaryColumnDescriptor {
            name: Some(String::from("ID")),
            repeat: 1,
            col_type: BinaryColumnType::Int,
            byte_width: 4,
        },
        BinaryColumnDescriptor {
            name: Some(String::from("NAME")),
            repeat: 12,
            col_type: BinaryColumnType::Ascii,
            byte_width: 12,
        },
        BinaryColumnDescriptor {
            name: Some(String::from("RA")),
            repeat: 1,
            col_type: BinaryColumnType::Double,
            byte_width: 8,
        },
        BinaryColumnDescriptor {
            name: Some(String::from("ACTIVE")),
            repeat: 1,
            col_type: BinaryColumnType::Logical,
            byte_width: 1,
        },
    ];
    let naxis2 = 3;
    let col_data = vec![
        BinaryColumnData::Int(vec![10, 20, 30]),
        BinaryColumnData::Ascii(vec![
            String::from("Alpha"),
            String::from("Beta"),
            String::from("Gamma"),
        ]),
        BinaryColumnData::Double(vec![180.5, 90.25, 45.125]),
        BinaryColumnData::Logical(vec![true, false, true]),
    ];

    // Serialize
    let cards = build_binary_table_cards(&columns, naxis2, 0).unwrap();
    let header_bytes = serialize_header(&cards).unwrap();
    let data_bytes = serialize_binary_table(&columns, &col_data, naxis2).unwrap();

    // Build full FITS
    let mut fits_bytes = empty_primary_hdu();
    fits_bytes.extend_from_slice(&header_bytes);
    fits_bytes.extend_from_slice(&data_bytes);

    // Parse back
    let fits = parse_fits(&fits_bytes).unwrap();
    assert_eq!(fits.len(), 2);

    let hdu = fits.get(1).unwrap();

    // Verify column data
    let id_col = read_binary_column(&fits_bytes, hdu, 0).unwrap();
    assert_eq!(id_col, BinaryColumnData::Int(vec![10, 20, 30]));

    let name_col = read_binary_column(&fits_bytes, hdu, 1).unwrap();
    match name_col {
        BinaryColumnData::Ascii(vals) => {
            assert_eq!(vals[0], "Alpha");
            assert_eq!(vals[1], "Beta");
            assert_eq!(vals[2], "Gamma");
        }
        other => panic!("Expected Ascii, got {:?}", other),
    }

    let ra_col = read_binary_column(&fits_bytes, hdu, 2).unwrap();
    match ra_col {
        BinaryColumnData::Double(vals) => {
            assert!((vals[0] - 180.5).abs() < 1e-10);
            assert!((vals[1] - 90.25).abs() < 1e-10);
            assert!((vals[2] - 45.125).abs() < 1e-10);
        }
        other => panic!("Expected Double, got {:?}", other),
    }

    let active_col = read_binary_column(&fits_bytes, hdu, 3).unwrap();
    assert_eq!(
        active_col,
        BinaryColumnData::Logical(vec![true, false, true])
    );
}

#[test]
fn roundtrip_binary_table_complex_columns() {
    let columns = vec![
        BinaryColumnDescriptor {
            name: Some(String::from("CF")),
            repeat: 1,
            col_type: BinaryColumnType::ComplexFloat,
            byte_width: 8,
        },
        BinaryColumnDescriptor {
            name: Some(String::from("CD")),
            repeat: 1,
            col_type: BinaryColumnType::ComplexDouble,
            byte_width: 16,
        },
    ];
    let naxis2 = 2;
    let col_data = vec![
        BinaryColumnData::ComplexFloat(vec![(1.0, 2.0), (-3.0, 4.0)]),
        BinaryColumnData::ComplexDouble(vec![(1.5, -2.5), (10.125, 20.25)]),
    ];

    let hdu_bytes = serialize_binary_table_hdu(&columns, &col_data, naxis2).unwrap();

    let mut fits_bytes = empty_primary_hdu();
    fits_bytes.extend_from_slice(&hdu_bytes);

    let fits = parse_fits(&fits_bytes).unwrap();
    let hdu = fits.get(1).unwrap();

    let cf = read_binary_column(&fits_bytes, hdu, 0).unwrap();
    assert_eq!(
        cf,
        BinaryColumnData::ComplexFloat(vec![(1.0, 2.0), (-3.0, 4.0)])
    );

    let cd = read_binary_column(&fits_bytes, hdu, 1).unwrap();
    assert_eq!(
        cd,
        BinaryColumnData::ComplexDouble(vec![(1.5, -2.5), (10.125, 20.25)])
    );
}

#[test]
fn roundtrip_binary_table_byte_short_long() {
    let columns = vec![
        BinaryColumnDescriptor {
            name: None,
            repeat: 4,
            col_type: BinaryColumnType::Byte,
            byte_width: 4,
        },
        BinaryColumnDescriptor {
            name: None,
            repeat: 1,
            col_type: BinaryColumnType::Short,
            byte_width: 2,
        },
        BinaryColumnDescriptor {
            name: None,
            repeat: 1,
            col_type: BinaryColumnType::Long,
            byte_width: 8,
        },
    ];
    let naxis2 = 2;
    let col_data = vec![
        BinaryColumnData::Byte(vec![10, 20, 30, 40, 50, 60, 70, 80]),
        BinaryColumnData::Short(vec![1000, -2000]),
        BinaryColumnData::Long(vec![i64::MAX, i64::MIN]),
    ];

    let hdu_bytes = serialize_binary_table_hdu(&columns, &col_data, naxis2).unwrap();
    let mut fits_bytes = empty_primary_hdu();
    fits_bytes.extend_from_slice(&hdu_bytes);

    let fits = parse_fits(&fits_bytes).unwrap();
    let hdu = fits.get(1).unwrap();

    let bytes_col = read_binary_column(&fits_bytes, hdu, 0).unwrap();
    assert_eq!(
        bytes_col,
        BinaryColumnData::Byte(vec![10, 20, 30, 40, 50, 60, 70, 80])
    );

    let short_col = read_binary_column(&fits_bytes, hdu, 1).unwrap();
    assert_eq!(short_col, BinaryColumnData::Short(vec![1000, -2000]));

    let long_col = read_binary_column(&fits_bytes, hdu, 2).unwrap();
    assert_eq!(long_col, BinaryColumnData::Long(vec![i64::MAX, i64::MIN]));
}

// ===========================================================================
// R1.6  ASCII table round-trip
// ===========================================================================

#[test]
fn roundtrip_ascii_table() {
    let columns = vec![
        AsciiColumnDescriptor {
            name: Some(String::from("NAME")),
            format: AsciiColumnFormat::Character(10),
            tbcol: 0,
        },
        AsciiColumnDescriptor {
            name: Some(String::from("COUNT")),
            format: AsciiColumnFormat::Integer(8),
            tbcol: 10,
        },
        AsciiColumnDescriptor {
            name: Some(String::from("FLUX")),
            format: AsciiColumnFormat::FloatE(15, 7),
            tbcol: 18,
        },
    ];

    let col_data = vec![
        AsciiColumnData::Character(vec![
            String::from("Vega"),
            String::from("Sirius"),
            String::from("Betelgeuse"),
        ]),
        AsciiColumnData::Integer(vec![100, 200, 300]),
        AsciiColumnData::Float(vec![1.234e5, -6.78e-3, 0.0]),
    ];

    let naxis1 = 33; // 10 + 8 + 15
    let naxis2 = 3;

    let cards = build_ascii_table_cards(&columns, naxis2).unwrap();
    let header_bytes = serialize_header(&cards).unwrap();
    let data_bytes = serialize_ascii_table(&columns, &col_data, naxis1).unwrap();

    // Build FITS with primary + ASCII table extension
    let mut fits_bytes = empty_primary_hdu();
    fits_bytes.extend_from_slice(&header_bytes);
    fits_bytes.extend_from_slice(&data_bytes);

    // Parse
    let fits = parse_fits(&fits_bytes).unwrap();
    assert_eq!(fits.len(), 2);

    let hdu = fits.get(1).unwrap();
    match &hdu.info {
        HduInfo::AsciiTable {
            naxis1: n1,
            naxis2: n2,
            tfields,
        } => {
            assert_eq!(*n1, naxis1);
            assert_eq!(*n2, naxis2);
            assert_eq!(*tfields, 3);
        }
        other => panic!("Expected AsciiTable, got {:?}", other),
    }

    // Verify character column
    let col0 = read_ascii_column(&fits_bytes, hdu, 0).unwrap();
    match col0 {
        AsciiColumnData::Character(vals) => {
            assert_eq!(vals[0], "Vega");
            assert_eq!(vals[1], "Sirius");
            assert_eq!(vals[2], "Betelgeuse");
        }
        other => panic!("Expected Character, got {:?}", other),
    }

    // Verify integer column
    let col1 = read_ascii_column(&fits_bytes, hdu, 1).unwrap();
    match col1 {
        AsciiColumnData::Integer(vals) => {
            assert_eq!(vals, vec![100, 200, 300]);
        }
        other => panic!("Expected Integer, got {:?}", other),
    }

    // Verify float column
    let col2 = read_ascii_column(&fits_bytes, hdu, 2).unwrap();
    match col2 {
        AsciiColumnData::Float(vals) => {
            assert_eq!(vals.len(), 3);
            assert!((vals[0] - 1.234e5).abs() / 1.234e5 < 1e-6);
            assert!((vals[1] - (-6.78e-3)).abs() / 6.78e-3 < 1e-6);
            assert_eq!(vals[2], 0.0);
        }
        other => panic!("Expected Float, got {:?}", other),
    }
}

#[test]
fn roundtrip_ascii_table_double_column() {
    let columns = vec![AsciiColumnDescriptor {
        name: Some(String::from("VALUE")),
        format: AsciiColumnFormat::DoubleE(25, 17),
        tbcol: 0,
    }];

    let col_data = vec![AsciiColumnData::Float(vec![1.5, -2.625])];

    let naxis1 = 25;
    let naxis2 = 2;

    let cards = build_ascii_table_cards(&columns, naxis2).unwrap();
    let header_bytes = serialize_header(&cards).unwrap();
    let data_bytes = serialize_ascii_table(&columns, &col_data, naxis1).unwrap();

    let mut fits_bytes = empty_primary_hdu();
    fits_bytes.extend_from_slice(&header_bytes);
    fits_bytes.extend_from_slice(&data_bytes);

    let fits = parse_fits(&fits_bytes).unwrap();
    let hdu = fits.get(1).unwrap();

    let col = read_ascii_column(&fits_bytes, hdu, 0).unwrap();
    match col {
        AsciiColumnData::Float(vals) => {
            assert_eq!(vals.len(), 2);
            assert!((vals[0] - 1.5).abs() < 1e-14);
            assert!((vals[1] - (-2.625)).abs() < 1e-14);
        }
        other => panic!("Expected Float, got {:?}", other),
    }
}

// ===========================================================================
// Additional structural tests
// ===========================================================================

#[test]
fn all_hdu_bytes_are_block_aligned() {
    // Verify that build_image_hdu produces block-aligned output for every BITPIX type
    let bitpix_and_sizes: &[(i64, usize)] =
        &[(8, 100), (16, 50), (32, 25), (64, 13), (-32, 25), (-64, 13)];
    for &(bitpix, size) in bitpix_and_sizes {
        let data = match bitpix {
            8 => ImageData::U8(vec![42; size]),
            16 => ImageData::I16(vec![42; size]),
            32 => ImageData::I32(vec![42; size]),
            64 => ImageData::I64(vec![42; size]),
            -32 => ImageData::F32(vec![42.0; size]),
            -64 => ImageData::F64(vec![42.0; size]),
            _ => unreachable!(),
        };
        let bytes = build_image_hdu(bitpix, &[size], &data).unwrap();
        assert_eq!(
            bytes.len() % BLOCK_SIZE,
            0,
            "HDU bytes not block-aligned for BITPIX={}",
            bitpix
        );
    }
}

#[test]
fn roundtrip_empty_primary_then_image_extension() {
    let primary = empty_primary_hdu();
    let pixels: Vec<f64> = vec![1.125, 2.25, 3.375, 4.5];
    let data = ImageData::F64(pixels.clone());
    let ext = build_image_extension(-64, &[4], &data, Some("TEST"));

    let mut fits_bytes = Vec::new();
    fits_bytes.extend_from_slice(&primary);
    fits_bytes.extend_from_slice(&ext);

    let fits = parse_fits(&fits_bytes).unwrap();
    assert_eq!(fits.len(), 2);

    let ext_hdu = fits.find_by_name("TEST").unwrap();
    let read_back = read_image_data(&fits_bytes, ext_hdu).unwrap();
    assert_eq!(read_back, ImageData::F64(pixels));
}

#[test]
fn roundtrip_image_section_and_region() {
    let width = 6;
    let height = 5;
    let pixels: Vec<i32> = (0..30).collect();
    let data = ImageData::I32(pixels.clone());
    let bytes = build_image_hdu(32, &[width, height], &data).unwrap();
    let fits = parse_fits(&bytes).unwrap();
    let hdu = fits.primary();

    // Read a flat section
    let section = read_image_section(&bytes, hdu, 6, 6).unwrap();
    match section {
        ImageData::I32(v) => assert_eq!(v, (6..12).collect::<Vec<i32>>()),
        other => panic!("Expected I32, got {:?}", other),
    }

    // Read a 2D region: columns [1,4), rows [2,4) => 3 cols, 2 rows = 6 pixels
    let region = read_image_region(&bytes, hdu, &[(1, 4), (2, 4)]).unwrap();
    match region {
        ImageData::I32(v) => {
            assert_eq!(v.len(), 6);
            // Pixel at (col=1,row=2) in FITS column-major: flat index = 1 + 2*6 = 13
            assert_eq!(v[0], 13);
            assert_eq!(v[1], 14);
            assert_eq!(v[2], 15);
            assert_eq!(v[3], 19);
            assert_eq!(v[4], 20);
            assert_eq!(v[5], 21);
        }
        other => panic!("Expected I32, got {:?}", other),
    }
}

#[test]
fn roundtrip_zero_length_image() {
    let data = ImageData::F32(Vec::new());
    let bytes = build_image_hdu(-32, &[], &data).unwrap();
    let fits = parse_fits(&bytes).unwrap();
    let hdu = fits.primary();

    match &hdu.info {
        HduInfo::Primary { naxes, .. } => assert!(naxes.is_empty()),
        other => panic!("Expected Primary, got {:?}", other),
    }

    let read_back = read_image_data(&bytes, hdu).unwrap();
    assert_eq!(read_back, ImageData::F32(Vec::new()));
}

#[test]
fn binary_table_serialized_data_is_block_padded() {
    let columns = vec![BinaryColumnDescriptor {
        name: None,
        repeat: 1,
        col_type: BinaryColumnType::Int,
        byte_width: 4,
    }];
    let col_data = vec![BinaryColumnData::Int(vec![1, 2, 3])];
    let data_bytes = serialize_binary_table(&columns, &col_data, 3).unwrap();
    assert_eq!(data_bytes.len() % BLOCK_SIZE, 0);
}

#[test]
fn ascii_table_serialized_data_is_block_padded() {
    let columns = vec![AsciiColumnDescriptor {
        name: None,
        format: AsciiColumnFormat::Integer(10),
        tbcol: 0,
    }];
    let col_data = vec![AsciiColumnData::Integer(vec![42, -7])];
    let data_bytes = serialize_ascii_table(&columns, &col_data, 10).unwrap();
    assert_eq!(data_bytes.len() % BLOCK_SIZE, 0);
}

#[test]
fn roundtrip_four_extension_file() {
    let primary = empty_primary_hdu();

    // Image extension 1
    let ext1_pixels = ImageData::U8(vec![10, 20, 30]);
    let ext1 = build_image_extension(8, &[3], &ext1_pixels, Some("EXT1"));

    // Image extension 2
    let ext2_pixels = ImageData::I32(vec![100, 200]);
    let ext2 = build_image_extension(32, &[2], &ext2_pixels, Some("EXT2"));

    // Binary table extension
    let bt_cols = vec![BinaryColumnDescriptor {
        name: Some(String::from("X")),
        repeat: 1,
        col_type: BinaryColumnType::Float,
        byte_width: 4,
    }];
    let bt_data = vec![BinaryColumnData::Float(vec![1.5, 2.5])];
    let bt_ext = serialize_binary_table_hdu(&bt_cols, &bt_data, 2).unwrap();

    // Image extension 3
    let ext3_pixels = ImageData::F64(vec![99.5]);
    let ext3 = build_image_extension(-64, &[1], &ext3_pixels, Some("EXT3"));

    let mut fits_bytes = Vec::new();
    fits_bytes.extend_from_slice(&primary);
    fits_bytes.extend_from_slice(&ext1);
    fits_bytes.extend_from_slice(&ext2);
    fits_bytes.extend_from_slice(&bt_ext);
    fits_bytes.extend_from_slice(&ext3);

    let fits = parse_fits(&fits_bytes).unwrap();
    assert_eq!(fits.len(), 5);

    // Verify each extension by name
    let e1 = fits.find_by_name("EXT1").unwrap();
    let e1_data = read_image_data(&fits_bytes, e1).unwrap();
    assert_eq!(e1_data, ImageData::U8(vec![10, 20, 30]));

    let e2 = fits.find_by_name("EXT2").unwrap();
    let e2_data = read_image_data(&fits_bytes, e2).unwrap();
    assert_eq!(e2_data, ImageData::I32(vec![100, 200]));

    let bt_hdu = fits.get(3).unwrap();
    let x_col = read_binary_column(&fits_bytes, bt_hdu, 0).unwrap();
    assert_eq!(x_col, BinaryColumnData::Float(vec![1.5, 2.5]));

    let e3 = fits.find_by_name("EXT3").unwrap();
    let e3_data = read_image_data(&fits_bytes, e3).unwrap();
    assert_eq!(e3_data, ImageData::F64(vec![99.5]));
}
