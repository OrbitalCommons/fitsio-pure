# mwalib

**Repository:** [MWATelescope/mwalib](https://github.com/MWATelescope/mwalib)
**Category:** Radio telescope data library
**FITS Centrality:** Core — the library's entire purpose is parsing MWA FITS files

## What It Does

Library for reading MWA radio telescope metadata (metafits) and visibility data (gpubox FITS files). Provides MetafitsContext, CorrelatorContext, and VoltageContext for different MWA data products.

## FITS Operations Used

### File Operations
- `FitsFile::open()` — read-only
- Custom macros: `fits_open!()`, `fits_open_hdu!()`, `fits_open_hdu_by_name!()`

### Header Operations
- `hdu.read_key::<T>()` with polymorphic types (i32, i64, f64, String)
- **Long string handling** via custom macros (`get_optional_fits_key_long_string!()`)
- Required vs optional key distinction with graceful missing-key handling

### Image Operations
- `get_fits_float_image_into_buffer!()` — reads float images into pre-allocated buffers
- `get_hdu_image_size!()` — queries NAXIS dimensions
- Used for gpubox visibility data

### Table Operations
- `hdu.read_col::<T>()` — reads table columns (metadata tables)
- Column types: f32, f64, i32, i64, String

## Data Types
- `f32` (image pixels, visibility data)
- `f64` (frequencies, calibration values)
- `i32`, `i64` (timestamps, observation IDs)
- `String` (metadata, coordinate frames)

## fitsio-pure Readiness Assessment

### What Works Today
- File open
- Header read for standard types
- Image read for all BITPIX types
- Table column reads for scalar types
- HDU access by index and name

### Gaps — Must Fix
- **Long string support:** Same as hyperdrive — CONTINUE card mechanism needed.
- **Read into pre-allocated buffer:** mwalib reads images into caller-provided buffers for zero-copy performance. Our `read_image()` always allocates a new Vec.
- **Image size query without reading data:** mwalib queries NAXIS dimensions separately from reading pixel data.

### Gaps — Nice to Have
- Error type that carries FITS filename and source location
- Multi-version format detection based on header keywords

### Verdict
**Close but blocked by long strings.** The core read operations are well-covered, but the long string gap and the buffer-reuse pattern are real issues for a performance-sensitive library like mwalib.
