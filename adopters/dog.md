# dog

**Repository:** [TrystanScottLambert/dog](https://github.com/TrystanScottLambert/dog)
**Category:** CLI tool — tabular data inspector (like `cat` for columnar formats)
**FITS Centrality:** Medium — FITS is one of three supported formats (FITS, CSV, Parquet)

## What It Does

Terminal data explorer for columnar data. Reads FITS binary tables, CSV, and Parquet files into Polars DataFrames for display.

## FITS Operations Used

### Table Operations (exclusively — no image support)
- `FitsFile::open()`
- `fptr.hdu(1)` — access extension HDU by index
- `hdu.read_key()` — TFIELDS count, TTYPE* (column names), TFORM* (column formats)
- **`hdu.read_col::<T>()`** — typed column reads for multiple types
- Custom TFORM parser for repeat counts (e.g., "101E" → 101 x f32)
- **Vector column expansion:** flattens array columns into N separate named columns

### Parallel I/O
- Each Rayon thread opens its own `FitsFile` handle independently
- Column reads parallelized across threads

## Data Types
- `Vec<f32>`, `Vec<f64>`, `Vec<i32>`, `Vec<i64>`, `Vec<String>`
- TFORM repeat counts for vector columns
- Polars DataFrame conversion

## fitsio-pure Readiness Assessment

### What Works Today
- File open
- HDU access by index
- Header read for standard types
- Column read for scalar types (i32, i64, f32, f64, String)

### Gaps — Must Fix
- **Vector/array columns (TFORM repeat counts):** dog parses TFORM strings like "101E" to detect array columns and expands them. Our binary table support needs to handle repeat counts properly and expose them through the compat API.
- **Thread-safe file handles:** dog opens independent FitsFile instances per thread. Our compat layer wraps an in-memory buffer — this should work since each thread would get its own copy, but needs testing.

### Gaps — Nice to Have
- Column-range reads for large tables (dog reads full columns currently)

### Verdict
**Close.** Scalar column reads work. Vector column handling (repeat-count TFORM) is the main gap. Since dog does its own TFORM parsing, the compat layer just needs to return the right data when read_col is called on a vector column — which may already work if the data is flattened. Worth testing.
