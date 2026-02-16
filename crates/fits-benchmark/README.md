# fits-benchmark

Comparative I/O benchmark between **fitsio-pure** (pure Rust) and **fitsio** (cfitsio C wrapper).

Three backends are benchmarked:
- **fitsio-pure (core)** -- direct use of `parse_fits`, `read_image_data`, `serialize_image_*`
- **fitsio-pure (compat)** -- the drop-in `fitsio`-compatible API (`FitsFile`, `ReadImage`, `WriteImage`)
- **fitsio (cfitsio)** -- the C library wrapper (requires `libcfitsio-dev`)

## Running

```sh
# Pure Rust only (core + compat)
cargo run -p fits-benchmark --features pure --no-default-features --release

# cfitsio only (requires libcfitsio-dev)
cargo run -p fits-benchmark --features cfitsio --no-default-features --release

# All three side-by-side
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

### fitsio-pure (core)

| Test                   |   Write ms |   Write MP/s |    Read ms |    Read MP/s |
|------------------------|------------|--------------|------------|--------------|
| f32 256x256            |       0.22 |        293.5 |       0.09 |        767.3 |
| f64 256x256            |       0.43 |        151.4 |       0.14 |        463.9 |
| i32 256x256            |       0.22 |        304.7 |       0.08 |        828.1 |
| f32 1024x1024          |       5.45 |        192.2 |       2.12 |        493.9 |
| f64 1024x1024          |      11.87 |         88.3 |       6.17 |        169.9 |
| i32 1024x1024          |       5.29 |        198.2 |       2.15 |        487.8 |
| f32 4096x4096          |     173.54 |         96.7 |     127.80 |        131.3 |
| f64 4096x4096          |     326.38 |         51.4 |     192.89 |         87.0 |
| i32 4096x4096          |     145.72 |        115.1 |      94.98 |        176.6 |
| f32 512x512x100        |     215.96 |        121.4 |     151.17 |        173.4 |
| f64 512x512x100        |     450.63 |         58.2 |     286.47 |         91.5 |
| i32 512x512x100        |     209.16 |        125.3 |     147.84 |        177.3 |

### fitsio-pure (compat)

| Test                   |   Write ms |   Write MP/s |    Read ms |    Read MP/s |
|------------------------|------------|--------------|------------|--------------|
| f32 256x256            |       0.24 |        273.0 |       0.08 |        773.8 |
| f64 256x256            |       0.46 |        143.8 |       0.14 |        479.6 |
| i32 256x256            |       0.23 |        279.7 |       0.08 |        848.9 |
| f32 1024x1024          |       5.21 |        201.1 |       2.31 |        454.3 |
| f64 1024x1024          |      23.34 |         44.9 |       5.27 |        199.0 |
| i32 1024x1024          |       4.96 |        211.5 |       2.24 |        468.6 |
| f32 4096x4096          |     174.26 |         96.3 |     142.04 |        118.1 |
| f64 4096x4096          |     348.93 |         48.1 |     275.64 |         60.9 |
| i32 4096x4096          |     176.75 |         94.9 |     143.53 |        116.9 |
| f32 512x512x100        |     283.68 |         92.4 |     224.58 |        116.7 |
| f64 512x512x100        |     547.06 |         47.9 |     421.40 |         62.2 |
| i32 512x512x100        |     273.78 |         95.8 |     212.52 |        123.4 |

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

The core API and compat layer have similar performance for small images. At larger sizes the compat layer adds overhead from the `extract_from_image_data` type-dispatch step and `ImageData` enum wrapping/unwrapping.

Compared to cfitsio, the core API is **~0.5x** for reads and **~0.4-0.7x** for writes at 1M+ pixel sizes. The compat layer is slightly slower than core due to the extra abstraction. For small images (256x256) all three backends read at similar speed.

The remaining gap vs cfitsio is due to:
- **Memory copying** -- fitsio-pure reads the file into `Vec<u8>`, then copies into an aligned `Vec<T>` for endian swapping. cfitsio reads directly into the caller's buffer.
- **Write rebuilding** -- fitsio-pure serializes to a new buffer and writes the whole file. cfitsio seeks and writes in-place.
