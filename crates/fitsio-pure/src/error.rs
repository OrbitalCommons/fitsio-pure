/// All errors that can occur during FITS I/O operations.
#[derive(Debug)]
pub enum Error {
    /// Malformed FITS header block.
    InvalidHeader,
    /// Premature end of data while reading.
    UnexpectedEof,
    /// Unrecognized BITPIX value.
    InvalidBitpix(i64),
    /// Malformed keyword name in a header card.
    InvalidKeyword,
    /// Unknown or unsupported XTENSION type.
    UnsupportedExtension,
    /// A header value could not be parsed correctly.
    InvalidValue,
    /// A required keyword was not found in the header.
    MissingKeyword(&'static str),
    /// An I/O error from the standard library.
    #[cfg(feature = "std")]
    Io(std::io::Error),
}

/// Convenience alias used throughout the crate.
pub type Result<T> = core::result::Result<T, Error>;

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::InvalidHeader => write!(f, "invalid FITS header"),
            Error::UnexpectedEof => write!(f, "unexpected end of file"),
            Error::InvalidBitpix(v) => write!(f, "invalid BITPIX value: {v}"),
            Error::InvalidKeyword => write!(f, "invalid keyword name"),
            Error::UnsupportedExtension => write!(f, "unsupported XTENSION type"),
            Error::InvalidValue => write!(f, "invalid header value"),
            Error::MissingKeyword(kw) => write!(f, "missing required keyword: {kw}"),
            #[cfg(feature = "std")]
            Error::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

#[cfg(feature = "std")]
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_invalid_header() {
        let e = Error::InvalidHeader;
        assert_eq!(e.to_string(), "invalid FITS header");
    }

    #[test]
    fn display_unexpected_eof() {
        let e = Error::UnexpectedEof;
        assert_eq!(e.to_string(), "unexpected end of file");
    }

    #[test]
    fn display_invalid_bitpix() {
        let e = Error::InvalidBitpix(-99);
        assert_eq!(e.to_string(), "invalid BITPIX value: -99");
    }

    #[test]
    fn display_invalid_keyword() {
        let e = Error::InvalidKeyword;
        assert_eq!(e.to_string(), "invalid keyword name");
    }

    #[test]
    fn display_unsupported_extension() {
        let e = Error::UnsupportedExtension;
        assert_eq!(e.to_string(), "unsupported XTENSION type");
    }

    #[test]
    fn display_invalid_value() {
        let e = Error::InvalidValue;
        assert_eq!(e.to_string(), "invalid header value");
    }

    #[test]
    fn display_missing_keyword() {
        let e = Error::MissingKeyword("NAXIS");
        assert_eq!(e.to_string(), "missing required keyword: NAXIS");
    }

    #[cfg(feature = "std")]
    #[test]
    fn display_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let e = Error::Io(io_err);
        assert_eq!(e.to_string(), "I/O error: file not found");
    }

    #[cfg(feature = "std")]
    #[test]
    fn io_error_from_conversion() {
        let io_err = std::io::Error::other("oops");
        let e: Error = io_err.into();
        assert!(matches!(e, Error::Io(_)));
    }

    #[test]
    fn result_type_alias() {
        let ok: Result<u32> = Ok(42);
        assert!(ok.is_ok());

        let err: Result<u32> = Err(Error::InvalidHeader);
        assert!(err.is_err());
    }

    #[test]
    fn debug_formatting() {
        let e = Error::InvalidBitpix(99);
        let debug = format!("{e:?}");
        assert!(debug.contains("InvalidBitpix"));
        assert!(debug.contains("99"));
    }

    #[cfg(feature = "std")]
    #[test]
    fn std_error_source() {
        use std::error::Error as StdError;

        let e = Error::InvalidHeader;
        assert!(e.source().is_none());

        let io_err = std::io::Error::other("inner");
        let e = Error::Io(io_err);
        assert!(e.source().is_some());
    }
}
