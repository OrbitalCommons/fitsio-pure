# FITSWebQL

**Repository:** [jvo203/FITSWebQL](https://github.com/jvo203/FITSWebQL)
**Category:** High-performance FITS web viewer
**FITS Centrality:** High — FITS reading is the core function

## What It Does

High-performance web-based FITS image viewer (Supercomputer Edition). Reads large FITS files, applies RBF neural network compression via OpenCL, and serves tiles to a web client.

## FITS Operations Used

### Read-Only
- `FitsFile::open()`
- `hdu(0)` access
- `read_key::<T>()` — BITPIX, NAXIS, NAXIS1, NAXIS2, NAXIS3
- **`read_section::<Vec<f32>>()`** — tiled section reads for large images (256x256 tiles)
- `pretty_print()` — HDU info display

### Constraints
- Float32 only (BITPIX must equal -32)
- Supports 2D and 3D images (via NAXIS3)
- NaN/finite value validation

## Data Types
- `f32` exclusively (BITPIX -32)
- Header values parsed as String then converted to integers

## fitsio-pure Readiness Assessment

### What Works Today
- File open
- Header read
- Section reads for f32

### Gaps — Must Fix
- **Tiled section reads for large images:** FITSWebQL reads 256x256 tiles from potentially very large FITS files. Our implementation loads the entire file into memory first. For multi-GB FITS cubes, this is impractical. Need streaming/seek-based section reads.

### Gaps — Nice to Have
- `pretty_print()` for HDU info display

### Verdict
**Not ready for production use.** The in-memory loading model is the fundamental issue. FITSWebQL processes large FITS files (potentially GB+) and reads sections on demand. Our architecture loads everything into memory upfront. This would work for small files but fails for the large-file use case that FITSWebQL is specifically designed for.
