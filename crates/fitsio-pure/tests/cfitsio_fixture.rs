//! Compat against `cfitsio-writer/cfitsio_keys_and_unsigned.fits` from the
//! fits-test-cases corpus: header cards and table columns written by
//! cfitsio's own routines (see the fixture's `.md`).
#![cfg(feature = "compat")]

use std::path::PathBuf;

use fitsio_pure::compat::errors::Error;
use fitsio_pure::compat::hdu::HduInfo;
use fitsio_pure::compat::headers::HeaderValue;
use fitsio_pure::compat::tables::ColumnDataType;
use fitsio_pure::compat::FitsFile;

fn fixture() -> Option<PathBuf> {
    let path = std::env::var("FITS_TEST_CASES")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fits-test-cases"))
        .join("cfitsio-writer/cfitsio_keys_and_unsigned.fits");
    path.is_file().then_some(path)
}

/// `<n>:` then `abc…xyz0…9 ` repeated, `n` characters in all, as the
/// fixture's program builds its text.
fn text(n: usize) -> String {
    let pattern = "abcdefghijklmnopqrstuvwxyz0123456789 ";
    let mut s = format!("{n}:");
    s.extend(pattern.chars().cycle().take(n - s.len()));
    s
}

fn status(err: Error) -> i32 {
    match err {
        Error::Fits(e) => e.status,
        other => panic!("expected a cfitsio status, got {other:?}"),
    }
}

#[test]
fn keys_read_as_cfitsio_wrote_them() {
    let Some(path) = fixture() else { return };
    let f = FitsFile::open(&path).unwrap();
    let hdu = f.primary_hdu().unwrap();
    let string = |key: &str| hdu.read_key::<HeaderValue<String>>(&f, key).unwrap();

    for (key, want) in [
        ("INTKEY", "1458492896"),
        ("EXPKEY", "1.5E+08"),
        ("FIXKEY", "154.24"),
        ("DBLKEY", "215680000."),
        ("FLTKEY", "0.5"),
        ("LOGKEY", "T"),
        ("EMPTYSTR", ""),
        ("LONGSTRN", "OGIP 1.0"),
        ("SHORTSTR", "fits on one card"),
    ] {
        assert_eq!(string(key).value, want, "{key}");
    }
    assert_eq!(
        status(hdu.read_key::<String>(&f, "NULLKEY").unwrap_err()),
        204
    );

    for (key, n, comment) in [
        ("STR67", 67, Some("67 characters")),
        ("STR68", 68, Some("68 characters")),
        ("STR67NC", 67, None),
        ("STR68NC", 68, None),
        ("STR69NC", 69, None),
        ("STR200", 200, Some("200 characters")),
        ("ESO OBS LONGSTR", 200, Some("HIERARCH long string")),
        (
            "HIERARCH ESO OBS LONGSTR",
            200,
            Some("HIERARCH long string"),
        ),
    ] {
        let read = string(key);
        assert_eq!(read.value, text(n), "{key}");
        assert_eq!(read.comment.as_deref(), comment, "{key}");
    }
    let quotes = string("QUOTES").value;
    assert!(quotes.starts_with("it's a value with 'quotes'") && quotes.ends_with("card's text"));

    assert_eq!(hdu.read_key::<i64>(&f, "STARTTIME").unwrap(), 1458492896);
    assert_eq!(hdu.read_key::<i64>(&f, "ESO DET NDIT").unwrap(), 42);
    assert_eq!(string("ESO OBS NAME").value, "short hierarch string");
}

#[test]
fn long_string_and_hierarch_cards_are_written_as_cfitsio_wrote_them() {
    let Some(path) = fixture() else { return };
    let quotes = "it's a value with 'quotes' placed so that one doubled quote falls near the split point of the card's text";
    let mut f = FitsFile::create_in_memory().unwrap();
    let hdu = f.primary_hdu().unwrap();
    hdu.write_key(&mut f, "STR69NC", text(69)).unwrap();
    hdu.write_key(&mut f, "STR200", (text(200), "200 characters"))
        .unwrap();
    hdu.write_key(&mut f, "QUOTES", (quotes, "embedded quotes"))
        .unwrap();
    hdu.write_key(
        &mut f,
        "STARTTIME",
        (1458492896i64, "9-character name, written as HIERARCH"),
    )
    .unwrap();
    hdu.write_key(
        &mut f,
        "HIERARCH ESO DET NDIT",
        (42i64, "explicit HIERARCH"),
    )
    .unwrap();
    hdu.write_key(
        &mut f,
        "HIERARCH ESO OBS NAME",
        ("short hierarch string", "HIERARCH string"),
    )
    .unwrap();
    hdu.write_key(
        &mut f,
        "HIERARCH ESO OBS LONGSTR",
        (text(200), "HIERARCH long string"),
    )
    .unwrap();
    let cards = |bytes: &[u8]| -> Vec<String> {
        bytes
            .chunks(80)
            .take_while(|c| !c.starts_with(b"END "))
            .map(|c| String::from_utf8_lossy(c).into_owned())
            .collect()
    };
    let ours: Vec<String> = cards(&f.into_bytes().unwrap()).split_off(3);
    let theirs = cards(&std::fs::read(&path).unwrap());
    let first = theirs
        .iter()
        .position(|c| c.starts_with("STR69NC "))
        .unwrap();
    assert_eq!(ours, theirs[first..first + ours.len()]);
    assert!(theirs[first + ours.len()].starts_with("COMMENT 10:"));
}

#[test]
fn unsigned_and_bit_columns_read_as_cfitsio_wrote_them() {
    let Some(path) = fixture() else { return };
    let f = FitsFile::open(&path).unwrap();
    let hdu = f.hdu("UNSIGNED").unwrap();
    let HduInfo::TableInfo {
        column_descriptions,
        num_rows,
    } = &hdu.info
    else {
        panic!("not a table")
    };
    assert_eq!(*num_rows, 5);
    let types: Vec<_> = column_descriptions
        .iter()
        .map(|c| c.data_type.typ)
        .collect();
    use ColumnDataType::*;
    assert_eq!(types, [Short, Int, LongLong, Byte, Short, Bit, Bit]);

    assert_eq!(
        hdu.read_col::<u16>(&f, "U16").unwrap(),
        [0, 1, 32767, 32768, 65535]
    );
    assert_eq!(
        hdu.read_col::<u32>(&f, "U32").unwrap(),
        [0, 1, 2147483647, 2147483648, 4294967295]
    );
    assert_eq!(
        hdu.read_col::<u64>(&f, "U64").unwrap(),
        [0, 1, (1 << 63) - 1, 1 << 63, u64::MAX]
    );
    assert_eq!(hdu.read_col::<i8>(&f, "I8").unwrap(), [-128, -1, 0, 1, 127]);
    assert_eq!(
        hdu.read_col::<u16>(&f, "U16VEC").unwrap(),
        [0, 32768, 65535, 1, 2, 3, 65534, 32767, 4, 5, 6, 7, 100, 200, 300]
    );
    let mut flag = [false; 5];
    for (row, flag) in flag.iter_mut().enumerate() {
        let mut bit = [false];
        hdu.read_col_bit(&f, "FLAG", row, 0, &mut bit).unwrap();
        *flag = bit[0];
    }
    assert_eq!(flag, [false, true, false, true, true]);
    for row in 0..5 {
        let mut bits = [false; 11];
        hdu.read_col_bit(&f, "BITS", row, 0, &mut bits).unwrap();
        let want: Vec<bool> = (0..11).map(|b| ((row + 1) * (b + 3)) % 3 == 0).collect();
        assert_eq!(bits.to_vec(), want, "row {row}");
    }
    assert_eq!(hdu.read_col::<u8>(&f, "BITS").unwrap()[2..4], [0x92, 0x40]);
}
