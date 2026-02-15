# serialimage

**Repository:** [sunipkm/serialimage](https://github.com/sunipkm/serialimage)
**Category:** Serializable image format with FITS support
**FITS Centrality:** High — central feature (optional via `fitsio` flag)

## What It Does

Serialization for DynamicImage with metadata. FITS is a primary output format for astronomical camera data, handling multi-channel images with camera telemetry headers.

## FITS Operations Used

### File Operations
- `FitsFile::create()` with `with_custom_primary()`
- Multi-HDU creation via `create_image()` for RGB channel separation

### Image Operations
- `write_image()` for channel data (R, G, B, Alpha as separate HDUs)
- Pixel types: u8, u16, f32

### Header Operations
- `write_key()` for camera telemetry: camera_name, timestamp, temperature, exposure (microseconds), ROI, binning, gain, offset, min/max gain
- Custom extended metadata support

### Compression
- Supports compression via FITS file extension syntax (same as refimage)

## Data Types
- `u8`, `u16`, `f32` pixel data
- String and numeric metadata
- Multi-channel layouts (separate HDUs per channel)

## fitsio-pure Readiness Assessment

### What Works Today
- File creation with custom primary
- Image write for u8, u16, f32
- Header write for all types
- Multi-HDU creation

### Gaps — Must Fix
- **Compression support** (same blocker as refimage)

### Gaps — Nice to Have
- FITS extension syntax for compression selection (e.g., `file.fits[compress]`)

### Verdict
**Mostly ready, blocked by compression.** The core write path is simple and well-covered by our compat layer. If users can accept uncompressed output, this is an easy win. The sunipkm ecosystem (serialimage → cameraunit → cameraunit_asi/fli) would all migrate together.
