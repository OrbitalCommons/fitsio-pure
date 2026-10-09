//! Error parity between `fitsio-pure`'s compat shims and the cfitsio-backed
//! `fitsio` crate.
//!
//! Each failure is triggered through both libraries, which must return the
//! same `Error` variant and, for `Error::Fits`, the same cfitsio status.

use std::path::Path;

use fitsio::errors::Error as CError;
use fitsio::images::{ImageDescription as CDesc, ImageType as CType};
use fitsio::FitsFile as CFits;

use fitsio_pure::compat::errors::Error as PError;
use fitsio_pure::compat::fitsfile::FitsFile as PFits;

/// The same exhaustive, wildcard-free match over every variant, as
/// asicam_rs, cameraunit_asi, cameraunit_fli and surge write it, compiled
/// against each library's `Error`.
macro_rules! describe {
    ($name:ident, $error:path) => {
        fn $name(e: &$error) -> String {
            use $error as Error;
            match e {
                Error::ExistingFile(path) => format!("ExistingFile({path})"),
                Error::Fits(fe) => format!("Fits({})", fe.status),
                Error::Index(ie) => format!("Index({:?})", ie.given),
                Error::IntoString(_) => "IntoString".into(),
                Error::Io(_) => "Io".into(),
                Error::Message(_) => "Message".into(),
                Error::Null(_) => "Null".into(),
                Error::NullPointer => "NullPointer".into(),
                Error::UnlockError => "UnlockError".into(),
                Error::Utf8(_) => "Utf8".into(),
            }
        }
    };
}

describe!(describe_c, fitsio::errors::Error);
describe!(describe_p, fitsio_pure::compat::errors::Error);

fn c_err<T>(r: fitsio::errors::Result<T>) -> String {
    describe_c(&r.err().expect("fitsio succeeded"))
}

fn p_err<T>(r: fitsio_pure::compat::errors::Result<T>) -> String {
    describe_p(&r.err().expect("compat succeeded"))
}

/// A file with an image extension, `STR = 'abc'`, `BIG = 7000000000` and
/// `UNDEF`, a keyword with no value.
fn fixture(path: &Path) {
    {
        let mut f = CFits::create(path).open().unwrap();
        let hdu = f.primary_hdu().unwrap();
        hdu.write_key(&mut f, "STR", "abc").unwrap();
        hdu.write_key(&mut f, "BIG", 7_000_000_000i64).unwrap();
        let desc = CDesc {
            data_type: CType::Float,
            dimensions: &[3, 4],
        };
        f.create_image("IMG", &desc).unwrap();
    }
    let mut bytes = std::fs::read(path).unwrap();
    let end = bytes.windows(8).position(|w| w == b"END     ").unwrap();
    let card = format!("{:<80}", "UNDEF   =                      / no value");
    bytes[end..end + 80].copy_from_slice(card.as_bytes());
    bytes[end + 80..end + 88].copy_from_slice(b"END     ");
    std::fs::write(path, bytes).unwrap();
}

/// Assert both libraries fail `$op` (an expression over `f`, `hdu` and
/// `img`) with `$expected`, opening `$path` with `$open`.
macro_rules! assert_parity {
    ($path:expr, $open:ident, $expected:expr, |$f:ident, $hdu:ident, $img:ident| $op:expr) => {{
        let c = {
            let mut $f = CFits::$open($path).unwrap();
            let $hdu = $f.primary_hdu().unwrap();
            let $img = $f.hdu("IMG").unwrap();
            let _ = (&$hdu, &$img);
            c_err($op)
        };
        // compat's methods take `&FitsFile` where fitsio's take `&mut`.
        #[allow(clippy::unnecessary_mut_passed)]
        let p = {
            #[allow(unused_mut)]
            let mut $f = PFits::$open($path).unwrap();
            let $hdu = $f.primary_hdu().unwrap();
            let $img = $f.hdu("IMG").unwrap();
            let _ = (&$hdu, &$img);
            p_err($op)
        };
        assert_eq!(c, $expected, "fitsio: {}", stringify!($op));
        assert_eq!(p, $expected, "compat: {}", stringify!($op));
    }};
}

#[test]
fn key_errors_match() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keys.fits");
    fixture(&path);

    assert_parity!(&path, open, "Fits(202)", |f, hdu, img| hdu
        .read_key::<i64>(&mut f, "NOPE"));
    assert_parity!(&path, open, "Fits(202)", |f, hdu, img| hdu
        .read_key::<String>(&mut f, "NOPE"));
    assert_parity!(&path, open, "Fits(204)", |f, hdu, img| hdu
        .read_key::<f64>(&mut f, "UNDEF"));
    assert_parity!(&path, open, "Fits(204)", |f, hdu, img| hdu
        .read_key::<String>(&mut f, "UNDEF"));
    assert_parity!(&path, open, "Fits(409)", |f, hdu, img| hdu
        .read_key::<i64>(&mut f, "STR"));
    assert_parity!(&path, open, "Fits(408)", |f, hdu, img| hdu
        .read_key::<f32>(&mut f, "STR"));
    assert_parity!(&path, open, "Fits(412)", |f, hdu, img| hdu
        .read_key::<i32>(&mut f, "BIG"));
}

#[test]
fn hdu_errors_match() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hdus.fits");
    fixture(&path);

    assert_parity!(&path, open, "Fits(301)", |f, hdu, img| f.hdu("NOPE"));
    assert_parity!(&path, open, "Fits(107)", |f, hdu, img| f.hdu(5));
}

#[test]
fn image_read_errors_match() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("image.fits");
    fixture(&path);

    assert_parity!(&path, open, "Fits(307)", |f, hdu, img| img
        .read_section::<Vec<f32>>(&mut f, 0, 100));
    assert_parity!(&path, open, "Fits(307)", |f, hdu, img| img
        .read_rows::<Vec<f32>>(&mut f, 2, 5));
    assert_parity!(&path, open, "Fits(307)", |f, hdu, img| img
        .read_region::<Vec<f32>>(&mut f, &[&(0..10), &(0..2)]));
}

#[test]
fn writes_to_a_read_only_file_match() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("readonly.fits");
    fixture(&path);

    assert_parity!(&path, open, "Fits(602)", |f, hdu, img| hdu
        .write_key(&mut f, "NEW", 1i64));
    assert_parity!(&path, open, "Fits(602)", |f, hdu, img| img
        .write_image(&mut f, &[0.0f32; 12]));
}

#[test]
fn bad_keyword_names_match() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("names.fits");
    fixture(&path);

    assert_parity!(&path, edit, "Fits(207)", |f, hdu, img| hdu
        .write_key(&mut f, "A=B", 1i64));
    assert_parity!(&path, edit, "Fits(207)", |f, hdu, img| hdu.write_key(
        &mut f,
        &"LONG".repeat(20),
        1i64
    ));
}

#[test]
fn file_errors_match() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("exists.fits");
    fixture(&path);

    let expected = format!("ExistingFile({})", path.display());
    assert_eq!(c_err(CFits::create(&path).open()), expected);
    assert_eq!(p_err(PFits::create(&path).open()), expected);

    let missing = dir.path().join("missing.fits");
    assert_eq!(c_err(CFits::open(&missing)), "Fits(104)");
    assert_eq!(p_err(PFits::open(&missing)), "Fits(104)");
    assert_eq!(c_err(CFits::edit(&missing)), "Fits(104)");
    assert_eq!(p_err(PFits::edit(&missing)), "Fits(104)");

    let no_dir = dir.path().join("no/such/dir.fits");
    assert_eq!(c_err(CFits::create(&no_dir).open()), "Fits(105)");
    assert_eq!(p_err(PFits::create(&no_dir).open()), "Fits(105)");
}

/// mwalib's and hyperdrive's shape: a missing or undefined key is `None`,
/// any other error is passed on.
fn optional_key_c(path: &Path, name: &str) -> Result<Option<i64>, CError> {
    let mut f = CFits::open(path)?;
    let hdu = f.primary_hdu()?;
    match hdu.read_key::<i64>(&mut f, name) {
        Ok(v) => Ok(Some(v)),
        Err(CError::Fits(fe)) if fe.status == 202 || fe.status == 204 => Ok(None),
        Err(CError::Null(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

fn optional_key_p(path: &Path, name: &str) -> Result<Option<i64>, PError> {
    use fitsio_pure::compat::errors::FitsError;
    let f = PFits::open(path)?;
    let hdu = f.primary_hdu()?;
    match hdu.read_key::<i64>(&f, name) {
        Ok(v) => Ok(Some(v)),
        Err(PError::Fits(FitsError { status: 202, .. })) => Ok(None),
        Err(PError::Fits(fe)) if fe.status == fitsio_pure::compat::sys::VALUE_UNDEFINED as i32 => {
            Ok(None)
        }
        Err(PError::Null(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

#[test]
fn absent_keys_are_none_in_both() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("optional.fits");
    fixture(&path);

    for name in ["NOPE", "UNDEF"] {
        assert_eq!(optional_key_c(&path, name).unwrap(), None);
        assert_eq!(optional_key_p(&path, name).unwrap(), None);
    }
    assert_eq!(optional_key_c(&path, "BIG").unwrap(), Some(7_000_000_000));
    assert_eq!(optional_key_p(&path, "BIG").unwrap(), Some(7_000_000_000));
    assert!(optional_key_c(&path, "STR").is_err());
    assert!(optional_key_p(&path, "STR").is_err());
}

#[test]
fn sys_constants_match() {
    use fitsio::sys as c;
    use fitsio_pure::compat::sys as p;

    assert_eq!(c::KEY_NO_EXIST, p::KEY_NO_EXIST);
    assert_eq!(c::VALUE_UNDEFINED, p::VALUE_UNDEFINED);
    assert_eq!(c::BAD_HDU_NUM, p::BAD_HDU_NUM);
    assert_eq!(c::END_OF_FILE, p::END_OF_FILE);
    assert_eq!(c::BAD_ROW_NUM, p::BAD_ROW_NUM);
    assert_eq!(c::NUM_OVERFLOW, p::NUM_OVERFLOW);
    assert_eq!(c::FILE_NOT_OPENED, p::FILE_NOT_OPENED);
    assert_eq!(c::OVERFLOW_ERR, p::OVERFLOW_ERR);
    assert_eq!(c::NO_CLOSE_ERROR, p::NO_CLOSE_ERROR);
}

#[test]
fn check_status_matches() {
    for status in [0, 104, 202, 204, 301, 307, 412] {
        let c = fitsio::errors::check_status(status);
        let p = fitsio_pure::compat::errors::check_status(status);
        match (c, p) {
            (Ok(()), Ok(())) => {}
            (Err(CError::Fits(c)), Err(PError::Fits(p))) => {
                assert_eq!((c.status, c.message), (p.status, p.message));
            }
            (c, p) => panic!("status {status}: fitsio {c:?}, compat {p:?}"),
        }
    }
}
