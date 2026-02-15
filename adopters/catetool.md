# catetool

**Repository:** [GreatAttractor/catetool](https://github.com/GreatAttractor/catetool)
**Category:** Eclipse image alignment tool
**FITS Centrality:** High — FITS is the primary image format

## What It Does

Image alignment for the Continental-America Telescopic Eclipse Experiment (CATE). Reads eclipse images, performs alignment, and writes corrected outputs.

## FITS Operations Used

### Dual API Approach
Uses both high-level `fitsio` and low-level `fitsio_sys`:

**High-level (fitsio):**
- `FitsFile::open()`, `FitsFile::create()` with `.with_custom_primary()`, `.overwrite()`
- `hdu.read_image()`, `hdu.write_image()`, `hdu.write_section()`
- `ImageDescription` with `ImageType::Float`

**Low-level (fitsio_sys):**
- Direct FFI for metadata reading without loading pixel data
- Raw keyword extraction from headers
- Constants: `TFLOAT`, `FLOAT_IMG` (-32)
- Row-order handling ("FITS rows are stored in reverse order")

## Data Types
- `f32` (ImageType::Float / BITPIX -32) exclusively
- 2D images only (validated at parse time)
- String metadata from headers

## fitsio-pure Readiness Assessment

### What Works Today
- File open/create with overwrite
- Image read/write for Float type
- Section write
- Header read

### Gaps — Must Fix
- **Metadata-only reads:** catetool reads header keywords without loading pixel data via low-level FFI. Our implementation always parses headers when opening a file (which is actually better), so this should work — but need to expose a way to read headers without triggering image decode.
- **fitsio_sys compatibility:** The low-level code path uses raw FFI. This won't work with our pure Rust implementation. catetool would need to drop their `fits2.rs` (low-level path) and use only the high-level API.

### Verdict
**Mostly ready.** The high-level API path works. The low-level FFI path is incompatible but could be replaced — catetool already has both implementations. They'd just use the high-level one exclusively.
