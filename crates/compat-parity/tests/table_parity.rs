//! Differential table-column parity between `fitsio-pure`'s compat shims and
//! the canonical cfitsio-backed `fitsio` crate.
//!
//! Each case has cfitsio create a one-column binary table using the column type
//! cfitsio picks for a Rust scalar. For the unsigned and signed-byte types that
//! means signed storage plus a `TZEROn` offset, which is the only way FITS can
//! represent them. Values are then written with one library and read back with
//! the other, in both directions, and must match exactly.

use std::path::{Path, PathBuf};

use fitsio::tables::{ColumnDataType as CColType, ColumnDescription as CColDesc};
use fitsio::FitsFile as CFits;

use fitsio_pure::compat::fitsfile::FitsFile as PureFits;
use fitsio_pure::compat::tables::{ReadsCol, ReadsColRange, WritesCol};

fn create_cfitsio_table(path: &Path, col_type: CColType) {
    let mut f = CFits::create(path).open().unwrap();
    let col = CColDesc::new("VAL").with_type(col_type).create().unwrap();
    f.create_table("DATA", &[col]).unwrap();
}

macro_rules! table_parity_case {
    ($name:ident, $t:ty, $c_ty:expr, [$($v:expr),+ $(,)?], [$($w:expr),+ $(,)?]) => {
        #[test]
        fn $name() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("table.fits");
            create_cfitsio_table(&path, $c_ty);

            // cfitsio writes, fitsio-pure reads.
            let written: Vec<$t> = vec![$($v),+];
            {
                let mut f = CFits::edit(&path).unwrap();
                let hdu = f.hdu("DATA").unwrap();
                hdu.write_col(&mut f, "VAL", &written).unwrap();
            }
            {
                let f = PureFits::open(&path).unwrap();
                let hdu = f.hdu(1usize).unwrap();
                let read: Vec<$t> = <$t as ReadsCol>::read_col(&f, &hdu, "VAL").unwrap();
                assert_eq!(read, written, "pure read_col of cfitsio-written column");
                let range: Vec<$t> =
                    <$t as ReadsColRange>::read_col_range(&f, &hdu, "VAL", 1, written.len() - 1)
                        .unwrap();
                assert_eq!(range, written[1..], "pure read_col_range of cfitsio-written column");
            }

            // fitsio-pure writes, cfitsio reads.
            let rewritten: Vec<$t> = vec![$($w),+];
            {
                let mut f = PureFits::edit(&path).unwrap();
                let hdu = f.hdu(1usize).unwrap();
                <$t as WritesCol>::write_col(&mut f, &hdu, "VAL", &rewritten).unwrap();
                f.flush().unwrap();
            }
            {
                let mut f = CFits::open(&path).unwrap();
                let hdu = f.hdu("DATA").unwrap();
                let read: Vec<$t> = hdu.read_col(&mut f, "VAL").unwrap();
                assert_eq!(read, rewritten, "cfitsio read of pure-written column");
            }
        }
    };
}

table_parity_case!(
    u8_column,
    u8,
    CColType::Byte,
    [0, 1, 127, 128, 255],
    [255, 200, 100, 1, 0]
);
table_parity_case!(
    i8_column,
    i8,
    CColType::SignedByte,
    [-128, -1, 0, 1, 127],
    [127, 64, 0, -64, -128]
);
table_parity_case!(
    i16_column,
    i16,
    CColType::Short,
    [i16::MIN, -1, 0, 1, i16::MAX],
    [i16::MAX, 7, 0, -7, i16::MIN]
);
table_parity_case!(
    u16_column,
    u16,
    CColType::UnsignedShort,
    [0, 1, 32767, 32768, u16::MAX],
    [u16::MAX, 40000, 32768, 2, 0]
);
table_parity_case!(
    i32_column,
    i32,
    CColType::Int,
    [i32::MIN, -1, 0, 1, i32::MAX],
    [i32::MAX, 12345, 0, -12345, i32::MIN]
);
table_parity_case!(
    u32_column,
    u32,
    CColType::UnsignedLong,
    [0, 1, 2147483647, 2147483648, u32::MAX],
    [u32::MAX, 3000000000, 2147483648, 2, 0]
);
table_parity_case!(
    i64_column,
    i64,
    CColType::LongLong,
    [i64::MIN, -1, 0, 1, i64::MAX],
    [i64::MAX, 1 << 40, 0, -(1 << 40), i64::MIN]
);
table_parity_case!(
    u64_column,
    u64,
    CColType::UnsignedLongLong,
    [0, 1, 1 << 63, (1 << 63) + 1, u64::MAX],
    [u64::MAX, (1 << 63) - 1, 1 << 63, 2, 0]
);
table_parity_case!(
    f32_column,
    f32,
    CColType::Float,
    [-1.5, 0.0, 0.25, 3.0e38, f32::MIN_POSITIVE],
    [2.5, -0.125, 0.0, -3.0e38, 1.0]
);
table_parity_case!(
    f64_column,
    f64,
    CColType::Double,
    [-1.5, 0.0, 0.25, 1.0e300, f64::MIN_POSITIVE],
    [2.5, -0.125, 0.0, -1.0e300, 1.0]
);

fn corpus_file(rel: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fits-test-cases")
        .join(rel);
    assert!(path.is_file(), "corpus file missing: {}", path.display());
    path
}

/// Read one column of a corpus file with both libraries and require equality.
fn corpus_read_parity<T>(rel: &str, hdu_index: usize, column: &str)
where
    T: ReadsCol + fitsio::tables::ReadsCol + PartialEq + std::fmt::Debug,
{
    let path = corpus_file(rel);

    let mut c = CFits::open(&path).unwrap();
    let c_hdu = c.hdu(hdu_index).unwrap();
    let expected: Vec<T> = c_hdu.read_col(&mut c, column).unwrap();

    let p = PureFits::open(&path).unwrap();
    let p_hdu = p.hdu(hdu_index).unwrap();
    let actual: Vec<T> = <T as ReadsCol>::read_col(&p, &p_hdu, column).unwrap();

    assert_eq!(actual, expected, "{rel} column {column}");
}

#[test]
fn logical_column_matches_cfitsio() {
    corpus_read_parity::<bool>("rust-fitsio/boolean_columns.fits", 1, "Whitening_Filter");
    corpus_read_parity::<bool>("fitsio-pure-fixtures/tb.fits", 1, "c4");
}

#[test]
fn scaled_float_column_matches_cfitsio() {
    // c3 is TFORM 1E with TSCAL3 = 3 and TZERO3 = 0.4.
    corpus_read_parity::<f32>("fitsio-pure-fixtures/tb.fits", 1, "c3");
    corpus_read_parity::<f64>("fitsio-pure-fixtures/tb.fits", 1, "c3");
}

#[test]
fn integer_column_read_as_wider_types_matches_cfitsio() {
    corpus_read_parity::<i16>("rust-fitsio/boolean_columns.fits", 1, "Tile");
    corpus_read_parity::<i32>("rust-fitsio/boolean_columns.fits", 1, "Tile");
    corpus_read_parity::<i64>("rust-fitsio/boolean_columns.fits", 1, "Tile");
    corpus_read_parity::<f64>("rust-fitsio/boolean_columns.fits", 1, "Tile");
}
