//! Tile-compressed image writing (`RICE_1`, `GZIP_1`).
//!
//! [`compress_image_hdu`] and [`write_compressed_image`] write an image as a
//! tile-compressed binary table extension, the `ZIMAGE` convention that
//! cfitsio, fpack and astropy read. The image is cut into tiles (one row
//! each by default), every tile is compressed on its own, and the
//! compressed bytes go into the table's heap. [`read_tiled_image`] reads the
//! result back, as do cfitsio and `astropy.io.fits`.
//!
//! Integer images are compressed losslessly. `f32` and `f64` images are
//! quantized to integers first, per tile, as cfitsio does: the step is the
//! tile's noise, estimated from the median of its pixel differences, divided
//! by the [`Quantize::level`], and `SUBTRACTIVE_DITHER_1` dithering keeps the
//! values unbiased. A tile that can't be quantized (a constant tile, say) is
//! stored losslessly with gzip, and NaN pixels are stored as nulls. With
//! `GZIP_1`, floats can also be stored losslessly with
//! [`TileCompression::lossless`].
//!
//! The output is one HDU, an extension, written to any
//! [`Write`] sink or returned as bytes, so it works in
//! memory and on `wasm32`. A file needs a primary HDU before it:
//!
//! ```
//! use fitsio_pure::compress::{compress_image_hdu, TileCompression};
//! use fitsio_pure::header::serialize_header;
//! use fitsio_pure::primary::build_primary_header;
//!
//! let pixels: Vec<u16> = (0..64 * 48).map(|i| (i * 7 % 4096) as u16).collect();
//! let mut file = serialize_header(&build_primary_header(8, &[]).unwrap()).unwrap();
//! file.extend(compress_image_hdu(&[64, 48], &pixels, &TileCompression::rice(), &[]).unwrap());
//!
//! let parsed = fitsio_pure::hdu::parse_fits(&file).unwrap();
//! let image = fitsio_pure::image::read_image_data(&file, &parsed.hdus[1]).unwrap();
//! # let _ = image;
//! ```
//!
//! The Rice encoder, the noise estimate and the quantization are ports of
//! cfitsio's `ricecomp.c`, `quantize.c` and `imcompress.c`. They were
//! checked against the encoders in sunipkm/refimage (MIT OR Apache-2.0),
//! whose design this module follows.
//!
//! [`read_tiled_image`]: crate::tiled::read_tiled_image

use alloc::format;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

use crate::block::padded_byte_len;
use crate::error::{Error, Result};
use crate::header::{serialize_header, Card};
use crate::io::Write;
use crate::tiled::{dither_table, N_RANDOM};
use crate::value::Value;

/// cfitsio's `NULL_VALUE`: the quantized integer for a null (NaN) pixel,
/// written as `ZBLANK`.
const NULL_VALUE: i32 = -2_147_483_647;

/// cfitsio's `N_RESERVED_VALUES`: integers kept free below the quantized range.
const N_RESERVED_VALUES: f64 = 10.0;

/// Rice coding block size, `ZVAL1` for `BLOCKSIZE`. cfitsio always uses 32.
const RICE_BLOCK: usize = 32;

/// A tile-compression algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {
    /// `RICE_1`: Rice coding of pixel differences, cfitsio's default.
    Rice,
    /// `GZIP_1`: DEFLATE of each tile's big-endian pixel bytes.
    Gzip,
}

/// How an image is cut into tiles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tiling {
    /// Tiles of whole image rows, this many rows each. One row is cfitsio's
    /// default.
    Rows(usize),
    /// Explicit tile dimensions, `NAXIS1` first. Missing trailing dimensions
    /// are 1; edge tiles are clipped to the image.
    Dims(Vec<usize>),
}

/// The dithering applied when quantizing floats, `ZQUANTIZ`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dither {
    /// `SUBTRACTIVE_DITHER_1`: add a seeded random offset before rounding and
    /// subtract it again when reading. cfitsio's default.
    Subtractive,
    /// `NO_DITHER`: round without dithering.
    None,
}

/// Where the dither sequence starts, `ZDITHER0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DitherSeed {
    /// Derived from the first tile's bytes, as cfitsio does for a negative
    /// seed request, so the same image always compresses the same way.
    Checksum,
    /// This seed, from 1 to 10000.
    Fixed(u16),
}

/// Quantization of `f32`/`f64` images.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quantize {
    /// cfitsio's `q`, held as `f64` so [`Quantize::step`] is exact; an `f32`
    /// level converts to it without change.
    level: f64,
    dither: Dither,
    seed: DitherSeed,
}

impl Default for Quantize {
    fn default() -> Self {
        Quantize {
            level: 4.0,
            dither: Dither::Subtractive,
            seed: DitherSeed::Checksum,
        }
    }
}

impl Quantize {
    /// cfitsio's defaults: level 4, `SUBTRACTIVE_DITHER_1`, seed from the
    /// data.
    pub fn new() -> Self {
        Self::default()
    }

    /// The quantization level `q`. A positive `q` sets each tile's step to
    /// its noise divided by `q`, so larger is finer; a negative `q` sets the
    /// step to `-q` in every tile. 0 means the default, 4.
    pub fn level(mut self, q: f32) -> Self {
        self.level = f64::from(q);
        self
    }

    /// A fixed quantization step for every tile, as a negative
    /// [`level`](Self::level) gives, but exact: `level` takes an `f32`, as
    /// cfitsio does, which rounds a step computed in `f64`.
    pub fn step(mut self, step: f64) -> Self {
        self.level = -step.abs();
        self
    }

    /// The dithering method.
    pub fn dither(mut self, dither: Dither) -> Self {
        self.dither = dither;
        self
    }

    /// The dither seed.
    pub fn seed(mut self, seed: DitherSeed) -> Self {
        self.seed = seed;
        self
    }
}

/// Settings for writing a tile-compressed image.
#[derive(Debug, Clone, PartialEq)]
pub struct TileCompression {
    algorithm: Algorithm,
    tiling: Tiling,
    gzip_level: u8,
    quantize: Option<Quantize>,
}

impl TileCompression {
    /// `RICE_1`, one row per tile, floats quantized with [`Quantize::new`].
    pub fn rice() -> Self {
        Self::new(Algorithm::Rice)
    }

    /// `GZIP_1`, one row per tile, DEFLATE level 1 as cfitsio uses, floats
    /// quantized with [`Quantize::new`].
    pub fn gzip() -> Self {
        Self::new(Algorithm::Gzip)
    }

    /// `algorithm` with the defaults above.
    pub fn new(algorithm: Algorithm) -> Self {
        TileCompression {
            algorithm,
            tiling: Tiling::Rows(1),
            gzip_level: 1,
            quantize: Some(Quantize::new()),
        }
    }

    /// The compression algorithm.
    pub fn algorithm(&self) -> Algorithm {
        self.algorithm
    }

    /// Tiles of `rows` whole image rows.
    pub fn tile_rows(mut self, rows: usize) -> Self {
        self.tiling = Tiling::Rows(rows);
        self
    }

    /// Tiles of these dimensions, `NAXIS1` first.
    pub fn tile_dims(mut self, dims: &[usize]) -> Self {
        self.tiling = Tiling::Dims(dims.to_vec());
        self
    }

    /// The tiling.
    pub fn tiling(mut self, tiling: Tiling) -> Self {
        self.tiling = tiling;
        self
    }

    /// The DEFLATE level for `GZIP_1`, 0 to 9.
    pub fn gzip_level(mut self, level: u8) -> Self {
        self.gzip_level = level.min(9);
        self
    }

    /// How floats are quantized.
    pub fn quantize(mut self, quantize: Quantize) -> Self {
        self.quantize = Some(quantize);
        self
    }

    /// Store floats without quantizing, `ZQUANTIZ = 'NONE'`. Only `GZIP_1`
    /// supports this.
    pub fn lossless(mut self) -> Self {
        self.quantize = None;
        self
    }
}

mod sealed {
    pub trait Sealed {}
}

/// A pixel type that can be written tile-compressed: `u8`, `i8`, `i16`,
/// `u16`, `i32`, `u32`, `f32` or `f64`.
///
/// `i8`, `u16` and `u32` are stored as cfitsio stores them: offset to the
/// next signed (or, for `i8`, unsigned) type, with `BZERO` recording the
/// offset. 64-bit integers can't be tile-compressed, as in cfitsio.
pub trait CompressPixel: sealed::Sealed + Copy {
    #[doc(hidden)]
    fn compress_tiles(pixels: &[Self], layout: &Layout, opts: &TileCompression) -> Result<Tiles>;
}

/// The stored form of an integer pixel type.
struct IntStorage {
    zbitpix: i64,
    bzero: Option<(i64, &'static str)>,
}

macro_rules! int_pixel {
    ($t:ty, $zbitpix:expr, $bzero:expr, |$v:ident| $stored:expr) => {
        impl sealed::Sealed for $t {}
        impl CompressPixel for $t {
            fn compress_tiles(
                pixels: &[Self],
                layout: &Layout,
                opts: &TileCompression,
            ) -> Result<Tiles> {
                let storage = IntStorage {
                    zbitpix: $zbitpix,
                    bzero: $bzero,
                };
                compress_ints(pixels, layout, opts, &storage, |$v: $t| $stored)
            }
        }
    };
}

int_pixel!(u8, 8, None, |v| i32::from(v));
int_pixel!(
    i8,
    8,
    Some((-128, "offset data range to that of signed byte")),
    |v| i32::from(v) + 128
);
int_pixel!(i16, 16, None, |v| i32::from(v));
int_pixel!(
    u16,
    16,
    Some((32768, "offset data range to that of unsigned short")),
    |v| i32::from(v) - 32768
);
int_pixel!(i32, 32, None, |v| v);
int_pixel!(
    u32,
    32,
    Some((2_147_483_648, "offset data range to that of unsigned long")),
    |v| (v ^ 0x8000_0000) as i32
);

impl sealed::Sealed for f32 {}
impl CompressPixel for f32 {
    fn compress_tiles(pixels: &[Self], layout: &Layout, opts: &TileCompression) -> Result<Tiles> {
        compress_floats(pixels, layout, opts)
    }
}

impl sealed::Sealed for f64 {}
impl CompressPixel for f64 {
    fn compress_tiles(pixels: &[Self], layout: &Layout, opts: &TileCompression) -> Result<Tiles> {
        compress_floats(pixels, layout, opts)
    }
}

/// Write `pixels` (`NAXIS1` fastest) as one tile-compressed image extension
/// and return its bytes, block-padded.
///
/// `naxes` lists the axis lengths, `NAXIS1` first. `extra` cards, such as
/// `EXTNAME`, follow the compression keywords; structural keywords among
/// them are an error.
pub fn compress_image_hdu<T: CompressPixel>(
    naxes: &[usize],
    pixels: &[T],
    compression: &TileCompression,
    extra: &[Card],
) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    write_hdu(&mut out, naxes, pixels, compression, extra, false)?;
    Ok(out)
}

#[cfg(feature = "compat")]
/// [`compress_image_hdu`] for an image that was the file's primary HDU, which
/// cfitsio marks with `ZSIMPLE` so it can be restored as one.
pub(crate) fn compress_primary_image_hdu<T: CompressPixel>(
    naxes: &[usize],
    pixels: &[T],
    compression: &TileCompression,
    extra: &[Card],
) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    write_hdu(&mut out, naxes, pixels, compression, extra, true)?;
    Ok(out)
}

/// Write `pixels` as one tile-compressed image extension to `sink`, as
/// [`compress_image_hdu`] does, and return the sink.
///
/// The tiles are compressed in memory first, since the header records the
/// heap's size; only the compressed bytes are held.
pub fn write_compressed_image<W: Write, T: CompressPixel>(
    mut sink: W,
    naxes: &[usize],
    pixels: &[T],
    compression: &TileCompression,
    extra: &[Card],
) -> Result<W> {
    let mut out = Vec::new();
    write_hdu(&mut out, naxes, pixels, compression, extra, false)?;
    sink.write_all(&out)?;
    Ok(sink)
}

fn write_hdu<T: CompressPixel>(
    out: &mut Vec<u8>,
    naxes: &[usize],
    pixels: &[T],
    compression: &TileCompression,
    extra: &[Card],
    zsimple: bool,
) -> Result<()> {
    for card in extra {
        if is_structural(card.keyword_str()) {
            return Err(Error::InvalidHeader(
                "extra cards can't set structural or compression keywords",
            ));
        }
    }
    let layout = Layout::new(naxes, &compression.tiling)?;
    if pixels.len() != layout.pixels() {
        return Err(Error::InvalidHeader("pixel count doesn't match NAXISn"));
    }
    let tiles = T::compress_tiles(pixels, &layout, compression)?;
    let table = tiles.table();
    let cards = header_cards(&layout, compression, &tiles, &table, extra, zsimple);
    out.extend_from_slice(&serialize_header(&cards)?);
    let data_start = out.len();
    out.extend_from_slice(&table.rows);
    for tile in &tiles.tiles {
        out.extend_from_slice(&tile.bytes);
    }
    let data_len = out.len() - data_start;
    out.resize(data_start + padded_byte_len(data_len), 0);
    Ok(())
}

/// Keywords the writer sets itself.
fn is_structural(keyword: &str) -> bool {
    const FIXED: [&str; 17] = [
        "SIMPLE", "XTENSION", "BITPIX", "NAXIS", "PCOUNT", "GCOUNT", "TFIELDS", "THEAP", "EXTEND",
        "ZIMAGE", "ZSIMPLE", "ZBITPIX", "ZNAXIS", "ZCMPTYPE", "ZQUANTIZ", "ZDITHER0", "ZBLANK",
    ];
    const INDEXED: [&str; 8] = [
        "NAXIS", "TTYPE", "TFORM", "ZNAXIS", "ZTILE", "ZNAME", "ZVAL", "TDIM",
    ];
    FIXED.contains(&keyword)
        || INDEXED.iter().any(|prefix| {
            keyword
                .strip_prefix(prefix)
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
}

/// The image's axes and its tile grid.
#[doc(hidden)]
pub struct Layout {
    naxes: Vec<usize>,
    ztile: Vec<usize>,
}

impl Layout {
    fn new(naxes: &[usize], tiling: &Tiling) -> Result<Self> {
        if naxes.is_empty() || naxes.contains(&0) {
            return Err(Error::InvalidHeader(
                "a tile-compressed image needs at least one non-empty axis",
            ));
        }
        let ztile: Vec<usize> = match tiling {
            Tiling::Rows(rows) => {
                if *rows == 0 {
                    return Err(Error::InvalidHeader("tiles need at least one row"));
                }
                naxes
                    .iter()
                    .enumerate()
                    .map(|(axis, &len)| match axis {
                        0 => len,
                        1 => (*rows).min(len),
                        _ => 1,
                    })
                    .collect()
            }
            Tiling::Dims(dims) => {
                if dims.len() > naxes.len() || dims.contains(&0) {
                    return Err(Error::InvalidHeader(
                        "tile dimensions must be non-zero, one per axis at most",
                    ));
                }
                naxes
                    .iter()
                    .enumerate()
                    .map(|(axis, &len)| dims.get(axis).copied().unwrap_or(1).min(len))
                    .collect()
            }
        };
        Ok(Layout {
            naxes: naxes.to_vec(),
            ztile,
        })
    }

    fn pixels(&self) -> usize {
        self.naxes.iter().product()
    }

    /// Every tile, in the order cfitsio numbers them: `NAXIS1` fastest.
    fn tiles(&self) -> Vec<TileRect> {
        let counts: Vec<usize> = self
            .naxes
            .iter()
            .zip(&self.ztile)
            .map(|(&len, &tile)| len.div_ceil(tile))
            .collect();
        let mut index = vec![0usize; self.naxes.len()];
        let mut tiles = Vec::with_capacity(counts.iter().product());
        loop {
            let start: Vec<usize> = index.iter().zip(&self.ztile).map(|(i, t)| i * t).collect();
            let extent = start
                .iter()
                .zip(&self.ztile)
                .zip(&self.naxes)
                .map(|((&s, &t), &len)| t.min(len - s))
                .collect();
            tiles.push(TileRect { start, extent });
            let mut axis = 0;
            loop {
                if axis == index.len() {
                    return tiles;
                }
                index[axis] += 1;
                if index[axis] < counts[axis] {
                    break;
                }
                index[axis] = 0;
                axis += 1;
            }
        }
    }

    /// The pixels of `tile`, in raster order within the tile.
    fn gather<T: Copy, S>(&self, pixels: &[T], tile: &TileRect, f: impl Fn(T) -> S) -> Vec<S> {
        let strides: Vec<usize> = self
            .naxes
            .iter()
            .scan(1usize, |stride, &len| {
                let this = *stride;
                *stride *= len;
                Some(this)
            })
            .collect();
        let mut out = Vec::with_capacity(tile.extent.iter().product());
        let mut index = vec![0usize; tile.extent.len()];
        loop {
            let first: usize = tile
                .start
                .iter()
                .zip(&index)
                .zip(&strides)
                .map(|((&s, &i), &stride)| (s + i) * stride)
                .sum();
            out.extend(pixels[first..first + tile.extent[0]].iter().map(|&v| f(v)));
            let mut axis = 1;
            loop {
                if axis >= index.len() {
                    return out;
                }
                index[axis] += 1;
                if index[axis] < tile.extent[axis] {
                    break;
                }
                index[axis] = 0;
                axis += 1;
            }
        }
    }
}

/// One tile's place in the image.
struct TileRect {
    start: Vec<usize>,
    extent: Vec<usize>,
}

impl TileRect {
    /// The tile as 2-D rows for the noise estimate, as cfitsio's
    /// `fits_calc_tile_rows` collapses it: the first axis longer than one
    /// pixel is the row, the rest multiply into the row count.
    fn rows(&self) -> (usize, usize) {
        let mut long = self.extent.iter().copied().filter(|&n| n > 1);
        let row = long.next().unwrap_or(1);
        (row, long.product())
    }
}

/// One compressed tile.
struct Tile {
    /// The tile's bytes in the heap.
    bytes: Vec<u8>,
    /// Whether the bytes are a gzip of the raw pixels in the
    /// `GZIP_COMPRESSED_DATA` column, used when a float tile can't be
    /// quantized.
    fallback: bool,
    /// `ZSCALE` and `ZZERO` of a quantized tile.
    scale: Option<(f64, f64)>,
}

/// Every compressed tile of an image, and what the header needs to say
/// about them.
#[doc(hidden)]
pub struct Tiles {
    tiles: Vec<Tile>,
    zbitpix: i64,
    /// `ZVAL2` for `BYTEPIX`.
    bytepix: usize,
    bzero: Option<(i64, &'static str)>,
    /// `ZQUANTIZ` and, when dithered, `ZDITHER0`, for a float image.
    quantize: Option<(Option<Dither>, u16)>,
    /// Whether any pixel was null, so `ZBLANK` is needed.
    nulls: bool,
}

/// The binary table: its rows, and the layout the header describes.
struct Table {
    rows: Vec<u8>,
    row_bytes: usize,
    /// `Q` descriptors, for a heap too big for 32-bit offsets.
    wide: bool,
    scale_columns: bool,
    fallback_column: bool,
    max_len: usize,
    max_fallback_len: usize,
    heap: usize,
}

impl Tiles {
    fn table(&self) -> Table {
        let heap: usize = self.tiles.iter().map(|t| t.bytes.len()).sum();
        let wide = heap > i32::MAX as usize;
        let descriptor = if wide { 16 } else { 8 };
        let scale_columns = self.quantize.is_some_and(|(method, _)| method.is_some());
        let fallback_column = self.tiles.iter().any(|t| t.fallback);
        let row_bytes = descriptor
            + if scale_columns { 16 } else { 0 }
            + if fallback_column { descriptor } else { 0 };
        let mut rows = Vec::with_capacity(row_bytes * self.tiles.len());
        let mut offset = 0usize;
        let put = |rows: &mut Vec<u8>, len: usize, offset: usize| {
            if wide {
                rows.extend_from_slice(&(len as u64).to_be_bytes());
                rows.extend_from_slice(&(offset as u64).to_be_bytes());
            } else {
                rows.extend_from_slice(&(len as u32).to_be_bytes());
                rows.extend_from_slice(&(offset as u32).to_be_bytes());
            }
        };
        for tile in &self.tiles {
            let len = tile.bytes.len();
            let (compressed, fallback) = if tile.fallback {
                ((0, 0), (len, offset))
            } else {
                ((len, offset), (0, 0))
            };
            put(&mut rows, compressed.0, compressed.1);
            if scale_columns {
                let (scale, zero) = tile.scale.unwrap_or((0.0, 0.0));
                rows.extend_from_slice(&scale.to_be_bytes());
                rows.extend_from_slice(&zero.to_be_bytes());
            }
            if fallback_column {
                put(&mut rows, fallback.0, fallback.1);
            }
            offset += len;
        }
        let max = |fallback: bool| {
            self.tiles
                .iter()
                .filter(|t| t.fallback == fallback)
                .map(|t| t.bytes.len())
                .max()
                .unwrap_or(0)
        };
        Table {
            rows,
            row_bytes,
            wide,
            scale_columns,
            fallback_column,
            max_len: max(false),
            max_fallback_len: max(true),
            heap,
        }
    }
}

fn card(keyword: &str, value: Value, comment: &str) -> Card {
    let mut name = [b' '; 8];
    name[..keyword.len()].copy_from_slice(keyword.as_bytes());
    Card {
        keyword: name,
        value: Some(value),
        comment: Some(comment.to_string()),
    }
}

fn int(n: usize) -> Value {
    Value::Integer(n as i64)
}

fn string(s: &str) -> Value {
    Value::String(s.to_string())
}

/// The header cfitsio writes for a compressed image, with `extra` after it.
fn header_cards(
    layout: &Layout,
    opts: &TileCompression,
    tiles: &Tiles,
    table: &Table,
    extra: &[Card],
    zsimple: bool,
) -> Vec<Card> {
    let vla = |max: usize| {
        let code = if table.wide { 'Q' } else { 'P' };
        format!("1{code}B({max})")
    };
    let mut columns = vec![("COMPRESSED_DATA", vla(table.max_len))];
    if table.scale_columns {
        columns.push(("ZSCALE", "1D".to_string()));
        columns.push(("ZZERO", "1D".to_string()));
    }
    if table.fallback_column {
        columns.push(("GZIP_COMPRESSED_DATA", vla(table.max_fallback_len)));
    }

    let mut cards = vec![
        card("XTENSION", string("BINTABLE"), "binary table extension"),
        card("BITPIX", int(8), "8-bit bytes"),
        card("NAXIS", int(2), "2-dimensional binary table"),
        card("NAXIS1", int(table.row_bytes), "width of table in bytes"),
        card("NAXIS2", int(tiles.tiles.len()), "number of rows in table"),
        card("PCOUNT", int(table.heap), "size of special data area"),
        card("GCOUNT", int(1), "one data group (required keyword)"),
        card(
            "TFIELDS",
            int(columns.len()),
            "number of fields in each row",
        ),
    ];
    for (i, (name, form)) in columns.into_iter().enumerate() {
        let n = i + 1;
        let kind = if form.starts_with("1D") {
            "data format of field: 8-byte DOUBLE"
        } else {
            "data format of field: variable length array"
        };
        cards.push(card(
            &format!("TTYPE{n}"),
            string(name),
            &format!("label for field {n:>3}"),
        ));
        cards.push(card(&format!("TFORM{n}"), Value::String(form), kind));
    }
    cards.push(card(
        "ZIMAGE",
        Value::Logical(true),
        "extension contains compressed image",
    ));
    if zsimple {
        cards.push(card(
            "ZSIMPLE",
            Value::Logical(true),
            "file does conform to FITS standard",
        ));
    }
    cards.push(card(
        "ZBITPIX",
        Value::Integer(tiles.zbitpix),
        "data type of original image",
    ));
    cards.push(card(
        "ZNAXIS",
        int(layout.naxes.len()),
        "dimension of original image",
    ));
    for (i, &len) in layout.naxes.iter().enumerate() {
        cards.push(card(
            &format!("ZNAXIS{}", i + 1),
            int(len),
            "length of original image axis",
        ));
    }
    for (i, &len) in layout.ztile.iter().enumerate() {
        cards.push(card(
            &format!("ZTILE{}", i + 1),
            int(len),
            "size of tiles to be compressed",
        ));
    }
    match tiles.quantize {
        Some((None, _)) => cards.push(card(
            "ZQUANTIZ",
            string("NONE"),
            "Lossless compression without quantization",
        )),
        Some((Some(Dither::Subtractive), seed)) => {
            cards.push(card(
                "ZQUANTIZ",
                string("SUBTRACTIVE_DITHER_1"),
                "Pixel Quantization Algorithm",
            ));
            cards.push(card(
                "ZDITHER0",
                int(usize::from(seed)),
                "dithering offset when quantizing floats",
            ));
        }
        Some((Some(Dither::None), _)) => cards.push(card(
            "ZQUANTIZ",
            string("NO_DITHER"),
            "No dithering during quantization",
        )),
        None => {}
    }
    let zcmptype = match opts.algorithm {
        Algorithm::Rice => "RICE_1",
        Algorithm::Gzip => "GZIP_1",
    };
    cards.push(card("ZCMPTYPE", string(zcmptype), "compression algorithm"));
    if tiles.nulls {
        cards.push(card(
            "ZBLANK",
            Value::Integer(NULL_VALUE.into()),
            "null value in the compressed integer array",
        ));
    }
    if opts.algorithm == Algorithm::Rice {
        cards.push(card(
            "ZNAME1",
            string("BLOCKSIZE"),
            "compression block size",
        ));
        cards.push(card("ZVAL1", int(RICE_BLOCK), "pixels per block"));
        cards.push(card(
            "ZNAME2",
            string("BYTEPIX"),
            "bytes per pixel (1, 2, 4, or 8)",
        ));
        cards.push(card(
            "ZVAL2",
            int(tiles.bytepix),
            "bytes per pixel (1, 2, 4, or 8)",
        ));
    }
    if let Some((bzero, comment)) = tiles.bzero {
        cards.push(card("BZERO", Value::Integer(bzero), comment));
        cards.push(card("BSCALE", int(1), "default scaling factor"));
    }
    cards.extend(extra.iter().cloned());
    cards
}

/// Compress an integer image, its pixels stored by `stored`.
fn compress_ints<T: Copy>(
    pixels: &[T],
    layout: &Layout,
    opts: &TileCompression,
    storage: &IntStorage,
    stored: impl Fn(T) -> i32,
) -> Result<Tiles> {
    let bytepix = (storage.zbitpix / 8) as usize;
    let tiles = layout
        .tiles()
        .iter()
        .map(|rect| {
            let values = layout.gather(pixels, rect, &stored);
            let bytes = match opts.algorithm {
                Algorithm::Rice => rice_encode(&values, bytepix),
                Algorithm::Gzip => {
                    let mut raw = Vec::with_capacity(values.len() * bytepix);
                    for v in values {
                        raw.extend_from_slice(&v.to_be_bytes()[4 - bytepix..]);
                    }
                    crate::gzip::compress(&raw, opts.gzip_level)
                }
            };
            Tile {
                bytes,
                fallback: false,
                scale: None,
            }
        })
        .collect();
    Ok(Tiles {
        tiles,
        zbitpix: storage.zbitpix,
        bytepix,
        bzero: storage.bzero,
        quantize: None,
        nulls: false,
    })
}

/// `f32` or `f64` pixels, quantized as cfitsio quantizes each.
trait Float: Copy + PartialOrd + core::ops::Sub<Output = Self> {
    const ZBITPIX: i64;
    /// cfitsio's `FLOATNULLVALUE`/`DOUBLENULLVALUE`, which stands in for a NaN
    /// when the dither seed is derived from the data.
    const NULL: Self;
    const MAX: Self;
    const MIN: Self;
    const ZERO: Self;
    fn to_f64(self) -> f64;
    fn from_f64(v: f64) -> Self;
    fn is_nan(self) -> bool;
    fn abs(self) -> Self;
    /// `2 * self`, `6 * self` and `4 * self` in this type's arithmetic.
    fn times(self, n: f64) -> Self;
    fn plus(self, other: Self) -> Self;
    fn byte_sum(self) -> u64;
    fn put_be(self, out: &mut Vec<u8>);
}

macro_rules! float {
    ($t:ty, $zbitpix:expr, $null:expr) => {
        impl Float for $t {
            const ZBITPIX: i64 = $zbitpix;
            const NULL: Self = $null;
            const MAX: Self = <$t>::MAX;
            const MIN: Self = <$t>::MIN;
            const ZERO: Self = 0.0;
            fn to_f64(self) -> f64 {
                self as f64
            }
            fn from_f64(v: f64) -> Self {
                v as $t
            }
            fn is_nan(self) -> bool {
                <$t>::is_nan(self)
            }
            fn abs(self) -> Self {
                if self < 0.0 {
                    -self
                } else {
                    self
                }
            }
            fn times(self, n: f64) -> Self {
                (n as $t) * self
            }
            fn plus(self, other: Self) -> Self {
                self + other
            }
            fn byte_sum(self) -> u64 {
                self.to_ne_bytes().iter().map(|&b| u64::from(b)).sum()
            }
            fn put_be(self, out: &mut Vec<u8>) {
                out.extend_from_slice(&self.to_be_bytes());
            }
        }
    };
}

float!(f32, -32, -9.11912E-36);
float!(f64, -64, -9.1191291391491E-36);

/// Compress a float image: quantized and Rice- or gzip-coded per tile, or,
/// for lossless `GZIP_1`, gzipped as it is.
fn compress_floats<F: Float>(
    pixels: &[F],
    layout: &Layout,
    opts: &TileCompression,
) -> Result<Tiles> {
    let rects = layout.tiles();
    let gzip_raw = |values: &[F]| {
        let mut raw = Vec::with_capacity(core::mem::size_of_val(values));
        for &v in values {
            v.put_be(&mut raw);
        }
        crate::gzip::compress(&raw, opts.gzip_level)
    };

    let Some(quantize) = opts.quantize else {
        if opts.algorithm != Algorithm::Gzip {
            return Err(Error::UnsupportedCompression(
                "lossless floating-point tiles need GZIP_1",
            ));
        }
        let tiles = rects
            .iter()
            .map(|rect| Tile {
                bytes: gzip_raw(&layout.gather(pixels, rect, |v| v)),
                fallback: false,
                scale: None,
            })
            .collect();
        return Ok(Tiles {
            tiles,
            zbitpix: F::ZBITPIX,
            bytepix: 4,
            bzero: None,
            quantize: Some((None, 0)),
            nulls: false,
        });
    };

    let seed = match quantize.seed {
        DitherSeed::Fixed(seed) if (1..=10_000).contains(&seed) => seed,
        DitherSeed::Fixed(_) => {
            return Err(Error::InvalidHeader("dither seed must be from 1 to 10000"))
        }
        // A hash of the first tile's bytes, nulls counted as cfitsio's null
        // value, as cfitsio derives it.
        DitherSeed::Checksum => {
            let first = layout.gather(
                pixels,
                &rects[0],
                |v: F| if v.is_nan() { F::NULL } else { v },
            );
            let sum = first
                .iter()
                .fold(0u64, |sum, v| sum.wrapping_add(v.byte_sum()));
            (sum % 10_000) as u16 + 1
        }
    };
    let table = dither_table();
    let mut nulls = false;
    let mut tiles = Vec::with_capacity(rects.len());
    for (index, rect) in rects.iter().enumerate() {
        let values = layout.gather(pixels, rect, |v| v);
        let (nx, ny) = rect.rows();
        let nullcheck = values.iter().any(|v| v.is_nan());
        nulls |= nullcheck;
        let row = match quantize.dither {
            Dither::Subtractive => index + usize::from(seed),
            Dither::None => 0,
        };
        let tile = match quantize_tile(&values, nx, ny, nullcheck, quantize.level, row, &table) {
            Some((ints, scale, zero)) => Tile {
                bytes: match opts.algorithm {
                    Algorithm::Rice => rice_encode(&ints, 4),
                    Algorithm::Gzip => {
                        let raw: Vec<u8> = ints.iter().flat_map(|v| v.to_be_bytes()).collect();
                        crate::gzip::compress(&raw, opts.gzip_level)
                    }
                },
                fallback: false,
                scale: Some((scale, zero)),
            },
            None => Tile {
                bytes: gzip_raw(&values),
                fallback: true,
                scale: None,
            },
        };
        tiles.push(tile);
    }
    Ok(Tiles {
        tiles,
        zbitpix: F::ZBITPIX,
        bytepix: 4,
        bzero: None,
        quantize: Some((Some(quantize.dither), seed)),
        nulls,
    })
}

/// C's `NINT`: round half away from zero.
fn nint(x: f64) -> i32 {
    if x >= 0.0 {
        (x + 0.5) as i32
    } else {
        (x - 0.5) as i32
    }
}

/// cfitsio's `fits_quantize_float`/`fits_quantize_double`: the tile's
/// quantized values, `ZSCALE` and `ZZERO`, or `None` where cfitsio stores the
/// tile losslessly instead.
///
/// `row` is cfitsio's 1-based dither row, `tile + ZDITHER0`, or 0 for no
/// dithering. NaN pixels are nulls when `nullcheck` is set.
fn quantize_tile<F: Float>(
    fdata: &[F],
    nxpix: usize,
    nypix: usize,
    nullcheck: bool,
    qlevel: f64,
    row: usize,
    table: &[f32],
) -> Option<(Vec<i32>, f64, f64)> {
    let nx = nxpix * nypix;
    if nx <= 1 {
        return None;
    }
    let noise;
    let delta = if qlevel >= 0.0 {
        noise = noise5(fdata, nxpix, nypix, nullcheck);
        let stdev = if nullcheck && noise.ngood == 0 {
            1.0
        } else {
            let mut stdev = noise.noise3;
            if noise.noise2 != 0.0 && noise.noise2 < stdev {
                stdev = noise.noise2;
            }
            if noise.noise5 != 0.0 && noise.noise5 < stdev {
                stdev = noise.noise5;
            }
            stdev
        };
        let delta = if qlevel == 0.0 {
            stdev / 4.0
        } else {
            stdev / qlevel
        };
        if delta == 0.0 {
            return None;
        }
        delta
    } else {
        noise = range(fdata, nullcheck);
        -qlevel
    };
    let (minval, maxval) = if nullcheck && noise.ngood == 0 && qlevel >= 0.0 {
        (F::ZERO, F::from_f64(1.0))
    } else {
        (noise.min, noise.max)
    };

    let spread = (maxval - minval).to_f64() / delta;
    if spread > 2.0 * 2_147_483_647.0 - N_RESERVED_VALUES {
        return None;
    }

    let mut iseed = 0;
    let mut nextrand = 0;
    if row > 0 {
        iseed = (row - 1) % N_RANDOM;
        nextrand = (f64::from(table[iseed]) * 500.0) as usize;
    }
    let mut dither = || {
        let r = f64::from(table[nextrand]);
        nextrand += 1;
        if nextrand == N_RANDOM {
            iseed += 1;
            if iseed == N_RANDOM {
                iseed = 0;
            }
            nextrand = (f64::from(table[iseed]) * 500.0) as usize;
        }
        r
    };

    let zeropt = if noise.ngood == nx {
        if spread < 2_147_483_647.0 - N_RESERVED_VALUES {
            // A multiple of delta, so repeated compression scales alike.
            let iqfactor = (minval.to_f64() / delta + 0.5) as i64;
            iqfactor as f64 * delta
        } else {
            minval.plus(maxval).to_f64() / 2.0
        }
    } else {
        minval.to_f64() - delta * (f64::from(NULL_VALUE) + N_RESERVED_VALUES)
    };

    let idata = fdata
        .iter()
        .map(|&f| {
            let null = nullcheck && f.is_nan();
            if row > 0 {
                let r = dither();
                if null {
                    NULL_VALUE
                } else {
                    nint((f.to_f64() - zeropt) / delta + r - 0.5)
                }
            } else if null {
                NULL_VALUE
            } else {
                nint((f.to_f64() - zeropt) / delta)
            }
        })
        .collect();
    Some((idata, delta, zeropt))
}

/// Pixel statistics of a tile: non-null count, range and noise estimates.
struct Noise<F> {
    ngood: usize,
    min: F,
    max: F,
    noise2: f64,
    noise3: f64,
    noise5: f64,
}

/// The non-null count and range alone, for an absolute quantization step.
fn range<F: Float>(array: &[F], nullcheck: bool) -> Noise<F> {
    let mut noise = Noise {
        ngood: 0,
        min: F::MAX,
        max: F::MIN,
        noise2: 0.0,
        noise3: 0.0,
        noise5: 0.0,
    };
    for &v in array {
        if nullcheck && v.is_nan() {
            continue;
        }
        if v < noise.min {
            noise.min = v;
        }
        if v > noise.max {
            noise.max = v;
        }
        noise.ngood += 1;
    }
    noise
}

/// cfitsio's `FnNoise5_float`/`FnNoise5_double`: the 2nd, 3rd and 5th order
/// median absolute differences of each row, and the median of those over the
/// rows, with the non-null count and range.
fn noise5<F: Float>(array: &[F], mut nx: usize, mut ny: usize, nullcheck: bool) -> Noise<F> {
    if nx < 9 {
        nx *= ny;
        ny = 1;
    }
    if nx < 9 {
        return range(&array[..nx], nullcheck);
    }
    let null = |v: F| nullcheck && v.is_nan();
    let mut ngood = 0usize;
    let (mut xmin, mut xmax) = (F::MAX, F::MIN);
    let mut differences2 = vec![F::ZERO; nx];
    let mut differences3 = vec![F::ZERO; nx];
    let mut differences5 = vec![F::ZERO; nx];
    let mut diffs2 = Vec::with_capacity(ny);
    let mut diffs3 = Vec::with_capacity(ny);
    let mut diffs5 = Vec::with_capacity(ny);

    'rows: for row in array.chunks_exact(nx).take(ny) {
        // The first eight valid pixels, each counted as it is found.
        let mut v = [F::ZERO; 9];
        let mut ii = 0;
        for slot in v.iter_mut().take(8) {
            while ii < nx && null(row[ii]) {
                ii += 1;
            }
            if ii == nx {
                continue 'rows;
            }
            *slot = row[ii];
            widen(row[ii], &mut xmin, &mut xmax);
            ngood += 1;
            ii += 1;
        }
        let [mut v1, mut v2, mut v3, mut v4, mut v5, mut v6, mut v7, mut v8, _] = v;
        let mut nvals = 0;
        let mut nvals2 = 0;
        while ii < nx {
            while ii < nx && null(row[ii]) {
                ii += 1;
            }
            if ii == nx {
                break;
            }
            let v9 = row[ii];
            widen(v9, &mut xmin, &mut xmax);
            if !(v5 == v6 && v6 == v7) {
                differences2[nvals2] = (v5 - v7).abs();
                nvals2 += 1;
            }
            if !(v3 == v4 && v4 == v5 && v5 == v6 && v6 == v7) {
                differences3[nvals] = (v5.times(2.0) - v3 - v7).abs();
                differences5[nvals] = (v5.times(6.0) - v3.times(4.0) - v7.times(4.0))
                    .plus(v1)
                    .plus(v9)
                    .abs();
                nvals += 1;
            } else {
                // Constant background is skipped but its pixels count.
                ngood += 1;
            }
            v1 = v2;
            v2 = v3;
            v3 = v4;
            v4 = v5;
            v5 = v6;
            v6 = v7;
            v7 = v8;
            v8 = v9;
            ii += 1;
        }
        ngood += nvals;
        match nvals {
            0 => continue,
            1 => {
                if nvals2 == 1 {
                    diffs2.push(differences2[0].to_f64());
                }
                diffs3.push(differences3[0].to_f64());
                diffs5.push(differences5[0].to_f64());
            }
            _ => {
                // cfitsio takes the median of `nvals` 2nd-order differences,
                // not `nvals2`; kept as it is for identical output.
                if nvals2 > 1 {
                    diffs2.push(quick_select(&mut differences2[..nvals]).to_f64());
                }
                diffs3.push(quick_select(&mut differences3[..nvals]).to_f64());
                diffs5.push(quick_select(&mut differences5[..nvals]).to_f64());
            }
        }
    }

    let median = |diffs: &mut Vec<f64>| match diffs.len() {
        0 => 0.0,
        1 => diffs[0],
        n => {
            diffs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
            (diffs[(n - 1) / 2] + diffs[n / 2]) / 2.0
        }
    };
    Noise {
        ngood,
        min: xmin,
        max: xmax,
        noise2: 1.0483579 * median(&mut diffs2),
        noise3: 0.6052697 * median(&mut diffs3),
        noise5: 0.1772048 * median(&mut diffs5),
    }
}

/// Extend the range `min..=max` to take in `v`.
fn widen<F: Float>(v: F, min: &mut F, max: &mut F) {
    if v < *min {
        *min = v;
    }
    if v > *max {
        *max = v;
    }
}

/// cfitsio's `quick_select_float`: the median of `arr` (the lower one for an
/// even count), reordering it in place.
fn quick_select<F: Float>(arr: &mut [F]) -> F {
    let n = arr.len();
    let (mut low, mut high) = (0usize, n - 1);
    let median = (low + high) / 2;
    loop {
        if high <= low {
            return arr[median];
        }
        if high == low + 1 {
            if arr[low] > arr[high] {
                arr.swap(low, high);
            }
            return arr[median];
        }
        let middle = (low + high) / 2;
        if arr[middle] > arr[high] {
            arr.swap(middle, high);
        }
        if arr[low] > arr[high] {
            arr.swap(low, high);
        }
        if arr[middle] > arr[low] {
            arr.swap(middle, low);
        }
        arr.swap(middle, low + 1);
        let mut ll = low + 1;
        let mut hh = high;
        loop {
            // No NaN reaches here: null pixels are skipped.
            loop {
                ll += 1;
                if arr[low] <= arr[ll] {
                    break;
                }
            }
            loop {
                hh -= 1;
                if arr[hh] <= arr[low] {
                    break;
                }
            }
            if hh < ll {
                break;
            }
            arr.swap(ll, hh);
        }
        arr.swap(low, hh);
        if hh <= median {
            low = ll;
        }
        if hh >= median {
            high = hh - 1;
        }
    }
}

/// cfitsio's `output_nbits` bit packer.
struct BitWriter {
    out: Vec<u8>,
    buffer: u32,
    bits_to_go: i32,
}

impl BitWriter {
    fn new(capacity: usize) -> Self {
        BitWriter {
            out: Vec::with_capacity(capacity),
            buffer: 0,
            bits_to_go: 8,
        }
    }

    /// Append the low `n` bits of `bits`, `n` at most 32.
    fn put(&mut self, bits: u32, mut n: i32) {
        let mask = |n: i32| if n >= 32 { u32::MAX } else { (1u32 << n) - 1 };
        if self.bits_to_go + n > 32 {
            // Put out the top bits_to_go (1 to 8) bits first.
            self.buffer = self.buffer.wrapping_shl(self.bits_to_go as u32);
            self.buffer |= (bits >> (n - self.bits_to_go)) & mask(self.bits_to_go);
            self.out.push(self.buffer as u8);
            n -= self.bits_to_go;
            self.bits_to_go = 8;
        }
        self.buffer = self.buffer.wrapping_shl(n as u32);
        self.buffer |= bits & mask(n);
        self.bits_to_go -= n;
        while self.bits_to_go <= 0 {
            self.out
                .push((self.buffer >> (-self.bits_to_go) as u32) as u8);
            self.bits_to_go += 8;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.bits_to_go < 8 {
            self.out
                .push(self.buffer.wrapping_shl(self.bits_to_go as u32) as u8);
        }
        self.out
    }
}

/// cfitsio's `fits_rcomp`, `fits_rcomp_short` and `fits_rcomp_byte`: Rice
/// code `values` as `bytepix`-byte integers in blocks of 32.
fn rice_encode(values: &[i32], bytepix: usize) -> Vec<u8> {
    let (fsbits, fsmax, bbits): (i32, i32, i32) = match bytepix {
        1 => (3, 6, 8),
        2 => (4, 14, 16),
        _ => (5, 25, 32),
    };
    let width = 8 * bytepix as u32;
    // Differences and sums wrap at the pixel width, as in C's narrow types.
    let narrow = |d: i32| match width {
        8 => i32::from(d as i8),
        16 => i32::from(d as i16),
        _ => d,
    };
    let psum_mask = match width {
        8 => 0xff,
        16 => 0xffff,
        _ => u32::MAX,
    };

    let mut w = BitWriter::new(values.len() * bytepix + values.len() / 4 + 16);
    let Some(&first) = values.first() else {
        return w.finish();
    };
    w.put(first as u32, bbits);
    let mut lastpix = first;
    let mut diff = [0u32; RICE_BLOCK];
    for block in values.chunks(RICE_BLOCK) {
        let thisblock = block.len();
        let mut pixelsum = 0.0f64;
        for (d, &nextpix) in diff.iter_mut().zip(block) {
            let pdiff = narrow(nextpix.wrapping_sub(lastpix));
            let shifted = pdiff.wrapping_shl(1);
            *d = if pdiff < 0 { !shifted } else { shifted } as u32;
            pixelsum += f64::from(*d);
            lastpix = nextpix;
        }
        let dpsum = ((pixelsum - (thisblock / 2) as f64 - 1.0) / thisblock as f64).max(0.0);
        let mut psum = ((dpsum as u64 as u32) & psum_mask) >> 1;
        let mut fs = 0;
        while psum > 0 {
            psum >>= 1;
            fs += 1;
        }
        if fs >= fsmax {
            // High entropy: the differences, uncoded.
            w.put((fsmax + 1) as u32, fsbits);
            for &d in &diff[..thisblock] {
                w.put(d, bbits);
            }
        } else if fs == 0 && pixelsum == 0.0 {
            // Every difference zero.
            w.put(0, fsbits);
        } else {
            w.put((fs + 1) as u32, fsbits);
            let fsmask = (1u32 << fs) - 1;
            for &v in &diff[..thisblock] {
                // The top bits as that many zeros and a one, then the low
                // `fs` bits as they are.
                let mut top = (v >> fs) as i32;
                if w.bits_to_go > top {
                    w.buffer = w.buffer.wrapping_shl((top + 1) as u32) | 1;
                    w.bits_to_go -= top + 1;
                } else {
                    w.buffer = w.buffer.wrapping_shl(w.bits_to_go as u32);
                    w.out.push(w.buffer as u8);
                    top -= w.bits_to_go;
                    while top >= 8 {
                        w.out.push(0);
                        top -= 8;
                    }
                    w.buffer = 1;
                    w.bits_to_go = 7 - top;
                }
                if fs > 0 {
                    w.buffer = w.buffer.wrapping_shl(fs as u32) | (v & fsmask);
                    w.bits_to_go -= fs;
                    while w.bits_to_go <= 0 {
                        w.out.push((w.buffer >> (-w.bits_to_go) as u32) as u8);
                        w.bits_to_go += 8;
                    }
                }
            }
        }
    }
    w.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hdu::parse_fits;
    use crate::image::{read_image_data, ImageData};
    use crate::primary::build_primary_header;

    /// A file of an empty primary HDU and the compressed image.
    fn file_with<T: CompressPixel>(
        naxes: &[usize],
        pixels: &[T],
        opts: &TileCompression,
    ) -> Vec<u8> {
        let mut file = serialize_header(&build_primary_header(8, &[]).unwrap()).unwrap();
        let name = card("EXTNAME", string("SCI"), "");
        file.extend(compress_image_hdu(naxes, pixels, opts, &[name]).unwrap());
        file
    }

    fn read_back(file: &[u8]) -> ImageData {
        let parsed = parse_fits(file).unwrap();
        read_image_data(file, &parsed.hdus[1]).unwrap()
    }

    fn options() -> Vec<TileCompression> {
        let mut out = Vec::new();
        for base in [TileCompression::rice(), TileCompression::gzip()] {
            out.push(base.clone());
            out.push(base.clone().tile_rows(3));
            out.push(base.tile_dims(&[16, 8]));
        }
        out
    }

    #[test]
    fn integer_images_round_trip_exactly() {
        let (w, h) = (57, 23);
        let ramp = |i: usize| (i as i64 * 977) % 70_001 - 35_000;
        let u8s: Vec<u8> = (0..w * h).map(|i| ramp(i) as u8).collect();
        let i8s: Vec<i8> = (0..w * h).map(|i| ramp(i) as i8).collect();
        let i16s: Vec<i16> = (0..w * h).map(|i| ramp(i) as i16).collect();
        let u16s: Vec<u16> = (0..w * h).map(|i| (ramp(i) + 35_000) as u16).collect();
        let i32s: Vec<i32> = (0..w * h).map(|i| (ramp(i) * 61_000) as i32).collect();
        let u32s: Vec<u32> = (0..w * h)
            .map(|i| (ramp(i) * 61_000 + i32::MAX as i64) as u32)
            .collect();
        for opts in options() {
            match read_back(&file_with(&[w, h], &u8s, &opts)) {
                ImageData::U8(v) => assert_eq!(v, u8s, "{opts:?}"),
                other => panic!("{other:?}"),
            }
            match read_back(&file_with(&[w, h], &i8s, &opts)) {
                ImageData::U8(v) => {
                    let back: Vec<i8> = v.iter().map(|&b| (i16::from(b) - 128) as i8).collect();
                    assert_eq!(back, i8s, "{opts:?}");
                }
                other => panic!("{other:?}"),
            }
            match read_back(&file_with(&[w, h], &i16s, &opts)) {
                ImageData::I16(v) => assert_eq!(v, i16s, "{opts:?}"),
                other => panic!("{other:?}"),
            }
            match read_back(&file_with(&[w, h], &u16s, &opts)) {
                ImageData::I16(v) => {
                    let back: Vec<u16> = v.iter().map(|&s| (i32::from(s) + 32768) as u16).collect();
                    assert_eq!(back, u16s, "{opts:?}");
                }
                other => panic!("{other:?}"),
            }
            match read_back(&file_with(&[w, h], &i32s, &opts)) {
                ImageData::I32(v) => assert_eq!(v, i32s, "{opts:?}"),
                other => panic!("{other:?}"),
            }
            match read_back(&file_with(&[w, h], &u32s, &opts)) {
                ImageData::I32(v) => {
                    let back: Vec<u32> = v.iter().map(|&s| (s as u32) ^ 0x8000_0000).collect();
                    assert_eq!(back, u32s, "{opts:?}");
                }
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn rice_handles_extreme_differences() {
        // Alternating extremes force the uncoded high-entropy blocks.
        let values: Vec<i32> = (0..100)
            .map(|i| if i % 2 == 0 { i32::MIN } else { i32::MAX })
            .collect();
        let file = file_with(&[100], &values, &TileCompression::rice());
        assert_eq!(read_back(&file), ImageData::I32(values));
        let zeros = vec![0i16; 100];
        let file = file_with(&[100], &zeros, &TileCompression::rice());
        assert_eq!(read_back(&file), ImageData::I16(zeros));
    }

    fn noisy(w: usize, h: usize) -> Vec<f32> {
        (0..w * h)
            .map(|i| 100.0 + 10.0 * ((i as f32) * 0.37).sin() + (i % 7) as f32)
            .collect()
    }

    #[test]
    fn floats_round_trip_within_the_quantization_step() {
        let (w, h) = (57, 23);
        let f32s = noisy(w, h);
        let f64s: Vec<f64> = f32s.iter().map(|&v| f64::from(v) * 1.5).collect();
        for opts in options() {
            let file = file_with(&[w, h], &f32s, &opts);
            let parsed = parse_fits(&file).unwrap();
            let step = max_zscale(&file, &parsed.hdus[1]);
            match read_back(&file) {
                ImageData::F32(v) => {
                    for (a, b) in v.iter().zip(&f32s) {
                        assert!(f64::from((a - b).abs()) <= step, "{opts:?}: {a} vs {b}");
                    }
                }
                other => panic!("{other:?}"),
            }
            let file = file_with(&[w, h], &f64s, &opts);
            let parsed = parse_fits(&file).unwrap();
            let step = max_zscale(&file, &parsed.hdus[1]);
            match read_back(&file) {
                ImageData::F64(v) => {
                    for (a, b) in v.iter().zip(&f64s) {
                        assert!((a - b).abs() <= step, "{opts:?}: {a} vs {b}");
                    }
                }
                other => panic!("{other:?}"),
            }
        }
    }

    /// The largest `ZSCALE` in the table.
    fn max_zscale(file: &[u8], hdu: &crate::hdu::Hdu) -> f64 {
        let rows = match hdu.info {
            crate::hdu::HduInfo::BinaryTable { naxis2, .. }
            | crate::hdu::HduInfo::CompressedImage { naxis2, .. } => naxis2,
            _ => panic!(),
        };
        (0..rows)
            .map(|r| {
                let at = hdu.data_start + r * 24 + 8;
                f64::from_be_bytes(file[at..at + 8].try_into().unwrap())
            })
            .fold(0.0, f64::max)
    }

    #[test]
    fn lossless_gzip_keeps_floats_exactly() {
        let f32s = noisy(31, 7);
        let opts = TileCompression::gzip().lossless();
        assert_eq!(
            read_back(&file_with(&[31, 7], &f32s, &opts)),
            ImageData::F32(f32s.clone())
        );
        assert!(
            compress_image_hdu(&[31, 7], &f32s, &TileCompression::rice().lossless(), &[]).is_err()
        );
    }

    #[test]
    fn constant_and_nan_tiles() {
        let (w, h) = (40, 6);
        let mut pixels = noisy(w, h);
        // Row 1 is constant: it can't be quantized and is stored losslessly.
        pixels[w..2 * w].fill(7.25);
        // Row 3 has NaNs, which come back as NaN.
        pixels[3 * w + 5] = f32::NAN;
        pixels[3 * w + 30] = f32::NAN;
        for opts in [TileCompression::rice(), TileCompression::gzip()] {
            let file = file_with(&[w, h], &pixels, &opts);
            let text = String::from_utf8_lossy(&file[2880..5760]).into_owned();
            assert!(text.contains("GZIP_COMPRESSED_DATA"));
            assert!(text.contains("ZBLANK"));
            match read_back(&file) {
                ImageData::F32(v) => {
                    assert!(v[w..2 * w].iter().all(|&x| x == 7.25));
                    assert!(v[3 * w + 5].is_nan() && v[3 * w + 30].is_nan());
                    assert_eq!(v.iter().filter(|x| x.is_nan()).count(), 2);
                }
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn dither_seed_is_derived_from_the_first_tile_or_given() {
        let pixels = noisy(20, 4);
        let header = |opts: &TileCompression| {
            let hdu = compress_image_hdu(&[20, 4], &pixels, opts, &[]).unwrap();
            crate::header::parse_header_blocks(&hdu).unwrap()
        };
        let seed = |cards: &[Card]| {
            cards
                .iter()
                .find(|c| c.keyword_str() == "ZDITHER0")
                .and_then(|c| c.value.clone())
        };
        let first: u64 = pixels[..20]
            .iter()
            .flat_map(|v| v.to_ne_bytes())
            .map(u64::from)
            .sum();
        let expected = Value::Integer((first % 10_000) as i64 + 1);
        assert_eq!(seed(&header(&TileCompression::rice())), Some(expected));
        let fixed = TileCompression::rice().quantize(Quantize::new().seed(DitherSeed::Fixed(77)));
        assert_eq!(seed(&header(&fixed)), Some(Value::Integer(77)));
        let none = TileCompression::rice().quantize(Quantize::new().dither(Dither::None));
        assert_eq!(seed(&header(&none)), None);
        let bad = TileCompression::rice().quantize(Quantize::new().seed(DitherSeed::Fixed(0)));
        assert!(compress_image_hdu(&[20, 4], &pixels, &bad, &[]).is_err());
    }

    #[test]
    fn a_step_is_used_exactly() {
        let pixels = noisy(20, 4);
        let step = 0.1f64 / 3.0;
        assert_ne!(f64::from(step as f32), step, "an f32 level would round it");
        let opts = TileCompression::rice().quantize(Quantize::new().step(step));
        let file = file_with(&[20, 4], &pixels, &opts);
        let parsed = parse_fits(&file).unwrap();
        assert_eq!(max_zscale(&file, &parsed.hdus[1]), step);
    }

    #[test]
    fn three_dimensional_tiles() {
        let naxes = [9, 7, 5];
        let pixels: Vec<i16> = (0..9 * 7 * 5).map(|i| (i * 37 % 1000) as i16).collect();
        for opts in [
            TileCompression::rice().tile_dims(&[4, 3, 2]),
            TileCompression::gzip().tile_dims(&[9, 1, 5]),
            TileCompression::rice().tile_rows(2),
        ] {
            assert_eq!(
                read_back(&file_with(&naxes, &pixels, &opts)),
                ImageData::I16(pixels.clone())
            );
        }
    }

    #[test]
    fn bad_input_is_an_error() {
        let pixels = vec![0u8; 10];
        let rice = TileCompression::rice();
        assert!(compress_image_hdu(&[3, 3], &pixels, &rice, &[]).is_err());
        assert!(compress_image_hdu(&[10, 0], &pixels, &rice, &[]).is_err());
        assert!(compress_image_hdu(&[10], &pixels, &rice.clone().tile_dims(&[0]), &[]).is_err());
        let naxis = card("NAXIS1", int(3), "");
        assert!(compress_image_hdu(&[10], &pixels, &rice, &[naxis]).is_err());
    }
}
