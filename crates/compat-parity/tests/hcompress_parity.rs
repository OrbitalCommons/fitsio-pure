//! `HCOMPRESS_1` and `PLIO_1` tile decoding, against astropy and cfitsio.
//!
//! `fixtures/hcompress/` holds images astropy 8.0.1 wrote (made by
//! `make_fixtures.py`, which also saves astropy's decode of each as
//! little-endian `f64` in `<name>.expected`), and refimage 1.0.0-pre6's two
//! HCOMPRESS outputs. fitsio-pure must decode each exactly as astropy and
//! cfitsio do. cfitsio-written images are checked the same way.

use std::path::{Path, PathBuf};

use fitsio::images::{ImageDescription, ImageType};
use fitsio::FitsFile as CFits;
use fitsio_pure::compat::fitsfile::FitsFile as PFits;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hcompress")
}

/// The compressed image (the last HDU) as physical values, by fitsio-pure.
fn pure_decode(path: &Path) -> Vec<f64> {
    let f = PFits::open(path).unwrap();
    let last = f.num_hdus().unwrap() - 1;
    f.hdu(last).unwrap().read_image(&f).unwrap()
}

/// The same, by cfitsio. A `float32` image is read as `f32`, as astropy and
/// fitsio-pure produce it; read as `f64`, cfitsio dequantizes straight to
/// double precision.
fn cfitsio_decode(path: &Path) -> Vec<f64> {
    use fitsio::hdu::HduInfo;
    let mut f = CFits::open(path).unwrap();
    let last = f.iter().count() - 1;
    let hdu = f.hdu(last).unwrap();
    match hdu.info {
        HduInfo::ImageInfo {
            image_type: ImageType::Float,
            ..
        } => {
            let v: Vec<f32> = hdu.read_image(&mut f).unwrap();
            v.into_iter().map(f64::from).collect()
        }
        _ => hdu.read_image(&mut f).unwrap(),
    }
}

/// Compare two decodes exactly, NaN equal to NaN.
fn assert_same(what: &str, got: &[f64], want: &[f64]) {
    assert_eq!(got.len(), want.len(), "{what}: pixel count");
    let bad = got
        .iter()
        .zip(want)
        .position(|(g, w)| !(g == w || (g.is_nan() && w.is_nan())));
    if let Some(i) = bad {
        panic!("{what}: pixel {i} is {} but should be {}", got[i], want[i]);
    }
}

#[test]
fn astropy_and_refimage_fixtures_decode_as_astropy_does() {
    let mut names: Vec<String> = std::fs::read_dir(fixtures())
        .unwrap()
        .filter_map(|e| e.unwrap().file_name().into_string().ok())
        .filter_map(|n| n.strip_suffix(".fits").map(String::from))
        .collect();
    names.sort();
    assert_eq!(names.len(), 16);
    for name in names {
        let path = fixtures().join(format!("{name}.fits"));
        let expected: Vec<f64> = std::fs::read(fixtures().join(format!("{name}.expected")))
            .unwrap()
            .chunks_exact(8)
            .map(|b| f64::from_le_bytes(b.try_into().unwrap()))
            .collect();
        let ours = pure_decode(&path);
        assert_same(&format!("{name} vs astropy"), &ours, &expected);
        assert_same(&format!("{name} vs cfitsio"), &ours, &cfitsio_decode(&path));
    }
}

/// cfitsio compresses with `spec` and decodes; fitsio-pure must decode the
/// same values.
fn cfitsio_round_trip<T: fitsio::images::WriteImage>(
    spec: &str,
    image_type: ImageType,
    pixels: &[T],
    w: usize,
    h: usize,
) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.fits");
    {
        let mut f = CFits::create(format!("{}[compress {spec}]", path.display()))
            .open()
            .unwrap();
        let desc = ImageDescription {
            data_type: image_type,
            dimensions: &[h, w],
        };
        let hdu = f.create_image("SCI", &desc).unwrap();
        hdu.write_image(&mut f, pixels).unwrap();
    }
    let what = format!("[compress {spec}] {image_type:?}");
    assert_same(&what, &pure_decode(&path), &cfitsio_decode(&path));
}

#[test]
fn cfitsio_hcompress_images_decode_as_cfitsio_does() {
    let (w, h) = (61, 37);
    let field = |i: usize| {
        let (x, y) = ((i % w) as f64, (i / w) as f64);
        800.0 + 250.0 * (x / 7.0).sin() * (y / 5.0).cos() + ((i * 7919) % 23) as f64
    };
    let i16s: Vec<i16> = (0..w * h).map(|i| field(i) as i16 - 700).collect();
    let i32s: Vec<i32> = (0..w * h).map(|i| (field(i) * 3000.0) as i32).collect();
    let f32s: Vec<f32> = (0..w * h).map(|i| field(i) as f32 * 0.37).collect();
    for spec in ["H", "H 16,16", "H; s 4", "HS; s 4", "HS 20,12; s 2"] {
        cfitsio_round_trip(spec, ImageType::Short, &i16s, w, h);
        cfitsio_round_trip(spec, ImageType::Long, &i32s, w, h);
        cfitsio_round_trip(spec, ImageType::Float, &f32s, w, h);
    }
}

#[test]
fn cfitsio_plio_images_decode_as_cfitsio_does() {
    let (w, h) = (61, 37);
    // PLIO holds non-negative values below 2^24, typically masks.
    let i32s: Vec<i32> = (0..w * h)
        .map(|i| ((i % w) / 6 + (i / w) / 4) as i32 % 5 * 77)
        .collect();
    let i16s: Vec<i16> = (0..w * h).map(|i| ((i * 31) % 1000) as i16).collect();
    for spec in ["P", "P 16,8"] {
        cfitsio_round_trip(spec, ImageType::Long, &i32s, w, h);
        cfitsio_round_trip(spec, ImageType::Short, &i16s, w, h);
    }
}
