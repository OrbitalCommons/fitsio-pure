# Changelog

## 0.21.4

### Added

- **`image::read_image_physical_f32` and `read_image_physical_into_f32`** read an image with BSCALE/BZERO applied, as `f32`, for plain and tile-compressed images.
  - **Values:** each is `read_image_physical`'s value cast to `f32`, bit for bit: computed in `f64` and rounded once, with BLANK pixels and NaN float pixels as NaN.
  - **Memory:** no `f64` copy of the image is made. A plain image converts straight from its bytes, and a compressed one from its decoded tiles.
  - **`parallel` feature:** the conversion runs in parallel with it, like the other pixel passes.
  - **Test:** a new test checks them against `read_image_physical` for plain images (BSCALE/BZERO, BLANK, NaN, every BITPIX), for compressed images of every type, and for the HCOMPRESS/PLIO fixtures.
  - **Benchmark:** in AstroBurst's benchmark at 64 threads with `parallel`, the compressed 26 MP files read in 28–50 ms against AstroBurst's 166–191 ms, and plain u16 in 16 ms against 18 ms.

## 0.21.3

### Added

- **`parallel` feature: decode on all cores.** It is opt-in, depends on `rayon` and `std`, and leaves default, `no_std` and `wasm32` builds as they were.
  - **Tile decode in parallel:** compressed images are split into bands, one per tile coordinate along the slowest axis, and the bands are decoded and assembled concurrently. Row tiles, 2-D tiles and cubes all parallelize, with no extra copy of the image.
  - **Big pixel passes in chunks:** big-endian decoding of plain images, `read_image_data_into_f32`/`f64`, and `read_image_physical`'s BSCALE/BZERO and BLANK→NaN. The physical read is now one fused pass, without the intermediate mask, with the feature on or off.
  - **Identical output:** a new test hashes the decoded bytes of every codec, type and tiling and of the HCOMPRESS/PLIO fixtures. It must match the same constant, which `main` also gave before this change; CI runs it with the feature on and off.
  - **Benchmark:** AstroBurst's FITS reader against fitsio-pure on 26 MP images, 64 threads, with bit-identical output:

    | file | AstroBurst | before | with `parallel` |
    |---|---|---|---|
    | RICE i16 | 176 ms | 391 ms | 140 ms |
    | RICE f32 | 168 ms | 467 ms | 143 ms |
    | GZIP_2 f32 | 183 ms | 655 ms | 156 ms |
    | GZIP_1 i32 | 188 ms | 582 ms | 154 ms |
    | plain f32 | 34 ms | 98 ms | 33 ms |

## 0.21.2

### Added

- **`HCOMPRESS_1` and `PLIO_1` tile-compressed images are read.** Before, both returned `UnsupportedCompression`.
  - **HCOMPRESS:** a port of cfitsio's `fits_hdecompress.c`, covering quadtree bit-plane decoding, the scale factor, the inverse H-transform and `SMOOTH` interpolation.
    - Like cfitsio, it computes in 32-bit integers for `ZBITPIX` 8 and 16 and in 64-bit otherwise, wrapping on overflow, so lossy images decode to cfitsio's values.
    - Quantized floats go through the existing dither path.
    - `SMOOTH` is read whether it is written as an integer (cfitsio) or a logical (astropy).
  - **PLIO:** a port of cfitsio's `pl_l2pi` line-list decoder.
  - **no_std and wasm32:** both decoders are pure Rust in the core, so they work there too. The `hcompress` crate on crates.io needs `std`, so it wasn't used.
  - **Bounds:** unlike the C, a truncated tile, or one whose dimensions don't match the tile it fills, is a `DecompressionError` rather than a read past the buffer.
- **Fixtures:** compat-parity gains 16 fixtures in `tests/fixtures/hcompress/`. Each decodes exactly as astropy 8.0.1 and cfitsio decode it:
  - astropy-written HCOMPRESS images: 8/16/32-bit, lossless and scaled, smoothed, quantized `f32`/`f64`, and odd tile sizes;
  - astropy-written PLIO images;
  - refimage 1.0.0-pre6's two HCOMPRESS outputs.

  The same test compresses images with cfitsio (`H`, `HS`, with scales and tile sizes, and `P`) and checks that fitsio-pure decodes them exactly as cfitsio does.

## 0.21.1

### Added

- **`compress::Quantize::step(f64)`** sets a fixed quantization step for every tile, exactly. A negative `level` does the same, but `level` takes an `f32`, as cfitsio's `q` does, so a step computed in `f64` was rounded, and pixels on rounding boundaries quantized one step apart. refimage's port keeps its whole-image step this way. Levels set with `level` behave as before.

## 0.21.0

### Added

- **Tile-compressed image writing: `RICE_1` and `GZIP_1`.** The new `compress` module writes an image as a `ZIMAGE` binary table extension. `compress_image_hdu` returns the bytes and `write_compressed_image` writes to any `Write` sink, so it works in memory, in `no_std` and on `wasm32`.
  - **Pixel types:** `u8`, `i8`, `i16`, `u16`, `i32`, `u32`, `f32` and `f64`. `i8`, `u16` and `u32` are stored with `BZERO`, as cfitsio stores them.
  - **Tiles:** whole rows (`tile_rows`, one row per tile by default) or explicit N-D dimensions (`tile_dims`). Edge tiles are clipped.
  - **Floats:** quantized per tile as cfitsio does. The step is the tile's noise divided by the quantization level `q`, with `SUBTRACTIVE_DITHER_1` (the default) or `NO_DITHER`. The dither seed is set by the caller or, by default, derived from the first tile, so it is deterministic. A tile that can't be quantized, such as a constant one, is gzipped losslessly into `GZIP_COMPRESSED_DATA`. NaNs are written as `ZBLANK` nulls. `GZIP_1` can also store floats losslessly (`lossless()`).
  - **Ported from cfitsio's encoders:** `ricecomp.c`, `quantize.c` (the MAD noise estimates and `quick_select`) and `imcompress.c`. On the same pixels and seed, every tile matches cfitsio's: Rice bytes, quantized values, `ZSCALE` and `ZZERO`, including the data-derived seed. A Rice-compressed integer image's whole data unit is identical to cfitsio's.
  - **Credit:** the design follows sunipkm/refimage's FITS writer (MIT OR Apache-2.0), whose encoders were the cross-check. On refimage's own test cases, the Rice tables and heaps are byte-identical to refimage 1.0.0-pre6's. Gzip tiles differ only in the gzip header's OS byte. Quantized floats differ because refimage uses one quantization step for the whole image, where cfitsio and this writer use one per tile.
- **Compat `create("file.fits[compress …]")`** tile-compresses the file's images when it is saved, as cfitsio's extended file name does.
  - `R` (the default) gives `RICE_1` and `G` gives `GZIP_1`, then optional tile dimensions, then `; q LEVEL` (or `q0 LEVEL`, without dithering).
  - The file is edited uncompressed in memory.
  - A primary image is saved as cfitsio saves it, in the first extension (marked `ZSIMPLE`) behind an empty primary HDU.
- **`gzip::compress`** writes a gzip member at a chosen DEFLATE level.

### Tests

- A compat-parity test compresses the same pixels with cfitsio (`[compress …]`) and with fitsio-pure, then compares every tile:
  - types `u8`, `i16`, `u16`, `i32`, `f32` and `f64`;
  - row tiles and 16×8 tiles, Rice and gzip;
  - compat's `[compress …]` path against cfitsio's.
- cfitsio reads every file back.
- astropy 8.0.1 decodes all 50 combinations of codec × type × tiling as written: integers exactly, quantized floats within one `ZSCALE`, and identically to fitsio-pure's own reader.

## 0.20.1

### Changed

- **Compat header writes rewrite one header in place.** Before, `write_key` re-parsed the file, re-serialized every HDU and copied every data unit into a new buffer. Now it rewrites only the changed HDU's header, a few kilobytes, over the old one. The data after it moves only when the header needs another 2880-byte block. Cards and pixels written are unchanged.
- **Compat image writes encode straight into the file.** `write_image`, `write_section` and `write_region` convert values into the data unit in one pass per `BITPIX`, without building an intermediate buffer. On an overflow error the values before it have been written, as cfitsio writes the values it can.
- **`create_image` appends without a temporary copy of the new HDU.**
- **`create().open()` writes the file when it is saved, not twice.** It creates the file, failing with `ExistingFile` or status 105 as before, but leaves it empty until the first `flush` or drop. cfitsio also writes its buffers at close. Before, the zero-filled primary image was written and fsynced at `open`, then written again when saved. With `overwrite()`, an existing file keeps its old contents until the save. Every save still replaces the file atomically.
- serialimage saving a 26 MP u8 luma image, a u16 RGB image and an f32 RGB image, each followed by about 15 keys:
  - `savefits` time went from 7.9 s to 1.1 s, against 0.57 s for cfitsio;
  - a single u16 RGB save went from 2.3 s to 0.38 s, and its peak RSS from 461 MB to 307 MB (156 MB of that is the caller's own pixels);
  - fsync is about 10 ms of each 0.38 s save.

### Added

- **Compat `FitsFile::file_path()`**, as in `fitsio`.

## 0.20.0

### Changed

- **Compat `FitsFile::open` reads only what is used.** (#38)
  - Opening a file reads its headers alone and keeps the file handle.
  - `read_key`, `hdu.info` and HDU lookups work from the parsed headers.
  - `read_image`, `read_section`, `read_rows` and `read_region` on an uncompressed image read only the pixels they return. Contiguous runs of a region are read in one go.
  - Table reads, tile-compressed images and `data()` read the whole file once and keep it, as before.
  - Cutting 50 sources from a 1 GB 16000×16000 `f32` mosaic with FastFitsCutter went from 42 s and 1 GB RSS to 0.2 s of CPU and 3.5 MB RSS. cfitsio takes 0.05 s and 11 MB. Most of the remaining wall time is the atomic, fsynced save of each cutout.
  - Gzip-compressed files are still decompressed into memory, and `edit`, `create` and the in-memory constructors still hold the file in memory.
- **Breaking:** `FitsFile::data()` returns `Result<&[u8]>`, since for a file opened from disk it reads the whole file on first use.
- **Compat `open` fails on a file that isn't FITS, as cfitsio does.** A file shorter than one block is status 108 (`READ_ERROR`), and one that doesn't start with `SIMPLE` is 252 (`UNKNOWN_REC`). Before, `open` succeeded and the first read failed.

### Added

- **`hdu::parse_fits_headers`** parses a FITS file's HDUs through a `read_at(offset, buf)` callback, asking only for header blocks. It gives the same result as `parse_fits` without reading data units.

## 0.19.0

### Changed

- **Breaking: compat `errors::Error` has `fitsio`'s variants and cfitsio's status codes.**
  - The enum has all ten of upstream's variants: `ExistingFile`, `Fits`, `Index`, `IntoString`, `Io`, `Message`, `Null`, `NullPointer`, `UnlockError` and `Utf8`. Exhaustive matches written against `fitsio` (asicam_rs, cameraunit_asi, cameraunit_fli, surge) now compile. `IndexError` and the upstream `From` impls are added too.
  - `Error::Fits` holds a `FitsError { status, message }` instead of the core `fitsio_pure::Error`. `status` is the code cfitsio returns for the same condition, so code that turns a missing key into `None` (mwalib, hyperdrive, twinkle) works:
    - a missing key is 202 (`KEY_NO_EXIST`), and a key with no value is 204 (`VALUE_UNDEFINED`);
    - an HDU name no HDU has is 301 (`BAD_HDU_NUM`), and an index past the last HDU is 107 (`END_OF_FILE`);
    - a read past the end of an image is 307 (`BAD_ROW_NUM`), and a value that doesn't fit the type asked for is 412 (`NUM_OVERFLOW`);
    - a string read as a number is 409 (408 for `f32`), and a bad keyword name is 207 (`BAD_KEYCHAR`);
    - a file that can't be opened is 104, and one that can't be created is 105;
    - errors from the core parser get the nearest cfitsio status, with the parser's description as the message. I/O errors stay `Error::Io`.
  - `create().open()` on an existing file without `overwrite()` returns `Error::ExistingFile(path)`, as upstream does.
  - Writing to a file opened read-only is an error, status 602, as in `fitsio`. Before, the change was made in memory and silently never saved. This covers `write_key`, the image writes, `create_image`, `create_table` and `write_col`.
  - `DescribesHdu::get_hdu` returns a `Result`, so the error carries the status above.
  - Messages follow upstream: `Display` matches `fitsio`'s, and a missing column is `Message("Cannot find column \"NAME\"")`.

### Added

- **`compat::sys`** holds cfitsio's status codes with the same values and types as `fitsio::sys`, so comparisons against `sys::KEY_NO_EXIST` compile unchanged.
- **`compat::errors::check_status`**, as in `fitsio`.
- A compat-parity test triggers each condition above through `fitsio` and compat and checks they return the same variant and status. It also compiles the same exhaustive ten-variant `match` against both.

## 0.18.1

### Fixed

- **Compat region reads and writes accept `0..1` ranges for axes the image doesn't have.** cfitsio reads only the first `NAXIS` ranges, so `fitsio` code can pass a `0..1` for each degenerate Stokes or frequency axis named in the header of a 2-D image. FastFitsCutter does this, and compat rejected it with `InvalidValue`. Trailing `0..1` ranges are now dropped. Any other trailing range is still an error: `fitsio` would return padding for it, or crash on an empty one. A compat-parity test checks the reads and writes against `fitsio`.
- **Compat keyword names are normalised as cfitsio normalises them.** Blanks around a name are ignored and written names are upper-cased, `HIERARCH` names included. Before, `read_key("CRVAL1  ")` failed on a name padded to 8 bytes as it sits on a card. `write_key` with that name returned `Ok` but wrote a card nothing could find, and a lower-case name wrote a lower-case keyword, which isn't valid FITS. FastFitsCutter copies keys by their padded card names, so its cutouts lost `CRVAL1`/`CRVAL2`. A compat-parity test compares the card names and values written against cfitsio.

## 0.18.0

### Added

- **Compat `FitsHdu` has `fitsio`'s fields and image methods.** (#92)
  - `hdu.info` and `hdu.number` are public fields, filled in when the handle is fetched, so `match &hdu.info { … }` and `hdu.number + 1` compile as upstream code writes them. The `info(&f)` method stays, for info that reflects later changes.
  - `hdu.read_image`, `read_section`, `read_rows` and `read_region` return `Vec<T>` or, with the `array` feature, `ArrayD<T>`, chosen by the binding, through a new `ReadsImage` trait.
  - `hdu.write_image`, `write_section` and `write_region` are added; region ranges are `NAXIS1` first.
  - `hdu.name()` returns `EXTNAME` or an empty string, and `hdu.read_cell_value()` reads one table cell.
  - A compat-parity test runs the same upstream-style `FitsHdu` code against `fitsio` and compat and gets identical results.
  - Not provided, for lack of callers: `resize`, `copy_to`, `delete`, `columns`, `row`/`read_row`, `write_col_range`, and the column add/insert/delete methods.

### Fixed

- **Compat image writes no longer corrupt the file when the data doesn't match the image.** `write_image` replaced the whole data unit with the bytes it was given. Too few pixels shrank the data unit and broke the file; too many grew it; and a type that didn't match `BITPIX` (an `f64` slice into a `Float` image) wrote the wrong bytes. Writes now go in place. More values than the image holds is an error, as in `fitsio`, and fewer leave the rest of the image unchanged. Values are converted to the stored type through `BZERO`/`BSCALE`, rounding as cfitsio does, and a value that doesn't fit, or a NaN in an integer image, is an error.

### Changed

- **Breaking:** `WriteImage` gains `write_section` and `write_region`, so implementations outside the crate must add them. `FitsHdu` now derives `PartialEq`.

## 0.17.0

### Changed

- **Breaking: compat image creation matches `fitsio`'s signatures.** (#101)
  - `ImageDescription<'a>` borrows its dimensions, `dimensions: &'a [usize]`, as upstream's does. Upstream code such as `dimensions: &[100, 100]` or `dimensions: &dims` now compiles unchanged; code written for earlier compat releases changes `vec![…]` to `&[…]`.
  - `create_image` takes any name that converts into a `String`, so `create_image("EXTNAME".to_string(), &desc)` compiles as well as `"EXTNAME"`.
  - `NewFitsFile` gains the description's lifetime, so `with_custom_primary` takes `&ImageDescription<'a>`, as in `fitsio`.
  - `fitsio`'s `create_image` and `with_custom_primary` rustdoc examples, expanded against both libraries in compat-parity, write HDUs that cfitsio reads with the same names, `BITPIX` and `NAXISn`.

## 0.16.0

### Changed

- **Compat `write_key` takes the value by value, as `fitsio` does.** `hdu.write_key(&mut f, "GAIN", 100i64)` used to be `E0308: expected &_` at 308 call sites in the graph's `fitsio` repos, including upstream's own documented example. A reference still works, so callers that passed `&value` compile unchanged. Only a `write_key::<T>` turbofish with a borrowed argument breaks. (#89)

### Added

- **Compat header key types match `fitsio`'s.** (#90)
  - `write_key` accepts every integer type from `i8` to `u64`, `f32`, `&str` and `String`, alongside `i64`, `f64` and `bool`, each on its own or paired with a comment as `(value, &str)` or `(value, String)`. `&str` alone was 61 call sites in 12 repos.
  - Narrow integers widen to the one FITS integer type. A `u64` above `i64::MAX` is an error.
  - An `f32` is written as the shortest decimal that reads back as it, so `0.1f32` is written as 0.1.
  - A comment replaces the card's comment. A write without one keeps the existing comment. A `HIERARCH` card carries the comment after its value, cut to fit the card, as cfitsio writes it.
- Compat `read_key` returns `i32` and `f32` alongside `i64`, `f64`, `bool` and `String`, or `HeaderValue<T>` of any of them to get the card's comment as well. An `i32` read of a value that doesn't fit is an error.
- The same `write_key` calls, compiled against `fitsio` and against `fitsio_pure::compat`, write files that the other library reads back with the same values and comments, in both directions (compat-parity).

## 0.15.4

### Fixed

- `ImageWriter` encodes samples about 3× faster from other crates. As with the reader in 0.15.3, the per-sample big-endian conversion wasn't inlinable across crates. Writing a 24-megapixel `f32` image to a sink went from 38 ms to 12 ms.

## 0.15.3

### Fixed

- `FitsReader::read_image` and `read_image_into_f32`/`_f64` decode about 1.3× faster from other crates. The per-pixel big-endian conversion wasn't inlinable across crates, so each pixel was a function call and the loop couldn't vectorize. A 24-megapixel `BITPIX = -32` frame loaded in 101 ms and now loads in 76 ms; a 61-megapixel `BITPIX = 16` frame went from 195 ms to 118 ms.

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
