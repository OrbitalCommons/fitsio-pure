# cameraunit / cameraunit_asi / cameraunit_fli

**Repositories:**
- [sunipkm/cameraunit](https://github.com/sunipkm/cameraunit)
- [sunipkm/cameraunit_asi](https://github.com/sunipkm/cameraunit_asi)
- [sunipkm/cameraunit_fli](https://github.com/sunipkm/cameraunit_fli)
**Category:** Astronomical camera interfaces
**FITS Centrality:** Low-Medium — FITS is the output format, delegated to serialimage

## What They Do

Generic camera interface trait (`cameraunit`) with implementations for ZWO ASI cameras (`cameraunit_asi`) and Finger Lakes Instrumentation cameras (`cameraunit_fli`). All use `serialimage` for FITS output.

## FITS Operations Used

Indirect — all FITS I/O is delegated to the `serialimage` crate:
- `DynamicSerialImage::savefits()` — saves captured frames as FITS
- Camera metadata (exposure, gain, temperature, offset) written as FITS headers
- u16 image data from camera sensors

## Data Types
- `u16` (16-bit unsigned — standard for astronomy CCDs)
- String metadata (camera name, timestamp)
- Numeric metadata (exposure time, gain, temperature)

## fitsio-pure Readiness Assessment

### What Works Today
- The actual FITS dependency is through `serialimage` — if serialimage switches, cameraunit follows automatically.

### Gaps
- Same as serialimage (see that analysis) — primarily compression support.

### Verdict
**Transitive dependency.** Fix serialimage/refimage and these come for free. Low individual effort but high ecosystem leverage — the sunipkm camera stack (cameraunit, cameraunit_asi, cameraunit_fli, refimage, serialimage) is ~5 repos that would all switch together.
