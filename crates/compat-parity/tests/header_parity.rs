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
