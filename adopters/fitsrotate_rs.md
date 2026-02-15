# fitsrotate_rs

**Repository:** [AlecThomson/fitsrotate_rs](https://github.com/AlecThomson/fitsrotate_rs)
**Category:** CLI tool — FITS cube axis rotation
**FITS Centrality:** High — FITS is the entire purpose

## What It Does

Reorders axes of FITS data cubes for performance optimization (e.g., frequency-first ordering for radio astronomy).

## FITS Operations Used

### Read Path
- `FitsFile::open()`, `primary_hdu()`
- `hdu.read_image()` — full image as ndarray
- `hdu.read_key()` — generic typed header reads (String, i64, f64)

### Write Path
- `FitsFile::create()` with `with_custom_primary()`
- `hdu.write_image()` — full image write
- `hdu.write_key()` — selective header copying
- `ImageDescription` with `ImageType::Double`

### Header Operations
- Selective WCS keyword copying: CTYPE*, CRVAL*, CDELT*, CRPIX*, CUNIT*
- Axis permutation reflected in header updates

## Data Types
- `ArrayD<f32>` (ndarray) — multi-dimensional cubes (2D, 3D, nD)
- Header values: String, i64, f64

## fitsio-pure Readiness Assessment

### What Works Today
- File open/create
- Image read/write for all types
- Header read/write
- Custom primary HDU creation

### Gaps — Must Fix
- **ndarray integration:** Same as f2i — needs `ArrayD<T>` return types or the user reshapes from Vec.

### Gaps — Nice to Have
- Nothing else significant.

### Verdict
**Nearly ready.** Full read-write cycle is covered. The ndarray gap is the same as f2i — a minor integration issue, not a fundamental capability gap. Good candidate for early adoption.
