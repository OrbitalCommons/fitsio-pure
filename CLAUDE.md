# fitsio-pure

A pure Rust FITS read/write library with a compatibility module that mirrors the `fitsio` crate's API.

## Architecture

- Core library: pure Rust FITS reader/writer (no C dependencies)
- `compat` module: mirrors the call signature of the `fitsio` crate for drop-in replacement

## Reference Materials

Reference materials live in `reference/` (gitignored). To populate them:

1. **rust-fitsio** - Existing Rust FITS library (wraps cfitsio). The `compat` module should match this crate's API:
   ```
   git clone https://github.com/simonrw/rust-fitsio reference/rust-fitsio
   ```

2. **cfitsio** - The canonical C FITS I/O library (upstream of rust-fitsio):
   ```
   git clone https://github.com/HEASARC/cfitsio reference/cfitsio
   ```

3. **FITS Standard 3.0 specification** (PDF + grep-friendly text):
   ```
   curl -L -o reference/fits_standard30aa.pdf "https://fits.gsfc.nasa.gov/standard30/fits_standard30aa.pdf"
   pdftotext -layout reference/fits_standard30aa.pdf reference/fits_standard30aa.txt
   ```

These files should never be checked into the repo.
