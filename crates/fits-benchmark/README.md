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
| f32 256x256            |       0.58 |        112.6 |       0.43 |        151.4 |
| f64 256x256            |       1.15 |         56.8 |       0.87 |         75.2 |
| i32 256x256            |       0.23 |        286.6 |       0.12 |        564.0 |
| f32 1024x1024          |      11.61 |         90.3 |      10.47 |        100.1 |
| f64 1024x1024          |      25.09 |         41.8 |      22.34 |         46.9 |
| i32 1024x1024          |       5.61 |        187.0 |       3.55 |        295.1 |
| f32 4096x4096          |     170.63 |         98.3 |     175.58 |         95.6 |
| f64 4096x4096          |     327.94 |         51.2 |     344.35 |         48.7 |
| i32 4096x4096          |     162.90 |        103.0 |     175.45 |         95.6 |
| f32 512x512x100        |     257.64 |        101.7 |     277.18 |         94.6 |
| f64 512x512x100        |     514.26 |         51.0 |     541.89 |         48.4 |
| i32 512x512x100        |     256.24 |        102.3 |     276.76 |         94.7 |

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

cfitsio is **2-3x faster** for writes and **3-7x faster** for reads, depending on data type and size.

The primary bottleneck in fitsio-pure is the **compat layer re-parsing the entire FITS structure on every operation** (`parse_fits()` is called per read/write). For the core byte-serialization path, fitsio-pure is competitive with cfitsio since both are doing the same big-endian byte swaps. The overhead comes from:

1. **Header re-parsing** -- the compat API re-parses all headers from raw bytes on each call
2. **Full-file read on open** -- `FitsFile::open()` reads the entire file into memory via `std::fs::read()`
3. **Full-file rewrite on write** -- each write rebuilds the entire byte buffer

These are architectural choices in the compat layer, not fundamental limitations. A stateful API that caches parsed headers and uses in-place writes would close the gap significantly.
