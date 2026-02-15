# xisf

**Repository:** [wrenby/xisf](https://github.com/wrenby/xisf)
**Category:** XISF format reader with FITS conversion
**FITS Centrality:** Medium — FITS is a conversion target (XISF → FITS)

## What It Does

Reader for the XISF (eXtensible Image Serialization Format) astronomy image format. Includes an example that converts XISF files to FITS.

## FITS Operations Used

### Write-Only (in conversion example)
- `FitsFile::create()` with `with_custom_primary()`
- `create_image()` — extended HDU creation for subsequent images
- `write_image()` — pixel data from XISF
- `write_key()` — BZERO for unsigned integer handling, EXTNAME for naming

### Data Type Mapping
- u8 → UnsignedByte
- u16 → UnsignedShort
- u32 → UnsignedLong
- u64 → LongLong (with BZERO offset)
- f32 → Float
- f64 → Double

### Dimension Handling
- Handles 3D arrays by trimming singleton leading dimensions
- Primary HDU for first image, extensions for subsequent

## Data Types
- Full range: u8, u16, u32, u64, f32, f64
- BZERO offset for unsigned types (especially u64)
- Multi-dimensional with degenerate axis trimming

## fitsio-pure Readiness Assessment

### What Works Today
- File create with custom primary
- Image write for standard types
- Header write (including BZERO)
- Extended HDU creation
- EXTNAME support

### Gaps — Must Fix
- **u64 with BZERO:** FITS has no native 64-bit unsigned type. xisf uses BITPIX=64 (i64) with BZERO=9223372036854775808 to represent u64. Need to verify our BSCALE/BZERO handling works for this extreme offset.
- **u32 via UnsignedLong:** Similar BZERO encoding for 32-bit unsigned. Less exotic but still needs verification.

### Verdict
**Likely ready.** The core write path is straightforward. The main risk is BZERO handling for unsigned types at the extremes (u64). If our BSCALE/BZERO calibration handles the full i64→u64 offset correctly, this works. Worth a targeted test.
