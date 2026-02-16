# Benchmarks

Comparative I/O throughput between **fitsio-pure** (pure Rust) and **fitsio** (cfitsio C wrapper).

All numbers measured on Linux 6.8 in `--release` mode. Your results will vary by hardware.

## Running

```sh
# Image benchmarks (both backends side-by-side)
cargo run -p fits-benchmark --bin fits-benchmark --features pure,cfitsio --no-default-features --release

# Column benchmarks (both backends side-by-side)
cargo run -p fits-benchmark --bin column-benchmark --features pure,cfitsio --no-default-features --release
```

## Image I/O

Writes and reads large image arrays (f32, f64, i32) at several sizes, reporting average wall-clock time per operation and throughput in megapixels/second (MP/s).

| Size | Pixels | Iterations |
|------|--------|------------|
| 256x256 | 65K | 50 |
| 1024x1024 | 1M | 20 |
| 4096x4096 | 16.8M | 5 |
| 512x512x100 | 26.2M | 3 |

### fitsio-pure

| Test                   |   Write ms |   Write MP/s |    Read ms |    Read MP/s |
|------------------------|------------|--------------|------------|--------------|
| f32 256x256            |       0.97 |         67.5 |       0.09 |        720.2 |
| f64 256x256            |       1.14 |         57.4 |       0.16 |        405.7 |
| i32 256x256            |       0.23 |        282.8 |       0.09 |        762.9 |
| f32 1024x1024          |      11.91 |         88.0 |       3.25 |        322.7 |
| f64 1024x1024          |      26.34 |         39.8 |       7.57 |        138.4 |
| i32 1024x1024          |       5.86 |        178.8 |       3.00 |        349.1 |
| f32 4096x4096          |     215.25 |         77.9 |     163.37 |        102.7 |
| f64 4096x4096          |     341.56 |         49.1 |     264.91 |         63.3 |
| i32 4096x4096          |     169.70 |         98.9 |     131.06 |        128.0 |
| f32 512x512x100        |     268.45 |         97.7 |     208.48 |        125.7 |
| f64 512x512x100        |     533.96 |         49.1 |     418.13 |         62.7 |
| i32 512x512x100        |     267.53 |         98.0 |     204.36 |        128.3 |

### fitsio (cfitsio)

| Test                   |   Write ms |   Write MP/s |    Read ms |    Read MP/s |
|------------------------|------------|--------------|------------|--------------|
| f32 256x256            |       0.28 |        236.2 |       0.09 |        761.7 |
| f64 256x256            |       0.45 |        144.8 |       0.12 |        525.2 |
| i32 256x256            |       0.27 |        238.4 |       0.08 |        804.4 |
| f32 1024x1024          |       3.62 |        289.6 |       1.07 |        981.4 |
| f64 1024x1024          |       7.11 |        147.4 |       2.77 |        378.4 |
| i32 1024x1024          |       3.64 |        287.8 |       1.02 |       1025.4 |
| f32 4096x4096          |      60.30 |        278.2 |      53.21 |        315.3 |
| f64 4096x4096          |     113.89 |        147.3 |     104.47 |        160.6 |
| i32 4096x4096          |      59.40 |        282.5 |      53.90 |        311.3 |
| f32 512x512x100        |      94.54 |        277.3 |      83.51 |        313.9 |
| f64 512x512x100        |     179.63 |        145.9 |     161.56 |        162.3 |
| i32 512x512x100        |      92.97 |        282.0 |      81.23 |        322.7 |

### Image I/O summary

cfitsio is **2-3x faster** for writes and **~3x faster** for reads at large sizes. For small images (256x256) the read gap has nearly closed.

## Binary Table Column I/O

Writes and reads single-column binary tables (f32, f64, i32, i64) at various row counts, reporting throughput in megarows/second (MR/s).

| Size | Rows | Iterations |
|------|------|------------|
| 1K rows | 1,000 | 100 |
| 10K rows | 10,000 | 50 |
| 100K rows | 100,000 | 20 |
| 1M rows | 1,000,000 | 5 |

### fitsio-pure

| Test                   |   Write ms |   Write MR/s |    Read ms |    Read MR/s |
|------------------------|------------|--------------|------------|--------------|
| f32 1K rows            |       0.06 |         17.5 |       0.01 |         67.9 |
| f64 1K rows            |       0.06 |         16.8 |       0.02 |         45.8 |
| i32 1K rows            |       0.07 |         14.9 |       0.01 |         70.9 |
| i64 1K rows            |       0.08 |         12.8 |       0.02 |         60.0 |
| f32 10K rows           |       0.37 |         26.7 |       0.05 |        199.8 |
| f64 10K rows           |       0.49 |         20.3 |       0.10 |        103.9 |
| i32 10K rows           |       0.45 |         22.3 |       0.05 |        201.0 |
| i64 10K rows           |       0.55 |         18.1 |       0.11 |         90.3 |
| f32 100K rows          |       4.07 |         24.6 |       0.74 |        135.5 |
| f64 100K rows          |       5.27 |         19.0 |       1.25 |         80.1 |
| i32 100K rows          |       4.55 |         22.0 |       0.43 |        231.1 |
| i64 100K rows          |       5.83 |         17.1 |       1.41 |         71.0 |
| f32 1M rows            |      42.42 |         23.6 |       9.09 |        110.0 |
| f64 1M rows            |      59.37 |         16.8 |      15.66 |         63.9 |
| i32 1M rows            |      48.63 |         20.6 |       4.95 |        202.0 |
| i64 1M rows            |      69.46 |         14.4 |      17.05 |         58.7 |

### fitsio (cfitsio)

| Test                   |   Write ms |   Write MR/s |    Read ms |    Read MR/s |
|------------------------|------------|--------------|------------|--------------|
| f32 1K rows            |       0.06 |         16.3 |       0.02 |         44.9 |
| f64 1K rows            |       0.06 |         15.5 |       0.03 |         37.9 |
| i32 1K rows            |       0.07 |         15.0 |       0.02 |         44.0 |
| i64 1K rows            |       0.07 |         14.2 |       0.03 |         37.3 |
| f32 10K rows           |       0.11 |         91.1 |       0.04 |        280.8 |
| f64 10K rows           |       0.15 |         65.0 |       0.05 |        213.2 |
| i32 10K rows           |       0.11 |         92.0 |       0.04 |        278.6 |
| i64 10K rows           |       0.18 |         55.5 |       0.04 |        225.5 |
| f32 100K rows          |       0.40 |        248.3 |       0.15 |        684.0 |
| f64 100K rows          |       0.68 |        146.3 |       0.24 |        411.8 |
| i32 100K rows          |       0.40 |        250.1 |       0.15 |        679.2 |
| i64 100K rows          |       0.68 |        147.3 |       0.23 |        428.8 |
| f32 1M rows            |       4.48 |        223.1 |       1.42 |        704.0 |
| f64 1M rows            |       8.60 |        116.2 |       4.41 |        226.5 |
| i32 1M rows            |       4.01 |        249.6 |       1.33 |        750.5 |
| i64 1M rows            |       8.18 |        122.2 |       4.31 |        231.9 |

### Column I/O summary

At small row counts (1K), performance is comparable. At 1M rows, cfitsio is **~10x faster** for writes and **~4-6x faster** for reads.

The write gap is because fitsio-pure serializes the entire binary table HDU (header + all row data) as a single byte buffer, while cfitsio writes column data in-place to an open file handle. The read gap comes from per-element byte-swapping and column extraction versus cfitsio's optimized C routines.

## Optimizations applied

- **HDU metadata caching** -- `FitsFile` caches parsed headers in a `RefCell`, avoiding re-parsing on repeated reads
- **Bulk endian conversion via bytemuck** -- `pod_collect_to_vec` + in-place byte swaps replace element-by-element conversion loops
- **Reduced allocations** -- read path avoids intermediate buffer copies

## Known bottlenecks

- **Full-buffer rebuilds on write** -- the entire FITS byte buffer is reconstructed on each write operation rather than modifying data in-place
- **Column extraction from row-major data** -- binary table data is stored row-major in FITS; extracting a single column requires striding through the full data block
