# fits-benchmark

Comparative I/O benchmark between **fitsio-pure** (pure Rust) and **fitsio** (cfitsio C wrapper).

Two binaries: `fits-benchmark` (image I/O) and `column-benchmark` (binary table column I/O).

## Running

```sh
# Image benchmarks -- both backends side-by-side
cargo run -p fits-benchmark --bin fits-benchmark --features pure,cfitsio --no-default-features --release

# Column benchmarks -- both backends side-by-side
cargo run -p fits-benchmark --bin column-benchmark --features pure,cfitsio --no-default-features --release

# Single backend only
cargo run -p fits-benchmark --bin fits-benchmark --features pure --no-default-features --release
cargo run -p fits-benchmark --bin column-benchmark --features cfitsio --no-default-features --release
```

Full results and analysis in [`docs/benchmarks.md`](../../docs/benchmarks.md).
