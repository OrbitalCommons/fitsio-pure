# Star_Tracker_Microcontroller

**Repository:** [TomCreusot/Star_Tracker_Microcontroller](https://github.com/TomCreusot/Star_Tracker_Microcontroller)
**Category:** Satellite star tracking system
**FITS Centrality:** Medium — reads astrometry.net output tables

## What It Does

Star tracker for spacecraft pointing direction identification. Uses astrometry.net for plate solving and reads FITS table results for star position correlation.

## FITS Operations Used

### Table Operations Only (no image I/O)
- `FitsFile::open()`
- `fits.hdu(1)` — secondary HDU access (binary table)
- `hdu.read_col()` — reads named columns: field_x, field_y, index_x, index_y, field_ra, field_dec, index_ra, index_dec, FLUX

## Data Types
- `f64` column data (celestial coordinates and pixel positions)
- No image types — purely tabular

## fitsio-pure Readiness Assessment

### What Works Today
- File open
- HDU access by index
- Column reads for f64

### Gaps
- None identified. This is a minimal table-read use case.

### Verdict
**Ready today.** Reads f64 columns from a binary table extension. Our compat layer covers this completely. Another good candidate for a first adoption alongside qrusthy.
