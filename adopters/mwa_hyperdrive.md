# mwa_hyperdrive

**Repository:** [MWATelescope/mwa_hyperdrive](https://github.com/MWATelescope/mwa_hyperdrive)
**Category:** Radio telescope calibration pipeline
**FITS Centrality:** Core — FITS is the primary data format for all I/O

## What It Does

Calibration software for the Murchison Widefield Array (MWA) radio telescope. Reads UV-FITS visibility data and MWA metafits files, performs calibration, and writes calibration solutions and source lists back to FITS.

## FITS Operations Used

### File Operations
- `FitsFile::open()`, `FitsFile::create()`
- HDU access by index and name

### Header Operations
- `read_key()` / `write_key()` for typed metadata
- **Long string handling** via FFI (`ffgkls`, `ffpkls`, `ffplsw`) for CONTINUE cards
- Comment writing via `ffpcom()`

### Image Operations
- `read_image()` / `write_image()` for calibration solution arrays
- `ImageDescription` with `ImageType::Double`
- Multi-dimensional arrays (1D through 4D)

### Table Operations
- `read_col()` / `write_col()` for catalog data
- `read_cell_value()` for individual cells
- `create_table()` with `ColumnDescription`
- **Array-in-cell columns** (32-element f64 dipole gains) via FFI (`ffgcno`, `ffgcv`)
- Column types: String, Double, Int, Short

### Low-Level FFI
- Direct `fitsio_sys` calls for features the high-level API can't handle
- Column lookup (`ffgcno`), memory management (`fffree`), long strings

## Data Types
- `f64` (primary: coordinates, fluxes, calibration solutions, timestamps)
- `f32` (image pixel data)
- `i32`, `i64`, `i16` (indices, flags)
- `String` (source names, tile names)
- Complex numbers (Jones matrices stored as 8 floats per 2x2 matrix)

## Tables Written
- COMPONENTS (source catalogs with point/Gaussian/shapelet properties)
- SHAPELETS (coefficient data)
- TIMEBLOCKS, CHANBLOCKS (observation metadata)
- TILES (antenna config with array-in-cell dipole data)
- AIPS AN (UVFITS antenna table)
- SOLUTIONS (4D calibration arrays)

## fitsio-pure Readiness Assessment

### What Works Today
- Basic file open/create
- Header read/write for standard types
- Image read/write for all BITPIX types
- Table column read/write for standard scalar types
- HDU access by index and name

### Gaps — Must Fix
- **Long string support (CONTINUE cards):** Not implemented. Hyperdrive stores BEAMFILE, MODELLER, CMDLINE as long strings. This is a hard blocker.
- **Array-in-cell columns:** 32-element arrays packed into single table cells. Currently requires FFI fallback in fitsio. Our binary table support would need repeat-count column handling.
- **Column range reads:** Hyperdrive reads specific row ranges from columns. Our compat layer reads full columns only.

### Gaps — Nice to Have
- HDU enumeration with graceful "not found" handling (status 301)
- Error context with file path + HDU number + source location

### Verdict
**Not ready.** Long string support and array-in-cell columns are hard blockers. This is the most demanding consumer on the list — getting hyperdrive working would validate the library for nearly all other use cases.
