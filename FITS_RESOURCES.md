# FITS Data Resources for Testing

This document lists repositories and specific FITS files that can be used to test the `fitsio-pure` implementation.

## Repositories

### 1. NASA HEASARC (FITS Support Office)
The canonical source for FITS documentation and sample files.
- **URL:** [https://fits.gsfc.nasa.gov/fits_samples.html](https://fits.gsfc.nasa.gov/fits_samples.html)
- **Description:** Contains individual sample files from various missions (HST, IUE, EUVE) and specialized test sets.

### 2. Astropy Test Data
The `astropy` library contains a comprehensive set of FITS files used for its own regression testing.
- **URL:** [https://github.com/astropy/astropy/tree/main/astropy/io/fits/tests/data](https://github.com/astropy/astropy/tree/main/astropy/io/fits/tests/data)
- **Description:** Excellent for testing edge cases, different BITPIX values, and table formats.

### 3. LSST / Rubin Observatory
LSST data often pushes the limits of FITS, especially with tiled image compression and large multi-extension files.
- **Data Preview 0:** [https://dp0-1.lsst.io/](https://dp0-1.lsst.io/)
- **Validation Data:** [https://github.com/lsst/validation_data_cfht](https://github.com/lsst/validation_data_cfht)

## Selected Test Files

The following files are recommended for initial validation:

| File Name | Type | Source | Purpose |
|-----------|------|--------|---------|
| `test0.fits` | Image | Astropy | Basic primary HDU image |
| `tb.fits` | Binary Table | Astropy | Basic binary table verification |
| `comp.fits` | Compressed | Astropy | Tiled image compression (Rice) |
| `NICMOSn4hk12010_mos.fits` | Multi-ext | HEASARC | Multiple image extensions |
| `IUElwp25637mxlo.fits` | Table | HEASARC | IUE spectrum in binary table |
| `WFPC2u5780205r_c0fx.fits` | Image Cube | HEASARC | 3D data cube (trimmed) |

## Validation Workflow

To validate `fitsio-pure` against these files:

1. **Download:** Use the provided script `scripts/fetch_samples.sh`.
2. **Metadata Check:** Compare output of `fitsinfo` with `astropy.io.fits.info()`.
3. **Data Check:** Use `scripts/validate_metadata.py` to dump header/data summaries from Astropy and compare with `fitsio-pure` output.
