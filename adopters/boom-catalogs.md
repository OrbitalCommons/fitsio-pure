# boom-catalogs

**Repository:** [boom-astro/boom-catalogs](https://github.com/boom-astro/boom-catalogs)
**Category:** Astronomical catalog ingestion pipeline
**FITS Centrality:** High — FITS is a primary input format for large catalogs

## What It Does

Ingests astronomical catalogs (NED, Milliquas, CatWISE2020, Gaia, etc.) from FITS binary tables into MongoDB for cross-matching by the BOOM alert broker.

## FITS Operations Used

### Table Operations (exclusively — no image I/O)
- `FitsFile::open()`
- `fptr.hdu(1)` — binary table HDU access
- **`hdu.read_col_range()`** — batch column reads with row ranges (critical for large catalogs)
- `HduInfo::TableInfo` — row count and table structure queries

### Batch Processing Pattern
- Reads rows in configurable batches (default 10,000)
- Async channel-based worker pool
- NaN/Inf filtering for nullable floats

### Uses `fitsio-derive`
- Derive macros for typed FITS row deserialization

## Data Types
- Heterogeneous columns: `String`, `f64`, `f32`, `bool`, `i32`
- Nullable handling: NaN/Inf/MIN/MAX → `Option<f64>`
- Coordinates: RA, Dec (f64)
- Scientific: magnitudes, redshifts, proper motions, parallax

## fitsio-pure Readiness Assessment

### What Works Today
- File open
- HDU access by index
- Basic column reads for scalar types

### Gaps — Must Fix
- **`read_col_range()`:** This is the critical operation — reading a column for a specific row range. Our compat layer has `read_col()` which reads full columns. For catalogs with millions of rows, reading everything at once is not viable. This is a hard blocker.
- **`HduInfo::TableInfo`:** Need to expose row count and column info from our table parsing.
- **`fitsio-derive` compatibility:** boom-catalogs uses derive macros from the `fitsio-derive` crate. We'd need either compatible derive macros or manual migration.
- **`bool` column type:** Need to verify our binary table logical column (TFORM 'L') maps correctly to Rust `bool`.

### Verdict
**Not ready.** `read_col_range()` is a hard blocker for any large-catalog use case. This is a meaningful API gap — partial column reads are essential for memory-efficient processing of astronomical catalogs that can have millions of rows.
