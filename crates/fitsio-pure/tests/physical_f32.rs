//! `read_image_physical_f32` equals `read_image_physical` cast to `f32`, bit
//! for bit, for plain and tile-compressed images.

use fitsio_pure::compress::{compress_image_hdu, CompressPixel, TileCompression};
use fitsio_pure::hdu::{parse_fits, Hdu};
use fitsio_pure::header::{serialize_header, Card};
use fitsio_pure::image::{
    read_image_physical, read_image_physical_f32, read_image_physical_into_f32,
};
use fitsio_pure::primary::build_primary_header;
use fitsio_pure::value::Value;

const W: usize = 131;
const H: usize = 77;

fn card(keyword: &str, value: Value) -> Card {
    let mut name = [b' '; 8];
    name[..keyword.len()].copy_from_slice(keyword.as_bytes());
    Card {
        keyword: name,
        value: Some(value),
        comment: None,
    }
}

/// Every image HDU of `file` reads the same through both paths.
fn assert_matches(what: &str, file: &[u8]) {
    let parsed = parse_fits(file).unwrap();
    let images: Vec<&Hdu> = parsed
        .hdus
        .iter()
        .filter(|h| read_image_physical(file, h).is_ok_and(|v| !v.is_empty()))
        .collect();
    assert!(!images.is_empty(), "{what}: no image");
    for hdu in images {
        let wide: Vec<u32> = read_image_physical(file, hdu)
            .unwrap()
            .into_iter()
            .map(|v| (v as f32).to_bits())
            .collect();
        let narrow: Vec<u32> = read_image_physical_f32(file, hdu)
            .unwrap()
            .into_iter()
            .map(f32::to_bits)
            .collect();
        assert_eq!(wide, narrow, "{what}");
        let mut short = vec![0f32; wide.len() - 1];
        assert!(
            read_image_physical_into_f32(file, hdu, &mut short).is_err(),
            "{what}"
        );
    }
}

fn plain<T: Copy>(bitpix: i64, pixels: &[T], extra: &[Card], be: impl Fn(T) -> Vec<u8>) -> Vec<u8> {
    let mut cards = build_primary_header(bitpix, &[W, H]).unwrap();
    cards.extend_from_slice(extra);
    let mut file = serialize_header(&cards).unwrap();
    let start = file.len();
    for &p in pixels {
        file.extend(be(p));
    }
    file.resize(
        start + fitsio_pure::block::padded_byte_len(file.len() - start),
        0,
    );
    file
}

fn compressed<T: CompressPixel>(pixels: &[T], opts: &TileCompression) -> Vec<u8> {
    let mut file = serialize_header(&build_primary_header(8, &[]).unwrap()).unwrap();
    file.extend(compress_image_hdu(&[W, H], pixels, opts, &[]).unwrap());
    file
}

#[test]
fn f32_physical_reads_equal_the_f64_ones_cast() {
    let field =
        |i: usize| 300.0 + 120.0 * ((i % W) as f64 / 11.0).sin() + ((i * 7919) % 17) as f64 / 3.0;
    let n = W * H;
    let u8s: Vec<u8> = (0..n).map(|i| field(i) as u8).collect();
    let i16s: Vec<i16> = (0..n)
        .map(|i| (field(i) * 70.0 - 20_000.0) as i16)
        .collect();
    let u16s: Vec<u16> = (0..n).map(|i| (field(i) * 90.0) as u16).collect();
    let i32s: Vec<i32> = (0..n).map(|i| (field(i) * 1e6) as i32).collect();
    let i64s: Vec<i64> = (0..n).map(|i| (field(i) * 1e12) as i64).collect();
    let mut f32s: Vec<f32> = (0..n).map(|i| field(i) as f32 * 0.1).collect();
    f32s[100] = f32::NAN;
    let f64s: Vec<f64> = f32s.iter().map(|&v| f64::from(v) / 3.0).collect();

    let scaling = [
        card("BSCALE", Value::Float(0.37)),
        card("BZERO", Value::Float(1234.5)),
    ];
    let blank16 = [
        card("BLANK", Value::Integer(i64::from(i16s[5]))),
        card("BSCALE", Value::Float(2.0)),
    ];
    let blank32 = [card("BLANK", Value::Integer(i64::from(i32s[9])))];
    let be = |b: &[u8]| b.to_vec();
    assert_matches("u8", &plain(8, &u8s, &scaling, |v| vec![v]));
    assert_matches(
        "i16 scaled",
        &plain(16, &i16s, &scaling, |v| be(&v.to_be_bytes())),
    );
    assert_matches(
        "i16 BLANK",
        &plain(16, &i16s, &blank16, |v| be(&v.to_be_bytes())),
    );
    assert_matches(
        "i32 BLANK",
        &plain(32, &i32s, &blank32, |v| be(&v.to_be_bytes())),
    );
    assert_matches("i64", &plain(64, &i64s, &scaling, |v| be(&v.to_be_bytes())));
    assert_matches("f32", &plain(-32, &f32s, &[], |v| be(&v.to_be_bytes())));
    assert_matches(
        "f32 scaled",
        &plain(-32, &f32s, &scaling, |v| be(&v.to_be_bytes())),
    );
    assert_matches(
        "f64",
        &plain(-64, &f64s, &scaling, |v| be(&v.to_be_bytes())),
    );

    for opts in [
        TileCompression::rice().tile_dims(&[32, 16]),
        TileCompression::gzip(),
        TileCompression::gzip().lossless(),
    ] {
        assert_matches("compressed u8", &compressed(&u8s, &opts));
        assert_matches("compressed i16", &compressed(&i16s, &opts));
        assert_matches("compressed u16", &compressed(&u16s, &opts));
        assert_matches("compressed i32", &compressed(&i32s, &opts));
        assert_matches("compressed f32", &compressed(&f32s, &opts));
        assert_matches("compressed f64", &compressed(&f64s, &opts));
    }

    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../compat-parity/tests/fixtures/hcompress");
    for entry in std::fs::read_dir(fixtures).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "fits") {
            assert_matches(&path.display().to_string(), &std::fs::read(&path).unwrap());
        }
    }
}
