# electra_stacking

**Repository:** [art-den/electra_stacking](https://github.com/art-den/electra_stacking)
**Category:** Astrophotography image stacking
**FITS Centrality:** Very High — FITS is a primary image format alongside RAW and TIFF

## What It Does

Software for stacking astronomical deep sky images. Reads light, dark, flat, and bias frames in FITS format, performs alignment and stacking, and writes results.

## FITS Operations Used

### Read Path
- `FitsFile::open()`, `primary_hdu()`
- `hdu.read_image()` — full image read
- `hdu.read_section()` — partial reads for multi-channel data
- `hdu.read_key()` — metadata: EXPTIME, GAIN, BAYERPAT, INSTRUME, CAMERA, FOCALLEN, FOCRATIO, TELESCOP, DATE-LOC, DATE-OBS, BLKLEVEL

### Write Path
- `FitsFile::create()` with `with_custom_primary()`, `.overwrite()`
- `hdu.write_image()` — writes processed images
- `hdu.write_region()` — region writes

### Image Types
- Full BITPIX coverage: UnsignedByte, Byte, Short, UnsignedShort, Long, UnsignedLong, LongLong, Float, Double
- 2D grayscale `[height, width]` and 3D RGB `[3, height, width]`

## Data Types
- `f32` (normalized processed data)
- All integer types for raw camera data
- String/numeric metadata for camera settings

## fitsio-pure Readiness Assessment

### What Works Today
- File open/create with overwrite
- Full image read/write for all BITPIX types
- Header read/write
- Section reads
- Region writes

### Gaps — Must Fix
- **`read_section()` for multi-channel extraction:** electra uses section reads to extract individual channels from 3D images. Need to verify our `read_section()` handles 3D data correctly.
- **Unsigned integer types (UnsignedShort, UnsignedLong):** FITS represents these via BZERO/BSCALE. Our implementation handles calibration but needs testing against electra's expectations.

### Gaps — Nice to Have
- Windows-specific CWD workarounds (electra has cfitsio-specific hacks for Windows paths)

### Verdict
**Likely ready.** This is a straightforward image I/O use case covering the full BITPIX range. The main risk is edge cases in unsigned integer handling and 3D section reads. A compatibility test against electra's test images would confirm readiness.
