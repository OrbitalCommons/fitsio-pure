# refimage

**Repository:** [sunipkm/refimage](https://github.com/sunipkm/refimage)
**Category:** Image data storage with FITS output
**FITS Centrality:** Medium — optional feature behind `fitsio` flag

## What It Does

Image data storage supporting owned data or references, with demosaic support. FITS is one output format, gated on a feature flag.

## FITS Operations Used

### File Operations
- `FitsFile::create()` with `with_custom_primary()`
- `FitsFile::open()` for verification
- Multi-HDU file creation via `create_image()`

### Image Operations
- `write_image()` for pixel data (grayscale, RGB, multi-channel)
- Supports: u8, u16, u32, u64, i8, i16, i32, i64, f32, f64
- 3D arrays: Height x Width x Channels

### Header Operations
- `write_key()` for metadata (numeric and string types)
- Camera metadata: timestamps, color spaces, Bayer patterns

### Compression
- **Uses FITS tile compression:** None, Gzip, Rice, Hcompress, Hscompress, Bzip2, Plio
- Compression is selected via file extension syntax

## Data Types
- Full numeric range: u8 through u64, i8 through i64, f32, f64
- String metadata
- Multi-channel image layouts

## fitsio-pure Readiness Assessment

### What Works Today
- File create with custom primary HDU
- Image write for all standard BITPIX types
- Header write for standard types
- Multi-HDU file creation

### Gaps — Must Fix
- **Tile compression:** refimage supports FITS tile compression (Rice, Gzip, Hcompress, etc.). fitsio-pure has zero compression support. This is the primary blocker.

### Gaps — Nice to Have
- u16, u32, u64 image types via BZERO/BSCALE (FITS natively uses signed integers — unsigned types require offset encoding)

### Verdict
**Blocked by compression.** If refimage users don't need compressed output, the compat layer covers the remaining operations. But compression is a prominent feature of refimage's FITS support, so removing it would be a regression.
