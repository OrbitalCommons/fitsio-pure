# FastFitsCutter

**Repository:** [tikk3r/FastFitsCutter](https://github.com/tikk3r/FastFitsCutter)
**Category:** CLI tool — spatial image cutout extraction
**FITS Centrality:** Very High — FITS is the entire purpose

## What It Does

Extracts sub-images (cutouts) from large FITS files using WCS coordinates with optional parallelization. Handles 2D through 4D data cubes.

## FITS Operations Used

### Read Path
- `FitsFile::open()`, `primary_hdu()`
- `hdu.read_key()` — extensive WCS keyword reads (NAXIS*, CDELT*, CRPIX*, CTYPE*, RADESYS, BUNIT, BZERO, LONPOLE, LATPOLE)
- **`hdu.read_region()`** — rectangular sub-region extraction (2D through 4D)
- `FitsFile::edit()` — edit mode for in-place modification

### Write Path
- `FitsFile::create()` with `with_custom_primary()`
- **`hdu.write_region()`** — writes rectangular sub-regions
- `hdu.write_key()` — WCS keyword propagation
- Generic key operations with `ReadsKey`/`WritesKey` trait bounds

### Advanced
- Multi-dimensional region I/O (2D, 3D, 4D)
- Dynamic header card discovery and propagation
- Dual header parsing: fitsio + fitsrs
- Rayon parallel processing

## Data Types
- `Vec<f64>` (image regions)
- `ImageType::Float`
- Header: i64, f64, String with generic trait bounds

## fitsio-pure Readiness Assessment

### What Works Today
- File open/create/edit
- Header read/write
- Full image read/write

### Gaps — Must Fix
- **Region I/O (`read_region` / `write_region`):** Our compat layer has `read_region()` for images but FastFitsCutter uses it with variadic range slices for 2D-4D data. Need to verify our implementation handles the same range specification format.
- **Generic trait bounds:** FastFitsCutter uses `ReadsKey + WritesKey + Default + PartialEq` bounds for generic header copying. Our traits need to match these bounds.

### Gaps — Nice to Have
- WCS integration (FastFitsCutter uses external `wcs` crate, not fitsio for this)
- Parallel region reads with independent file handles per thread

### Verdict
**Probably ready with minor work.** Region I/O is the key feature — if our `read_region()` implementation matches the fitsio API signature (range slices per axis), this could work. Worth a focused compatibility test.
