# eventide

**Repository:** [asierzapata/eventide](https://github.com/asierzapata/eventide)
**Category:** Desktop astrophotography image processing
**FITS Centrality:** High — FITS is the primary image format

## What It Does

Desktop application for astrophotography image processing. Loads FITS images, classifies frames by type (Light, Dark, Flat, Bias), computes statistics, and processes.

## FITS Operations Used

### Read Path (read-only)
- `FitsFile::open()`
- `hdu.read_image()` with type conversion
- `hdu.read_key()` — EXPTIME, CCD-TEMP, FILTER, FRAME
- Full ImageType enum matching for format detection

### Data Processing
- Batch FITS file loading from directories
- Frame type classification from FRAME header keyword
- Image statistics: min, max, mean, median, std_dev
- `ArrayD<f32>` via ndarray for dynamic-rank arrays

## Data Types
- All ImageType variants for detection
- `f32` as working precision (via ndarray)
- String metadata (frame type, filter name)
- Numeric metadata (exposure, temperature)

## fitsio-pure Readiness Assessment

### What Works Today
- File open
- Image read for all BITPIX types
- Header read
- ImageType enum matching

### Gaps — Must Fix
- **ndarray integration:** Same as f2i/fitsrotate_rs — needs `ArrayD<f32>` or manual reshape from Vec.

### Gaps — Nice to Have
- Batch directory loading (application-level, not library)

### Verdict
**Nearly ready.** Read-only use case with straightforward image + header access. The ndarray gap is the only issue, and it's a minor integration concern shared with several other adopters.
