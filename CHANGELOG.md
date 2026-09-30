# Changelog

## 0.13.4

### Fixed

- `compat::FitsFile::hdu("name")` matches names ignoring case and falls back to `HDUNAME`, honoring only the first card of each, as cfitsio does. It previously required an exact `EXTNAME` match, so a lookup that works with `fitsio` could fail with "HDU not found" (#87).

## 0.13.3

### Added

- Gzip-compressed FITS files (`.fits.gz`, `.fit.gz`) open transparently through compat `FitsFile::open` and `FitsFile::from_bytes`, as they do in cfitsio. `FitsFile::edit` refuses them, as cfitsio does, since saving would replace the compressed file with plain bytes (#85).
- A core `gzip` module with `is_gzip` and `decompress`. Each member's CRC-32 and length are checked, and concatenated members and trailing zero padding are handled (#85).

### Fixed

- `GZIP_1` and `GZIP_2` tile data is now integrity-checked: a corrupt tile is an error instead of possibly wrong pixels (#85).

## 0.13.2

### Added

- In-memory files in the compat layer, which `fitsio` has no equivalent for (#83):
  - `FitsFile::from_bytes` opens FITS bytes you already hold, read-only, without writing a temp file;
  - `FitsFile::create_in_memory` starts a writable file with no backing path;
  - `FitsFile::into_bytes` returns the bytes, flushing a writable file opened from a path first.

  `flush` and `Drop` never touch disk for an in-memory file.

## 0.13.1

### Fixed

- Tile-compressed images read with the right pixel values in every case below; each previously returned `Ok` with wrong values (#81):
  - quantized float tiles written with `SUBTRACTIVE_DITHER_1` or `SUBTRACTIVE_DITHER_2` (the fpack and astropy default) are dithered back correctly, including the reserved zero value of `SUBTRACTIVE_DITHER_2`;
  - `GZIP_2` tiles are un-shuffled for every `BITPIX` other than 8;
  - tiles stored uncompressed in the `GZIP_COMPRESSED_DATA` or `UNCOMPRESSED_DATA` fallback column are read from that column instead of as zeros;
  - `ZBLANK` null pixels decode to NaN instead of a large negative number.
- `NOCOMPRESS` tiles decode instead of returning an error (#81).

### Changed

- Reading a `HCOMPRESS_1` or `PLIO_1` image still returns `UnsupportedCompression`, and the error now names the algorithm (#81).

## 0.13.0

### Added

- Compat `ReadsCol` for `bool`, so `read_col` works on FITS logical (`L`) columns (#72, thanks @TrystanScottLambert).
- Compat `ReadsCol`, `ReadsColRange` and `WritesCol` for `u8`, `i8`, `i16`, `u16`, `u32` and `u64`, and `ReadsColRange` for `bool`, matching upstream `fitsio`'s column types (#74).
- `ImageType::Byte` for the signed-byte storage convention (`BITPIX = 8` with `BZERO = -128`) (#76).

### Fixed

- Compat table column reads and writes now apply `TSCALn`/`TZEROn` the way cfitsio does, so scaled and unsigned columns return physical values instead of raw storage values (#74).
- Compat image reads now apply `BSCALE`/`BZERO`, which were dropped on every read (`BSCALE`) and on all but the unsigned types (`BZERO`), and `hdu.info()` reports the physical type as cfitsio's `fits_get_img_equivtype` does — a `BITPIX = 16` frame with `BZERO = 32768` is `UnsignedShort`, not `Short` (#76).
- Tile-compressed images whose tiles are narrower than the image (`ZTILE1 < ZNAXIS1`) now reassemble correctly. Decoded tiles were appended end to end instead of being placed at their position in the tile grid, so the image read back `Ok` with most pixels in the wrong place (#77).

### Changed

- Compat numeric column reads convert from any numeric column type, and writes convert to the column's storage type. A value that doesn't fit the target type is now an error instead of wrapping (#74).
- Compat image reads return an error when a physical value doesn't fit the requested element type, where they previously clamped silently (#76).
- **Breaking:** `compat::FitsFile` is now `Sync`, so one open file can be shared across threads instead of being reopened and reparsed per thread. Its parse cache is a `OnceLock`, and `FitsFile::parsed()` returns `&FitsData` instead of `std::cell::Ref<'_, FitsData>` (#78).
