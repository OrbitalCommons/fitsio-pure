# fitsio-pure

A pure Rust implementation of the FITS (Flexible Image Transport System) file format for reading and writing astronomical data. No C dependencies.

Includes a `compat` module that mirrors the API of the [`fitsio`](https://github.com/simonrw/rust-fitsio) crate for drop-in replacement.

## Reference Materials

- [FITS Standard 3.0 Specification](https://fits.gsfc.nasa.gov/standard30/fits_standard30aa.pdf) - The official IAU FITS format definition
- [cfitsio](https://github.com/HEASARC/cfitsio) - The canonical C FITS I/O library
- [rust-fitsio](https://github.com/simonrw/rust-fitsio) - Existing Rust FITS bindings (wraps cfitsio); the `compat` module targets this API
