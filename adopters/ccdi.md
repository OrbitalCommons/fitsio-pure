# ccdi

**Repository:** [viktorchvatal/ccdi](https://github.com/viktorchvatal/ccdi)
**Category:** CCD camera imaging service (Moravian Instruments)
**FITS Centrality:** Medium-High — FITS is the save/process pipeline format

## What It Does

CCD imaging software with a web-based GUI for Moravian Instruments cameras. Captures frames, saves to FITS, and provides image enhancement tools.

## FITS Operations Used

### Write Path
- `FitsFile::create()` with `.overwrite()`
- `hdu.write_image()` — raw camera frames
- `hdu.write_key()` — DATE-OBS (ISO8601), EXPTIME

### Read Path
- `FitsFile::open()`
- `hdu.read_section()` — multi-channel reads for RGB separation
- Channel layout: `[channels, height, width]` dimension order

## Data Types
- `ImageType::UnsignedShort` (raw camera data)
- `ImageType::Float` (processed images)
- `f32` arrays for channel data
- Timestamp and exposure metadata

## fitsio-pure Readiness Assessment

### What Works Today
- File create with overwrite
- Image write for UnsignedShort and Float
- Header write
- Section reads

### Gaps — Must Fix
- **Multi-channel section reads:** Same concern as electra_stacking — need to verify `read_section()` correctly handles 3D `[channels, height, width]` data.

### Verdict
**Likely ready.** Simple write path with basic read for processing. The 3D section read is the only risk area. Good candidate for adoption after verifying channel extraction works correctly.
