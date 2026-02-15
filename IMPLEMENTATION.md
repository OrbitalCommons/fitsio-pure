# Implementation Plan: fitsio-pure

Pure Rust FITS reader/writer. Core must compile to `wasm32`. Support binaries handle disk I/O. A `compat` module provides API parity with the [`fitsio`](https://github.com/simonrw/rust-fitsio) crate.

## Architecture

```
fitsio-pure/
├── crates/
│   ├── fitsio-pure/          # Core library (no_std compatible, wasm32-safe)
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── block.rs      # 2880-byte block abstraction
│   │       ├── header.rs     # Keyword record parsing/writing
│   │       ├── value.rs      # Header value types (string, int, float, complex, logical)
│   │       ├── hdu.rs        # HDU struct, HduInfo enum
│   │       ├── image.rs      # Image HDU data handling
│   │       ├── table.rs      # ASCII table data handling
│   │       ├── bintable.rs   # Binary table data handling
│   │       ├── primary.rs    # Primary HDU specifics
│   │       ├── extension.rs  # Extension HDU dispatch
│   │       ├── io.rs         # Read/Write trait abstractions (no std::fs)
│   │       ├── endian.rs     # Big-endian byte conversion
│   │       └── error.rs      # Error types
│   ├── fitsio-compat/        # Compat layer mirroring fitsio crate API
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── fitsfile.rs   # FitsFile, NewFitsFile, FileOpenMode
│   │       ├── hdu.rs        # FitsHdu, HduInfo, DescribesHdu
│   │       ├── images.rs     # ImageDescription, ImageType, ReadImage, WriteImage
│   │       ├── tables.rs     # Column types, ReadsCol, WritesCol, FitsRow
│   │       ├── headers.rs    # ReadsKey, WritesKey, HeaderValue
│   │       └── errors.rs     # Error enum matching fitsio's
│   └── fitsio-tools/         # CLI binaries (disk I/O, not wasm)
│       └── src/
│           └── bin/
│               ├── fitsinfo.rs   # Print HDU summary
│               └── fitsconv.rs   # Convert between formats
```

## Work DAG

Tasks are grouped into layers. All tasks within a layer can be done in parallel. Each layer depends only on the layers above it.

```
Layer 0: Project Scaffolding
    ├── [S0] Cargo workspace setup
    └── [S1] CI setup (cargo test, cargo clippy, wasm32 build check)

Layer 1: Primitives (no dependencies between these)
    ├── [P0] block.rs    — 2880-byte block read/write/pad
    ├── [P1] endian.rs   — big-endian i16/i32/i64/f32/f64 conversion
    ├── [P2] error.rs    — error types
    ├── [P3] value.rs    — header value types: string, integer, float, logical, complex
    └── [P4] io.rs       — Read/Write/Seek trait abstractions over byte streams

Layer 2: Header Parsing (depends on P0–P4)
    ├── [H0] header.rs parse  — parse 80-char keyword records from blocks
    ├── [H1] header.rs write  — serialize keyword records into blocks
    └── [H2] header.rs validate — required-keyword checks per HDU type

Layer 3: HDU Skeleton (depends on H0–H2)
    ├── [D0] primary.rs   — parse/write primary HDU header (SIMPLE, BITPIX, NAXIS, etc.)
    ├── [D1] extension.rs — parse XTENSION and dispatch to image/table/bintable
    └── [D2] hdu.rs       — HduInfo enum, HDU navigation, iteration

Layer 4: Data Units (all parallel, each depends on Layer 2 + relevant Layer 3 item)
    ├── [I0] image.rs read   — read image data (all BITPIX types), apply BSCALE/BZERO
    ├── [I1] image.rs write  — write image data, compute padding
    ├── [T0] table.rs read   — read ASCII table (TFORMn: A, I, F, E, D), TBCOLn positioning
    ├── [T1] table.rs write  — write ASCII table
    ├── [B0] bintable.rs read  — read binary table (fixed-length columns: L,X,B,I,J,K,E,D,C,M,A)
    ├── [B1] bintable.rs write — write binary table
    ├── [B2] bintable.rs heap  — variable-length array (P descriptor) read/write
    └── [I2] image.rs region — sub-region / row / section reads

Layer 5: Round-Trip Integration (depends on all of Layer 4)
    ├── [R0] Read real-world FITS files — download a small set of public .fits test files, verify parse
    ├── [R1] Write-then-read round-trip — create files in-memory, re-read, assert equality
    └── [R2] wasm32 build gate — ensure `cargo build --target wasm32-unknown-unknown -p fitsio-pure` passes

Layer 6: Compat Crate (depends on Layer 4, parallel within)
    ├── [C0] errors.rs     — map fitsio-pure errors to fitsio-style Error enum
    ├── [C1] fitsfile.rs   — FitsFile::open/edit/create, NewFitsFile builder, FileOpenMode
    ├── [C2] hdu.rs        — FitsHdu with read_key/write_key, DescribesHdu trait
    ├── [C3] images.rs     — ImageDescription, ImageType, ReadImage/WriteImage traits
    ├── [C4] tables.rs     — ColumnDescription, ColumnDataType, ReadsCol/WritesCol, Column enum
    ├── [C5] headers.rs    — ReadsKey/WritesKey traits, HeaderValue<T>
    └── [C6] FitsRow derive — proc-macro or manual impl for struct↔row mapping

Layer 7: Compat Testing (depends on Layer 6)
    ├── [CT0] Port fitsio's own test suite — adapt tests from reference/rust-fitsio to run against compat
    └── [CT1] API signature parity check — compile-time test that compat exposes the same public types

Layer 8: CLI Tools (depends on Layer 4, parallel with Layer 6)
    ├── [L0] fitsinfo binary — print HDU list, shapes, keywords
    └── [L1] fitsconv binary — basic format conversion / HDU extraction
```

## Detailed Task Descriptions

### Layer 0 — Scaffolding

**[S0] Cargo workspace**
- Create `Cargo.toml` workspace with three members
- `fitsio-pure`: `[lib]`, default-features include `std`, with `no_std` support behind feature flag
- `fitsio-compat`: depends on `fitsio-pure`
- `fitsio-tools`: depends on `fitsio-pure`, `[[bin]]` targets

**[S1] CI**
- GitHub Actions: `cargo test --workspace`, `cargo clippy`, `cargo build --target wasm32-unknown-unknown -p fitsio-pure`

### Layer 1 — Primitives

**[P0] block.rs**
- `const BLOCK_SIZE: usize = 2880`
- `const CARD_SIZE: usize = 80`
- `const CARDS_PER_BLOCK: usize = 36`
- Read/write full blocks from a byte stream
- Pad final block with zeros (data) or ASCII spaces (header)
- Compute number of blocks needed for N bytes

**[P1] endian.rs**
- `read_i16_be`, `read_i32_be`, `read_i64_be`, `read_f32_be`, `read_f64_be` from `&[u8]`
- Corresponding write functions to `&mut [u8]`
- Bulk conversion: `convert_buffer_to_native<T>(buf: &mut [u8])` for image arrays
- Use `u16::from_be_bytes` etc. — no unsafe needed

**[P2] error.rs**
- `enum Error { InvalidHeader, UnexpectedEof, InvalidBitpix, InvalidKeyword, UnsupportedExtension, Io(io::Error), ... }`
- `type Result<T> = core::result::Result<T, Error>`

**[P3] value.rs**
- `enum Value { Logical(bool), Integer(i64), Float(f64), String(String), ComplexInt(i64,i64), ComplexFloat(f64,f64) }`
- Parse from 80-char card bytes 10..80
- Serialize back to fixed-format bytes

**[P4] io.rs**
- Trait `FitsRead: Read + Seek` and `FitsWrite: Write + Seek`
- `Cursor<Vec<u8>>` impl for in-memory / wasm use
- `std::fs::File` impl behind `std` feature gate

### Layer 2 — Headers

**[H0] Parse keywords**
- Split 80-byte cards into keyword name (0..8), value indicator (8..10), value+comment (10..80)
- Handle CONTINUE long-string convention
- Return `Vec<Card>` where `Card { keyword: [u8; 8], value: Option<Value>, comment: Option<String> }`

**[H1] Write keywords**
- Serialize `Card` back to 80 bytes with correct alignment
- Ensure keyword names are uppercase, left-justified, space-padded to 8
- Right-justify numeric values in columns 11–30
- Append END card; pad remaining block with blank cards

**[H2] Validate required keywords**
- Given an HDU type, check presence and ordering of mandatory keywords (Tables 7, 10, 13, 14, 17 in the spec)
- Return structured errors listing what's missing

### Layer 3 — HDU Skeleton

**[D0] Primary HDU**
- Parse SIMPLE, BITPIX, NAXIS, NAXISn
- Compute data byte count: `|BITPIX/8| × NAXIS1 × ... × NAXISn × GCOUNT × (PCOUNT + product_of_axes)`
- Skip data section (seek forward) or read into buffer

**[D1] Extensions**
- Read XTENSION value → dispatch to image/table/bintable parser
- Parse shared mandatory keywords (BITPIX, NAXIS, NAXISn, PCOUNT, GCOUNT)

**[D2] HDU navigation**
- `struct FitsData { hdus: Vec<Hdu> }` lazily populated
- Index by number or by EXTNAME string
- Iterator over HDUs

### Layer 4 — Data Units

**[I0] Image read**
- Read raw bytes → convert from big-endian based on BITPIX
- Apply BSCALE/BZERO if present (output as f64 or original type)
- Support all BITPIX values: 8, 16, 32, 64, -32, -64
- Fortran-order indexing (first axis varies fastest)

**[I1] Image write**
- Accept typed slice → convert to big-endian bytes
- Write BITPIX/NAXIS/NAXISn keywords
- Pad final block

**[I2] Image region**
- Read sub-regions, row ranges, and flat sections without loading entire image
- Seek to correct byte offset for the requested slice

**[T0] ASCII table read**
- Parse TFORMn (A, I, F, E, D) and TBCOLn
- Read row-by-row, extract fields by byte position
- Return typed column vectors

**[T1] ASCII table write**
- Format values according to TFORMn
- Position fields at TBCOLn
- Pad rows to NAXIS1 bytes

**[B0] Binary table read (fixed)**
- Parse TFORMn (rT format codes: L, X, B, I, J, K, E, D, C, M, A)
- Read row buffer, split into columns by byte offset
- Convert each column from big-endian to native

**[B1] Binary table write (fixed)**
- Serialize typed column data into row-major byte buffer
- Write TFORMn, TFIELDS keywords

**[B2] Binary table heap**
- Read variable-length arrays via P descriptors
- Manage THEAP offset, read/write heap area after main table

### Layer 5 — Integration

**[R0] Real-world files**
- Download 3–5 small public FITS files (Hubble, Chandra, or synthetic)
- Parse each, verify HDU counts, keyword values, image dimensions

**[R1] Round-trip**
- Create image HDU in-memory → serialize → re-parse → assert pixel equality
- Create binary table → serialize → re-parse → assert column equality
- Create multi-extension file → round-trip

**[R2] wasm32 gate**
- `cargo build --target wasm32-unknown-unknown -p fitsio-pure` must succeed
- No `std::fs`, no `std::net` in core crate path

### Layer 6 — Compat Crate

**[C0] Error mapping**
- `compat::Error` enum with variants matching `fitsio::errors::Error`
- `From<fitsio_pure::Error>` impl

**[C1] FitsFile**
- `FitsFile::open(path)`, `FitsFile::edit(path)`, `FitsFile::create(path) -> NewFitsFile`
- `NewFitsFile` builder with `.with_custom_primary()`, `.overwrite()`, `.open()`
- `hdu()`, `primary_hdu()`, `num_hdus()`, `iter()`
- `create_table()`, `create_image()`

**[C2] FitsHdu**
- `read_key<T: ReadsKey>()`, `write_key<T: WritesKey>()`
- Image: `read_image()`, `read_section()`, `read_rows()`, `read_region()`, `write_image()`, `write_section()`, `write_region()`, `resize()`
- Table: `read_col()`, `read_cell_value()`, `row()`, `write_col()`, `write_col_range()`, `columns()`
- `copy_to()`, `delete()`, `name()`

**[C3] Image types**
- `ImageDescription { data_type, dimensions }`
- `enum ImageType { UnsignedByte, Byte, Short, ..., Double }`
- `ReadImage` / `WriteImage` traits for `Vec<u8>`, `Vec<i16>`, ..., `Vec<f64>`

**[C4] Table types**
- `ColumnDescription` builder → `ConcreteColumnDescription`
- `ColumnDataDescription { repeat, width, typ }`
- `enum ColumnDataType { Logical, Bit, Byte, ..., String }`
- `enum Column { Int32 { name, data }, Float { name, data }, ... }`
- `ReadsCol` / `WritesCol` traits

**[C5] Header types**
- `ReadsKey` / `WritesKey` traits for `i32, i64, f32, f64, bool, String`
- `HeaderValue<T> { value, comment }`

**[C6] FitsRow**
- Proc-macro derive or manual trait for struct ↔ table row mapping
- `#[fitsio(colname = "...")]` attribute support

### Layer 7 — Compat Tests

**[CT0] Port test suite**
- Adapt tests from `reference/rust-fitsio/fitsio/tests/` to compile against `fitsio-compat`
- Goal: same test logic, same assertions, different import path

**[CT1] API parity**
- Compile-time checks that all public types/traits from `fitsio` are re-exported by `fitsio-compat`

### Layer 8 — CLI Tools

**[L0] fitsinfo**
- Open file, iterate HDUs, print type/shape/keywords

**[L1] fitsconv**
- Extract single HDU to new file, or convert image BITPIX type

## Parallelism Summary

| Phase | Parallel tasks | Depends on |
|---|---|---|
| Layer 0 | S0, S1 | — |
| Layer 1 | P0, P1, P2, P3, P4 | S0 |
| Layer 2 | H0, H1, H2 | P0–P4 |
| Layer 3 | D0, D1, D2 | H0–H2 |
| Layer 4 | I0, I1, I2, T0, T1, B0, B1, B2 | Layer 2 + Layer 3 |
| Layer 5 | R0, R1, R2 | Layer 4 |
| Layer 6 | C0, C1, C2, C3, C4, C5, C6 | Layer 4 |
| Layer 7 | CT0, CT1 | Layer 6 |
| Layer 8 | L0, L1 | Layer 4 |

Layers 5, 6, and 8 are all independent of each other and can proceed in parallel once Layer 4 is complete.

## Testing Strategy

Every layer has its own test surface:

- **Layer 1**: Unit tests with hardcoded byte sequences. Endian round-trips, block padding, value parsing.
- **Layer 2**: Parse/write round-trip on hand-crafted 80-byte cards. Validate required keywords against known-good and known-bad headers.
- **Layer 3**: Construct minimal valid FITS byte streams in-memory, parse HDU structure.
- **Layer 4**: Construct single-HDU FITS files in-memory with known pixel/column values. Assert exact equality after read.
- **Layer 5**: Real .fits files from public archives. Golden-file tests.
- **Layer 6–7**: Port fitsio crate tests; compile-time type checks.
- **Layer 8**: Integration tests invoking binaries with test files, checking stdout output.

All tests in Layers 1–4 run in `no_std` mode (in-memory buffers) to guarantee wasm32 compatibility.
