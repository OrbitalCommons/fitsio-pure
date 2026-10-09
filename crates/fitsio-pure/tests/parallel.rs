//! Decoding gives the same bytes with the `parallel` feature on and off.
//!
//! The test hashes every decode below and compares the hash with one
//! constant. CI runs it with and without the feature, so both builds must
//! produce exactly these bytes.

use fitsio_pure::compress::{compress_image_hdu, CompressPixel, TileCompression};
use fitsio_pure::hdu::parse_fits;
use fitsio_pure::header::{serialize_header, Card};
use fitsio_pure::image::{
    read_image_data, read_image_data_into_f32, read_image_data_into_f64, read_image_physical,
    ImageData,
};
use fitsio_pure::primary::build_primary_header;
use fitsio_pure::value::Value;

/// FNV-1a, 64-bit.
struct Hash(u64);

impl Hash {
    fn bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x100_0000_01b3);
        }
    }
}

const W: usize = 401;
const H: usize = 263;

/// Every decode of every HDU after the primary in `file`.
fn hash_file(hash: &mut Hash, file: &[u8]) {
    let parsed = parse_fits(file).unwrap();
    for hdu in &parsed.hdus {
        let Ok(raw) = read_image_data(file, hdu) else {
            continue;
        };
        match &raw {
            ImageData::U8(v) => hash.bytes(v),
            ImageData::I16(v) => v.iter().for_each(|x| hash.bytes(&x.to_le_bytes())),
            ImageData::I32(v) => v.iter().for_each(|x| hash.bytes(&x.to_le_bytes())),
            ImageData::I64(v) => v.iter().for_each(|x| hash.bytes(&x.to_le_bytes())),
            ImageData::F32(v) => v
                .iter()
                .for_each(|x| hash.bytes(&x.to_bits().to_le_bytes())),
            ImageData::F64(v) => v
                .iter()
                .for_each(|x| hash.bytes(&x.to_bits().to_le_bytes())),
        }
        for v in read_image_physical(file, hdu).unwrap() {
            hash.bytes(&v.to_bits().to_le_bytes());
        }
        let n = match &raw {
            ImageData::U8(v) => v.len(),
            ImageData::I16(v) => v.len(),
            ImageData::I32(v) => v.len(),
            ImageData::I64(v) => v.len(),
            ImageData::F32(v) => v.len(),
            ImageData::F64(v) => v.len(),
        };
        let mut f32s = vec![0f32; n];
        if read_image_data_into_f32(file, hdu, &mut f32s).is_ok() {
            f32s.iter()
                .for_each(|x| hash.bytes(&x.to_bits().to_le_bytes()));
        }
        let mut f64s = vec![0f64; n];
        if read_image_data_into_f64(file, hdu, &mut f64s).is_ok() {
            f64s.iter()
                .for_each(|x| hash.bytes(&x.to_bits().to_le_bytes()));
        }
    }
}

fn compressed<T: CompressPixel>(
    naxes: &[usize],
    pixels: &[T],
    opts: &TileCompression,
    extra: &[Card],
) -> Vec<u8> {
    let mut file = serialize_header(&build_primary_header(8, &[]).unwrap()).unwrap();
    file.extend(compress_image_hdu(naxes, pixels, opts, extra).unwrap());
    file
}

fn plain<T: Copy>(
    bitpix: i64,
    pixels: &[T],
    extra: Vec<Card>,
    be: impl Fn(T) -> Vec<u8>,
) -> Vec<u8> {
    let mut cards = build_primary_header(bitpix, &[W, H]).unwrap();
    cards.extend(extra);
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

fn card(keyword: &str, value: Value) -> Card {
    let mut name = [b' '; 8];
    name[..keyword.len()].copy_from_slice(keyword.as_bytes());
    Card {
        keyword: name,
        value: Some(value),
        comment: None,
    }
}

#[test]
fn decodes_are_the_same_with_and_without_the_parallel_feature() {
    let field = |i: usize| {
        let (x, y) = ((i % W) as f64, (i / W) as f64);
        500.0 + 200.0 * (x / 13.0).sin() * (y / 9.0).cos() + ((i * 7919) % 31) as f64
    };
    let n = W * H;
    let u8s: Vec<u8> = (0..n).map(|i| field(i) as u8).collect();
    let i16s: Vec<i16> = (0..n).map(|i| (field(i) * 40.0) as i16).collect();
    let u16s: Vec<u16> = (0..n).map(|i| (field(i) * 90.0) as u16).collect();
    let i32s: Vec<i32> = (0..n).map(|i| (field(i) * 1e5) as i32).collect();
    let mut f32s: Vec<f32> = (0..n).map(|i| field(i) as f32).collect();
    f32s[n / 3] = f32::NAN;
    f32s[2 * W..3 * W].fill(1.5);
    let f64s: Vec<f64> = f32s.iter().map(|&v| f64::from(v) * 1.25).collect();

    let mut hash = Hash(0xcbf2_9ce4_8422_2325);
    let codecs = [
        TileCompression::rice(),
        TileCompression::rice().tile_dims(&[64, 64]),
        TileCompression::gzip().tile_rows(7),
        TileCompression::gzip().lossless().tile_dims(&[100, 30]),
    ];
    for opts in &codecs {
        hash_file(&mut hash, &compressed(&[W, H], &u8s, opts, &[]));
        hash_file(&mut hash, &compressed(&[W, H], &i16s, opts, &[]));
        hash_file(&mut hash, &compressed(&[W, H], &u16s, opts, &[]));
        hash_file(&mut hash, &compressed(&[W, H], &i32s, opts, &[]));
        hash_file(&mut hash, &compressed(&[W, H], &f32s, opts, &[]));
        hash_file(&mut hash, &compressed(&[W, H], &f64s, opts, &[]));
    }
    // A cube, banded along its third axis.
    let cube: Vec<i16> = (0..W * 50 * 6).map(|i| (field(i) * 10.0) as i16).collect();
    hash_file(
        &mut hash,
        &compressed(
            &[W, 50, 6],
            &cube,
            &TileCompression::rice().tile_dims(&[64, 16, 1]),
            &[],
        ),
    );
    // Plain images, with BSCALE/BZERO and BLANK.
    let scaled = vec![
        card("BSCALE", Value::Float(0.5)),
        card("BZERO", Value::Float(32768.0)),
    ];
    hash_file(
        &mut hash,
        &plain(16, &i16s, scaled, |v| v.to_be_bytes().to_vec()),
    );
    hash_file(
        &mut hash,
        &plain(
            32,
            &i32s,
            vec![card("BLANK", Value::Integer(i64::from(i32s[17])))],
            |v| v.to_be_bytes().to_vec(),
        ),
    );
    hash_file(
        &mut hash,
        &plain(-32, &f32s, vec![], |v| v.to_be_bytes().to_vec()),
    );
    hash_file(
        &mut hash,
        &plain(-64, &f64s, vec![], |v| v.to_be_bytes().to_vec()),
    );
    hash_file(&mut hash, &plain(8, &u8s, vec![], |v| vec![v]));
    // HCOMPRESS and PLIO fixtures written by astropy and refimage.
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../compat-parity/tests/fixtures/hcompress");
    let mut names: Vec<_> = std::fs::read_dir(&fixtures)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "fits"))
        .collect();
    names.sort();
    assert_eq!(names.len(), 16);
    for path in names {
        hash_file(&mut hash, &std::fs::read(path).unwrap());
    }

    assert_eq!(hash.0, EXPECTED, "decoded bytes changed: {:#018x}", hash.0);
}

/// The hash of every decode above: the same with the feature on or off, and
/// the same as before the feature existed.
const EXPECTED: u64 = 0x9c91_e043_ae52_cf26;
