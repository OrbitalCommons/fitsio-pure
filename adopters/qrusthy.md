# qrusthy

**Repository:** [emaadparacha/qrusthy](https://github.com/emaadparacha/qrusthy)
**Category:** Camera SDK wrapper (QHY cameras)
**FITS Centrality:** Medium — FITS is the output format for captured frames

## What It Does

Rust wrapper for the QHYCCD SDK for QHY cameras. Captures single frames and saves to FITS.

## FITS Operations Used

- `FitsFile::create()` with `with_custom_primary()`
- `primary_hdu()` access
- `write_image()` — u16 image data from camera buffer
- `write_key()` — exposure_time, temperature, gain, offset metadata

## Data Types
- `u16` (16-bit unsigned from camera sensor)
- 2D images `[height, width]`
- Numeric and string header metadata

## fitsio-pure Readiness Assessment

### What Works Today
- File creation with custom primary HDU
- u16 image write (via BZERO/BSCALE encoding)
- Header write for all types
- Primary HDU access

### Gaps
- None significant. This is a minimal, straightforward use case.

### Verdict
**Ready today.** Simplest adopter on the list. Single-frame u16 image write with basic headers. Our compat layer covers everything qrusthy needs. Good candidate for a first adoption success story.
