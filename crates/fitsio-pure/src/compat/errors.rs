/// All errors that can occur in the fitsio-compat crate.
#[derive(Debug)]
pub enum Error {
    /// An error from the fitsio-pure core library.
    Fits(crate::Error),
    /// A standard I/O error.
    Io(std::io::Error),
    /// A free-form error message.
    Message(String),
}

impl From<crate::Error> for Error {
    fn from(e: crate::Error) -> Self {
        Error::Fits(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Fits(e) => write!(f, "FITS error: {e}"),
            Error::Io(e) => write!(f, "I/O error: {e}"),
            Error::Message(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Fits(e) => Some(e),
            Error::Io(e) => Some(e),
            Error::Message(_) => None,
        }
    }
}

/// Convenience result type for the compat crate.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_fits_error() {
        let e: Error = crate::Error::InvalidHeader.into();
        assert!(matches!(e, Error::Fits(crate::Error::InvalidHeader)));
    }

    #[test]
    fn from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "not found");
        let e: Error = io_err.into();
        assert!(matches!(e, Error::Io(_)));
    }

    #[test]
    fn display_fits_error() {
        let e = Error::Fits(crate::Error::InvalidHeader);
        let s = e.to_string();
        assert!(s.contains("FITS error"));
    }

    #[test]
    fn display_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        let e = Error::Io(io_err);
        let s = e.to_string();
        assert!(s.contains("I/O error"));
    }

    #[test]
    fn display_message() {
        let e = Error::Message("something went wrong".into());
        assert_eq!(e.to_string(), "something went wrong");
    }

    #[test]
    fn error_source() {
        use std::error::Error as StdError;

        let e = Error::Message("msg".into());
        assert!(e.source().is_none());

        let e = Error::Fits(crate::Error::InvalidHeader);
        assert!(e.source().is_some());
    }
}
