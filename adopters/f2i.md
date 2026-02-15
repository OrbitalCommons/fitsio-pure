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

### Gaps
- None remaining. The `array` feature provides `ReadImage for ArrayD<T>`.

### Verdict
**Ready today.** The `array` feature provides `ArrayD<f32>` return types matching fitsio's API.
