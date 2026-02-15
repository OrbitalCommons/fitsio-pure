# Validation Report: fitsio-pure

This report documents the validation of `fitsio-pure` against real-world astronomical FITS files.

## Summary of Results

| File | Status | Findings |
|------|--------|----------|
| `test0.fits` | PASS | Metadata and multi-extension image structure correctly parsed. |
| `tb.fits` | PASS | Binary table structure correctly parsed. |
| `NICMOSn4hk12010_mos.fits` | PASS | Handled 5 image extensions (SCI, ERR, DQ, SAMP, TIME) correctly. |
| `IUElwp25637mxlo.fits` | PASS | Binary table metadata for IUE spectrum correctly extracted. |
| `WFPC2u5780205r_c0fx.fits` | PASS | 3D data cube primary HDU and associated table correctly parsed. |
| `comp.fits` | PARTIAL | HDU identified as BINTABLE (correct at low level), but lacks high-level tiled compression support. |

## Identified Gaps and Issues

### 1. Lack of Tiled Image Compression Support
Files using FITS Tiled Image Compression (e.g., `comp.fits`) are stored as Binary Tables with specific keywords (`ZIMAGE`, `ZCMPTYPE`).
- **Current Behavior:** `fitsio-pure` identifies these as standard `BINTABLE` extensions.
- **Expected Behavior:** Ideally, the library should provide a way to transparently decompress these into `ImageData`.

### 2. Variable Length Array (VLA) Parsing Error
Binary tables using `P` or `Q` descriptors (e.g., `TFORM1 = '1PB'`) cause parsing errors in `read_binary_column`.
- **Root Cause:** `parse_tform_binary` in `bintable.rs` does not recognize the `P` or `Q` prefix and fails when trying to parse the repeat count.
- **Impact:** Any binary table with variable-length columns cannot be read.

### 3. BSCALE/BZERO in Tools
While the core library supports BSCALE/BZERO via `read_image_physical`, the `fitsinfo` tool does not display whether these keywords are present or what the physical range of the data is.

## Recommendations for Improvement

1. **Update TFORM Parser:** Modify `parse_tform_binary` to support `P` and `Q` descriptors.
2. **VLA Data Support:** Implement reading from the heap area (already partially implemented via `pcount` and `THEAP`) for VLAs.
3. **Compression Awareness:** Add a check for `ZIMAGE = T` in HDU parsing to flag compressed images.
4. **Enhance CLI Tools:** Update `fitsinfo` to display more detailed column information for tables and calibration info for images.
