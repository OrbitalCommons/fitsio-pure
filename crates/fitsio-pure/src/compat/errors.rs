//! Errors and error handling, as in `fitsio::errors`.
//!
//! [`Error`] has the same variants as `fitsio`'s, so exhaustive matches written
//! against `fitsio` compile, and [`FitsError::status`] carries cfitsio's status
//! code for each condition compat detects, so code that turns
//! [`sys::KEY_NO_EXIST`] into `None` works unchanged.

use std::ffi::{IntoStringError, NulError};
use std::io;
use std::ops::Range;
use std::str::Utf8Error;
use std::string::FromUtf8Error;

use super::sys;

/// All errors that can occur in the compat layer.
#[derive(Debug)]
pub enum Error {
    /// A FITS error, with cfitsio's status code.
    Fits(FitsError),
    /// An invalid index was requested.
    Index(IndexError),
    /// A free-form error message.
    Message(String),
    /// A string held an interior nul byte.
    Null(NulError),
    /// UTF-8 conversion error.
    Utf8(Utf8Error),
    /// A standard I/O error.
    Io(io::Error),
    /// C string conversion error.
    IntoString(IntoStringError),
    /// The file to create already exists.
    ExistingFile(String),
    /// A lock on a shared file was poisoned.
    UnlockError,
    /// A null pointer was given.
    NullPointer,
}

/// A FITS error: cfitsio's status code and its message.
#[derive(Debug, PartialEq, Eq)]
pub struct FitsError {
    /// cfitsio status code; see [`super::sys`].
    pub status: i32,
    /// cfitsio's text for the status, or for errors from the core parser, its
    /// description of the problem.
    pub message: String,
}

impl FitsError {
    /// The error for cfitsio status `status`, with cfitsio's message.
    pub(crate) fn new(status: u32) -> FitsError {
        let status = status as i32;
        FitsError {
            status,
            message: status_message(status).to_string(),
        }
    }
}

/// Error raised when the user requests invalid indexes for data.
#[derive(Debug, PartialEq, Eq)]
pub struct IndexError {
    /// Error message.
    pub message: String,
    /// The range requested.
    pub given: Range<usize>,
}

/// Convenience result type for the compat layer.
pub type Result<T> = std::result::Result<T, Error>;

/// `Ok(())` for status 0, otherwise the [`Error::Fits`] for `status`.
pub fn check_status(status: i32) -> Result<()> {
    match status {
        0 => Ok(()),
        _ => Err(Error::Fits(FitsError {
            status,
            message: status_message(status).to_string(),
        })),
    }
}

impl Error {
    /// The [`Error::Fits`] for cfitsio status `status`.
    pub(crate) fn status(status: u32) -> Error {
        Error::Fits(FitsError::new(status))
    }
}

impl From<FitsError> for Error {
    fn from(e: FitsError) -> Self {
        Error::Fits(e)
    }
}

impl From<IndexError> for Error {
    fn from(e: IndexError) -> Self {
        Error::Index(e)
    }
}

impl From<&str> for Error {
    fn from(e: &str) -> Self {
        Error::Message(e.to_string())
    }
}

impl From<NulError> for Error {
    fn from(e: NulError) -> Self {
        Error::Null(e)
    }
}

impl From<FromUtf8Error> for Error {
    fn from(e: FromUtf8Error) -> Self {
        Error::Utf8(e.utf8_error())
    }
}

impl From<Utf8Error> for Error {
    fn from(e: Utf8Error) -> Self {
        Error::Utf8(e)
    }
}

impl From<Box<dyn std::error::Error>> for Error {
    fn from(e: Box<dyn std::error::Error>) -> Self {
        let message = match e.source() {
            Some(cause) => format!("Error: {e} caused by {cause}"),
            None => format!("Error: {e}"),
        };
        Error::Message(message)
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<IntoStringError> for Error {
    fn from(e: IntoStringError) -> Self {
        Error::IntoString(e)
    }
}

impl<T> From<std::sync::PoisonError<T>> for Error {
    fn from(_: std::sync::PoisonError<T>) -> Self {
        Error::UnlockError
    }
}

/// Errors from the core library become [`Error::Fits`] with the cfitsio
/// status for the same condition, except I/O errors, which stay [`Error::Io`].
impl From<crate::Error> for Error {
    fn from(e: crate::Error) -> Self {
        use crate::Error as E;
        let status = match &e {
            E::Io(_) => {
                let E::Io(io) = e else { unreachable!() };
                return Error::Io(io);
            }
            E::InvalidHeader(_) => sys::UNKNOWN_REC,
            E::UnexpectedEof => sys::END_OF_FILE,
            E::InvalidBitpix(_) => sys::BAD_BITPIX,
            E::InvalidKeyword => sys::BAD_KEYCHAR,
            E::UnsupportedExtension(_) => sys::UNKNOWN_EXT,
            E::InvalidValue => 1,
            E::MissingKeyword(kw) => missing_keyword_status(kw),
            E::UnsupportedCompression(_) | E::DecompressionError(_) => sys::DATA_DECOMPRESSION_ERR,
        };
        Error::Fits(FitsError {
            status: status as i32,
            message: e.to_string(),
        })
    }
}

/// cfitsio's status for a missing structural keyword.
fn missing_keyword_status(keyword: &str) -> u32 {
    match keyword {
        "SIMPLE" => sys::NO_SIMPLE,
        "BITPIX" => sys::NO_BITPIX,
        "NAXIS" => sys::NO_NAXIS,
        "XTENSION" => sys::NO_XTENSION,
        "PCOUNT" => sys::NO_PCOUNT,
        "GCOUNT" => sys::NO_GCOUNT,
        "TFIELDS" => sys::NO_TFIELDS,
        "TBCOLn" => sys::NO_TBCOL,
        "TFORMn" => sys::NO_TFORM,
        kw if kw.starts_with("NAXIS") => sys::NO_NAXES,
        kw if kw.starts_with('Z') => sys::DATA_DECOMPRESSION_ERR,
        _ => sys::KEY_NO_EXIST,
    }
}

/// cfitsio's message for `status`, as `ffgerr` gives it.
fn status_message(status: i32) -> &'static str {
    match status {
        0 => "OK - no error",
        1 => "non-CFITSIO program error",
        101 => "same input and output files",
        103 => "attempt to open too many files",
        104 => "could not open the named file",
        105 => "couldn't create the named file",
        106 => "error writing to FITS file",
        107 => "tried to move past end of file",
        108 => "error reading from FITS file",
        110 => "could not close the file",
        111 => "array dimensions too big",
        112 => "cannot write to readonly file",
        113 => "could not allocate memory",
        114 => "invalid fitsfile pointer",
        115 => "NULL input pointer",
        116 => "error seeking file position",
        117 => "bad value for file download timeout setting",
        121 => "invalid URL prefix",
        122 => "too many I/O drivers",
        123 => "I/O driver init failed",
        124 => "no I/O driver for this URLtype",
        125 => "parse error in input file URL",
        126 => "parse error in range list",
        151 => "bad argument (shared mem drvr)",
        152 => "null ptr arg (shared mem drvr)",
        153 => "no free shared memory handles",
        154 => "share mem drvr not initialized",
        155 => "IPC system error (shared mem)",
        156 => "no memory (shared mem drvr)",
        157 => "share mem resource deadlock",
        158 => "lock file open/create failed",
        159 => "can't resize share mem block",
        201 => "header already has keywords",
        202 => "keyword not found in header",
        203 => "keyword number out of bounds",
        204 => "keyword value is undefined",
        205 => "string missing closing quote",
        206 => "error in indexed keyword name",
        207 => "illegal character in keyword",
        208 => "required keywords out of order",
        209 => "keyword value not positive int",
        210 => "END keyword not found",
        211 => "illegal BITPIX keyword value",
        212 => "illegal NAXIS keyword value",
        213 => "illegal NAXISn keyword value",
        214 => "illegal PCOUNT keyword value",
        215 => "illegal GCOUNT keyword value",
        216 => "illegal TFIELDS keyword value",
        217 => "negative table row size",
        218 => "negative number of rows",
        219 => "named column not found",
        220 => "illegal SIMPLE keyword value",
        221 => "first keyword not SIMPLE",
        222 => "second keyword not BITPIX",
        223 => "third keyword not NAXIS",
        224 => "missing NAXISn keywords",
        225 => "first keyword not XTENSION",
        226 => "CHDU not an ASCII table",
        227 => "CHDU not a binary table",
        228 => "PCOUNT keyword not found",
        229 => "GCOUNT keyword not found",
        230 => "TFIELDS keyword not found",
        231 => "missing TBCOLn keyword",
        232 => "missing TFORMn keyword",
        233 => "CHDU not an IMAGE extension",
        234 => "illegal TBCOLn keyword value",
        235 => "CHDU not a table extension",
        236 => "column exceeds width of table",
        237 => "more than 1 matching col. name",
        241 => "row width not = field widths",
        251 => "unknown FITS extension type",
        252 => "1st key not SIMPLE or XTENSION",
        253 => "END keyword is not blank",
        254 => "Header fill area not blank",
        255 => "Data fill area invalid",
        261 => "illegal TFORM format code",
        262 => "unknown TFORM datatype code",
        263 => "illegal TDIMn keyword value",
        264 => "invalid BINTABLE heap pointer",
        301 => "illegal HDU number",
        302 => "column number < 1 or > tfields",
        304 => "negative byte address",
        306 => "negative number of elements",
        307 => "bad first row number",
        308 => "bad first element number",
        309 => "not an ASCII (A) column",
        310 => "not a logical (L) column",
        311 => "bad ASCII table datatype",
        312 => "bad binary table datatype",
        314 => "null value not defined",
        317 => "not a variable length column",
        320 => "illegal number of dimensions",
        321 => "1st pixel no. > last pixel no.",
        322 => "BSCALE or TSCALn = 0.",
        323 => "illegal axis length < 1",
        340 => "not group table",
        341 => "HDU already member of group",
        342 => "group member not found",
        343 => "group not found",
        344 => "bad group id",
        345 => "too many HDUs tracked",
        346 => "HDU alread tracked",
        347 => "bad Grouping option",
        348 => "identical pointers (groups)",
        360 => "malloc failed in parser",
        361 => "file read error in parser",
        362 => "null pointer arg (parser)",
        363 => "empty line (parser)",
        364 => "cannot unread > 1 line",
        365 => "parser too deeply nested",
        366 => "file open failed (parser)",
        367 => "hit EOF (parser)",
        368 => "bad argument (parser)",
        369 => "unexpected token (parser)",
        401 => "bad int to string conversion",
        402 => "bad float to string conversion",
        403 => "keyword value not integer",
        404 => "keyword value not logical",
        405 => "keyword value not floating pt",
        406 => "keyword value not double",
        407 => "bad string to int conversion",
        408 => "bad string to float conversion",
        409 => "bad string to double convert",
        410 => "illegal datatype code value",
        411 => "illegal no. of decimals",
        412 => "datatype conversion overflow",
        413 => "error compressing image",
        414 => "error uncompressing image",
        420 => "bad date or time conversion",
        431 => "syntax error in expression",
        432 => "expression result wrong type",
        433 => "vector result too large",
        434 => "missing output column",
        435 => "bad data in parsed column",
        436 => "output extension of wrong type",
        501 => "WCS angle too large",
        502 => "bad WCS coordinate",
        503 => "error in WCS calculation",
        504 => "bad WCS projection type",
        505 => "WCS keywords not found",
        // Not a cfitsio status: `fitsio` reports writes to a read-only file
        // with it.
        602 => "cannot alter readonly file",
        _ => "unknown error status",
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Fits(e) => write!(f, "Fits error: {e:?}"),
            Error::Message(s) => write!(f, "Error: {s}"),
            Error::Null(e) => e.fmt(f),
            Error::Utf8(e) => e.fmt(f),
            Error::Index(e) => write!(f, "Error: {e:?}"),
            Error::Io(e) => e.fmt(f),
            Error::IntoString(e) => e.fmt(f),
            Error::ExistingFile(filename) => write!(f, "File {filename} already exists"),
            Error::UnlockError => write!(f, "Invalid concurrent access to fits file"),
            Error::NullPointer => write!(f, "Null pointer specified"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Null(e) => Some(e),
            Error::Utf8(e) => Some(e),
            Error::Io(e) => Some(e),
            Error::IntoString(e) => Some(e),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_errors_get_cfitsio_statuses() {
        let status = |e: crate::Error| match Error::from(e) {
            Error::Fits(e) => e.status,
            other => panic!("expected Fits, got {other:?}"),
        };
        assert_eq!(status(crate::Error::InvalidBitpix(7)), 211);
        assert_eq!(status(crate::Error::MissingKeyword("NAXIS")), 223);
        assert_eq!(status(crate::Error::MissingKeyword("NAXISn")), 224);
        assert_eq!(status(crate::Error::MissingKeyword("TFORMn")), 232);
        assert_eq!(status(crate::Error::UnsupportedExtension("FOO")), 251);
    }

    #[test]
    fn core_io_error_stays_io() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "not found");
        let e: Error = crate::Error::Io(io_err).into();
        assert!(matches!(e, Error::Io(_)));
    }

    #[test]
    fn check_status_matches_fitsio() {
        assert!(check_status(0).is_ok());
        match check_status(202) {
            Err(Error::Fits(e)) => {
                assert_eq!(e.status, sys::KEY_NO_EXIST as i32);
                assert_eq!(e.message, "keyword not found in header");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn display_matches_fitsio() {
        assert_eq!(Error::Message("bad".into()).to_string(), "Error: bad");
        assert_eq!(
            Error::ExistingFile("a.fits".into()).to_string(),
            "File a.fits already exists"
        );
        assert_eq!(
            Error::status(sys::BAD_HDU_NUM).to_string(),
            "Fits error: FitsError { status: 301, message: \"illegal HDU number\" }"
        );
    }

    #[test]
    fn error_source() {
        use std::error::Error as StdError;

        assert!(Error::Message("msg".into()).source().is_none());
        let io_err = io::Error::new(io::ErrorKind::NotFound, "gone");
        assert!(Error::Io(io_err).source().is_some());
    }
}
