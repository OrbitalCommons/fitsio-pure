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

#[test]
fn hdu_lookup_by_name_ignores_case_like_cfitsio() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("names.fits");
    {
        let mut f = CFits::create(&path).open().unwrap();
        let col = CColDesc::new("VAL")
            .with_type(CColType::Double)
            .create()
            .unwrap();
        f.create_table("Events", &[col]).unwrap();
    }
    let lookups = ["Events", "EVENTS", "events", "Event", "Alias"];
    let agree = |path: &Path| {
        let mut c = CFits::open(path).unwrap();
        let p = PureFits::open(path).unwrap();
        for name in lookups {
            assert_eq!(p.hdu(name).is_ok(), c.hdu(name).is_ok(), "lookup of {name}");
        }
        assert!(p.hdu("events").is_ok());
    };
    agree(&path);

    // write_key appends a second EXTNAME card; both libraries honor only the first.
    {
        let mut f = CFits::edit(&path).unwrap();
        let hdu = f.hdu("Events").unwrap();
        hdu.write_key(&mut f, "EXTNAME", "Alias").unwrap();
    }
    agree(&path);
}

/// Writing past the last row extends the table, in fitsio-pure as in cfitsio.
/// Starting from cfitsio's empty table, the same writes through each library
/// must leave tables cfitsio reads identically.
#[test]
fn write_col_extends_the_table_like_cfitsio() {
    let dir = tempfile::tempdir().unwrap();
    let create = |path: &Path| {
        let mut f = CFits::create(path).open().unwrap();
        let a = CColDesc::new("A")
            .with_type(CColType::Int)
            .create()
            .unwrap();
        let b = CColDesc::new("B")
            .with_type(CColType::Double)
            .create()
            .unwrap();
        f.create_table("DATA", &[a, b]).unwrap();
    };
    let a = [1i32, 2, 3, 4, 5];
    let b = [0.5f64, 1.5, 2.5];

    let by_cfitsio = dir.path().join("cfitsio.fits");
    create(&by_cfitsio);
    {
        let mut f = CFits::edit(&by_cfitsio).unwrap();
        let hdu = f.hdu("DATA").unwrap();
        hdu.write_col(&mut f, "A", &a).unwrap();
        hdu.write_col(&mut f, "B", &b).unwrap();
    }

    let by_pure = dir.path().join("pure.fits");
    create(&by_pure);
    {
        let mut f = PureFits::edit(&by_pure).unwrap();
        let hdu = f.hdu("DATA").unwrap();
        hdu.write_col(&mut f, "A", &a).unwrap();
        hdu.write_col(&mut f, "B", &b).unwrap();
        f.flush().unwrap();
    }

    let read = |path: &Path| {
        let mut f = CFits::open(path).unwrap();
        let hdu = f.hdu("DATA").unwrap();
        let rows = match &hdu.info {
            fitsio::hdu::HduInfo::TableInfo { num_rows, .. } => *num_rows,
            other => panic!("not a table: {other:?}"),
        };
        let a: Vec<i32> = hdu.read_col(&mut f, "A").unwrap();
        let b: Vec<f64> = hdu.read_col(&mut f, "B").unwrap();
        (rows, a, b)
    };
    let expected = read(&by_cfitsio);
    assert_eq!(expected, (5, a.to_vec(), vec![0.5, 1.5, 2.5, 0.0, 0.0]));
    assert_eq!(read(&by_pure), expected);
}

/// String cells end at the first NUL, and lose trailing blanks when they have
/// none, as cfitsio's `fits_read_col_str` returns them, for whole-column and
/// single-cell reads. Only NUL padding is compared: `fitsio` deletes every
/// NUL from what cfitsio returns, so bytes after a NUL reappear through it.
#[test]
fn string_cells_end_at_the_first_nul_like_cfitsio() {
    let rows: [&[u8; 8]; 4] = [
        b"RRI\0\0\0\0\0",
        b"AB \0\0\0\0\0",
        b" lead\0  ",
        b"noNULs  ",
    ];
    let mut bytes = Vec::new();
    let card = |text: &str| format!("{text:<80}").into_bytes();
    for text in [
        "SIMPLE  =                    T",
        "BITPIX  =                    8",
        "NAXIS   =                    0",
        "EXTEND  =                    T",
        "END",
    ] {
        bytes.extend(card(text));
    }
    bytes.resize(2880, b' ');
    for text in [
        "XTENSION= 'BINTABLE'",
        "BITPIX  =                    8",
        "NAXIS   =                    2",
        "NAXIS1  =                    8",
        "NAXIS2  =                    4",
        "PCOUNT  =                    0",
        "GCOUNT  =                    1",
        "TFIELDS =                    1",
        "TTYPE1  = 'NAME    '",
        "TFORM1  = '8A      '",
        "EXTNAME = 'DATA    '",
        "END",
    ] {
        bytes.extend(card(text));
    }
    bytes.resize(2 * 2880, b' ');
    for row in rows {
        bytes.extend_from_slice(row);
    }
    bytes.resize(3 * 2880, 0);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nul.fits");
    std::fs::write(&path, bytes).unwrap();

    let mut c = CFits::open(&path).unwrap();
    let c_hdu = c.hdu(1).unwrap();
    let expected: Vec<String> = c_hdu.read_col(&mut c, "NAME").unwrap();
    let p = PureFits::open(&path).unwrap();
    let p_hdu = p.hdu(1).unwrap();
    let actual: Vec<String> = p_hdu.read_col(&p, "NAME").unwrap();
    assert_eq!(actual, expected);
    for row in 0..rows.len() {
        let c_cell: String = c_hdu.read_cell_value(&mut c, "NAME", row).unwrap();
        let p_cell: String = p_hdu.read_cell_value(&p, "NAME", row).unwrap();
        assert_eq!(p_cell, c_cell, "row {row}");
    }
}

/// A column with exactly the name given is found as `fitsio` finds it.
/// Any other name picks a column as cfitsio's `ffgcno` with `CASEINSEN`
/// picks it: ignoring case, with `*`, `?` and `#` wildcards and a column
/// number as a fallback, and with its status when nothing or more than one
/// column matches.
#[test]
fn column_lookup_matches_fitsio_then_ffgcno() {
    let names = [
        "Corrections",
        "TILE1",
        "TILE22",
        "AB*DE",
        "flag",
        "FLAG",
        "FLAGS",
        "X 2",
        "Y",
    ];
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("names.fits");
    {
        let mut f = CFits::create(&path).open().unwrap();
        let cols: Vec<_> = names
            .iter()
            .map(|n| CColDesc::new(*n).with_type(CColType::Int).create().unwrap())
            .collect();
        f.create_table("DATA", &cols).unwrap();
    }
    {
        // `fitsio` writes through `ffgcno`, which can't tell `flag` from
        // `FLAG`, so write each column's index through compat.
        let mut f = PureFits::edit(&path).unwrap();
        let hdu = f.hdu(1).unwrap();
        for (i, name) in names.iter().enumerate() {
            <i32 as WritesCol>::write_col(&mut f, &hdu, name, &[i as i32]).unwrap();
        }
    }
    let templates = [
        "Corrections",
        "corrections",
        "CORRECTIONS ",
        " corrections",
        "Corr*ions",
        "*s",
        "tile#",
        "TILE?",
        "TILE??",
        "TILE*",
        "t*2",
        "T*1",
        "AB*DE",
        "ab*",
        "a?*e",
        "flag",
        "FLAG",
        "Flag",
        "flag*",
        "FLAG?",
        "x 2",
        "X*",
        "*",
        "?",
        "y",
        "3",
        "9",
        "0",
        "10",
        "missing",
        "",
    ];
    let mut c = CFits::open(&path).unwrap();
    let c_hdu = c.hdu(1).unwrap();
    let p = PureFits::open(&path).unwrap();
    let p_hdu = p.hdu(1).unwrap();
    for template in templates {
        let expected = match c_hdu.read_col::<i32>(&mut c, template) {
            Ok(values) => Ok(values),
            Err(_) => {
                let (mut status, mut colnum) = (0, 0);
                let name = std::ffi::CString::new(template).unwrap();
                unsafe {
                    fitsio::sys::ffgcno(
                        c.as_raw(),
                        0,
                        name.as_ptr() as *mut _,
                        &mut colnum,
                        &mut status,
                    );
                }
                match status {
                    0 => Ok(vec![colnum - 1]),
                    status => Err(status),
                }
            }
        };
        let actual = <i32 as ReadsCol>::read_col(&p, &p_hdu, template).map_err(|e| match e {
            fitsio_pure::compat::errors::Error::Fits(e) => e.status,
            other => panic!("{template:?}: {other:?}"),
        });
        assert_eq!(actual, expected, "{template:?}");
        if let Ok(values) = &actual {
            let cell: i32 = p_hdu.read_cell_value(&p, template, 0).unwrap();
            assert_eq!(cell, values[0], "{template:?}");
        }
    }
}

/// Every `.fits` file under `dir`, recursively, smaller than 20 MB.
fn fits_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            fits_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "fits")
            && path.metadata().unwrap().len() < 20 << 20
        {
            out.push(path);
        }
    }
}

/// `HduInfo::TableInfo` describes every corpus binary table as `fitsio` does.
/// `fitsio` panics on complex and variable-length columns, so tables with
/// those are skipped.
#[test]
fn table_info_matches_fitsio_on_the_corpus() {
    use fitsio::hdu::HduInfo as CInfo;
    use fitsio_pure::compat::hdu::HduInfo as PInfo;

    let mut files = Vec::new();
    fits_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fits-test-cases"),
        &mut files,
    );
    files.sort();
    let mut tables = 0;
    for path in files {
        let Ok(p) = PureFits::open(&path) else {
            continue;
        };
        let Ok(mut c) = CFits::open(&path) else {
            continue;
        };
        for i in 0..p.num_hdus().unwrap() {
            let Ok(p_hdu) = p.hdu(i) else { continue };
            let PInfo::TableInfo {
                column_descriptions,
                num_rows,
            } = &p_hdu.info
            else {
                continue;
            };
            let is_binary =
                p_hdu.read_key::<String>(&p, "XTENSION").ok().as_deref() == Some("BINTABLE");
            let tforms: Vec<String> = (1..=column_descriptions.len())
                .map(|n| p_hdu.read_key::<String>(&p, &format!("TFORM{n}")).unwrap())
                .collect();
            if !is_binary || tforms.iter().any(|t| t.contains(['C', 'M', 'P', 'Q'])) {
                continue;
            }
            let c_hdu = c.hdu(i).unwrap();
            let CInfo::TableInfo {
                column_descriptions: c_columns,
                num_rows: c_rows,
            } = &c_hdu.info
            else {
                panic!("{} HDU {i}: fitsio sees no table", path.display());
            };
            assert_eq!(num_rows, c_rows, "{} HDU {i}", path.display());
            assert_eq!(
                column_descriptions.len(),
                c_columns.len(),
                "{} HDU {i}",
                path.display()
            );
            for (p_col, c_col) in column_descriptions.iter().zip(c_columns) {
                let what = format!("{} HDU {i} column {:?}", path.display(), c_col.name);
                assert_eq!(p_col.name, c_col.name, "{what}");
                assert_eq!(p_col.data_type.repeat, c_col.data_type.repeat, "{what}");
                assert_eq!(p_col.data_type.width, c_col.data_type.width, "{what}");
                assert_eq!(
                    String::from(p_col.data_type.typ),
                    String::from(c_col.data_type.typ),
                    "{what}"
                );
            }
            tables += 1;
        }
    }
    assert!(tables >= 15, "only {tables} corpus tables compared");
}

/// Tables `create_table` makes from the same descriptions as `fitsio` have
/// the same columns, and each library reads the other's.
#[test]
fn create_table_matches_fitsio() {
    use fitsio_pure::compat::tables::{ColumnDataType as PColType, ColumnDescription as PColDesc};
    let types = [
        (CColType::Logical, PColType::Logical),
        (CColType::Bit, PColType::Bit),
        (CColType::Byte, PColType::Byte),
        (CColType::SignedByte, PColType::SignedByte),
        (CColType::Short, PColType::Short),
        (CColType::UnsignedShort, PColType::UnsignedShort),
        (CColType::Int, PColType::Int),
        (CColType::Long, PColType::Long),
        (CColType::UnsignedLong, PColType::UnsignedLong),
        (CColType::Float, PColType::Float),
        (CColType::Text, PColType::Text),
        (CColType::Double, PColType::Double),
        (CColType::LongLong, PColType::LongLong),
        (CColType::UnsignedLongLong, PColType::UnsignedLongLong),
        (CColType::String, PColType::String),
    ];
    let dir = tempfile::tempdir().unwrap();
    let c_path = dir.path().join("c.fits");
    let p_path = dir.path().join("p.fits");
    {
        let mut f = CFits::create(&c_path).open().unwrap();
        let cols: Vec<_> = types
            .iter()
            .enumerate()
            .map(|(i, (t, _))| {
                CColDesc::new(format!("C{i}"))
                    .with_type(*t)
                    .that_repeats(3)
                    .create()
                    .unwrap()
            })
            .collect();
        f.create_table("ALL".to_string(), &cols).unwrap();
    }
    {
        let mut f = PureFits::create(&p_path).open().unwrap();
        let cols: Vec<_> = types
            .iter()
            .enumerate()
            .map(|(i, (_, t))| {
                PColDesc::new(format!("C{i}"))
                    .with_type(*t)
                    .that_repeats(3)
                    .create()
                    .unwrap()
            })
            .collect();
        f.create_table("ALL".to_string(), &cols).unwrap();
    }
    // The same TFORMn and TZEROn, read by cfitsio.
    for path in [&c_path, &p_path] {
        let mut c = CFits::open(path).unwrap();
        let hdu = c.hdu("ALL").unwrap();
        let mut keys = Vec::new();
        for n in 1..=types.len() {
            let tform: String = hdu.read_key(&mut c, &format!("TFORM{n}")).unwrap();
            let tzero: Option<f64> = hdu.read_key(&mut c, &format!("TZERO{n}")).ok();
            keys.push((tform, tzero));
        }
        let mut c2 = CFits::open(&c_path).unwrap();
        let hdu2 = c2.hdu("ALL").unwrap();
        for (n, (tform, tzero)) in keys.into_iter().enumerate() {
            let want_tform: String = hdu2.read_key(&mut c2, &format!("TFORM{}", n + 1)).unwrap();
            let want_tzero: Option<f64> = hdu2.read_key(&mut c2, &format!("TZERO{}", n + 1)).ok();
            assert_eq!(
                (tform, tzero),
                (want_tform, want_tzero),
                "{} column {n}",
                path.display()
            );
        }
    }
    // And both libraries describe both files alike.
    for path in [&c_path, &p_path] {
        let mut c = CFits::open(path).unwrap();
        let p = PureFits::open(path).unwrap();
        let fitsio::hdu::HduInfo::TableInfo {
            column_descriptions: c_cols,
            ..
        } = c.hdu("ALL").unwrap().info
        else {
            panic!("not a table");
        };
        let fitsio_pure::compat::hdu::HduInfo::TableInfo {
            column_descriptions: p_cols,
            ..
        } = p.hdu("ALL").unwrap().info
        else {
            panic!("not a table");
        };
        let c_cols: Vec<_> = c_cols
            .iter()
            .map(|c| {
                (
                    c.name.clone(),
                    c.data_type.repeat,
                    String::from(c.data_type.typ),
                )
            })
            .collect();
        let p_cols: Vec<_> = p_cols
            .iter()
            .map(|c| {
                (
                    c.name.clone(),
                    c.data_type.repeat,
                    String::from(c.data_type.typ),
                )
            })
            .collect();
        assert_eq!(p_cols, c_cols, "{}", path.display());
    }
}

/// The raw data unit of HDU 1 of `path`: `NAXIS1 * NAXIS2` bytes.
fn table_bytes(path: &Path) -> Vec<u8> {
    let mut c = CFits::open(path).unwrap();
    let hdu = c.hdu(1).unwrap();
    let naxis1: i64 = hdu.read_key(&mut c, "NAXIS1").unwrap();
    let naxis2: i64 = hdu.read_key(&mut c, "NAXIS2").unwrap();
    let p = PureFits::open(path).unwrap();
    let data = p.data().unwrap();
    let start = p.parsed().unwrap().hdus[1].data_start;
    data[start..start + (naxis1 * naxis2) as usize].to_vec()
}

/// Bit columns written and read as cfitsio's `ffpclk` (numbers to `1X`),
/// `ffpclx` (bits, at a row and bit offset, past the end of a row and of
/// the table), `ffgcx` (bits) and `ffgcvb` (packed bytes) handle them.
#[test]
fn bit_columns_match_cfitsio() {
    let pattern: Vec<bool> = (0..40).map(|i| i % 3 == 0).collect();
    let dir = tempfile::tempdir().unwrap();
    let make = |path: &Path| {
        let mut f = CFits::create(path).open().unwrap();
        let cols = [
            CColDesc::new("F1")
                .with_type(CColType::Bit)
                .create()
                .unwrap(),
            CColDesc::new("F12")
                .with_type(CColType::Bit)
                .that_repeats(12)
                .create()
                .unwrap(),
        ];
        f.create_table("T", &cols).unwrap();
    };
    let c_path = dir.path().join("c.fits");
    let p_path = dir.path().join("p.fits");
    make(&c_path);
    make(&p_path);
    {
        let mut c = CFits::edit(&c_path).unwrap();
        let hdu = c.hdu(1).unwrap();
        hdu.write_col(&mut c, "F1", &[1i32, 0, 3, 255]).unwrap();
        let mut bits: Vec<std::os::raw::c_char> = pattern.iter().map(|&b| b as _).collect();
        let mut status = 0;
        unsafe {
            fitsio::sys::ffpclx(c.as_raw(), 2, 2, 1, 12, bits.as_mut_ptr(), &mut status);
            fitsio::sys::ffpclx(c.as_raw(), 2, 6, 5, 20, bits.as_mut_ptr(), &mut status);
        }
        assert_eq!(status, 0);
    }
    {
        let mut p = PureFits::edit(&p_path).unwrap();
        let hdu = p.hdu(1).unwrap();
        hdu.write_col(&mut p, "F1", &[1i32, 0, 3, 255]).unwrap();
        hdu.write_col_bit(&mut p, "F12", 1, 0, &pattern[..12])
            .unwrap();
        hdu.write_col_bit(&mut p, "F12", 5, 4, &pattern[..20])
            .unwrap();
    }
    assert_eq!(table_bytes(&p_path), table_bytes(&c_path));

    // Reads, from both files, as cfitsio reads them.
    let mut c = CFits::open(&c_path).unwrap();
    let c_hdu = c.hdu(1).unwrap();
    // `fitsio`'s `read_col` reads one element per row, so read the 7 rows'
    // 14 packed bytes with `ffgcvb`, as hyperdrive does.
    let mut c_bytes = vec![0u8; 14];
    let mut status = 0;
    unsafe {
        fitsio::sys::ffgcvb(
            c.as_raw(),
            2,
            1,
            1,
            14,
            0,
            c_bytes.as_mut_ptr(),
            &mut 0,
            &mut status,
        );
    }
    assert_eq!(status, 0);
    let c_numbers: Vec<i32> = c_hdu.read_col(&mut c, "F1").unwrap();
    for (first_row, first_bit, n) in [(0, 0, 1), (5, 4, 20), (1, 0, 24), (0, 13, 3), (6, 0, 16)] {
        let mut c_bits: Vec<std::os::raw::c_char> = vec![0; n];
        let mut status = 0;
        unsafe {
            fitsio::sys::ffgcx(
                c.as_raw(),
                2,
                first_row as i64 + 1,
                first_bit as i64 + 1,
                n as i64,
                c_bits.as_mut_ptr(),
                &mut status,
            );
        }
        assert_eq!(status, 0);
        let want: Vec<bool> = c_bits.iter().map(|&b| b != 0).collect();
        for path in [&c_path, &p_path] {
            let p = PureFits::open(path).unwrap();
            let p_hdu = p.hdu(1).unwrap();
            let mut got = vec![false; n];
            p_hdu
                .read_col_bit(&p, "F12", first_row, first_bit, &mut got)
                .unwrap();
            assert_eq!(
                got,
                want,
                "{} row {first_row} bit {first_bit} n {n}",
                path.display()
            );
            let bytes: Vec<u8> = p_hdu.read_col(&p, "F12").unwrap();
            assert_eq!(bytes, c_bytes);
            let numbers: Vec<i32> = p_hdu.read_col(&p, "F1").unwrap();
            assert_eq!(numbers, c_numbers);
        }
    }

    // A first bit past the row's two bytes is BAD_ELEM_NUM in both.
    let p = PureFits::open(&p_path).unwrap();
    let mut one = [false];
    let err = p
        .hdu(1)
        .unwrap()
        .read_col_bit(&p, "F12", 0, 16, &mut one)
        .unwrap_err();
    match err {
        fitsio_pure::compat::errors::Error::Fits(e) => assert_eq!(e.status, 308),
        other => panic!("{other:?}"),
    }
}

/// Rows appended one call at a time, as birli appends mwaf flags, give a
/// file byte-identical to cfitsio's: `ffpclx` for a bit column, and
/// `write_col_range` (`ffpcl`) for numeric columns.
#[test]
fn row_by_row_appends_match_cfitsio() {
    const ROWS: usize = 10_000;
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base.fits");
    {
        let mut f = CFits::create(&base).open().unwrap();
        let flags = CColDesc::new("FLAGS")
            .with_type(CColType::Bit)
            .that_repeats(128)
            .create()
            .unwrap();
        f.create_table("FLAGS", &[flags]).unwrap();
        let cols = [
            CColDesc::new("ID")
                .with_type(CColType::Int)
                .create()
                .unwrap(),
            CColDesc::new("XYZ")
                .with_type(CColType::Double)
                .that_repeats(3)
                .create()
                .unwrap(),
        ];
        f.create_table("ROWS", &cols).unwrap();
    }
    let row_bits = |row: usize| -> Vec<bool> { (0..128).map(|b| (row * 7 + b) % 5 == 0).collect() };
    let c_path = dir.path().join("c.fits");
    let p_path = dir.path().join("p.fits");
    std::fs::copy(&base, &c_path).unwrap();
    std::fs::copy(&base, &p_path).unwrap();
    {
        let mut c = CFits::edit(&c_path).unwrap();
        let rows_hdu = c.hdu("ROWS").unwrap();
        for row in 0..500 {
            rows_hdu
                .write_col_range(&mut c, "ID", &[row as i32], &(row..row + 1))
                .unwrap();
            let xyz = [row as f64, 0.5, -(row as f64)];
            let mut status = 0;
            unsafe {
                fitsio::sys::ffpcld(
                    c.as_raw(),
                    2,
                    row as i64 + 1,
                    1,
                    3,
                    xyz.as_ptr() as *mut _,
                    &mut status,
                );
            }
            assert_eq!(status, 0);
        }
        let _flags = c.hdu("FLAGS").unwrap();
        for row in 0..ROWS {
            let mut bits: Vec<std::os::raw::c_char> =
                row_bits(row).iter().map(|&b| b as _).collect();
            let mut status = 0;
            unsafe {
                fitsio::sys::ffpclx(
                    c.as_raw(),
                    1,
                    row as i64 + 1,
                    1,
                    128,
                    bits.as_mut_ptr(),
                    &mut status,
                );
            }
            assert_eq!(status, 0);
        }
    }
    let started = std::time::Instant::now();
    {
        let mut p = PureFits::edit(&p_path).unwrap();
        let rows_hdu = p.hdu("ROWS").unwrap();
        for row in 0..500 {
            rows_hdu
                .write_col_range(&mut p, "ID", &[row as i32], &(row..row + 1))
                .unwrap();
            let xyz = [row as f64, 0.5, -(row as f64)];
            rows_hdu
                .write_col_range(&mut p, "XYZ", &xyz, &(row..row + 3))
                .unwrap();
        }
        let flags = p.hdu("FLAGS").unwrap();
        for row in 0..ROWS {
            flags
                .write_col_bit(&mut p, "FLAGS", row, 0, &row_bits(row))
                .unwrap();
        }
    }
    eprintln!("compat appends took {:?}", started.elapsed());
    let c_bytes = std::fs::read(&c_path).unwrap();
    let p_bytes = std::fs::read(&p_path).unwrap();
    assert_eq!(p_bytes.len(), c_bytes.len());
    assert!(p_bytes == c_bytes, "files differ");
}
