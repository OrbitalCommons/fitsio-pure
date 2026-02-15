# Marlu

**Repository:** [MWATelescope/Marlu](https://github.com/MWATelescope/Marlu)
**Category:** Radio astronomy coordinate transforms and I/O
**FITS Centrality:** Low — FITS is optional, gated behind `cfitsio` feature

## What It Does

Coordinate transformations, Jones matrices, and I/O utilities for MWA. FITS support is specifically for UVFITS output — one of several output formats (also supports Measurement Set).

## FITS Operations Used

### Unusual: Almost Entirely Low-Level FFI
Marlu bypasses the high-level fitsio API and uses `fitsio_sys` directly:
- `ffinit()` — create files
- `ffclos()` — close files
- `ffmahd()` — move to HDU
- `ffphpr()` — write group header (UVFITS random groups format)
- `ffpgpe()` — write group parameters as floats
- `ffcrtb()` — create binary table
- `ffpcls()`, `ffpcld()`, `ffpcli()` — write typed columns

### Header Operations (via custom helpers)
- WCS keywords: CTYPE, CRVAL, CRPIX, CDELT for RA/DEC/FREQ axes
- Observation metadata: OBSRA, OBSDEC, TELESCOP, INSTRUME, DATE-OBS
- STOKES axis handling (polarization)

### Table Operations
- Creates antenna binary table extension
- Column formats: 8A (strings), 3D (double arrays), 1J (integers), 1E (floats)

## Data Types
- `f32` (visibility data, group parameters)
- `f64` (frequencies, coordinates, WCS values)
- `i32`, `i64` (metadata, dimensions)
- `String` (antenna names)

## fitsio-pure Readiness Assessment

### What Works Today
- Basic binary table creation and column writing
- Header writing for standard types
- Image writing

### Gaps — Must Fix
- **UVFITS random groups format:** Marlu writes UVFITS using the random groups convention (`GROUPS=T`, `PCOUNT>0`). This is a specialized FITS format where each "row" is a group with parameters + data. fitsio-pure does not support random groups HDUs.
- **Low-level FFI parity:** Marlu uses raw fitsio_sys calls. Our compat layer would need to either support these same functions or provide high-level equivalents.

### Gaps — Nice to Have
- WCS keyword helpers (CTYPE/CRVAL/CRPIX/CDELT pattern)
- STOKES axis encoding

### Verdict
**Not ready.** Random groups UVFITS support is a specialized format not in our roadmap. Marlu would need either random groups support or a rewrite of its UVFITS writer. Since FITS support is optional in Marlu, this is low priority — they could keep using cfitsio for this one feature.
