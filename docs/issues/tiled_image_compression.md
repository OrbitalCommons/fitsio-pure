# Issue: Support for FITS Tiled Image Compression

## Overview
FITS Tiled Image Compression (also known as "Compressed Images") is a widely used convention where an image is stored as a FITS Binary Table extension. Each row of the table contains a compressed tile of the image.

Currently, `fitsio-pure` correctly identifies these extensions as `BINTABLE` (following the low-level structure), but it does not provide high-level support for identifying them as images or decompressing the data.

## Details
- **Identifying Keywords:** Compressed images are identified by `XTENSION = 'BINTABLE'` and `ZIMAGE = T`.
- **Metadata:** Keywords starting with `Z` (e.g., `ZBITPIX`, `ZNAXIS`, `ZCMPTYPE`) describe the original image before compression.
- **Example File:** `test_data/comp.fits` (from Astropy test data).
- **Current Behavior:** `fitsinfo` reports a `BINTABLE` extension with 1 column and 300 rows.
- **Expected Behavior:** The library should ideally provide a way to access this as an `ImageData` variant, handling the decompression (e.g., Rice, GZIP, PLIO, H-Compress) transparently or via a specialized reader.

## Implementation Gaps
1.  **HDU Classification:** `parse_hdu_info` in `hdu.rs` should check for `ZIMAGE = T`.
2.  **TFORM Parsing:** These tables often use `P` or `Q` descriptors (Variable Length Arrays) for the compressed data, which are currently not supported by `parse_tform_binary`.
3.  **Decompression Algorithms:** Need Rust implementations of the standard FITS compression algorithms (Rice is the most common).

## Impact
Many modern astronomical datasets (e.g., from LSST/Rubin) rely heavily on tiled image compression. Without this support, `fitsio-pure` cannot read standard science images from these surveys.
