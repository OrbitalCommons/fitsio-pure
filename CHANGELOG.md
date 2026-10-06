# Changelog

## 0.15.2

### Fixed

- Cards whose value indicator lacks the space after `=` (`CTYPE1  ='RA---TAN'`) are read with their value, as cfitsio reads them. They parsed as having no value, so the keyword read as missing. `edit::update_card` replaces such a card instead of inserting a duplicate.

## 0.15.1

### Fixed

- String values that start after column 11 (`OBJECT  =        'M31'`), which free-format FITS allows and seiza's writer, among others, produces, are read. `parse_value` only recognized a string whose opening quote was the first byte of the value field, so these cards parsed with no value and compat `read_key` reported the keyword missing. They now read as cfitsio and astropy read them.

## 0.15.0

### Added

- **Streaming image writer.** `image_writer::ImageWriter` writes an image HDU to any `Write` without building the data unit in memory. It writes the header, converts samples to big-endian through a fixed 1 MiB buffer as they arrive (`write_samples`, `write_iter`, or `write_physical` to apply the header's `BSCALE`/`BZERO`), and `finish()` checks the count against `NAXISn`, pads to the block boundary and returns the sink. Writing more or fewer samples than the header declares, or samples of the wrong type for `BITPIX`, is an error. `ImageWriter::image_extension` starts an `XTENSION = 'IMAGE'` HDU on the returned sink, so multi-HDU files stream too. The bytes are identical to `build_image_hdu`'s for every `BITPIX`. It also works with the crate's `io::Write` under `no_std`, where `Error` gains an `Io(io::IoError)` variant. (#109)
- **Atomic file writes.** `io::AtomicFile` writes to a temporary file next to the target and, on `commit()`, flushes, `sync_all`s, renames it over the target and fsyncs the directory, so a crash or error mid-write never leaves a partial file. Dropping it uncommitted removes the temporary file. Replacing a file keeps its permissions on Unix. `io::write_atomic(path, bytes)` does it in one call. It composes with `ImageWriter`: stream into an `AtomicFile`, then `finish()?.commit()?`. (#100)
- **Streaming reads** with `stream::FitsReader`, over any `std::io::Read`, one HDU at a time. (#38, #95)
  - `next_hdu()` reads only the next header, so an HDU's cards, `HduInfo` and image dimensions come without reading its data unit. `read_primary_header` and `read_header` do this for the primary HDU alone.
  - `skip_data()` moves past a data unit without keeping it: by seeking on a seekable reader, otherwise by reading and discarding.
  - `read_image()`, `read_image_into_f32()` and `read_image_into_f64()` decode an image in 1 MiB chunks straight into the typed output, so peak memory is the pixels plus one chunk, and the padding after the data is not read. Tile-compressed images are read whole and decompressed.
  - `FitsReader::open`, `seekable`, `with_len` and `from_slice` know the input length, so a declared data size larger than the input is an error before anything is allocated. With an unknown length, the output grows as data arrives. Truncated data is `UnexpectedEof`.
  - HDUs, cards and pixels are identical to `parse_fits` and `read_image_data` for every file in fits-test-cases.
- **In-place header updates** with `edit::update_card`, over `Read + Write + Seek`, and `edit::update_card_in_file`. The matching card is overwritten, or the new card takes `END`'s slot and `END` moves down; nothing else in the file changes. Structural keywords (`BITPIX`, `NAXISn`, `BZERO`, `TFORMn`, `CHECKSUM`, …) and values needing `CONTINUE` cards are refused, as is an insert into a full header. `edit::update_card_growing` instead grows a full header by one block and moves the rest of the file down, as cfitsio does. cfitsio reads the updated string, integer and float values, and the pixels, unchanged. (#110)

### Changed

- Compat `FitsFile` saves (`flush`, `Drop`, and `create(..).open()`) go through `io::write_atomic`, so an interrupted save leaves the previous file intact instead of a truncated one. (#100)
- **Breaking: `Value::Undefined`** represents a card with a value indicator and nothing after it (`FILTER  =    / no filter`), which FITS treats as an undefined value, distinct from the empty string `''`. `parse_card` used to return `value: None` for these, and `format_card` then wrote `FILTER  no filter`, dropping the `=` and the comment slash. Undefined cards now round-trip exactly. Exhaustive `match`es on `Value` need a new arm. Compat `read_key` on an undefined value returns an error for every type, as cfitsio does. (#108)
- Header keywords may contain any printable ASCII. A single lowercase or otherwise non-standard keyword (such as `date-obs`) used to fail the whole header with `InvalidKeyword`; it is now read as written, as cfitsio and astropy do. Only non-printable bytes are rejected. Compat `read_key` matches keywords ignoring case, as cfitsio does. (#111)

## 0.14.1

### Added

- `FitsFile::create(path).with_custom_primary(&desc)` and `FitsFile::create_in_memory_with_custom_primary(&desc)` make the primary HDU an image, as in `fitsio`, so pixels can be written through `primary_hdu()` instead of an extension. The image is named `_PRIMARY`, as rust-fitsio names it. Its dimensions are row-major, like `create_image`'s, and unsigned types get `BZERO`/`BSCALE`. NAXIS, pixels and shapes match cfitsio's for 2-D and 3-D non-square images in both directions.
- `FitsFile`, `FileOpenMode` and `HeaderValue` are re-exported at `fitsio_pure::compat`, mirroring `fitsio`'s crate root, so `use fitsio::FitsFile;` ports as `use fitsio_pure::compat::FitsFile;`.

### Fixed

- An image HDU with `NAXIS = 0` no longer gets a one-pixel data unit.

## 0.14.0

### Changed

- **Breaking: compat image dimensions use `fitsio`'s axis order.** `ImageDescription.dimensions` and the `shape` in `HduInfo::ImageInfo` are row-major, slowest axis first, as in `fitsio`: `[rows, columns]` for a 2-D image, the reverse of FITS `NAXISn` order. They were in `NAXISn` order, so code ported from `fitsio` wrote and read every non-square image at the wrong width, silently. `create_image(…, &ImageDescription { dimensions: vec![5, 7], .. })` now writes `NAXIS1 = 7`, `NAXIS2 = 5`, as cfitsio does. (#103)

  Code written against the old order must reverse its dimensions and shapes. `ndarray` reads (`ArrayD::read_image`) already returned row-major arrays and are unchanged. `read_region` ranges stay `NAXIS1` first, as in cfitsio. The core (non-compat) API keeps FITS `NAXISn` order.

## 0.13.5

### Fixed

Three writes that lost data and reported success:

- **Writing past the end of a table extends it, as in cfitsio.** Compat `write_col` on a table created by `create_table` (which starts with 0 rows) returned `Ok` and wrote nothing. Now new rows are added, zero-filled, and `NAXIS2` is updated; a heap after the rows moves down with `THEAP`. (#99)
  - Writing fewer values than the table has rows fills those rows and leaves the rest, where it used to panic.
  - Data that ends partway through a row is an error.
  - Core `bintable::write_binary_column` writes as many rows as the data fills, and returns an error rather than dropping values that don't fit. The new `bintable::column_data_rows` gives the row count.
- **String values longer than 68 characters are written in full** using `CONTINUE` cards, laid out byte-for-byte as cfitsio's `fits_write_key_longstr` writes them. They were cut at 68 characters. A comment on the last `CONTINUE` card is now read back as the keyword's comment. (#93)
- **Compat keyword names longer than 8 characters, or containing spaces, use the `HIERARCH` convention**, as cfitsio does. `write_key("ESO DET CHIP1 ID", …)` used to write the illegal card `ESO DET = …`, merging names that share 8 characters. `read_key` now finds `HIERARCH` keys by name, ignoring case, with or without the `HIERARCH ` prefix. A `HIERARCH` card too long for 80 bytes is an error. (#102)

  The core `Card` type is unchanged: a `HIERARCH` card still has the keyword `HIERARCH`, with the rest of its text in `comment`.

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
