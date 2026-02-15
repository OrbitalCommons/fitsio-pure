# twinkle

**Repository:** [twinkle-astronomy/twinkle](https://github.com/twinkle-astronomy/twinkle)
**Category:** Observatory management, telescope control, image capture and analysis
**FITS Centrality:** High -- FITS is the primary image transport and storage format across multiple crates
**Current FITS dependency:** `fitsrs ^0.3.2` (pure-Rust), also `fitsio ^0.21.2` in the legacy `fits_inspect` crate

## What It Does

Twinkle is a Rust-based observatory management system with a web UI. It consists of multiple crates:

- **`indi`** -- An INDI protocol client library for controlling astronomical equipment (cameras, mounts, filter wheels, focusers, flat panels). Published on crates.io as v5.1.1.
- **`twinkle_api`** -- Shared types including `FitsImage`, the core FITS reading abstraction used across the stack. Depends directly on `fitsrs`.
- **`twinkle_server`** -- Backend server that captures images, runs calibration workflows (flats), and serves FITS data over HTTP/WebSocket. Depends on `fitsrs`.
- **`fits_inspect`** -- Image analysis and calibration library. Depends on both `fitsrs` and `fitsio`.
- **`egui-frontend`** -- WebAssembly-based UI that receives FITS image data. Uses `fitsrs` transitively through `twinkle_api`.

## How fitsrs Is Used

### Core FITS Reading Abstraction (`twinkle_api/src/fits.rs`)

The central FITS interface is `FitsImage`, defined in `twinkle_api`:

```rust
use fitsrs::Fits;
use std::io::Cursor;

pub struct FitsImage<'a> {
    data: &'a [u8],
}

impl<'a> FitsImage<'a> {
    pub fn read_image(&self) -> Result<ArrayD<u16>, fitsrs::error::Error> {
        let reader = Cursor::new(self.data);
        let fits = Fits::from_reader(reader);
        read_fits(fits)
    }
}
```

This wraps in-memory FITS bytes (received as INDI BLOB data from cameras) and parses them on demand. The `AsFits` trait extends `Vec<u8>` to produce `FitsImage` instances.

### fitsrs API Surface Used

The actual fitsrs types and methods consumed are narrow:

1. **`Fits::from_reader(reader)`** -- Construct a FITS iterator from a `Read` implementor (`Cursor<&[u8]>` or `BufReader<File>`)
2. **`hdu_list.next()`** -- Iterate HDUs; only the first `HDU::Primary` is ever used
3. **`hdu.get_header()`** -- Access the header object
4. **`header.get_xtension()`** -- Get the extension metadata
5. **`xtension.get_naxisn(1)` / `get_naxisn(2)`** -- Read NAXIS1/NAXIS2 dimensions
6. **`header.get_parsed::<i64>("BZERO")`** -- Read a typed header value by keyword name
7. **`hdu_list.get_data(&hdu).pixels()`** -- Get pixel iterator (only `Pixels::I16` variant used)
8. **`hdu_list.get_data(&hdu).raw_bytes()`** -- Get raw byte slice for manual decoding
9. **`fitsrs::error::Error`** -- The error type, including `DynamicError(String)` for custom errors

### Pixel Decoding Strategy

Twinkle does **not** use fitsrs's built-in pixel decoding for its primary path. Instead, `twinkle_api/src/fits.rs` calls `raw_bytes()` and does its own big-endian to little-endian byte swapping with BZERO adjustment, including a WASM SIMD-optimized path:

```rust
// Non-SIMD path
let x = i16::from_be_bytes([bytes[j], bytes[j + 1]]);
result[i] = (x as i32 - bzero as i32) as u16;
```

The `fits_inspect` crate has a secondary path that uses `Pixels::I16` iterator for file-based FITS reading, converting `i16` to `u16` via `(x as i32 - 32768) as u16`.

### Output Data Type

All FITS image data is converted to `ArrayD<u16>` (ndarray dynamic-dimension array of unsigned 16-bit). This is the standard format for astronomical CCD camera data with BZERO=32768.

### Where FITS Reading Happens

| Location | Reader Input | Uses |
|---|---|---|
| `twinkle_api/src/fits.rs` | `Cursor<&[u8]>` (in-memory BLOB) | `raw_bytes()` + manual decode |
| `fits_inspect/src/lib.rs` | `BufReader<File>` (disk files) | `Pixels::I16` iterator |
| `twinkle_server/src/flats/logic.rs` | Via `twinkle_api::fits::AsFits` | `read_image()` |
| `twinkle_server/src/indi.rs` | Via `twinkle_api::fits::FitsImage` | `read_image()` |
| `egui-frontend/src/indi/agent.rs` | Via deserialized `FitsImage` | `read_image()` |

### Header Reading

Limited header access: only NAXIS1, NAXIS2, and BZERO are read from the FITS header. The `fits_inspect` crate additionally reads FILTER, OFFSET, GAIN, EXPTIME via `fitsio` (not `fitsrs`).

### Error Handling

Uses `fitsrs::error::Error` as the error type, including:
- `Error::DynamicError(String)` for custom error messages
- `From<&str> for Error` conversion (used for ndarray shape mismatch errors)

## Data Types

- **BITPIX 16 only** -- All camera data is 16-bit signed integer with BZERO=32768
- Output: `ArrayD<u16>` (ndarray)
- No floating-point image data, no 8-bit, no 32/64-bit integer images
- Only primary HDU is accessed; no extension HDUs

## WASM Compatibility

The `indi` crate has a `wasm` feature and the `egui-frontend` compiles to WebAssembly. The `twinkle_api` crate (which contains the FITS reading code) is used in the WASM frontend. This means the FITS library must work without `std::fs` -- only `Read` from `&[u8]` is needed. This is a strong alignment point with fitsio-pure's no_std support.

## fitsio-pure Readiness Assessment

### What fitsio-pure Already Provides

- **Header parsing:** `parse_fits()` reads all header cards including NAXIS1, NAXIS2, BZERO from `&[u8]`
- **BITPIX 16 image reading:** `read_image_data()` returns `ImageData::I16(Vec<i16>)`
- **BSCALE/BZERO extraction:** `extract_bscale_bzero()` and `apply_bscale_bzero()`
- **From-memory parsing:** `parse_fits()` works on `&[u8]` slices directly -- no `Read` trait needed, even simpler than fitsrs
- **no_std compatible:** Core library works without `std`, perfect for WASM

### Porting Path

The migration would involve replacing `twinkle_api/src/fits.rs` (~120 lines of code):

**Before (fitsrs):**
```rust
let fits = Fits::from_reader(Cursor::new(self.data));
// iterate HDUs, get header, get raw_bytes, manual decode
```

**After (fitsio-pure):**
```rust
let fits = fitsio_pure::hdu::parse_fits(self.data)?;
let primary = fits.primary();
let image = fitsio_pure::image::read_image_data(self.data, primary)?;
let (bscale, bzero) = fitsio_pure::image::extract_bscale_bzero(&primary.cards);
// Convert ImageData::I16 to u16 with bzero offset
```

fitsio-pure's `parse_fits(&[u8])` API is actually simpler than fitsrs's streaming iterator for this use case, since all data is already in memory.

### Gaps -- Must Fix

- **ndarray integration:** Twinkle needs `ArrayD<u16>` output. fitsio-pure returns `Vec<i16>`. The conversion (`i16` + BZERO -> `u16`, then reshape to ndarray) is straightforward but would need to live either in a utility function or as an ndarray feature in fitsio-pure.

### Gaps -- Nice to Have

- **Read trait support:** The `fits_inspect` crate reads from `BufReader<File>`. Currently fitsio-pure's `parse_fits` requires a complete `&[u8]` slice. For disk files, this means reading the entire file into memory first, which is fine for the image sizes involved (typically < 50 MB).

### Non-Issues

- **fitsio (cfitsio) dependency in `fits_inspect`:** This crate uses `fitsio` for `FitsFile::open()`, `read_image()`, and `read_key()` in a few places, but these are being migrated to fitsrs already (there are commented-out fitsio code blocks). The fitsio dependency appears to be in the process of removal.
- **Pixel decoding performance:** Twinkle currently does its own SIMD-optimized byte swapping. With fitsio-pure, the endian conversion is already handled by `read_image_data()`, but they could continue using their custom SIMD path on `raw_bytes()` if needed for WASM performance.
- **WASM target:** fitsio-pure's no_std + alloc design is a perfect match.
- **Header writing:** Not needed -- twinkle only reads FITS data, never writes it (raw BLOB bytes are saved directly to disk).

### Verdict

**Strong candidate for adoption.** The fitsrs usage is narrow and well-isolated in a single ~120-line file (`twinkle_api/src/fits.rs`). The porting effort is minimal:

1. Replace `fitsrs` dependency with `fitsio-pure` in `twinkle_api/Cargo.toml` and `twinkle_server/Cargo.toml`
2. Rewrite the `read_fits()` function in `twinkle_api/src/fits.rs` to use `parse_fits()` + `read_image_data()`
3. Convert `ImageData::I16` to `Vec<u16>` with BZERO offset, reshape to `ArrayD<u16>`
4. Update error types

The ndarray gap is the only blocking issue, and it's a ~10-line conversion function. The WASM/no_std story is a significant selling point for this project since their frontend runs in WebAssembly. Estimated effort: a few hours of work for a developer familiar with the codebase.
