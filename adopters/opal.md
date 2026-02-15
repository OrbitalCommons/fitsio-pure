# opal

**Repository:** [tgblackburn/opal](https://github.com/tgblackburn/opal)
**Category:** Particle physics simulation (PIC code)
**FITS Centrality:** Low — FITS is output-only for histogram visualization

## What It Does

Parallel, relativistic 1D3V particle-in-cell code. Uses FITS solely to write simulation output histograms for external visualization.

## FITS Operations Used

### Write-Only
- `FitsFile::create()` with `ImageDescription`
- `write_image()` — 2D and 1D histogram data as f64 arrays
- `write_key()` — WCS metadata (CRPIX, CRVAL, CDELT, CNAME, CUNIT, BUNIT, TOTAL, OBJECT, DATAMIN, DATAMAX)

## Data Types
- `f64` arrays (histogram bin counts)
- String metadata (axis labels, units, object names)
- Numeric metadata (WCS coordinates, data range)

## fitsio-pure Readiness Assessment

### What Works Today
- File create
- Image write for f64
- Header write for all types

### Gaps
- None. This is a minimal write-only use case.

### Verdict
**Ready today.** Simplest write-only use case on the list. Creates FITS images with WCS headers — fully covered by our compat layer. Third good candidate for immediate adoption alongside qrusthy and Star_Tracker_Microcontroller.
