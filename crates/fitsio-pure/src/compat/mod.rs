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

#[cfg(test)]
mod tests {
    #[test]
    fn reexports_core() {
        assert_eq!(crate::BLOCK_SIZE, 2880);
    }
}
