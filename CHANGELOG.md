# Changelog

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
