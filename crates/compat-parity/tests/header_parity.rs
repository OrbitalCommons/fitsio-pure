//! Differential header-keyword roundtrip parity between `fitsio-pure`'s compat
//! shims and the cfitsio-backed `fitsio` crate.
//!
//! Keywords are written to the primary HDU with one library and read back with
//! the other, in both directions.

use fitsio::FitsFile as CFits;

use fitsio_pure::compat::fitsfile::FitsFile as PureFits;
use fitsio_pure::compat::headers::{ReadsKey as PureReadsKey, WritesKey as PureWritesKey};

const INT_KEY: &str = "INTKEY";
const FLT_KEY: &str = "FLTKEY";
const STR_KEY: &str = "STRKEY";

const INT_VAL: i64 = -12345;
const FLT_VAL: f64 = 3.140625; // exactly representable in f64
const STR_VAL: &str = "hello world";

#[test]
fn header_keys_pure_to_cfitsio() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pure_to_c.fits");

    {
        let mut f = PureFits::create(&path).open().unwrap();
        let hdu = f.primary_hdu().unwrap();
        <i64 as PureWritesKey>::write_key(&mut f, &hdu, INT_KEY, &INT_VAL).unwrap();
        <f64 as PureWritesKey>::write_key(&mut f, &hdu, FLT_KEY, &FLT_VAL).unwrap();
        <String as PureWritesKey>::write_key(&mut f, &hdu, STR_KEY, &STR_VAL.to_string()).unwrap();
        f.flush().unwrap();
    }

    let mut fptr = CFits::open(&path).expect("cfitsio failed to open pure-written file");
    let hdu = fptr.primary_hdu().unwrap();
    let int_read: i64 = hdu.read_key(&mut fptr, INT_KEY).unwrap();
    let flt_read: f64 = hdu.read_key(&mut fptr, FLT_KEY).unwrap();
    let str_read: String = hdu.read_key(&mut fptr, STR_KEY).unwrap();

    assert_eq!(int_read, INT_VAL);
    assert_eq!(flt_read, FLT_VAL);
    assert_eq!(str_read.trim(), STR_VAL);
}

#[test]
fn header_keys_cfitsio_to_pure() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c_to_pure.fits");

    {
        let mut fptr = CFits::create(&path).open().unwrap();
        let hdu = fptr.primary_hdu().unwrap();
        hdu.write_key(&mut fptr, INT_KEY, INT_VAL).unwrap();
        hdu.write_key(&mut fptr, FLT_KEY, FLT_VAL).unwrap();
        hdu.write_key(&mut fptr, STR_KEY, STR_VAL).unwrap();
    }

    let f = PureFits::open(&path).expect("fitsio-pure failed to open cfitsio-written file");
    let hdu = f.primary_hdu().unwrap();
    let int_read: i64 = <i64 as PureReadsKey>::read_key(&f, &hdu, INT_KEY).unwrap();
    let flt_read: f64 = <f64 as PureReadsKey>::read_key(&f, &hdu, FLT_KEY).unwrap();
    let str_read: String = <String as PureReadsKey>::read_key(&f, &hdu, STR_KEY).unwrap();

    assert_eq!(int_read, INT_VAL);
    assert_eq!(flt_read, FLT_VAL);
    assert_eq!(str_read.trim(), STR_VAL);
}

/// Long string values use the CONTINUE convention. cfitsio's own long-string
/// calls (`fits_write_key_longstr` / `fits_read_key_longstr`) are the
/// reference; `fitsio`'s `write_key` and `read_key` truncate at 68 characters.
mod long_strings {
    use std::ffi::{CStr, CString};
    use std::path::Path;

    use super::*;

    const KEY: &str = "LONGVAL";
    const COMMENT: &str = "a comment";

    fn value() -> String {
        let quoted = "it's 'quoted' and ";
        (0..4)
            .map(|i| format!("{quoted}{}", "abcdefghij".repeat(i + 1)))
            .collect()
    }

    fn c_read(path: &Path) -> (String, String) {
        let mut f = CFits::open(path).unwrap();
        let key = CString::new(KEY).unwrap();
        let mut status = 0;
        let mut comment = [0 as std::os::raw::c_char; 81];
        unsafe {
            let mut value: *mut std::os::raw::c_char = std::ptr::null_mut();
            fitsio::sys::ffgkls(
                f.as_raw(),
                key.as_ptr(),
                &mut value,
                comment.as_mut_ptr(),
                &mut status,
            );
            assert_eq!(status, 0, "cfitsio could not read {KEY}");
            let read = CStr::from_ptr(value).to_string_lossy().into_owned();
            fitsio::sys::fffree(value as *mut _, &mut status);
            (
                read,
                CStr::from_ptr(comment.as_ptr())
                    .to_string_lossy()
                    .into_owned(),
            )
        }
    }

    #[test]
    fn long_string_pure_to_cfitsio() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pure.fits");
        {
            let mut f = PureFits::create(&path).open().unwrap();
            let hdu = f.primary_hdu().unwrap();
            <String as PureWritesKey>::write_key(&mut f, &hdu, KEY, &value()).unwrap();
        }
        assert_eq!(c_read(&path).0, value());
    }

    fn c_write(path: &Path, comment: &str) {
        let mut f = CFits::create(path).open().unwrap();
        let (key, val, cmt) = (
            CString::new(KEY).unwrap(),
            CString::new(value()).unwrap(),
            CString::new(comment).unwrap(),
        );
        let mut status = 0;
        unsafe {
            fitsio::sys::ffpkls(
                f.as_raw(),
                key.as_ptr(),
                val.as_ptr(),
                cmt.as_ptr(),
                &mut status,
            )
        };
        assert_eq!(status, 0);
    }

    /// The keyword's card and its CONTINUE cards, as written.
    fn long_string_cards(path: &Path) -> Vec<String> {
        let bytes = std::fs::read(path).unwrap();
        let cards: Vec<String> = bytes[..2880]
            .chunks(80)
            .map(|c| String::from_utf8_lossy(c).into_owned())
            .collect();
        let start = cards.iter().position(|c| c.starts_with(KEY)).unwrap();
        cards[start..]
            .iter()
            .take_while(|c| c.starts_with(KEY) || c.starts_with("CONTINUE"))
            .cloned()
            .collect()
    }

    #[test]
    fn long_string_cards_match_cfitsio() {
        let dir = tempfile::tempdir().unwrap();
        let by_c = dir.path().join("c.fits");
        c_write(&by_c, "");
        let by_pure = dir.path().join("pure.fits");
        {
            let mut f = PureFits::create(&by_pure).open().unwrap();
            let hdu = f.primary_hdu().unwrap();
            <String as PureWritesKey>::write_key(&mut f, &hdu, KEY, &value()).unwrap();
        }
        assert_eq!(long_string_cards(&by_pure), long_string_cards(&by_c));
    }

    #[test]
    fn long_string_cfitsio_to_pure() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.fits");
        c_write(&path, COMMENT);
        assert_eq!(c_read(&path), (value(), COMMENT.to_string()));

        let f = PureFits::open(&path).unwrap();
        let hdu = f.primary_hdu().unwrap();
        assert_eq!(
            <String as PureReadsKey>::read_key(&f, &hdu, KEY).unwrap(),
            value()
        );
        let cards =
            fitsio_pure::header::parse_header_blocks(&std::fs::read(&path).unwrap()).unwrap();
        let card = cards.iter().find(|c| c.keyword_str() == KEY).unwrap();
        assert_eq!(card.comment.as_deref(), Some(COMMENT));
    }
}

/// Keywords longer than 8 characters use the HIERARCH convention, which
/// cfitsio writes automatically and reads by any spelling of the name.
mod hierarch {
    use super::*;

    const NAMES: [(&str, &str); 2] = [
        ("ESO DET CHIP1 ID", "CCD-42"),
        ("ESO INS FILT1 NAME", "R_SPECIAL"),
    ];

    #[test]
    fn hierarch_pure_to_cfitsio() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pure.fits");
        {
            let mut f = PureFits::create(&path).open().unwrap();
            let hdu = f.primary_hdu().unwrap();
            for (name, value) in NAMES {
                <String as PureWritesKey>::write_key(&mut f, &hdu, name, &value.to_string())
                    .unwrap();
            }
            <f64 as PureWritesKey>::write_key(&mut f, &hdu, "ESO TEL TEMP", &FLT_VAL).unwrap();
            <i64 as PureWritesKey>::write_key(&mut f, &hdu, "ESO DET NDIT", &INT_VAL).unwrap();
        }
        let mut f = CFits::open(&path).unwrap();
        let hdu = f.primary_hdu().unwrap();
        for (name, value) in NAMES {
            for spelling in [
                name.to_string(),
                format!("HIERARCH {name}"),
                name.to_lowercase(),
            ] {
                let read: String = hdu.read_key(&mut f, &spelling).unwrap();
                assert_eq!(read, value, "cfitsio reading {spelling:?}");
            }
        }
        let temp: f64 = hdu.read_key(&mut f, "ESO TEL TEMP").unwrap();
        let ndit: i64 = hdu.read_key(&mut f, "ESO DET NDIT").unwrap();
        assert_eq!((temp, ndit), (FLT_VAL, INT_VAL));
    }

    #[test]
    fn hierarch_cfitsio_to_pure() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.fits");
        {
            let mut f = CFits::create(&path).open().unwrap();
            let hdu = f.primary_hdu().unwrap();
            for (name, value) in NAMES {
                hdu.write_key(&mut f, name, value).unwrap();
            }
            hdu.write_key(&mut f, "ESO TEL TEMP", FLT_VAL).unwrap();
            hdu.write_key(&mut f, "ESO DET NDIT", INT_VAL).unwrap();
        }
        let f = PureFits::open(&path).unwrap();
        let hdu = f.primary_hdu().unwrap();
        for (name, value) in NAMES {
            for spelling in [
                name.to_string(),
                format!("HIERARCH {name}"),
                name.to_lowercase(),
            ] {
                let read = <String as PureReadsKey>::read_key(&f, &hdu, &spelling).unwrap();
                assert_eq!(read, value, "fitsio-pure reading {spelling:?}");
            }
        }
        assert_eq!(
            <f64 as PureReadsKey>::read_key(&f, &hdu, "ESO TEL TEMP").unwrap(),
            FLT_VAL
        );
        assert_eq!(
            <i64 as PureReadsKey>::read_key(&f, &hdu, "ESO DET NDIT").unwrap(),
            INT_VAL
        );
    }
}

/// An undefined value (`KEY =` with nothing after it) and a non-standard
/// lowercase keyword, read the way cfitsio reads them.
#[test]
fn undefined_values_and_lowercase_keywords_match_cfitsio() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("odd.fits");
    let mut bytes: Vec<u8> = Vec::new();
    for text in [
        "SIMPLE  =                    T",
        "BITPIX  =                    8",
        "NAXIS   =                    0",
        "FILTER  =                      / no filter",
        "date-obs= '2026-10-06'",
        "END",
    ] {
        bytes.extend_from_slice(format!("{text:<80}").as_bytes());
    }
    bytes.resize(2880, b' ');
    std::fs::write(&path, &bytes).unwrap();

    let mut c = CFits::open(&path).unwrap();
    let c_hdu = c.primary_hdu().unwrap();
    let p = PureFits::open(&path).unwrap();
    let p_hdu = p.primary_hdu().unwrap();

    let c_undefined: Result<String, _> = c_hdu.read_key(&mut c, "FILTER");
    let p_undefined = <String as PureReadsKey>::read_key(&p, &p_hdu, "FILTER");
    assert!(c_undefined.is_err() && p_undefined.is_err());
    assert!(p_undefined.unwrap_err().to_string().contains("undefined"));

    for name in ["date-obs", "DATE-OBS"] {
        let c_value: String = c_hdu.read_key(&mut c, name).unwrap();
        let p_value = <String as PureReadsKey>::read_key(&p, &p_hdu, name).unwrap();
        assert_eq!(p_value, c_value, "{name}");
    }

    // Writing the header back keeps the undefined card intact.
    let cards = fitsio_pure::header::parse_header_blocks(&bytes).unwrap();
    let rewritten = fitsio_pure::header::serialize_header(
        &cards
            .into_iter()
            .filter(|c| !c.is_end())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let filter_card = rewritten
        .chunks(80)
        .find(|c| c.starts_with(b"FILTER"))
        .unwrap();
    assert_eq!(
        std::str::from_utf8(filter_card).unwrap().trim_end(),
        "FILTER  =                      / no filter"
    );
}

/// Free-format string values may start after column 11, as seiza's writer and
/// other software write them right-justified.
#[test]
fn indented_string_values_read_as_cfitsio_reads_them() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("indented.fits");
    let mut bytes: Vec<u8> = [
        "SIMPLE  =                    T",
        "BITPIX  =                    8",
        "NAXIS   =                    0",
        "OBJECT  =                'M31' / target",
        "EMPTY   =                   ''",
        "END",
    ]
    .iter()
    .flat_map(|card| format!("{card:<80}").into_bytes())
    .collect();
    bytes.resize(2880, b' ');
    std::fs::write(&path, &bytes).unwrap();

    let mut fptr = CFits::open(&path).unwrap();
    let chdu = fptr.primary_hdu().unwrap();
    let c_object: String = chdu.read_key(&mut fptr, "OBJECT").unwrap();
    let c_empty: String = chdu.read_key(&mut fptr, "EMPTY").unwrap();

    let f = PureFits::open(&path).unwrap();
    let hdu = f.primary_hdu().unwrap();
    let object = <String as PureReadsKey>::read_key(&f, &hdu, "OBJECT").unwrap();
    let empty = <String as PureReadsKey>::read_key(&f, &hdu, "EMPTY").unwrap();

    assert_eq!(c_object, "M31");
    assert_eq!((object, empty), (c_object, c_empty));
}
