# map2fig

**Repository:** [dncnwtts/map2fig](https://github.com/dncnwtts/map2fig)
**Category:** CLI tool / library -- publication-quality HEALPix sky map visualization
**Current FITS dependency:** `fitsrs ^0.4` (pure Rust, no cfitsio)
**FITS Centrality:** High -- FITS file reading is the sole data input path
**License:** MIT

## What It Does

map2fig is a fast Rust tool for rendering HEALPix sky maps from FITS files into PDF and PNG plots. It supports Mollweide, Hammer, and Gnomonic projections with 80+ colormaps, multiple scaling modes (linear, log, symlog, asinh, histogram equalization), coordinate system transformations (Galactic, Equatorial, Ecliptic), graticule overlays, masking, and LaTeX label rendering. It is positioned as a Rust alternative to Python's healpy `mollview`.

The crate is brand new (first published 2026-02-14, currently at v0.5.0).

## FITS Operations Used

### Binary Table Reading (primary path)

All FITS I/O lives in two files: `src/fits.rs` and `src/healpix.rs`, with supplemental mask loading in `src/mask.rs`.

**Core data reading** (`fits.rs: read_healpix_column`):
- Open FITS file, iterate HDUs looking for `XBinaryTable`
- Read header keywords: `NSIDE` (Integer), `INDXSCHM` (String), `ORDERING` (String)
- Select columns by index using `ColumnId::Index(n)`
- Extract cell values via pattern matching on `DataValue::Double`, `DataValue::Float`, `DataValue::Integer`, `DataValue::Long`
- All values converted to `Vec<f64>`

**Sparse map support** (EXPLICIT indexing scheme):
- When `INDXSCHM == "EXPLICIT"`, reads two columns simultaneously: column 0 (PIXEL indices) and column `col_idx + 1` (data values)
- Builds a full-size `12 * NSIDE^2` array, filling missing pixels with `f64::NEG_INFINITY`
- Uses Rayon for parallel extraction of pixel/value pairs

**Metadata reading** (`healpix.rs: read_healpix_meta`):
- Iterates all HDU types (`XImage`, `XBinaryTable`, `XASCIITable`)
- Extracts `ORDERING` (String: "RING" or "NESTED"), `NSIDE` (Integer), `COORDSYS` (String: "G"/"C"/"E")
- Uses generic `Header<X>` trait from fitsrs

**Mask loading** (`mask.rs: PixelMask::from_fits_file`):
- Reads first binary table, column 0, interprets as boolean mask (values > 0.5 = valid)
- Calculates NSIDE from pixel count: `nside = sqrt(npix / 12)`

### Memory-mapped I/O

- Uses `memmap2::Mmap` to map FITS files into memory
- Wraps mapped bytes in `Cursor<&[u8]>` for fitsrs `Fits::from_reader()`
- Custom `MmapFitsReader` struct implementing `Read + BufRead + Seek` over mmap

### Metadata Caching

- Caches FITS metadata (NSIDE, ordering, indexing scheme) to `~/.cache/map2fig/` as JSON
- Caches column data as raw f64 binary files with a header (magic, version, pixel count)
- Cache invalidated by file modification time

## fitsrs Types and Functions Used

```rust
// Imports from fitsrs
use fitsrs::Fits;
use fitsrs::HDU;
use fitsrs::card::Value;
use fitsrs::hdu::data::bintable::{ColumnId, DataValue};
use fitsrs::hdu::header::Header;

// Construction
Fits::from_reader(reader)  // reader: BufReader<File> or Cursor<&[u8]>

// Iteration
fits.next() -> Option<Result<HDU>>

// HDU matching
HDU::XBinaryTable(hdu)
HDU::XImage(hdu)
HDU::XASCIITable(hdu)

// Header access
hdu.get_header() -> &Header<X>
header.get("KEYWORD") -> Option<&Value>

// Value matching
Value::String { value, .. }
Value::Integer { value, .. }

// Binary table data access
fits.get_data(&hdu) -> DataAccess
data.table_data() -> TableData
table.select_fields(&[ColumnId::Index(n)]) -> Iterator<Item = DataValue>

// DataValue matching
DataValue::Double { value, .. }
DataValue::Float { value, .. }
DataValue::Integer { value, .. }
DataValue::Long { value, .. }
```

## Data Types Read

| Data Type | fitsrs Variant | Usage |
|-----------|---------------|-------|
| f64 | `DataValue::Double` | Primary map data, mask values |
| f32 | `DataValue::Float` | Map data (converted to f64) |
| i32 | `DataValue::Integer` | Map data, pixel indices (converted to f64 or i64) |
| i64 | `DataValue::Long` | Pixel indices in sparse maps |

## fitsio-pure Readiness Assessment

### What Works Today

fitsio-pure already has complete binary table support with all the data types map2fig needs:

- **Binary table column reading by index:** `read_binary_column(data, hdu, col_index)` -- fully implemented
- **All required data types:** `BinaryColumnData::Double`, `Float`, `Int`, `Long` -- all present
- **Header keyword access:** Cards can be searched by keyword and values extracted as `Value::Integer`, `Value::String`, etc.
- **HDU iteration:** `parse_fits()` returns all HDUs which can be iterated and type-matched via `HduInfo::BinaryTable`
- **Multi-column reads:** Reading different columns from the same table -- supported

### What Does NOT Apply

- No image HDU reading needed (map2fig only uses binary tables)
- No FITS writing needed (map2fig is read-only)
- No ASCII table data reading needed (only header metadata is extracted from ASCII tables)

### Differences Requiring Adaptation

1. **API paradigm mismatch (streaming vs. in-memory):** fitsrs uses a streaming iterator pattern (`Fits::from_reader` + `fits.next()` + `fits.get_data()`). fitsio-pure uses an in-memory pattern (`parse_fits(&data)` returning all HDUs at once). map2fig would need to load the entire file into memory first (which it already does via mmap) and then call `parse_fits()`.

2. **Column access by name vs. index:** fitsrs uses `ColumnId::Index(n)` for field selection from a streaming table iterator. fitsio-pure uses `read_binary_column(data, hdu, col_index)` which reads an entire column at once. The end result is the same -- map2fig accesses columns by integer index.

3. **Header value types:** fitsrs uses `Value::String { value, .. }` and `Value::Integer { value, .. }` (struct variants with named fields). fitsio-pure uses `Value::String(s)` and `Value::Integer(n)` (tuple variants). This is a mechanical find-and-replace.

4. **HDU type discrimination:** fitsrs uses `HDU::XBinaryTable(hdu)` pattern matching. fitsio-pure uses `hdu.info` matching against `HduInfo::BinaryTable { .. }`. Conceptually identical.

5. **Return type for column data:** fitsrs returns individual `DataValue` enum variants per cell. fitsio-pure returns `BinaryColumnData` which is a typed vector (e.g., `BinaryColumnData::Double(Vec<f64>)`). This is actually more convenient for map2fig since it eliminates the per-cell match -- you get the entire column as a `Vec<f64>` directly.

### Porting Estimate

**Difficulty: Low-Medium.** The FITS usage is concentrated in 3 files (~400 lines of FITS-specific code total). The operations are straightforward: open file, find binary table, read header keywords, read columns by index, convert to `Vec<f64>`.

Key porting steps:
1. Replace `Fits::from_reader()` with `parse_fits(&bytes)` (~5 call sites)
2. Replace streaming HDU iteration with indexed HDU access (~3 loops)
3. Replace `DataValue` per-cell matching with `BinaryColumnData` per-column matching (~3 match blocks, becomes simpler)
4. Replace `header.get("KEY")` + `Value::String { value, .. }` with card search + `Value::String(s)` (~10 header reads)
5. Replace `ColumnId::Index(n)` + `select_fields()` with `read_binary_column(data, hdu, n)` (~5 call sites)

The mmap and caching infrastructure is independent of fitsrs and would remain unchanged.

**Estimated effort:** 2-4 hours for an experienced Rust developer, plus testing with real HEALPix FITS files.

### Why This Is Interesting

- map2fig currently uses fitsrs (another pure-Rust FITS library), so there is no cfitsio dependency to remove -- the value proposition is different. The pitch would be: fitsio-pure has a broader feature set, active development, and the compat layer for projects that might also want to interoperate with fitsio-based code.
- map2fig is performance-sensitive (they have extensive benchmarking and optimization tiers). fitsio-pure's in-memory approach with direct byte-offset column reading could be faster than fitsrs's streaming approach for large maps.
- The crate is very new (first release Feb 2026) so the author may be open to switching FITS backends early.
