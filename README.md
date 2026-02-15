![CI](https://github.com/OrbitalCommons/fitsio-pure/actions/workflows/ci.yml/badge.svg) ![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg) ![crates.io](https://img.shields.io/crates/v/fitsio-pure.svg) ![docs.rs](https://docs.rs/fitsio-pure/badge.svg)

# fitsio-pure

A pure Rust implementation of the FITS (Flexible Image Transport System) file format for reading and writing astronomical data. No C dependencies.

Includes a `compat` module that mirrors the API of the [`fitsio`](https://github.com/simonrw/rust-fitsio) crate for drop-in replacement.

## Reference Materials

- [FITS Standard 3.0 Specification](https://fits.gsfc.nasa.gov/standard30/fits_standard30aa.pdf) - The official IAU FITS format definition
- [cfitsio](https://github.com/HEASARC/cfitsio) - The canonical C FITS I/O library
- [rust-fitsio](https://github.com/simonrw/rust-fitsio) - Existing Rust FITS bindings (wraps cfitsio); the `compat` module targets this API

## Testing and Validation

This project uses a combination of synthetic round-trip tests and validation against real-world astronomical data.

### Validation Approach
To ensure correctness, we validate `fitsio-pure` against official sample files and `astropy.io.fits`.
1.  **Resources:** See [FITS_RESOURCES.md](FITS_RESOURCES.md) for a list of data sources (NASA, Astropy, LSST).
2.  **Automated Fetching:** Use `scripts/fetch_samples.sh` to download test files.
3.  **Cross-Validation:** Use `scripts/validate_metadata.py` to generate ground-truth metadata from `astropy` for comparison.
4.  **Reporting:** Discovered gaps are documented in [VALIDATION_REPORT.md](VALIDATION_REPORT.md).

For more details on the current status, see the [Validation Report](VALIDATION_REPORT.md).
