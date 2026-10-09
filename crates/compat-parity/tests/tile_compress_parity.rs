//! Tile-compressed image writing, checked against cfitsio.
//!
//! The same pixels are compressed by cfitsio (through `fitsio`'s
//! `"file.fits[compress …]"` extended filename) and by
//! `fitsio_pure::compress`. Each tile must compress to the same bytes: Rice
//! output byte for byte, gzip output after inflating (zlib and miniz_oxide
//! deflate differently), and quantized floats with the same `ZSCALE` and
//! `ZZERO`. A Rice-compressed integer image's whole data unit, table and
//! heap, is identical. cfitsio must read our files back to the original
//! values.

use std::path::Path;

use fitsio::images::{ImageDescription, ImageType};
use fitsio::FitsFile as CFits;
use fitsio_pure::compat::fitsfile::FitsFile as PFits;

use fitsio_pure::compress::{
    compress_image_hdu, CompressPixel, DitherSeed, Quantize, TileCompression,
};
use fitsio_pure::hdu::{parse_fits, Hdu};
use fitsio_pure::header::{serialize_header, Card};
use fitsio_pure::primary::build_primary_header;
use fitsio_pure::value::Value;

/// cfitsio fills its dither table on first use without a lock, so tests
/// running in parallel serialize their cfitsio writes.
static CFITSIO: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn cfitsio_lock() -> std::sync::MutexGuard<'static, ()> {
    CFITSIO.lock().unwrap_or_else(|e| e.into_inner())
}

const W: usize = 57;
const H: usize = 23;
const SEED: i32 = 77;

/// One tile's row of the compressed table.
#[derive(Debug, PartialEq)]
struct TileRow {
    compressed: Vec<u8>,
    scale: Option<(f64, f64)>,
    fallback: Vec<u8>,
}

/// The rows of the compressed image in HDU 1 of `path`.
fn tile_rows(path: &Path) -> Vec<TileRow> {
    let bytes = std::fs::read(path).unwrap();
    let parsed = parse_fits(&bytes).unwrap();
    let hdu: &Hdu = &parsed.hdus[1];
    let int = |key: &str| {
        hdu.cards
            .iter()
            .find(|c| c.keyword_str() == key)
            .and_then(|c| match c.value {
                Some(Value::Integer(n)) => Some(n as usize),
                _ => None,
            })
            .unwrap()
    };
    let (naxis1, naxis2, tfields) = (int("NAXIS1"), int("NAXIS2"), int("TFIELDS"));
    let mut columns = Vec::new();
    let mut offset = 0;
    for n in 1..=tfields {
        let text = |key: String| {
            hdu.cards
                .iter()
                .find(|c| c.keyword_str() == key)
                .and_then(|c| match &c.value {
                    Some(Value::String(s)) => Some(s.trim().to_string()),
                    _ => None,
                })
                .unwrap()
        };
        let name = text(format!("TTYPE{n}"));
        let form = text(format!("TFORM{n}"));
        let width = if form.starts_with("1Q") { 16 } else { 8 };
        columns.push((name, offset, form));
        offset += width;
    }
    assert_eq!(offset, naxis1);
    let table = &bytes[hdu.data_start..];
    let heap = &table[naxis1 * naxis2..];
    let column = |name: &str| columns.iter().find(|(n, _, _)| n == name);
    (0..naxis2)
        .map(|row| {
            let at = |offset: usize| row * naxis1 + offset;
            let vla = |name: &str| match column(name) {
                Some((_, offset, _)) => {
                    let d = &table[at(*offset)..];
                    let len = u32::from_be_bytes(d[..4].try_into().unwrap()) as usize;
                    let start = u32::from_be_bytes(d[4..8].try_into().unwrap()) as usize;
                    heap[start..start + len].to_vec()
                }
                None => Vec::new(),
            };
            let double = |name: &str| {
                column(name).map(|(_, offset, _)| {
                    f64::from_be_bytes(table[at(*offset)..at(*offset) + 8].try_into().unwrap())
                })
            };
            TileRow {
                compressed: vla("COMPRESSED_DATA"),
                scale: double("ZSCALE").zip(double("ZZERO")),
                fallback: vla("GZIP_COMPRESSED_DATA"),
            }
        })
        .collect()
}

fn inflate(bytes: &[u8]) -> Vec<u8> {
    if bytes.is_empty() {
        Vec::new()
    } else {
        fitsio_pure::gzip::decompress(bytes).unwrap()
    }
}

/// cfitsio writes `pixels` with `[compress spec]` and seed `SEED`.
fn cfitsio_writes<T: fitsio::images::WriteImage>(
    path: &Path,
    spec: &str,
    image_type: ImageType,
    pixels: &[T],
) {
    let _lock = cfitsio_lock();
    // `G2` (GZIP_2) and `lossless` aren't cfitsio spec syntax; they are set
    // through the API instead.
    let (spec, lossless) = match spec.strip_suffix(" lossless") {
        Some(spec) => (spec, true),
        None => (spec, false),
    };
    let (spec, gzip2) = match spec.strip_prefix("G2") {
        Some(rest) => (format!("G{rest}"), true),
        None => (spec.to_string(), false),
    };
    let name = format!("{}[compress {spec}]", path.display());
    let mut f = CFits::create(&name).open().unwrap();
    let mut status = 0;
    unsafe {
        fitsio::sys::fits_set_dither_seed(f.as_raw(), SEED, &mut status);
        if gzip2 {
            fitsio::sys::fits_set_compression_type(f.as_raw(), 22, &mut status);
        }
        if lossless {
            // cfitsio's NO_QUANTIZE.
            fitsio::sys::fits_set_quantize_level(f.as_raw(), 9999.0, &mut status);
        }
    }
    assert_eq!(status, 0);
    let desc = ImageDescription {
        data_type: image_type,
        dimensions: &[H, W],
    };
    let hdu = f.create_image("SCI", &desc).unwrap();
    hdu.write_image(&mut f, pixels).unwrap();
}

/// fitsio-pure writes the same file.
fn pure_writes<T: CompressPixel>(path: &Path, opts: &TileCompression, pixels: &[T]) {
    let mut file = serialize_header(&build_primary_header(8, &[]).unwrap()).unwrap();
    let name = Card {
        keyword: *b"EXTNAME ",
        value: Some(Value::String("SCI".into())),
        comment: None,
    };
    file.extend(compress_image_hdu(&[W, H], pixels, opts, &[name]).unwrap());
    std::fs::write(path, file).unwrap();
}

/// The tiles of both files compress alike.
fn assert_same_tiles(c: &Path, p: &Path, gzip: bool, what: &str) {
    let (c, p) = (tile_rows(c), tile_rows(p));
    assert_eq!(c.len(), p.len(), "{what}: tile count");
    for (i, (c, p)) in c.iter().zip(&p).enumerate() {
        assert_eq!(c.scale, p.scale, "{what}: ZSCALE/ZZERO of tile {i}");
        if gzip {
            assert_eq!(
                inflate(&c.compressed),
                inflate(&p.compressed),
                "{what}: tile {i}"
            );
        } else {
            assert_eq!(c.compressed, p.compressed, "{what}: tile {i}");
        }
        assert_eq!(
            inflate(&c.fallback),
            inflate(&p.fallback),
            "{what}: fallback tile {i}"
        );
    }
}

/// The data unit of HDU 1, the compressed image's table and heap, of the
/// file at `path`.
fn image_data_unit(path: &Path) -> Vec<u8> {
    let bytes = std::fs::read(path).unwrap();
    let parsed = parse_fits(&bytes).unwrap();
    bytes[parsed.hdus[1].data_start..].to_vec()
}

/// cfitsio reads `path` back as `T`.
fn cfitsio_reads<T: fitsio::images::ReadImage>(path: &Path) -> T {
    let mut f = CFits::open(path).unwrap();
    let hdu = f.hdu("SCI").unwrap();
    hdu.read_image(&mut f).unwrap()
}

fn noisy() -> Vec<f32> {
    let mut v: Vec<f32> = (0..W * H)
        .map(|i| 100.0 + 10.0 * ((i as f32) * 0.37).sin() + (i % 7) as f32)
        .collect();
    // A constant row, which can't be quantized and is stored losslessly.
    v[2 * W..3 * W].fill(42.5);
    v
}

/// `[compress R]`, `[compress R 16,8]` and their gzip twins, with our options.
fn codecs() -> Vec<(&'static str, TileCompression, bool)> {
    let seed = Quantize::new().seed(DitherSeed::Fixed(SEED as u16));
    vec![
        ("R", TileCompression::rice().quantize(seed), false),
        (
            "R 16,8",
            TileCompression::rice().tile_dims(&[16, 8]).quantize(seed),
            false,
        ),
        ("G", TileCompression::gzip().quantize(seed), true),
        ("G2", TileCompression::gzip2().quantize(seed), true),
        (
            "G2 16,8",
            TileCompression::gzip2().tile_dims(&[16, 8]).quantize(seed),
            true,
        ),
        (
            "G 16,8",
            TileCompression::gzip().tile_dims(&[16, 8]).quantize(seed),
            true,
        ),
    ]
}

macro_rules! integer_parity {
    ($name:ident, $t:ty, $image_type:expr, |$i:ident| $value:expr) => {
        #[test]
        fn $name() {
            let pixels: Vec<$t> = (0..W * H).map(|$i| $value).collect();
            let dir = tempfile::tempdir().unwrap();
            for (spec, opts, gzip) in codecs() {
                let (c, p) = (dir.path().join("c.fits"), dir.path().join("p.fits"));
                let _ = std::fs::remove_file(&c);
                cfitsio_writes(&c, spec, $image_type, &pixels);
                pure_writes(&p, &opts, &pixels);
                let what = format!("{} [compress {spec}]", stringify!($t));
                assert_same_tiles(&c, &p, gzip, &what);
                if !gzip {
                    assert!(image_data_unit(&c) == image_data_unit(&p), "{what}: data unit");
                }
                assert_eq!(cfitsio_reads::<Vec<$t>>(&p), pixels, "{what}");
            }
        }
    };
}

integer_parity!(
    u8_tiles_match_cfitsio,
    u8,
    ImageType::UnsignedByte,
    |i| (i * 37 % 251) as u8
);
integer_parity!(
    i16_tiles_match_cfitsio,
    i16,
    ImageType::Short,
    |i| ((i * 977) % 60_000) as i16
);
integer_parity!(
    u16_tiles_match_cfitsio,
    u16,
    ImageType::UnsignedShort,
    |i| (i as u16).wrapping_mul(977)
);
integer_parity!(i32_tiles_match_cfitsio, i32, ImageType::Long, |i| (i
    as i32)
    .wrapping_mul(1_234_567));

#[test]
fn f32_tiles_match_cfitsio() {
    let pixels = noisy();
    let dir = tempfile::tempdir().unwrap();
    for (spec, opts, gzip) in codecs() {
        let (c, p) = (dir.path().join("c.fits"), dir.path().join("p.fits"));
        let _ = std::fs::remove_file(&c);
        cfitsio_writes(&c, spec, ImageType::Float, &pixels);
        pure_writes(&p, &opts, &pixels);
        let what = format!("f32 [compress {spec}]");
        // The constant row's lossless tile is gzip, so the data units differ
        // even for Rice; the tiles themselves match.
        assert_same_tiles(&c, &p, gzip, &what);
        let from_c: Vec<f32> = cfitsio_reads(&c);
        let from_p: Vec<f32> = cfitsio_reads(&p);
        assert_eq!(from_c, from_p, "{what}");
        if !spec.contains(',') {
            // Row tiles: the constant row is a tile of its own.
            assert_eq!(
                from_p[2 * W..3 * W],
                pixels[2 * W..3 * W],
                "{what}: lossless row"
            );
        }
    }
}

#[test]
fn f64_tiles_match_cfitsio() {
    let pixels: Vec<f64> = noisy()
        .iter()
        .map(|&v| f64::from(v) * 1.25 + 0.001)
        .collect();
    let dir = tempfile::tempdir().unwrap();
    for (spec, opts, gzip) in codecs() {
        let (c, p) = (dir.path().join("c.fits"), dir.path().join("p.fits"));
        let _ = std::fs::remove_file(&c);
        cfitsio_writes(&c, spec, ImageType::Double, &pixels);
        pure_writes(&p, &opts, &pixels);
        let what = format!("f64 [compress {spec}]");
        assert_same_tiles(&c, &p, gzip, &what);
        let from_c: Vec<f64> = cfitsio_reads(&c);
        let from_p: Vec<f64> = cfitsio_reads(&p);
        assert_eq!(from_c, from_p, "{what}");
    }
}

/// Lossless float tiles, as AstroBurst's `write_planes_gzip2_lossless`
/// writes them, compress alike and read back exactly.
#[test]
fn lossless_float_tiles_match_cfitsio() {
    let f32s = noisy();
    let f64s: Vec<f64> = f32s.iter().map(|&v| f64::from(v) / 3.0).collect();
    let dir = tempfile::tempdir().unwrap();
    for (spec, opts) in [
        ("G lossless", TileCompression::gzip().lossless()),
        ("G2 lossless", TileCompression::gzip2().lossless()),
        (
            "G2 16,8 lossless",
            TileCompression::gzip2().lossless().tile_dims(&[16, 8]),
        ),
    ] {
        let (c, p) = (dir.path().join("c.fits"), dir.path().join("p.fits"));
        let _ = std::fs::remove_file(&c);
        cfitsio_writes(&c, spec, ImageType::Float, &f32s);
        pure_writes(&p, &opts, &f32s);
        assert_same_tiles(&c, &p, true, &format!("f32 [compress {spec}]"));
        assert_eq!(cfitsio_reads::<Vec<f32>>(&p), f32s, "f32 [compress {spec}]");

        let _ = std::fs::remove_file(&c);
        cfitsio_writes(&c, spec, ImageType::Double, &f64s);
        pure_writes(&p, &opts, &f64s);
        assert_same_tiles(&c, &p, true, &format!("f64 [compress {spec}]"));
        assert_eq!(cfitsio_reads::<Vec<f64>>(&p), f64s, "f64 [compress {spec}]");
    }
}

/// cfitsio's data-derived seed, a negative request, matches ours.
#[test]
fn checksum_seed_matches_cfitsio() {
    let pixels = noisy();
    let dir = tempfile::tempdir().unwrap();
    let (c, p) = (dir.path().join("c.fits"), dir.path().join("p.fits"));
    {
        let _lock = cfitsio_lock();
        let name = format!("{}[compress R]", c.display());
        let mut f = CFits::create(&name).open().unwrap();
        let mut status = 0;
        unsafe { fitsio::sys::fits_set_dither_seed(f.as_raw(), -1, &mut status) };
        let desc = ImageDescription {
            data_type: ImageType::Float,
            dimensions: &[H, W],
        };
        let hdu = f.create_image("SCI", &desc).unwrap();
        hdu.write_image(&mut f, &pixels).unwrap();
    }
    pure_writes(&p, &TileCompression::rice(), &pixels);
    let seed = |path: &Path| -> i64 {
        let mut f = CFits::open(path).unwrap();
        let hdu = f.hdu("SCI").unwrap();
        // fitsio reads the compressed header's keywords through the image.
        hdu.read_key(&mut f, "ZDITHER0").unwrap()
    };
    assert_eq!(seed(&c), seed(&p));
    assert_same_tiles(&c, &p, false, "checksum seed");
}

/// serialimage's save: a custom primary image and more channels as
/// extensions, written through compat with `[compress]`. cfitsio reads
/// every channel and key back.
#[test]
fn compat_compress_spec_writes_what_cfitsio_reads() {
    use fitsio_pure::compat::images::{ImageDescription as PDesc, ImageType as PType};

    let red: Vec<u16> = (0..W * H).map(|i| (i as u16).wrapping_mul(977)).collect();
    let green: Vec<u16> = red.iter().map(|v| v.wrapping_add(12_345)).collect();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rgb.fits");
    {
        let desc = PDesc {
            data_type: PType::UnsignedShort,
            dimensions: &[H, W],
        };
        let mut f = PFits::create(format!("{}[compress]", path.display()))
            .with_custom_primary(&desc)
            .open()
            .unwrap();
        let hdu = f.primary_hdu().unwrap();
        hdu.write_image(&mut f, &red).unwrap();
        let ghdu = f.create_image("GREEN", &desc).unwrap();
        ghdu.write_image(&mut f, &green).unwrap();
        hdu.write_key(&mut f, "CHANNELS", 2).unwrap();
        hdu.write_key(&mut f, "CAMERA", "Test Cam").unwrap();
    }

    let mut f = CFits::open(&path).unwrap();
    // An empty primary HDU, then both channels compressed.
    assert_eq!(f.iter().count(), 3);
    let primary = f.hdu("_PRIMARY").unwrap();
    assert_eq!(primary.read_image::<Vec<u16>>(&mut f).unwrap(), red);
    assert_eq!(primary.read_key::<i64>(&mut f, "CHANNELS").unwrap(), 2);
    assert_eq!(
        primary.read_key::<String>(&mut f, "CAMERA").unwrap(),
        "Test Cam"
    );
    let green_hdu = f.hdu("GREEN").unwrap();
    assert_eq!(green_hdu.read_image::<Vec<u16>>(&mut f).unwrap(), green);

    let bytes = std::fs::read(&path).unwrap();
    let parsed = parse_fits(&bytes).unwrap();
    let zsimple = parsed.hdus[1]
        .cards
        .iter()
        .any(|c| c.keyword_str() == "ZSIMPLE");
    assert!(zsimple, "the primary image is marked ZSIMPLE");

    // compat reads it back too.
    let f = PFits::open(&path).unwrap();
    let primary = f.hdu("_PRIMARY").unwrap();
    assert_eq!(primary.read_image::<Vec<u16>>(&f).unwrap(), red);
    assert_eq!(primary.read_key::<i64>(&f, "CHANNELS").unwrap(), 2);
    assert_eq!(
        f.hdu("GREEN").unwrap().read_image::<Vec<u16>>(&f).unwrap(),
        green
    );
}

/// The same `[compress …]` spec through cfitsio and through compat
/// compresses an image extension to the same tiles.
#[test]
fn compat_compress_spec_matches_cfitsio() {
    use fitsio_pure::compat::images::{ImageDescription as PDesc, ImageType as PType};

    let ints: Vec<i32> = (0..W * H)
        .map(|i| (i as i32).wrapping_mul(1_234_567))
        .collect();
    let floats = noisy();
    let dir = tempfile::tempdir().unwrap();
    for spec in ["", " R 16,8", " G", " G 16,8; q 8", " R; q0 2"] {
        let (c, p) = (dir.path().join("c.fits"), dir.path().join("p.fits"));
        let _ = std::fs::remove_file(&c);
        let _ = std::fs::remove_file(&p);
        {
            let _lock = cfitsio_lock();
            let mut f = CFits::create(format!("{}[compress{spec}]", c.display()))
                .open()
                .unwrap();
            let mut status = 0;
            unsafe { fitsio::sys::fits_set_dither_seed(f.as_raw(), -1, &mut status) };
            for (name, ty) in [("INTS", ImageType::Long), ("FLOATS", ImageType::Float)] {
                let desc = ImageDescription {
                    data_type: ty,
                    dimensions: &[H, W],
                };
                let hdu = f.create_image(name, &desc).unwrap();
                match name {
                    "INTS" => hdu.write_image(&mut f, &ints).unwrap(),
                    _ => hdu.write_image(&mut f, &floats).unwrap(),
                }
            }
        }
        {
            let mut f = PFits::create(format!("{}[compress{spec}]", p.display()))
                .open()
                .unwrap();
            for (name, ty) in [("INTS", PType::Long), ("FLOATS", PType::Float)] {
                let desc = PDesc {
                    data_type: ty,
                    dimensions: &[H, W],
                };
                let hdu = f.create_image(name, &desc).unwrap();
                match name {
                    "INTS" => hdu.write_image(&mut f, &ints).unwrap(),
                    _ => hdu.write_image(&mut f, &floats).unwrap(),
                }
            }
        }
        for hdu in [1, 2] {
            let rows = |path: &Path| {
                let bytes = std::fs::read(path).unwrap();
                let parsed = parse_fits(&bytes).unwrap();
                let one = dir.path().join("one.fits");
                let mut single = bytes[..parsed.hdus[1].header_start].to_vec();
                let h = &parsed.hdus[hdu];
                single.extend_from_slice(&bytes[h.header_start..h.data_start + h.data_len]);
                std::fs::write(&one, single).unwrap();
                tile_rows(&one)
            };
            let (cr, pr) = (rows(&c), rows(&p));
            assert_eq!(cr.len(), pr.len(), "[compress{spec}] HDU {hdu}");
            for (i, (cr, pr)) in cr.iter().zip(&pr).enumerate() {
                assert_eq!(cr.scale, pr.scale, "[compress{spec}] HDU {hdu} tile {i}");
                assert_eq!(
                    inflate_or_same(&cr.compressed, spec),
                    inflate_or_same(&pr.compressed, spec),
                    "[compress{spec}] HDU {hdu} tile {i}"
                );
            }
        }
        let read = |path: &Path| -> (Vec<i32>, Vec<f32>) {
            let mut f = CFits::open(path).unwrap();
            let ints = f.hdu("INTS").unwrap().read_image(&mut f).unwrap();
            let floats = f.hdu("FLOATS").unwrap().read_image(&mut f).unwrap();
            (ints, floats)
        };
        assert_eq!(read(&c), read(&p), "[compress{spec}]");
        assert_eq!(read(&p).0, ints);
    }
}

/// Gzip tiles inflated, Rice tiles as they are.
fn inflate_or_same(bytes: &[u8], spec: &str) -> Vec<u8> {
    if spec.trim_start().starts_with('G') {
        inflate(bytes)
    } else {
        bytes.to_vec()
    }
}
