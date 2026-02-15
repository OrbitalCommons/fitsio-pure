# RapidFits

**Repository:** [chrischtel/RapidFits](https://github.com/chrischtel/RapidFits)
**Category:** Desktop FITS viewer (Tauri/WGPU)
**FITS Centrality:** High — FITS visualization is the core function

## What It Does

Desktop FITS image viewer built with Tauri and WGPU. Loads FITS files, computes statistics, and renders with GPU-accelerated zoom/pan/stretch controls.

## FITS Operations Used

### Read-Only
- `FitsFile::open()`
- `primary_hdu()`
- `hdu.read_image()` — reads as `Vec<f32>`
- `HduInfo::ImageInfo { shape, .. }` — dimension extraction

### Processing
- Image statistics: min, max, mean, stddev, median
- NaN/infinite value filtering
- Percentile-based auto-stretching
- GPU texture upload via WGPU

## Data Types
- `f32` images
- 2D only (validates `shape.len() == 2`)
- Shape metadata as usize array

## fitsio-pure Readiness Assessment

### What Works Today
- File open
- Primary HDU access
- Image read as Vec<f32>
- HduInfo with shape

### Gaps
- None significant. This is a minimal read-only use case.

### Verdict
**Ready today.** Simplest read-only viewer on the list. Opens file, reads primary image as f32, gets dimensions. Fully covered by our compat layer. Great candidate for adoption.
