//! Compatibility layer mirroring the [`fitsio`](https://crates.io/crates/fitsio) crate API.
#![allow(missing_docs)]

/// Error types for the compat layer.
pub mod errors;
/// FITS file open/create/save operations.
pub mod fitsfile;
/// HDU handle and metadata queries.
pub mod hdu;
/// Header keyword read/write traits.
pub mod headers;
/// Image pixel read/write traits and types.
pub mod images;
/// ndarray integration (requires the `array` feature).
#[cfg(feature = "array")]
pub mod ndarray_compat;
/// Table column read/write traits and types.
pub mod tables;

/// Crate-root re-exports matching `fitsio`'s, so `use fitsio::FitsFile;` ports as
/// `use fitsio_pure::compat::FitsFile;`.
pub use fitsfile::{FileOpenMode, FitsFile};
pub use headers::HeaderValue;

#[cfg(test)]
mod tests {
    #[test]
    fn reexports_core() {
        assert_eq!(crate::BLOCK_SIZE, 2880);
    }

    #[test]
    fn fitsio_root_reexports() {
        use crate::compat::images::{ImageDescription, ImageType};
        use crate::compat::{FileOpenMode, FitsFile, HeaderValue};

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("reexports.fits");
        {
            let mut f = FitsFile::create(&path).open().unwrap();
            let desc = ImageDescription {
                data_type: ImageType::Short,
                dimensions: vec![2, 3],
            };
            let hdu = f.create_image("SCI", &desc).unwrap();
            hdu.write_key(&mut f, "EXPTIME", &42i64).unwrap();
        }

        let f = FitsFile::open(&path).unwrap();
        assert_eq!(f.mode(), FileOpenMode::ReadOnly);
        let hdu = f.hdu("SCI").unwrap();
        let exptime = HeaderValue {
            value: hdu.read_key::<i64>(&f, "EXPTIME").unwrap(),
            comment: None,
        };
        assert_eq!(exptime.value, 42);
    }
}
