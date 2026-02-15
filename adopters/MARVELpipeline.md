# MARVELpipeline

**Repository:** [IvS-KULeuven/MARVELpipeline](https://github.com/IvS-KULeuven/MARVELpipeline)
**Category:** Spectroscopy data reduction pipeline
**FITS Centrality:** Core — FITS is the data format for the entire pipeline

## What It Does

Data processing and radial velocity pipeline for the MARVEL spectrograph. Multi-stage pipeline: bias subtraction → flat fielding → order extraction → etalon peak fitting → cosmic ray removal.

## FITS Operations Used

### Read Path
- `FitsFile::open()`, `primary_hdu()`, `hdu(N)`
- `read_image()` — 2D/3D arrays (u32, i32, f64)
- `read_key()` — STD_DARK and other calibration metadata

### Write Path
- `FitsFile::create()` with `ImageDescription`
- `write_image()` — processed images
- `ImageType::UnsignedShort`, `ImageType::Double`

### Table Operations
- `ColumnDescription` with `ColumnDataType::{Float, String, Double}`
- Table creation for fit parameter results

### ndarray
- Uses `fitsio` "array" feature — `ArrayD<T>` integration

## Data Types
- `u32`, `i32`, `f64` image arrays
- Table columns: Float, String, Double
- ndarray `ArrayD<T>` throughout

## fitsio-pure Readiness Assessment

### What Works Today
- File open/create
- Image read/write for all types
- Header read/write
- Table creation and column writing

### Gaps — Resolved
- ~~**ndarray integration:**~~ Shipped via the `array` feature.

### Gaps — Needs Verification
- **u32 image type:** FITS doesn't have native u32 — it's represented via BZERO on BITPIX=32. Need to verify our unsigned integer round-trip works for u32.

### Verdict
**Ready today.** Pipeline operations are straightforward (read image → process → write image). The `array` feature provides ndarray integration. Only remaining risk is u32 BZERO round-trip edge case.
