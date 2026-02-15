![CI](https://github.com/OrbitalCommons/fitsio-pure/actions/workflows/ci.yml/badge.svg) ![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg) ![crates.io](https://img.shields.io/crates/v/fitsio-pure.svg) ![docs.rs](https://docs.rs/fitsio-pure/badge.svg)

# fitsio-pure

A pure Rust implementation of the FITS (Flexible Image Transport System) file format. No C dependencies, no unsafe code, and fully compatible with `wasm32` targets.

## Why pure Rust?

The existing [`fitsio`](https://github.com/simonrw/rust-fitsio) crate wraps the C library `cfitsio`, which requires a system-level install and C toolchain. This creates friction:

- **Cross-platform builds:** `cfitsio` must be compiled or installed separately on macOS, Windows, and Linux. On macOS and Windows especially, getting the right library version and linking flags is a common pain point. `fitsio-pure` builds with `cargo build` on any platform with no external dependencies.
- **Faster builds:** No C compilation step, no `pkg-config`, no linking against system libraries. Cold builds are significantly faster.
- **WebAssembly:** Compiles directly to `wasm32-unknown-unknown` with `--no-default-features`. No emscripten, no wasm-bindgen shims for a C library.
- **Cross-compilation:** `cargo build --target aarch64-unknown-linux-gnu` just works. No cross-compilation toolchain for C needed.

## Features

| Feature | Default | Description |
|---------|---------|-------------|
| `std` | yes | Standard library support |
| `compat` | no | Drop-in replacement API matching the [`fitsio`](https://github.com/simonrw/rust-fitsio) crate |
| `cli` | no | CLI binaries: `fitsinfo`, `fitsconv` |
| `array` | no | ndarray integration (`ArrayD<T>` support via `ReadImage`) |

The core library is `no_std` compatible (with `alloc`) and compiles to `wasm32-unknown-unknown`.

## Using the compat API as a drop-in replacement for fitsio

Enable the `compat` feature to get an API that mirrors the `fitsio` crate. In most cases, switching only requires changing your dependency and `use` paths.

### Cargo.toml

Replace your `fitsio` dependency:

```toml
# Before
[dependencies]
fitsio = "0.21"

# After
[dependencies]
fitsio-pure = { git = "https://github.com/OrbitalCommons/fitsio-pure", features = ["compat"] }
```

### Import changes

```rust
// Before (fitsio)
use fitsio::FitsFile;
use fitsio::images::{ImageDescription, ImageType, ReadImage, WriteImage};
use fitsio::hdu::HduInfo;
use fitsio::headers::ReadsKey;
use fitsio::tables::{ColumnDescription, ColumnDataDescription, ColumnDataType, ReadsCol};

// After (fitsio-pure with compat feature)
use fitsio_pure::compat::fitsfile::FitsFile;
use fitsio_pure::compat::images::{ImageDescription, ImageType, ReadImage, WriteImage};
use fitsio_pure::compat::hdu::HduInfo;
use fitsio_pure::compat::headers::ReadsKey;
use fitsio_pure::compat::tables::{ColumnDescription, ColumnDataDescription, ColumnDataType, ReadsCol};
```

### Creating a FITS file and writing an image

```rust
use fitsio_pure::compat::fitsfile::FitsFile;
use fitsio_pure::compat::images::{ImageDescription, ImageType, WriteImage};

let mut fitsfile = FitsFile::create("output.fits")
    .overwrite()
    .open()
    .unwrap();

let description = ImageDescription {
    data_type: ImageType::Float,
    dimensions: vec![100, 100],
};

let hdu = fitsfile.create_image("SCI", &description).unwrap();

let pixels: Vec<f32> = vec![0.0; 100 * 100];
f32::write_image(&mut fitsfile, &hdu, &pixels).unwrap();
```

### Reading image data

```rust
use fitsio_pure::compat::fitsfile::FitsFile;
use fitsio_pure::compat::images::ReadImage;

let fitsfile = FitsFile::open("input.fits").unwrap();
let hdu = fitsfile.hdu("SCI").unwrap();

let pixels: Vec<f32> = f32::read_image(&fitsfile, &hdu).unwrap();
```

### Reading and writing header keywords

```rust
use fitsio_pure::compat::fitsfile::FitsFile;
use fitsio_pure::compat::headers::{ReadsKey, WritesKey};

let mut fitsfile = FitsFile::edit("data.fits").unwrap();
let hdu = fitsfile.primary_hdu().unwrap();

// Write a keyword
hdu.write_key(&mut fitsfile, "OBJECT", &"NGC 1234".to_string()).unwrap();

// Read it back
let object: String = hdu.read_key(&fitsfile, "OBJECT").unwrap();
```

### Reading table columns

```rust
use fitsio_pure::compat::fitsfile::FitsFile;
use fitsio_pure::compat::tables::ReadsCol;

let fitsfile = FitsFile::open("catalog.fits").unwrap();
let hdu = fitsfile.hdu("CATALOG").unwrap();

let ra: Vec<f64> = f64::read_col(&fitsfile, &hdu, "RA").unwrap();
let dec: Vec<f64> = f64::read_col(&fitsfile, &hdu, "DEC").unwrap();
```

## API comparison: fitsio vs fitsio-pure compat

The table below shows equivalent operations side by side.

| Operation | fitsio | fitsio-pure compat |
|---|---|---|
| Open file | `FitsFile::open(path)?` | `FitsFile::open(path)?` |
| Edit file | `FitsFile::edit(path)?` | `FitsFile::edit(path)?` |
| Create file | `FitsFile::create(path).open()?` | `FitsFile::create(path).open()?` |
| Overwrite | `FitsFile::create(path).overwrite().open()?` | `FitsFile::create(path).overwrite().open()?` |
| Get primary HDU | `f.primary_hdu()?` | `f.primary_hdu()?` |
| Get HDU by name | `f.hdu("SCI")?` | `f.hdu("SCI")?` |
| Get HDU by index | `f.hdu(0)?` | `f.hdu(0usize)?` |
| Read header key | `hdu.read_key::<T>(&mut f, name)?` | `hdu.read_key::<T>(&f, name)?` |
| Write header key | `hdu.write_key(&mut f, name, val)?` | `hdu.write_key(&mut f, name, val)?` |
| Read image | `hdu.read_image(&mut f)?` | `T::read_image(&f, &hdu)?` |
| Write image | `hdu.write_image(&mut f, &data)?` | `T::write_image(&mut f, &hdu, &data)?` |
| Read column | `hdu.read_col::<T>(&mut f, name)?` | `T::read_col(&f, &hdu, name)?` |
| HDU info | `hdu.info` | `hdu.info(&f)?` |
| Number of HDUs | `f.num_hdus()?` | `f.num_hdus()?` |

Key differences from `fitsio`:

- No mutable borrow required for read operations (reads take `&FitsFile`, not `&mut FitsFile`).
- Image and column read/write use associated functions (`T::read_image(...)`) instead of methods on the HDU.
- HDU info requires a reference to the file since there is no cached C-side state.

## Supported types

**Image pixel types:** `u8`, `i16`, `i32`, `i64`, `f32`, `f64`

**Header keyword types:** `i64`, `f64`, `bool`, `String`

**Table column types:** `i32`, `i64`, `f32`, `f64`, `String`

## CLI tools

Enable the `cli` feature to build the CLI binaries:

- `fitsinfo` -- Print a summary of all HDUs in a FITS file.
- `fitsconv` -- Convert between FITS and other formats.

```sh
cargo run --features cli --bin fitsinfo -- path/to/file.fits
cargo run --features cli --bin fitsconv -- --help
```

## Reference Materials

- [FITS Standard 3.0 Specification](https://fits.gsfc.nasa.gov/standard30/fits_standard30aa.pdf) -- The official IAU FITS format definition
- [cfitsio](https://github.com/HEASARC/cfitsio) -- The canonical C FITS I/O library
- [rust-fitsio](https://github.com/simonrw/rust-fitsio) -- Existing Rust FITS bindings (wraps cfitsio); the `compat` module targets this API

## Testing and Validation

This project uses a combination of synthetic round-trip tests and validation against real-world astronomical data.

### Validation Approach
To ensure correctness, we validate `fitsio-pure` against official sample files and `astropy.io.fits`.
1.  **Resources:** See [FITS_RESOURCES.md](FITS_RESOURCES.md) for a list of data sources (NASA, Astropy, LSST).
2.  **Automated Fetching:** Use `scripts/fetch_samples.sh` to download test files.
3.  **Cross-Validation:** Use `scripts/validate_metadata.py` to generate ground-truth metadata from `astropy` for comparison.
4.  **Reporting:** Discovered gaps are documented in [VALIDATION_REPORT.md](VALIDATION_REPORT.md).

For more details on the current status, see the [Validation Report](VALIDATION_REPORT.md).
