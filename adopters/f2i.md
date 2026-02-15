# f2i

**Repository:** [Fingel/f2i](https://github.com/Fingel/f2i)
**Category:** CLI tool — FITS preview and image conversion
**FITS Centrality:** High — FITS reading is the core function

## What It Does

Terminal FITS image previewer. Reads FITS files, generates thumbnails, and converts to PNG/JPEG for quick inspection.

## FITS Operations Used

- `FitsFile::open()` — read-only
- `FitsFile::iter()` — iterate through HDUs
- `hdu.read_image()` — read full image data
- Uses `fitsio` "array" feature for ndarray integration (`ArrayD<f32>`)

## Data Types
- `f32` images via ndarray
- Standard 2D image data

## fitsio-pure Readiness Assessment

### What Works Today
- File open
- Image read for all BITPIX types
- HDU iteration

### Gaps — Must Fix
- **ndarray integration:** f2i uses the `fitsio` "array" feature which returns `ArrayD<f32>`. Our compat layer returns `Vec<T>`. Users would need to reshape manually, or we'd need to add ndarray support.

### Gaps — Nice to Have
- Nothing else — this is a minimal read-only use case.

### Verdict
**Nearly ready.** The only gap is ndarray return types. If f2i can accept `Vec<f32>` and reshape to ndarray themselves (trivial), this works today. Adding an optional `array` feature to our compat layer would make it seamless.
