//! `Hdu::from_cards` over a bare data unit: AstroBurst's tile-decoder unit
//! tests (`src/infra/fits/compress/tiles.rs`), which build a synthetic header
//! from `(key, value)` strings, ported to fitsio-pure.

use fitsio_pure::compress::{compress_image_hdu, TileCompression};
use fitsio_pure::hdu::{parse_fits, Hdu};
use fitsio_pure::header::{serialize_header, Card};
use fitsio_pure::image::read_image_physical_f32;
use fitsio_pure::primary::build_primary_header;
use fitsio_pure::value::Value;

const NULL_VALUE: i32 = -2_147_483_647;
const QUANT_INTS: [i32; 6] = [0, 1, 2, -3, 40, NULL_VALUE];
const QUANT_INTS_I16: [i16; 5] = [0, 1, 2, -3, 40];
const QUANT_EXPECTED: [f32; 5] = [100.0, 100.5, 101.0, 98.5, 120.0];

/// A card from a header parser's `(key, value)` strings.
fn card(key: &str, value: &str) -> Card {
    let value = if let Ok(n) = value.parse::<i64>() {
        Value::Integer(n)
    } else if let Ok(f) = value.parse::<f64>() {
        Value::Float(f)
    } else if value == "T" || value == "F" {
        Value::Logical(value == "T")
    } else {
        Value::String(value.to_string())
    };
    Card::new(key, value).unwrap()
}

fn be_i32(values: &[i32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_be_bytes()).collect()
}

fn be_i16(values: &[i16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_be_bytes()).collect()
}

fn gzip1(raw: &[u8]) -> Vec<u8> {
    fitsio_pure::gzip::compress(raw, 6)
}

/// GZIP_2: byte `j` of pixel `i` to `j * pixels + i`, then gzip.
fn gzip2(raw: &[u8], width: usize) -> Vec<u8> {
    let pixels = raw.len() / width;
    let mut shuffled = vec![0u8; raw.len()];
    for (i, pixel) in raw.chunks_exact(width).enumerate() {
        for (j, &b) in pixel.iter().enumerate() {
            shuffled[j * pixels + i] = b;
        }
    }
    gzip1(&shuffled)
}

/// The Rice tile fitsio-pure writes for a one-tile 1-D image of `values`.
fn rice_tile<T: fitsio_pure::compress::CompressPixel>(values: &[T]) -> Vec<u8> {
    let mut file = serialize_header(&build_primary_header(8, &[]).unwrap()).unwrap();
    file.extend(
        compress_image_hdu(&[values.len()], values, &TileCompression::rice(), &[]).unwrap(),
    );
    let parsed = parse_fits(&file).unwrap();
    let hdu = &parsed.hdus[1];
    let data = &file[hdu.data_start..];
    let len = u32::from_be_bytes(data[..4].try_into().unwrap()) as usize;
    let row_width = hdu
        .cards
        .iter()
        .find(|c| c.keyword_str() == "NAXIS1")
        .and_then(|c| match c.value {
            Some(Value::Integer(n)) => Some(n as usize),
            _ => None,
        })
        .unwrap();
    data[row_width..row_width + len].to_vec()
}

struct SingleTile<'a> {
    zcmptype: &'a str,
    zbitpix: i64,
    npix: usize,
    heap: Vec<u8>,
    quant: Option<(f64, f64, &'a str)>,
    extra: Vec<(&'a str, &'a str)>,
}

/// A one-row tile table over a bare data unit, and its HDU.
fn single_tile_hdu(t: &SingleTile) -> (Vec<u8>, Hdu) {
    let row_width: usize = if t.quant.is_some() { 24 } else { 8 };
    let mut data = Vec::new();
    data.extend_from_slice(&(t.heap.len() as i32).to_be_bytes());
    data.extend_from_slice(&0i32.to_be_bytes());
    if let Some((zscale, zzero, _)) = t.quant {
        data.extend_from_slice(&zscale.to_be_bytes());
        data.extend_from_slice(&zzero.to_be_bytes());
    }
    data.extend_from_slice(&t.heap);

    let npix = t.npix.to_string();
    let zbitpix = t.zbitpix.to_string();
    let naxis1 = row_width.to_string();
    let tfields = if t.quant.is_some() { "3" } else { "1" };
    let mut pairs: Vec<(&str, &str)> = vec![
        ("XTENSION", "BINTABLE"),
        ("BITPIX", "8"),
        ("NAXIS", "2"),
        ("NAXIS1", &naxis1),
        ("NAXIS2", "1"),
        ("TFIELDS", tfields),
        ("TTYPE1", "COMPRESSED_DATA"),
        ("TFORM1", "1PB(0)"),
        ("ZIMAGE", "T"),
        ("ZCMPTYPE", t.zcmptype),
        ("ZBITPIX", &zbitpix),
        ("ZNAXIS", "2"),
        ("ZNAXIS1", &npix),
        ("ZNAXIS2", "1"),
        ("ZTILE1", &npix),
        ("ZTILE2", "1"),
    ];
    if let Some((_, _, zquantiz)) = t.quant {
        pairs.extend([
            ("TTYPE2", "ZSCALE"),
            ("TFORM2", "1D"),
            ("TTYPE3", "ZZERO"),
            ("TFORM3", "1D"),
            ("ZBLANK", "-2147483647"),
        ]);
        if !zquantiz.is_empty() {
            pairs.extend([("ZQUANTIZ", zquantiz), ("ZDITHER0", "1")]);
        }
    }
    pairs.extend(t.extra.iter().copied());
    let cards = pairs.iter().map(|&(k, v)| card(k, v)).collect();
    (data, Hdu::from_cards(cards, 0, 0).unwrap())
}

fn decode(t: &SingleTile) -> fitsio_pure::Result<Vec<f32>> {
    let (data, hdu) = single_tile_hdu(t);
    read_image_physical_f32(&data, &hdu)
}

fn tile<'a>(
    zcmptype: &'a str,
    zbitpix: i64,
    npix: usize,
    heap: Vec<u8>,
    quant: Option<(f64, f64, &'a str)>,
) -> SingleTile<'a> {
    SingleTile {
        zcmptype,
        zbitpix,
        npix,
        heap,
        quant,
        extra: Vec::new(),
    }
}

fn assert_quantized_no_dither(pixels: &[f32]) {
    assert_eq!(pixels.len(), QUANT_INTS.len());
    assert_eq!(&pixels[..5], &QUANT_EXPECTED[..]);
    assert!(
        pixels[5].is_nan(),
        "ZBLANK pixel must decode to NaN, got {}",
        pixels[5]
    );
}

#[test]
fn quantized_columns_without_zquantiz_decode_as_no_dither() {
    for (zcmptype, heap) in [
        ("RICE_1", rice_tile(&QUANT_INTS)),
        ("GZIP_1", gzip1(&be_i32(&QUANT_INTS))),
        ("NOCOMPRESS", be_i32(&QUANT_INTS)),
    ] {
        let pixels = decode(&tile(
            zcmptype,
            -32,
            QUANT_INTS.len(),
            heap,
            Some((0.5, 100.0, "")),
        ))
        .unwrap();
        assert_quantized_no_dither(&pixels);
    }
}

#[test]
fn zscale_and_zzero_header_keywords_scale_every_tile() {
    let mut t = tile(
        "RICE_1",
        -32,
        QUANT_INTS.len(),
        rice_tile(&QUANT_INTS),
        None,
    );
    t.extra = vec![
        ("ZSCALE", "0.5"),
        ("ZZERO", "100.0"),
        ("ZBLANK", "-2147483647"),
    ];
    assert_quantized_no_dither(&decode(&t).unwrap());
}

#[test]
fn a_rice_float_image_without_any_scale_is_refused_instead_of_returning_codes() {
    let err = decode(&tile(
        "RICE_1",
        -32,
        QUANT_INTS.len(),
        rice_tile(&QUANT_INTS),
        None,
    ))
    .expect_err("integer codes must not be passed off as float pixels");
    assert!(err.to_string().contains("ZSCALE/ZZERO"), "{err}");
}

#[test]
fn zquantiz_none_keeps_a_lossless_float_tile_unscaled() {
    let values = [1.5f32, -2.25, 1e10, 0.0, 3.0];
    let raw: Vec<u8> = values.iter().flat_map(|v| v.to_be_bytes()).collect();
    let pixels = decode(&tile(
        "GZIP_2",
        -32,
        values.len(),
        gzip2(&raw, 4),
        Some((0.5, 100.0, "NONE")),
    ))
    .unwrap();
    assert_eq!(pixels, values.to_vec());
}

#[test]
fn gzip2_lossless_float64_tile_decodes() {
    let values = [1.5f64, -2.25, 1e10, 0.0, 3.0];
    let raw: Vec<u8> = values.iter().flat_map(|v| v.to_be_bytes()).collect();
    let pixels = decode(&tile("GZIP_2", -64, values.len(), gzip2(&raw, 8), None)).unwrap();
    let expected: Vec<f32> = values.iter().map(|&v| v as f32).collect();
    assert_eq!(pixels, expected);
}

#[test]
fn gzip1_lossless_int64_tile_decodes() {
    let values = [7i64, -8, 1 << 40, 0];
    let raw: Vec<u8> = values.iter().flat_map(|v| v.to_be_bytes()).collect();
    let pixels = decode(&tile("GZIP_1", 64, values.len(), gzip1(&raw), None)).unwrap();
    let expected: Vec<f32> = values.iter().map(|&v| v as f32).collect();
    assert_eq!(pixels, expected);
}

#[test]
fn gzip1_quantized_tile_payload_is_int32() {
    let t = tile(
        "GZIP_1",
        -32,
        QUANT_INTS.len(),
        gzip1(&be_i32(&QUANT_INTS)),
        Some((0.5, 100.0, "NO_DITHER")),
    );
    assert_quantized_no_dither(&decode(&t).unwrap());
}

#[test]
fn nocompress_quantized_tile_payload_is_int32() {
    let t = tile(
        "NOCOMPRESS",
        -32,
        QUANT_INTS.len(),
        be_i32(&QUANT_INTS),
        Some((0.5, 100.0, "NO_DITHER")),
    );
    assert_quantized_no_dither(&decode(&t).unwrap());
}

#[test]
fn gzip1_quantized_tile_int16_payload() {
    let t = tile(
        "GZIP_1",
        -32,
        QUANT_INTS_I16.len(),
        gzip1(&be_i16(&QUANT_INTS_I16)),
        Some((0.5, 100.0, "NO_DITHER")),
    );
    assert_eq!(decode(&t).unwrap(), QUANT_EXPECTED.to_vec());
}

#[test]
fn gzip2_quantized_tile_int16_payload() {
    let t = tile(
        "GZIP_2",
        -32,
        QUANT_INTS_I16.len(),
        gzip2(&be_i16(&QUANT_INTS_I16), 2),
        Some((0.5, 100.0, "NO_DITHER")),
    );
    assert_eq!(decode(&t).unwrap(), QUANT_EXPECTED.to_vec());
}

#[test]
fn quantized_tile_with_bad_byte_count_is_an_error() {
    let err = decode(&tile(
        "NOCOMPRESS",
        -32,
        6,
        vec![1u8; 7],
        Some((0.5, 100.0, "NO_DITHER")),
    ))
    .expect_err("7 bytes for 6 pixels must be reported as an error");
    assert!(err.to_string().contains("byte count"), "{err}");
}

/// cfitsio's dither table and the dequantization of tile 0 with
/// `ZDITHER0 = 1`, computed independently of the reader.
fn dequantize_dithered(ints: &[i32], scale: f64, zero: f64) -> Vec<f64> {
    let (a, m) = (16807.0f64, 2_147_483_647.0f64);
    let mut seed = 1.0f64;
    let table: Vec<f32> = (0..10_000)
        .map(|_| {
            let temp = a * seed;
            seed = temp - m * (temp / m).trunc();
            (seed / m) as f32
        })
        .collect();
    let mut next = (f64::from(table[0]) * 500.0) as usize;
    ints.iter()
        .map(|&iv| {
            let v = if iv == NULL_VALUE {
                f64::NAN
            } else {
                (f64::from(iv) - f64::from(table[next]) + 0.5) * scale + zero
            };
            next += 1;
            v
        })
        .collect()
}

#[test]
fn gzip2_quantized_tile_applies_subtractive_dither() {
    let t = tile(
        "GZIP_2",
        -64,
        QUANT_INTS.len(),
        gzip2(&be_i32(&QUANT_INTS), 4),
        Some((0.5, 100.0, "SUBTRACTIVE_DITHER_1")),
    );
    let pixels = decode(&t).unwrap();
    let expected = dequantize_dithered(&QUANT_INTS, 0.5, 100.0);
    for (i, (&got, &want)) in pixels.iter().zip(&expected).enumerate() {
        if want.is_nan() {
            assert!(got.is_nan(), "pixel {i}: expected NaN, got {got}");
        } else {
            assert_eq!(got, want as f32, "pixel {i}");
            assert!(
                (got - QUANT_EXPECTED[i]).abs() <= 0.25 + 1e-6,
                "pixel {i}: {got}"
            );
        }
    }
}

#[test]
fn rice_quantized_float64_tile_defaults_to_bytepix_4() {
    let t = tile(
        "RICE_1",
        -64,
        QUANT_INTS.len(),
        rice_tile(&QUANT_INTS),
        Some((0.5, 100.0, "NO_DITHER")),
    );
    assert_quantized_no_dither(&decode(&t).unwrap());
}

#[test]
fn truncated_rice_tile_is_an_error_not_a_panic() {
    let values: Vec<i16> = (0..64).map(|i| ((i * 37 % 101) - 50) as i16).collect();
    let encoded = rice_tile(&values);
    // cfitsio defaults BYTEPIX to 4, so a 16-bit tile names its width.
    let mut t = tile(
        "RICE_1",
        16,
        values.len(),
        encoded[..encoded.len() / 2].to_vec(),
        None,
    );
    t.extra = vec![
        ("ZNAME1", "BLOCKSIZE"),
        ("ZVAL1", "32"),
        ("ZNAME2", "BYTEPIX"),
        ("ZVAL2", "2"),
    ];
    let err = decode(&t).expect_err("half a Rice stream must be reported as an error");
    assert!(err.to_string().contains("truncated"), "{err}");
    t.heap = encoded;
    let whole: Vec<f32> = values.iter().map(|&v| f32::from(v)).collect();
    assert_eq!(decode(&t).unwrap(), whole);
}

#[test]
fn from_cards_matches_parse_fits() {
    let pixels: Vec<i16> = (0..400).map(|i| (i * 13 % 300) as i16).collect();
    let mut file = serialize_header(&build_primary_header(8, &[]).unwrap()).unwrap();
    file.extend(compress_image_hdu(&[20, 20], &pixels, &TileCompression::rice(), &[]).unwrap());
    let parsed = parse_fits(&file).unwrap();
    let want = &parsed.hdus[1];
    let got = Hdu::from_cards(want.cards.clone(), want.header_start, want.data_start).unwrap();
    assert_eq!(format!("{:?}", got.info), format!("{:?}", want.info));
    assert_eq!(got.data_len, want.data_len);
    let primary = Hdu::from_cards(parsed.hdus[0].cards.clone(), 0, 2880).unwrap();
    assert_eq!(
        format!("{:?}", primary.info),
        format!("{:?}", parsed.hdus[0].info)
    );
    assert!(Card::new("lower", Value::Integer(1)).is_err());
    assert!(Card::new("TOOLONGKEY", Value::Integer(1)).is_err());
}
