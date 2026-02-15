# twinkle

**Repository:** [twinkle-astronomy/twinkle](https://github.com/twinkle-astronomy/twinkle)
**Category:** Observatory management and analysis
**FITS Centrality:** High — FITS is the primary image format across multiple modules

## What It Does

Astronomical observatory management software with web-based UI. Handles image capture, calibration (flat, dark, bias), focus analysis, and collimation measurement.

## FITS Operations Used

### Read Path
- `FitsFile::open()`
- `hdu.read_image()` with type conversion across all ImageTypes
- `hdu.read_key()` — FRAME, OFFSET, GAIN, EXPTIME
- ndarray conversion: `ArrayD<u16>`

### Frame Classification
- Calibration frame handling: Flat, Dark, Bias, DarkFlat
- Statistics computation and storage

## Data Types
- Full ImageType enum coverage (Byte through Double)
- `u16` (preferred for astronomical CCD data)
- Calibration metadata (frame type, gain, offset, exposure)

## fitsio-pure Readiness Assessment

### What Works Today
- File open
- Image read for all BITPIX types
- Header read
- ImageType enum matching

### Gaps — Must Fix
- **ndarray integration:** Uses `ArrayD<u16>` — same gap as f2i, eventide, fitsrotate_rs.

### Verdict
**Nearly ready.** Read-only image + metadata use case. The ndarray gap is the only issue. Same story as the other astrophotography tools.
