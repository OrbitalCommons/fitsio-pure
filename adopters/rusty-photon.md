# rusty-photon

**Repository:** [ivonnyssen/rusty-photon](https://github.com/ivonnyssen/rusty-photon)
**Category:** PHD2 astrophotography guider integration
**FITS Centrality:** Medium — FITS is the output format for guide star images

## What It Does

Astrophotography guider that captures guide star images from PHD2 (via base64 encoding) and writes them to FITS files for archival.

## FITS Operations Used

### Write-Only
- `FitsFile::create()` with `ImageDescription`
- `primary_hdu()` access
- `hdu.write_image()` — u16 pixel data from decoded base64
- `hdu.write_key()` — TELESCOP, OBSERVER, and other metadata
- File pre-removal (fitsio requires clean slate)

### Type Handling
- `ImageType::UnsignedShort`
- Base64-decoded u16 data (little-endian conversion)
- Dimensions in FITS column-major order `[width, height]`

## Data Types
- `u16` (16-bit unsigned guide star images)
- String metadata (telescope, observer)
- Numeric metadata

## fitsio-pure Readiness Assessment

### What Works Today
- File create
- Image write for UnsignedShort
- Header write
- Primary HDU access

### Gaps — Must Fix
- **File overwrite behavior:** rusty-photon manually removes existing files before `FitsFile::create()` because cfitsio errors on existing files. Our implementation may handle this differently — need to verify overwrite semantics.

### Verdict
**Ready today.** Simple write-only use case: decode base64 → write u16 image → add headers. Our compat layer covers this. Another easy adoption target.
