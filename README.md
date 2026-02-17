![CI](https://github.com/OrbitalCommons/fitsio-pure/actions/workflows/ci.yml/badge.svg) ![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg) ![crates.io](https://img.shields.io/crates/v/fitsio-pure.svg) ![docs.rs](https://docs.rs/fitsio-pure/badge.svg)

# fitsio-pure

A pure Rust implementation of the FITS (Flexible Image Transport System) file format. No C dependencies, no unsafe code, and fully compatible with `wasm32` targets.

## Why pure Rust?

The existing [`fitsio`](https://github.com/simonrw/rust-fitsio) crate wraps the C library `cfitsio`, which requires a system-level install and C toolchain. This creates friction:

- **Cross-platform builds:** `cfitsio` must be compiled or installed separately on macOS, Windows, and Linux. `fitsio-pure` builds with `cargo build` on any platform with no external dependencies.
- **Faster builds:** No C compilation step, no `pkg-config`, no linking against system libraries.
- **WebAssembly:** Compiles directly to `wasm32-unknown-unknown` with `--no-default-features`.
- **Cross-compilation:** `cargo build --target aarch64-unknown-linux-gnu` just works.

## Features

| Feature | Default | Description |
|---------|---------|-------------|
| `std` | yes | Standard library support |
| `compat` | no | Drop-in replacement API matching the [`fitsio`](https://github.com/simonrw/rust-fitsio) crate |
| `cli` | no | CLI binaries: `fitsinfo`, `fitsconv` |
| `array` | no | ndarray integration (`ArrayD<T>` support via `ReadImage`) |

The core library is `no_std` compatible (with `alloc`) and compiles to `wasm32-unknown-unknown`.

## Supported types

**Image pixel types:** `u8`, `i16`, `i32`, `i64`, `f32`, `f64`

**Header keyword types:** `i64`, `f64`, `bool`, `String`

**Table column types:** `i32`, `i64`, `f32`, `f64`, `String`

## Compat API (drop-in replacement for fitsio)

Enable the `compat` feature to get an API that mirrors the `fitsio` crate. In most cases, switching only requires changing your dependency and `use` paths.

See **[docs/compat-guide.md](docs/compat-guide.md)** for migration instructions, code examples, and a full API comparison table.

## Comparison with other Rust FITS libraries

| | **fitsio-pure** | **fitsio** | **fitsrs** |
|---|---|---|---|
| **Pure Rust** | ✅ | ❌ | ✅ |
| **External deps** | None | cfitsio + C toolchain | None |
| **`wasm32` / `no_std`** | ✅ / ✅ | ❌ / ❌ | ❌ / ❌ |
| **Read images** | ✅ All BITPIX | ✅ All BITPIX | ✅ All BITPIX |
| **Write images** | ✅ All BITPIX | ✅ All BITPIX | ✅ All BITPIX |
| **Binary tables** | ✅ Read + write | ✅ Read + write | ⚠️ Read only |
| **ASCII tables** | ✅ Read + write | ✅ Read + write | ⚠️ Raw bytes |
| **Random groups** | ⚠️ Read | ✅ Read + write | ❌ |
| **Tile compression** | ⚠️ Parsed as table | ✅ Transparent | ⚠️ GZIP/RICE |
| **Header keywords** | ✅ Read + write | ✅ Read + write | ⚠️ Read only |
| **ndarray** | ✅ | ✅ | ❌ |

See **[docs/comparison.md](docs/comparison.md)** for the full table including speeds, async I/O, BSCALE/BZERO, variable-length arrays, and download counts.

## CLI tools

Enable the `cli` feature to build the CLI binaries:

- `fitsinfo` -- Print a summary of all HDUs in a FITS file.
- `fitsconv` -- Convert between FITS and other formats.

```sh
cargo run --features cli --bin fitsinfo -- path/to/file.fits
cargo run --features cli --bin fitsconv -- --help
```

## Benchmarks

Performance is approaching cfitsio on small arrays and within 3x on large images. Column writes at scale are the widest gap (~10x at 1M rows).

See **[docs/benchmarks.md](docs/benchmarks.md)** for full I/O throughput comparisons.

## Testing and Validation

CI runs against a curated corpus of 63 real-world FITS files from the [`fits-test-cases`](https://github.com/OrbitalCommons/fits-test-cases) repository, covering:

- Primary images across all BITPIX types (8/16/32/64-bit integer, 32/64-bit float)
- Multi-extension files (up to 9 HDUs) from HST, EUVE, and other missions
- Binary and ASCII table extensions
- Random groups format (UVFITS)
- 3D cubes and 4D+ hypercubes
- Unsigned 16-bit images via BZERO=32768
- HEALPix tiles
- Synthetic test patterns (gradients, checkerboards, mixed types, extreme aspect ratios)

In addition to corpus validation, the test suite includes in-memory round-trip tests for all supported data types and structures, ensuring `wasm32` compatibility without filesystem access.

## Reference Materials

- [FITS Standard 3.0 Specification](https://fits.gsfc.nasa.gov/standard30/fits_standard30aa.pdf) -- The official IAU FITS format definition
- [cfitsio](https://github.com/HEASARC/cfitsio) -- The canonical C FITS I/O library
- [rust-fitsio](https://github.com/simonrw/rust-fitsio) -- Existing Rust FITS bindings (wraps cfitsio); the `compat` module targets this API
