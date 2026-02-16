# fits-benchmark

Comparative I/O benchmark between **fitsio-pure** (pure Rust) and **fitsio** (cfitsio C wrapper).

## Running

```sh
# Pure Rust only
cargo run -p fits-benchmark --features pure --no-default-features --release

# cfitsio only (requires libcfitsio-dev)
cargo run -p fits-benchmark --features cfitsio --no-default-features --release

# Both side-by-side
cargo run -p fits-benchmark --features pure,cfitsio --no-default-features --release
```

## What it measures

Writes and reads large image arrays (f32, f64, i32) at several sizes, reporting
average wall-clock time per operation and throughput in megapixels/second.

| Size | Pixels | Iterations |
|------|--------|------------|
| 256x256 | 65K | 50 |
| 1024x1024 | 1M | 20 |
| 4096x4096 | 16.8M | 5 |
| 512x512x100 | 26.2M | 3 |

## Results

Measured on Linux 6.8, AMD/Intel (your numbers will vary).

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

### Analysis

cfitsio is **2-3x faster** for writes and **~3x faster** for reads at large sizes. For small images (256x256) the read gap has nearly closed.

Recent optimizations to fitsio-pure:
- **HDU metadata caching** -- `FitsFile` now caches parsed headers in a `RefCell`, avoiding re-parsing on repeated reads
- **Bulk endian conversion via bytemuck** -- `pod_collect_to_vec` + in-place byte swaps replace element-by-element conversion loops
- **Reduced allocations** -- read path avoids intermediate buffer copies

The remaining write overhead comes from full-buffer rebuilds on each write. The remaining read gap at large sizes is dominated by I/O and memory bandwidth rather than parsing.
