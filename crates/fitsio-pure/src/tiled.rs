//! Tile-compressed image decompression for FITS.
//!
//! Supports RICE_1/RICE_ONE, GZIP_1, GZIP_2, and NOCOMPRESS per the FITS
//! tiled image compression convention. HCOMPRESS_1 and PLIO_1 are rejected
//! with [`Error::UnsupportedCompression`].

use alloc::string::String;
use alloc::vec::Vec;

use crate::endian::{read_f64_be, read_i32_be, read_i64_be};
use crate::error::{Error, Result};
use crate::hdu::{Hdu, HduInfo};
use crate::header::Card;
use crate::image::ImageData;
use crate::value::Value;

// ---------------------------------------------------------------------------
// Column layout helpers
// ---------------------------------------------------------------------------

/// A variable-length array column of the tile table.
struct HeapColumn {
    /// Byte offset of the descriptor within a table row.
    offset: usize,
    /// `Q` (64-bit) rather than `P` (32-bit) descriptor.
    wide: bool,
    /// Bytes per array element.
    elem_size: usize,
}

struct ColumnInfo {
    compressed_data: HeapColumn,
    /// cfitsio stores a tile it could not compress here, gzipped.
    gzip_data: Option<HeapColumn>,
    /// Pre-2011 cfitsio stored such a tile here, uncompressed.
    uncompressed_data: Option<HeapColumn>,
    zscale_offset: Option<usize>,
    zzero_offset: Option<usize>,
    zblank_offset: Option<usize>,
}

fn card_string_value(cards: &[Card], keyword: &str) -> Option<String> {
    cards.iter().find_map(|c| {
        if c.keyword_str() == keyword {
            match &c.value {
                Some(Value::String(s)) => Some(s.trim().into()),
                _ => None,
            }
        } else {
            None
        }
    })
}

/// Parse the binary table column layout to find the tile data columns and
/// the per-tile ZSCALE, ZZERO, and ZBLANK columns.
fn parse_column_layout(cards: &[Card], tfields: usize) -> Result<ColumnInfo> {
    use crate::bintable::BinaryColumnType;

    let mut columns = Vec::with_capacity(tfields);
    let mut offset = 0usize;
    for i in 1..=tfields {
        let name = card_string_value(cards, &alloc::format!("TTYPE{}", i)).unwrap_or_default();
        let tform = card_string_value(cards, &alloc::format!("TFORM{}", i))
            .ok_or(Error::InvalidHeader("missing TFORM in compressed image"))?;
        let (repeat, col_type) = crate::bintable::parse_tform_binary(&tform)?;
        let width = match col_type {
            BinaryColumnType::Bit => repeat.div_ceil(8),
            _ => repeat * crate::bintable::binary_type_byte_size(&col_type),
        };
        columns.push((name, offset, col_type));
        offset += width;
    }

    let find = |wanted: &str| columns.iter().find(|(name, ..)| name == wanted);
    let offset_of = |wanted: &str| find(wanted).map(|&(_, offset, _)| offset);
    let heap_column = |wanted: &str| -> Result<Option<HeapColumn>> {
        let Some((_, offset, col_type)) = find(wanted) else {
            return Ok(None);
        };
        let (wide, elem) = match col_type {
            BinaryColumnType::VarArrayP(elem) => (false, *elem),
            BinaryColumnType::VarArrayQ(elem) => (true, *elem),
            _ => {
                return Err(Error::InvalidHeader(
                    "tile data column is not a variable array",
                ))
            }
        };
        let elem_size = match elem {
            'B' => 1,
            'I' => 2,
            'J' | 'E' => 4,
            'K' | 'D' => 8,
            _ => return Err(Error::InvalidHeader("unsupported tile data element type")),
        };
        Ok(Some(HeapColumn {
            offset: *offset,
            wide,
            elem_size,
        }))
    };

    Ok(ColumnInfo {
        compressed_data: heap_column("COMPRESSED_DATA")?
            .ok_or(Error::InvalidHeader("missing COMPRESSED_DATA column"))?,
        gzip_data: heap_column("GZIP_COMPRESSED_DATA")?,
        uncompressed_data: heap_column("UNCOMPRESSED_DATA")?,
        zscale_offset: offset_of("ZSCALE"),
        zzero_offset: offset_of("ZZERO"),
        zblank_offset: offset_of("ZBLANK"),
    })
}

// ---------------------------------------------------------------------------
// P-descriptor / heap reading
// ---------------------------------------------------------------------------

/// Read a 32-bit P-descriptor: (element_count, heap_byte_offset).
fn read_p_descriptor(data: &[u8]) -> (usize, usize) {
    let count = read_i32_be(data) as u32 as usize;
    let offset = read_i32_be(&data[4..]) as u32 as usize;
    (count, offset)
}

/// Read a 64-bit Q-descriptor: (element_count, heap_byte_offset).
fn read_q_descriptor(data: &[u8]) -> (usize, usize) {
    let count = read_i64_be(data) as u64 as usize;
    let offset = read_i64_be(&data[8..]) as u64 as usize;
    (count, offset)
}

/// Extract a tile's bytes from the heap for a given row.
///
/// Returns `(data_slice, count)` where `count` is the number of tile bytes.
/// For Rice decompression the slice extends beyond `count` so that the
/// bit-stream reader can safely over-read by a few bytes, matching the
/// cfitsio behaviour.
fn extract_tile_bytes<'a>(
    fits_data: &'a [u8],
    data_start: usize,
    naxis1: usize,
    naxis2: usize,
    row: usize,
    column: &HeapColumn,
) -> Result<(&'a [u8], usize)> {
    let desc_pos = data_start + row * naxis1 + column.offset;
    let desc_len = if column.wide { 16 } else { 8 };
    if desc_pos + desc_len > fits_data.len() {
        return Err(Error::UnexpectedEof);
    }
    let (count, heap_offset) = if column.wide {
        read_q_descriptor(&fits_data[desc_pos..])
    } else {
        read_p_descriptor(&fits_data[desc_pos..])
    };
    let count = count * column.elem_size;
    let heap_start = data_start + naxis1 * naxis2;
    let tile_start = heap_start + heap_offset;
    let tile_end = tile_start + count;
    if tile_end > fits_data.len() {
        return Err(Error::UnexpectedEof);
    }
    Ok((&fits_data[tile_start..], count))
}

// ---------------------------------------------------------------------------
// Rice decompression
// ---------------------------------------------------------------------------

/// Position of the most significant 1-bit for each byte value 0..255.
const NONZERO_COUNT: [i32; 256] = [
    0, 1, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 4, 4, 4, 4, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6,
    7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7,
    7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7,
    8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8,
    8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8,
    8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8,
    8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8,
];

struct RiceParams {
    fsbits: i32,
    fsmax: i32,
    bbits: i32,
    bytes_per_val: usize,
}

impl RiceParams {
    fn for_bytepix(rice_bytepix: usize) -> Result<Self> {
        match rice_bytepix {
            1 => Ok(RiceParams {
                fsbits: 3,
                fsmax: 6,
                bbits: 8,
                bytes_per_val: 1,
            }),
            2 => Ok(RiceParams {
                fsbits: 4,
                fsmax: 14,
                bbits: 16,
                bytes_per_val: 2,
            }),
            4 => Ok(RiceParams {
                fsbits: 5,
                fsmax: 25,
                bbits: 32,
                bytes_per_val: 4,
            }),
            _ => Err(Error::UnsupportedCompression("unsupported Rice bytepix")),
        }
    }
}

/// Decompress Rice-encoded tile data into i32 pixel values.
fn rice_decompress(
    compressed: &[u8],
    num_pixels: usize,
    blocksize: usize,
    params: &RiceParams,
) -> Result<Vec<i32>> {
    if compressed.len() < params.bytes_per_val {
        return Err(Error::DecompressionError("Rice data too short"));
    }

    let mut output = Vec::with_capacity(num_pixels);
    let mut pos = 0usize;

    // Read first pixel uncompressed (big-endian).
    // Unlike cfitsio which uses unsigned types, we track lastpix as i32.
    let lastpix: i32 = match params.bytes_per_val {
        1 => compressed[0] as i8 as i32,
        2 => {
            let v = ((compressed[0] as u16) << 8) | (compressed[1] as u16);
            v as i16 as i32
        }
        4 => read_i32_be(compressed),
        _ => return Err(Error::DecompressionError("unsupported Rice bytepix")),
    };
    pos += params.bytes_per_val;

    if num_pixels == 0 {
        return Ok(output);
    }
    if pos >= compressed.len() {
        output.resize(num_pixels, lastpix);
        return Ok(output);
    }

    // Initialize bit buffer
    let mut b: u32 = compressed[pos] as u32;
    pos += 1;
    let mut nbits: i32 = 8;
    let mut lastpix = lastpix;

    let nx = num_pixels as i32;
    let nblock = blocksize as i32;
    // Start at pixel 0 -- the first FS block includes pixel 0
    // which gets lastpix in the low-entropy case.
    let mut pixel_idx: i32 = 0;

    while pixel_idx < nx {
        let imax = (pixel_idx + nblock).min(nx);

        // Read FS value (fsbits bits)
        nbits -= params.fsbits;
        while nbits < 0 {
            if pos >= compressed.len() {
                // Pad with zeros if we run out of data
                b <<= 8;
            } else {
                b = (b << 8) | (compressed[pos] as u32);
                pos += 1;
            }
            nbits += 8;
        }
        let fs = ((b >> nbits) as i32) - 1;
        b &= (1u32 << nbits) - 1;

        if fs < 0 {
            // Low entropy: all diffs are 0, pixels identical to lastpix
            while pixel_idx < imax {
                output.push(lastpix);
                pixel_idx += 1;
            }
        } else if fs == params.fsmax {
            // High entropy: uncompressed differences (bbits per pixel)
            while pixel_idx < imax {
                // Read bbits bits
                let mut k = params.bbits - nbits;
                let mut diff = (b as u64) << k;

                k -= 8;
                while k >= 0 {
                    if pos < compressed.len() {
                        b = compressed[pos] as u32;
                        pos += 1;
                    } else {
                        b = 0;
                    }
                    diff |= (b as u64) << k;
                    k -= 8;
                }

                if nbits > 0 {
                    if pos < compressed.len() {
                        b = compressed[pos] as u32;
                        pos += 1;
                    } else {
                        b = 0;
                    }
                    diff |= (b >> (-k)) as u64;
                    b &= (1u32 << nbits) - 1;
                } else {
                    b = 0;
                }

                let mut diff = diff as u32;
                // Zigzag decode
                if (diff & 1) == 0 {
                    diff >>= 1;
                } else {
                    diff = !(diff >> 1);
                }
                lastpix = (diff as i32).wrapping_add(lastpix);
                output.push(lastpix);
                pixel_idx += 1;
            }
        } else {
            // Normal Rice encoding
            while pixel_idx < imax {
                // Count leading zeros
                while b == 0 {
                    nbits += 8;
                    if pos < compressed.len() {
                        b = compressed[pos] as u32;
                        pos += 1;
                    } else {
                        b = 0;
                        break;
                    }
                }
                let nzero = nbits - NONZERO_COUNT[b as usize & 0xFF];
                nbits -= nzero + 1;
                if !(0..=31).contains(&nbits) {
                    // Data exhausted mid-stream; fill remaining with lastpix.
                    while pixel_idx < imax {
                        output.push(lastpix);
                        pixel_idx += 1;
                    }
                    break;
                }
                b ^= 1u32 << nbits;

                // Read fs trailing bits
                nbits -= fs;
                while nbits < 0 {
                    if pos < compressed.len() {
                        b = (b << 8) | (compressed[pos] as u32);
                        pos += 1;
                    } else {
                        b <<= 8;
                    }
                    nbits += 8;
                }

                let mut diff = ((nzero as u32) << fs) | (b >> nbits);
                b &= (1u32 << nbits) - 1;

                // Zigzag decode
                if (diff & 1) == 0 {
                    diff >>= 1;
                } else {
                    diff = !(diff >> 1);
                }
                lastpix = (diff as i32).wrapping_add(lastpix);
                output.push(lastpix);
                pixel_idx += 1;
            }
        }
    }

    Ok(output)
}

// ---------------------------------------------------------------------------
// GZIP decompression
// ---------------------------------------------------------------------------

/// Inflate gzip-compressed tile data.
fn gzip_decompress(compressed: &[u8]) -> Result<Vec<u8>> {
    // Try gzip format first (magic bytes 1f 8b), then zlib, then raw deflate.
    if crate::gzip::is_gzip(compressed) {
        return crate::gzip::decompress(compressed);
    }
    miniz_oxide::inflate::decompress_to_vec_zlib(compressed)
        .or_else(|_| miniz_oxide::inflate::decompress_to_vec(compressed))
        .map_err(|_| Error::DecompressionError("zlib/deflate inflate failed"))
}

/// Convert big-endian decompressed bytes to i16 values.
fn bytes_to_i16(data: &[u8]) -> Vec<i16> {
    data.chunks_exact(2)
        .map(|c| i16::from_be_bytes([c[0], c[1]]))
        .collect()
}

/// Convert big-endian decompressed bytes to i32 values.
fn bytes_to_i32(data: &[u8]) -> Vec<i32> {
    data.chunks_exact(4)
        .map(|c| i32::from_be_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Convert big-endian decompressed bytes to i64 values.
fn bytes_to_i64(data: &[u8]) -> Vec<i64> {
    data.chunks_exact(8)
        .map(|c| i64::from_be_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
        .collect()
}

/// Convert big-endian decompressed bytes to f32 values.
fn bytes_to_f32(data: &[u8]) -> Vec<f32> {
    data.chunks_exact(4)
        .map(|c| f32::from_be_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Convert big-endian decompressed bytes to f64 values.
fn bytes_to_f64(data: &[u8]) -> Vec<f64> {
    data.chunks_exact(8)
        .map(|c| f64::from_be_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
        .collect()
}

/// Undo GZIP_2 byte shuffling of `pixels` elements.
///
/// GZIP_2 stores the most significant byte of every element, then the next
/// byte plane, and so on. The element width is implied by the tile length.
fn unshuffle(data: &[u8], pixels: usize) -> Vec<u8> {
    if pixels == 0 || !data.len().is_multiple_of(pixels) {
        return data.to_vec();
    }
    let width = data.len() / pixels;
    let mut out = alloc::vec![0u8; data.len()];
    for (plane, bytes) in data.chunks_exact(pixels).enumerate() {
        for (i, &b) in bytes.iter().enumerate() {
            out[i * width + plane] = b;
        }
    }
    out
}

/// Tile codecs whose output is plain big-endian pixel bytes.
#[derive(Clone, Copy)]
enum ByteCodec {
    /// GZIP_1
    Gzip,
    /// GZIP_2: gzip over byte-shuffled pixels.
    ShuffledGzip,
    /// NOCOMPRESS: the tile is stored as-is.
    Raw,
}

impl ByteCodec {
    fn decode(self, tile: &[u8], pixels: usize) -> Result<Vec<u8>> {
        match self {
            ByteCodec::Gzip => gzip_decompress(tile),
            ByteCodec::ShuffledGzip => Ok(unshuffle(&gzip_decompress(tile)?, pixels)),
            ByteCodec::Raw => Ok(tile.to_vec()),
        }
    }
}

/// A pixel type decodable from big-endian bytes.
trait BePixel: Copy {
    fn from_be_slice(data: &[u8]) -> Vec<Self>;
}

macro_rules! be_pixel {
    ($($t:ty => $conv:expr),* $(,)?) => {
        $(impl BePixel for $t {
            fn from_be_slice(data: &[u8]) -> Vec<Self> {
                $conv(data)
            }
        })*
    };
}

be_pixel!(
    u8 => <[u8]>::to_vec,
    i16 => bytes_to_i16,
    i32 => bytes_to_i32,
    i64 => bytes_to_i64,
    f32 => bytes_to_f32,
    f64 => bytes_to_f64,
);

// ---------------------------------------------------------------------------
// Tile geometry
// ---------------------------------------------------------------------------

/// The tile grid of a compressed image: how `ZTILEn` tiles the `ZNAXISn` image.
///
/// Tiles are stored one per binary-table row in raster order, fastest axis
/// first. A tile is a *rectangle* of the image, so reassembly is a 2-D scatter,
/// not an append: only when `ZTILE1 == ZNAXIS1` does a tile span whole image
/// rows and appending happen to give the right answer.
struct TileGrid {
    /// Image dimensions (`ZNAXISn`), fastest axis first.
    dims: Vec<usize>,
    /// Tile dimensions (`ZTILEn`), fastest axis first.
    tile: Vec<usize>,
    /// Number of tiles along each axis.
    counts: Vec<usize>,
}

impl TileGrid {
    fn new(dims: &[usize], tile: &[usize]) -> Self {
        // A missing or zero ZTILEn defaults to the full axis length, matching
        // the convention's "one tile spans the axis" reading.
        let tile: Vec<usize> = dims
            .iter()
            .enumerate()
            .map(|(i, &d)| match tile.get(i) {
                Some(&t) if t > 0 => t.min(d),
                _ => d,
            })
            .collect();
        let counts = dims
            .iter()
            .zip(&tile)
            .map(|(&d, &t)| d.div_ceil(t))
            .collect();
        Self {
            dims: dims.to_vec(),
            tile,
            counts,
        }
    }

    /// Total number of tiles in the grid.
    fn len(&self) -> usize {
        self.counts.iter().product()
    }

    /// Grid coordinate of tile `index`, fastest axis first.
    fn coord(&self, index: usize) -> Vec<usize> {
        let mut rest = index;
        self.counts
            .iter()
            .map(|&c| {
                let v = rest % c;
                rest /= c;
                v
            })
            .collect()
    }

    /// Extent of tile `index` along each axis. Edge tiles are *clipped* to the
    /// image, so they hold fewer pixels than `ZTILEn` implies.
    fn extent(&self, index: usize) -> Vec<usize> {
        let coord = self.coord(index);
        coord
            .iter()
            .enumerate()
            .map(|(ax, &c)| {
                let start = c * self.tile[ax];
                self.tile[ax].min(self.dims[ax].saturating_sub(start))
            })
            .collect()
    }

    /// Number of pixels actually stored in tile `index`.
    fn tile_pixels(&self, index: usize) -> usize {
        self.extent(index).iter().product()
    }

    /// Scatter one decoded tile into the flat image buffer.
    ///
    /// The fastest axis of a tile is contiguous in both source and destination,
    /// so each tile row is a single copy; the loop walks the higher axes.
    fn blit<T: Copy>(&self, index: usize, src: &[T], dst: &mut [T]) {
        let coord = self.coord(index);
        let extent = self.extent(index);
        let run = extent[0].min(src.len());
        if run == 0 {
            return;
        }
        // Strides of the flat image buffer, fastest axis first.
        let mut strides = Vec::with_capacity(self.dims.len());
        let mut s = 1usize;
        for &d in &self.dims {
            strides.push(s);
            s *= d;
        }
        // Offset of the tile's origin in the image.
        let origin: usize = coord
            .iter()
            .enumerate()
            .map(|(ax, &c)| c * self.tile[ax] * strides[ax])
            .sum();

        let rows: usize = extent[1..].iter().product();
        let mut higher = alloc::vec![0usize; extent.len().saturating_sub(1)];
        for r in 0..rows {
            let src_off = r * extent[0];
            if src_off >= src.len() {
                break;
            }
            let dst_off = origin
                + higher
                    .iter()
                    .enumerate()
                    .map(|(i, &h)| h * strides[i + 1])
                    .sum::<usize>();
            let n = run
                .min(src.len() - src_off)
                .min(dst.len().saturating_sub(dst_off));
            if n > 0 {
                dst[dst_off..dst_off + n].copy_from_slice(&src[src_off..src_off + n]);
            }
            // Odometer over the higher axes of this tile.
            for (i, h) in higher.iter_mut().enumerate() {
                *h += 1;
                if *h < extent[i + 1] {
                    break;
                }
                *h = 0;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Top-level decompression
// ---------------------------------------------------------------------------

/// Read and decompress a tile-compressed FITS image.
///
/// The HDU must have `HduInfo::CompressedImage`. This function extracts
/// each compressed tile from the binary table heap, decompresses it, and
/// reassembles the full image.
pub fn read_tiled_image(fits_data: &[u8], hdu: &Hdu) -> Result<ImageData> {
    let (
        zbitpix,
        znaxes,
        zcmptype,
        ztile,
        blocksize,
        rice_bytepix,
        naxis1,
        naxis2,
        pcount,
        tfields,
    ) = match &hdu.info {
        HduInfo::CompressedImage {
            zbitpix,
            znaxes,
            zcmptype,
            ztile,
            blocksize,
            rice_bytepix,
            naxis1,
            naxis2,
            pcount,
            tfields,
        } => (
            *zbitpix,
            znaxes.as_slice(),
            zcmptype.as_str(),
            ztile.as_slice(),
            *blocksize,
            *rice_bytepix,
            *naxis1,
            *naxis2,
            *pcount,
            *tfields,
        ),
        _ => return Err(Error::InvalidHeader("not a compressed image HDU")),
    };

    let _ = pcount; // used implicitly via heap

    let total_pixels: usize = if znaxes.is_empty() {
        0
    } else {
        znaxes.iter().copied().product()
    };

    if total_pixels == 0 {
        return match zbitpix {
            8 => Ok(ImageData::U8(Vec::new())),
            16 => Ok(ImageData::I16(Vec::new())),
            32 => Ok(ImageData::I32(Vec::new())),
            64 => Ok(ImageData::I64(Vec::new())),
            -32 => Ok(ImageData::F32(Vec::new())),
            -64 => Ok(ImageData::F64(Vec::new())),
            other => Err(Error::InvalidBitpix(other)),
        };
    }

    let codec = match zcmptype {
        "RICE_1" | "RICE_ONE" => None,
        "GZIP_1" => Some(ByteCodec::Gzip),
        "GZIP_2" => Some(ByteCodec::ShuffledGzip),
        "NOCOMPRESS" => Some(ByteCodec::Raw),
        "HCOMPRESS_1" => return Err(Error::UnsupportedCompression("HCOMPRESS_1")),
        "PLIO_1" => return Err(Error::UnsupportedCompression("PLIO_1")),
        _ => return Err(Error::UnsupportedCompression("unrecognized ZCMPTYPE")),
    };

    let col_info = parse_column_layout(&hdu.cards, tfields)?;
    let grid = TileGrid::new(znaxes, ztile);

    // For float types with quantization, we need ZSCALE/ZZERO
    let is_quantized = (zbitpix == -32 || zbitpix == -64)
        && col_info.zscale_offset.is_some()
        && col_info.zzero_offset.is_some();

    if let Some(codec) = codec {
        decompress_byte_tiles(
            fits_data,
            hdu,
            zbitpix,
            total_pixels,
            &grid,
            naxis1,
            naxis2,
            codec,
            &col_info,
            is_quantized,
        )
    } else {
        let params = RiceParams::for_bytepix(rice_bytepix)?;
        decompress_rice_tiles(
            fits_data,
            hdu,
            zbitpix,
            total_pixels,
            &grid,
            naxis1,
            naxis2,
            blocksize,
            &params,
            &col_info,
            is_quantized,
        )
    }
}

/// Decode every tile and scatter it into a flat image buffer.
///
/// `decode` turns one tile's compressed bytes into that tile's pixel values,
/// in tile-local raster order; `TileGrid::blit` places them at the tile's
/// rectangle in the image. Reassembly is a scatter rather than an append
/// because a tile is a rectangle: appending is only correct in the special
/// case `ZTILE1 == ZNAXIS1`, where a tile happens to span whole image rows.
///
/// A tile with an empty `COMPRESSED_DATA` entry is one cfitsio could not
/// compress (e.g. a constant float tile); its unquantized pixels are read
/// from the fallback column instead of passing through `decode`.
#[allow(clippy::too_many_arguments)]
fn scatter_tiles<T, F>(
    fits_data: &[u8],
    hdu: &Hdu,
    total_pixels: usize,
    grid: &TileGrid,
    naxis1: usize,
    naxis2: usize,
    col_info: &ColumnInfo,
    fill: T,
    mut decode: F,
) -> Result<Vec<T>>
where
    T: BePixel,
    F: FnMut(usize, &[u8], usize, usize) -> Result<Vec<T>>,
{
    let tile = |row: usize, column: &HeapColumn| {
        extract_tile_bytes(fits_data, hdu.data_start, naxis1, naxis2, row, column)
    };
    let mut output = alloc::vec![fill; total_pixels];
    for row in 0..grid.len().min(naxis2) {
        let (tile_data, tile_count) = tile(row, &col_info.compressed_data)?;
        let vals = if tile_count > 0 {
            decode(row, tile_data, tile_count, grid.tile_pixels(row))?
        } else if let Some(column) = &col_info.uncompressed_data {
            let (data, count) = tile(row, column)?;
            T::from_be_slice(&data[..count])
        } else if let Some(column) = &col_info.gzip_data {
            let (data, count) = tile(row, column)?;
            T::from_be_slice(&gzip_decompress(&data[..count])?)
        } else {
            return Err(Error::DecompressionError("empty tile"));
        };
        grid.blit(row, &vals, &mut output);
    }
    Ok(output)
}

#[allow(clippy::too_many_arguments)]
fn decompress_rice_tiles(
    fits_data: &[u8],
    hdu: &Hdu,
    zbitpix: i64,
    total_pixels: usize,
    grid: &TileGrid,
    naxis1: usize,
    naxis2: usize,
    blocksize: usize,
    params: &RiceParams,
    col_info: &ColumnInfo,
    is_quantized: bool,
) -> Result<ImageData> {
    // Per-tile quantization, read from the tile's own binary-table row.
    let quant = |row: usize| TileQuant::read(fits_data, hdu, naxis1, row, col_info);
    let dither = Dither::from_cards(&hdu.cards);

    macro_rules! scatter {
        ($fill:expr, $conv:expr) => {
            scatter_tiles(
                fits_data,
                hdu,
                total_pixels,
                grid,
                naxis1,
                naxis2,
                col_info,
                $fill,
                |row, tile_data, _tile_count, pixels_in_tile| {
                    let vals = rice_decompress(tile_data, pixels_in_tile, blocksize, params)?;
                    Ok($conv(row, vals))
                },
            )?
        };
    }

    if is_quantized && zbitpix == -32 {
        Ok(ImageData::F32(scatter!(0.0f32, |row, vals: Vec<i32>| {
            dither
                .dequantize(row, &vals, &quant(row))
                .iter()
                .map(|&v| v as f32)
                .collect::<Vec<f32>>()
        })))
    } else if is_quantized && zbitpix == -64 {
        Ok(ImageData::F64(scatter!(0.0f64, |row, vals: Vec<i32>| {
            dither.dequantize(row, &vals, &quant(row))
        })))
    } else {
        match zbitpix {
            8 => Ok(ImageData::U8(scatter!(0u8, |_row, vals: Vec<i32>| vals
                .iter()
                .map(|&v| v as u8)
                .collect::<Vec<u8>>()))),
            16 => Ok(ImageData::I16(scatter!(0i16, |_row, vals: Vec<i32>| vals
                .iter()
                .map(|&v| v as i16)
                .collect::<Vec<i16>>()))),
            32 => Ok(ImageData::I32(scatter!(0i32, |_row, vals: Vec<i32>| vals))),
            64 => Ok(ImageData::I64(scatter!(0i64, |_row, vals: Vec<i32>| vals
                .iter()
                .map(|&v| v as i64)
                .collect::<Vec<i64>>()))),
            other => Err(Error::InvalidBitpix(other)),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn decompress_byte_tiles(
    fits_data: &[u8],
    hdu: &Hdu,
    zbitpix: i64,
    total_pixels: usize,
    grid: &TileGrid,
    naxis1: usize,
    naxis2: usize,
    codec: ByteCodec,
    col_info: &ColumnInfo,
    is_quantized: bool,
) -> Result<ImageData> {
    let quant = |row: usize| TileQuant::read(fits_data, hdu, naxis1, row, col_info);
    let dither = Dither::from_cards(&hdu.cards);

    macro_rules! scatter {
        ($fill:expr, $conv:expr) => {
            scatter_tiles(
                fits_data,
                hdu,
                total_pixels,
                grid,
                naxis1,
                naxis2,
                col_info,
                $fill,
                |row, tile_data, tile_count, pixels_in_tile| {
                    let raw = codec.decode(&tile_data[..tile_count], pixels_in_tile)?;
                    Ok($conv(row, raw, pixels_in_tile))
                },
            )?
        };
    }

    if is_quantized && zbitpix == -32 {
        Ok(ImageData::F32(scatter!(
            0.0f32,
            |row, raw: Vec<u8>, _n: usize| {
                dither
                    .dequantize(row, &bytes_to_i32(&raw), &quant(row))
                    .iter()
                    .map(|&v| v as f32)
                    .collect::<Vec<f32>>()
            }
        )))
    } else if is_quantized && zbitpix == -64 {
        Ok(ImageData::F64(scatter!(
            0.0f64,
            |row, raw: Vec<u8>, _n: usize| {
                dither.dequantize(row, &bytes_to_i32(&raw), &quant(row))
            }
        )))
    } else {
        match zbitpix {
            // cfitsio may encode 8- and 16-bit GZIP tiles as i32; detect by
            // decompressed length and narrow, otherwise take the bytes as-is.
            8 => Ok(ImageData::U8(scatter!(
                0u8,
                |_row, raw: Vec<u8>, n: usize| if raw.len() == n * 4 {
                    bytes_to_i32(&raw)
                        .iter()
                        .map(|&v| v as u8)
                        .collect::<Vec<u8>>()
                } else {
                    raw
                }
            ))),
            16 => Ok(ImageData::I16(scatter!(
                0i16,
                |_row, raw: Vec<u8>, n: usize| if raw.len() == n * 4 {
                    bytes_to_i32(&raw)
                        .iter()
                        .map(|&v| v as i16)
                        .collect::<Vec<i16>>()
                } else {
                    bytes_to_i16(&raw)
                }
            ))),
            32 => Ok(ImageData::I32(scatter!(
                0i32,
                |_row, raw: Vec<u8>, _n: usize| bytes_to_i32(&raw)
            ))),
            64 => Ok(ImageData::I64(scatter!(
                0i64,
                |_row, raw: Vec<u8>, _n: usize| bytes_to_i64(&raw)
            ))),
            -32 => Ok(ImageData::F32(scatter!(
                0.0f32,
                |_row, raw: Vec<u8>, _n: usize| bytes_to_f32(&raw)
            ))),
            -64 => Ok(ImageData::F64(scatter!(
                0.0f64,
                |_row, raw: Vec<u8>, _n: usize| bytes_to_f64(&raw)
            ))),
            other => Err(Error::InvalidBitpix(other)),
        }
    }
}

/// Length of cfitsio's dither random table (`N_RANDOM`).
const N_RANDOM: usize = 10_000;

/// Quantized value that `SUBTRACTIVE_DITHER_2` reserves for an exact 0.0.
const ZERO_VALUE: i32 = -2_147_483_646;

/// How quantized floats were dithered, from `ZQUANTIZ` and `ZDITHER0`.
enum Dither {
    None,
    Subtractive {
        /// `ZDITHER0`, the 1-based seed of the first tile.
        seed: usize,
        /// `SUBTRACTIVE_DITHER_2`, which keeps exact zeros undithered.
        keep_zero: bool,
        table: Vec<f32>,
    },
}

impl Dither {
    fn from_cards(cards: &[Card]) -> Self {
        let method = card_string_value(cards, "ZQUANTIZ");
        let keep_zero = match method.as_deref() {
            Some("SUBTRACTIVE_DITHER_1") => false,
            Some("SUBTRACTIVE_DITHER_2") => true,
            _ => return Dither::None,
        };
        let seed = cards
            .iter()
            .find(|c| c.keyword_str() == "ZDITHER0")
            .and_then(|c| match c.value {
                Some(Value::Integer(n)) if n > 0 => Some(n as usize),
                _ => None,
            })
            .unwrap_or(1);
        Dither::Subtractive {
            seed,
            keep_zero,
            table: dither_table(),
        }
    }

    /// Restore physical values for one tile (0-based `tile`); null pixels
    /// become NaN.
    fn dequantize(&self, tile: usize, vals: &[i32], quant: &TileQuant) -> Vec<f64> {
        let &TileQuant { scale, zero, blank } = quant;
        match self {
            Dither::None => vals
                .iter()
                .map(|&iv| {
                    if Some(iv) == blank {
                        f64::NAN
                    } else {
                        zero + scale * iv as f64
                    }
                })
                .collect(),
            Dither::Subtractive {
                seed,
                keep_zero,
                table,
            } => {
                let mut iseed = (tile + seed - 1) % N_RANDOM;
                let mut next = (table[iseed] * 500.0) as usize;
                vals.iter()
                    .map(|&iv| {
                        let v = if Some(iv) == blank {
                            f64::NAN
                        } else if *keep_zero && iv == ZERO_VALUE {
                            0.0
                        } else {
                            (iv as f64 - table[next] as f64 + 0.5) * scale + zero
                        };
                        next += 1;
                        if next == N_RANDOM {
                            iseed = (iseed + 1) % N_RANDOM;
                            next = (table[iseed] * 500.0) as usize;
                        }
                        v
                    })
                    .collect()
            }
        }
    }
}

/// cfitsio's `fits_init_randoms`: a Park–Miller sequence scaled to (0, 1).
fn dither_table() -> Vec<f32> {
    let a = 16807.0f64;
    let m = 2_147_483_647.0f64;
    let mut seed = 1.0f64;
    (0..N_RANDOM)
        .map(|_| {
            let temp = a * seed;
            seed = temp - m * ((temp / m) as i64) as f64;
            (seed / m) as f32
        })
        .collect()
}

/// Per-tile quantization: ZSCALE, ZZERO, and the integer marking a null
/// pixel (the tile's `ZBLANK` column, else the `ZBLANK` header keyword).
struct TileQuant {
    scale: f64,
    zero: f64,
    blank: Option<i32>,
}

impl TileQuant {
    fn read(fits_data: &[u8], hdu: &Hdu, naxis1: usize, row: usize, col_info: &ColumnInfo) -> Self {
        let row_start = hdu.data_start + row * naxis1;
        let header_blank = hdu.cards.iter().find_map(|c| match c.value {
            Some(Value::Integer(n)) if c.keyword_str() == "ZBLANK" => Some(n as i32),
            _ => None,
        });
        TileQuant {
            scale: read_f64_be(&fits_data[row_start + col_info.zscale_offset.unwrap()..]),
            zero: read_f64_be(&fits_data[row_start + col_info.zzero_offset.unwrap()..]),
            blank: col_info
                .zblank_offset
                .map(|off| read_i32_be(&fits_data[row_start + off..]))
                .or(header_blank),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rice_params() {
        let p8 = RiceParams::for_bytepix(1).unwrap();
        assert_eq!(p8.fsbits, 3);
        assert_eq!(p8.fsmax, 6);

        let p16 = RiceParams::for_bytepix(2).unwrap();
        assert_eq!(p16.fsbits, 4);
        assert_eq!(p16.fsmax, 14);

        let p32 = RiceParams::for_bytepix(4).unwrap();
        assert_eq!(p32.fsbits, 5);
        assert_eq!(p32.fsmax, 25);
    }

    #[test]
    fn test_p_descriptor() {
        let mut data = [0u8; 8];
        data[0..4].copy_from_slice(&100u32.to_be_bytes());
        data[4..8].copy_from_slice(&200u32.to_be_bytes());
        let (count, offset) = read_p_descriptor(&data);
        assert_eq!(count, 100);
        assert_eq!(offset, 200);
    }

    #[test]
    fn test_nonzero_count_table() {
        assert_eq!(NONZERO_COUNT[0], 0);
        assert_eq!(NONZERO_COUNT[1], 1);
        assert_eq!(NONZERO_COUNT[2], 2);
        assert_eq!(NONZERO_COUNT[3], 2);
        assert_eq!(NONZERO_COUNT[128], 8);
        assert_eq!(NONZERO_COUNT[255], 8);
    }

    #[test]
    fn test_rice_low_entropy() {
        // Construct a Rice-compressed stream: first pixel = 42 (i16),
        // then one block of all-zeros (fs = -1, encoded as fs+1 = 0).
        let params = RiceParams::for_bytepix(2).unwrap();
        let blocksize = 4;

        // First pixel: 42 as big-endian i16
        let mut data = vec![0u8, 42];
        // FS value: 0 means fs = -1 (low entropy). FSBITS=4 so we need 4 bits = 0000.
        // Pack 0000_0000 into one byte (the 4 fs bits are 0000, rest is padding)
        data.push(0x00);

        let result = rice_decompress(&data, 5, blocksize, &params).unwrap();
        assert_eq!(result, vec![42, 42, 42, 42, 42]);
    }

    #[test]
    fn test_dither_table_matches_cfitsio() {
        let table = dither_table();
        assert_eq!(table.len(), N_RANDOM);
        // cfitsio's fits_init_randoms checks that the 10000th seed is 1043618065.
        let last = (table[N_RANDOM - 1] as f64 * 2_147_483_647.0).round();
        assert!((last - 1_043_618_065.0).abs() < 256.0);
    }

    fn quantize_cards(method: &str, seed: i64) -> Vec<Card> {
        vec![
            Card {
                keyword: *b"ZQUANTIZ",
                value: Some(Value::String(method.into())),
                comment: None,
            },
            Card {
                keyword: *b"ZDITHER0",
                value: Some(Value::Integer(seed)),
                comment: None,
            },
        ]
    }

    #[test]
    fn test_dequantize_subtractive_dither_1() {
        let dither = Dither::from_cards(&quantize_cards("SUBTRACTIVE_DITHER_1", 3));
        let table = dither_table();
        // Tile 1 with ZDITHER0 = 3 starts at iseed = 3.
        let next = (table[3] * 500.0) as usize;
        let out = dither.dequantize(1, &[10, -4], &quant(2.0, 100.0, None));
        assert_eq!(out[0], (10.0 - table[next] as f64 + 0.5) * 2.0 + 100.0);
        assert_eq!(out[1], (-4.0 - table[next + 1] as f64 + 0.5) * 2.0 + 100.0);
    }

    #[test]
    fn test_dequantize_subtractive_dither_2_keeps_zero() {
        let dither = Dither::from_cards(&quantize_cards("SUBTRACTIVE_DITHER_2", 1));
        let out = dither.dequantize(0, &[ZERO_VALUE, 7], &quant(0.5, 10.0, None));
        assert_eq!(out[0], 0.0);
        assert_ne!(out[1], 10.0 + 0.5 * 7.0);
    }

    #[test]
    fn test_dequantize_no_dither() {
        let dither = Dither::from_cards(&quantize_cards("NO_DITHER", 1));
        assert_eq!(
            dither.dequantize(0, &[4], &quant(0.5, 10.0, None)),
            vec![12.0]
        );
    }

    #[test]
    fn test_dequantize_zblank_is_nan() {
        for method in ["NO_DITHER", "SUBTRACTIVE_DITHER_1", "SUBTRACTIVE_DITHER_2"] {
            let dither = Dither::from_cards(&quantize_cards(method, 1));
            let out = dither.dequantize(0, &[i32::MIN, 4], &quant(0.5, 10.0, Some(i32::MIN)));
            assert!(out[0].is_nan(), "{method}");
            assert!(out[1].is_finite(), "{method}");
        }
    }

    #[test]
    fn test_unshuffle() {
        // Two i16 values 0x0102, 0x0304 shuffled as MSBs then LSBs.
        assert_eq!(unshuffle(&[1, 3, 2, 4], 2), vec![1, 2, 3, 4]);
        // One i32 per pixel: four byte planes.
        let shuffled = [0xA0, 0xB0, 0xA1, 0xB1, 0xA2, 0xB2, 0xA3, 0xB3];
        assert_eq!(
            unshuffle(&shuffled, 2),
            vec![0xA0, 0xA1, 0xA2, 0xA3, 0xB0, 0xB1, 0xB2, 0xB3]
        );
        // One byte per pixel is the identity.
        assert_eq!(unshuffle(&[7, 8, 9], 3), vec![7, 8, 9]);
    }

    fn quant(scale: f64, zero: f64, blank: Option<i32>) -> TileQuant {
        TileQuant { scale, zero, blank }
    }

    fn string_card(keyword: &str, value: &str) -> Card {
        let mut kw = [b' '; 8];
        kw[..keyword.len()].copy_from_slice(keyword.as_bytes());
        Card {
            keyword: kw,
            value: Some(Value::String(value.into())),
            comment: None,
        }
    }

    fn integer_card(keyword: &str, value: i64) -> Card {
        let mut kw = [b' '; 8];
        kw[..keyword.len()].copy_from_slice(keyword.as_bytes());
        Card {
            keyword: kw,
            value: Some(Value::Integer(value)),
            comment: None,
        }
    }

    fn gzip(data: &[u8]) -> Vec<u8> {
        miniz_oxide::deflate::compress_to_vec_zlib(data, 6)
    }

    /// Build a one-tile-per-row compressed image HDU over `rows` of
    /// `(column name, tile bytes)` plus optional fixed ZSCALE/ZZERO columns.
    fn tiled_hdu(
        zcmptype: &str,
        zbitpix: i64,
        width: usize,
        heap_columns: &[(&str, &str)],
        rows: &[Vec<Vec<u8>>],
        quant: Option<(f64, f64)>,
        extra: Vec<Card>,
    ) -> (Vec<u8>, Hdu) {
        let mut cards = vec![
            string_card("ZQUANTIZ", "NO_DITHER"),
            integer_card("ZVAL1", 32),
            integer_card("ZVAL2", 4),
        ];
        cards.extend(extra);
        let mut naxis1 = 0;
        for (i, (name, tform)) in heap_columns.iter().enumerate() {
            cards.push(string_card(&alloc::format!("TTYPE{}", i + 1), name));
            cards.push(string_card(&alloc::format!("TFORM{}", i + 1), tform));
            naxis1 += 8;
        }
        let mut tfields = heap_columns.len();
        if quant.is_some() {
            for name in ["ZSCALE", "ZZERO"] {
                tfields += 1;
                cards.push(string_card(&alloc::format!("TTYPE{tfields}"), name));
                cards.push(string_card(&alloc::format!("TFORM{tfields}"), "1D"));
                naxis1 += 8;
            }
        }

        let mut table = Vec::new();
        let mut heap = Vec::new();
        for row in rows {
            for (bytes, (_, tform)) in row.iter().zip(heap_columns) {
                let elem = if tform.contains("PI") { 2 } else { 1 };
                table.extend_from_slice(&((bytes.len() / elem) as u32).to_be_bytes());
                table.extend_from_slice(&(heap.len() as u32).to_be_bytes());
                heap.extend_from_slice(bytes);
            }
            if let Some((scale, zero)) = quant {
                table.extend_from_slice(&scale.to_be_bytes());
                table.extend_from_slice(&zero.to_be_bytes());
            }
        }
        table.extend(heap);

        let hdu = Hdu {
            info: HduInfo::CompressedImage {
                zbitpix,
                znaxes: vec![width, rows.len()],
                zcmptype: zcmptype.into(),
                ztile: vec![width, 1],
                blocksize: 32,
                rice_bytepix: 4,
                naxis1,
                naxis2: rows.len(),
                pcount: 0,
                tfields,
            },
            header_start: 0,
            data_start: 0,
            data_len: table.len(),
            cards,
        };
        (table, hdu)
    }

    fn i16_be(vals: &[i16]) -> Vec<u8> {
        vals.iter().flat_map(|v| v.to_be_bytes()).collect()
    }

    #[test]
    fn test_gzip2_unshuffles_i16() {
        let vals = [0x0102i16, -3, 0x7f00];
        let shuffled = unshuffle_inverse(&i16_be(&vals), 2);
        let (data, hdu) = tiled_hdu(
            "GZIP_2",
            16,
            3,
            &[("COMPRESSED_DATA", "1PB")],
            &[vec![gzip(&shuffled)]],
            None,
            vec![],
        );
        match read_tiled_image(&data, &hdu).unwrap() {
            ImageData::I16(v) => assert_eq!(v, vals),
            other => panic!("expected I16, got {other:?}"),
        }
    }

    /// GZIP_2's encoder: byte plane `k` of every element, in turn.
    fn unshuffle_inverse(data: &[u8], width: usize) -> Vec<u8> {
        (0..width)
            .flat_map(|k| data.iter().skip(k).step_by(width).copied())
            .collect()
    }

    #[test]
    fn test_nocompress_reads_raw_pixels() {
        let vals = [5i16, -6, 7, 300];
        let (data, hdu) = tiled_hdu(
            "NOCOMPRESS",
            16,
            2,
            &[("COMPRESSED_DATA", "1PB")],
            &[vec![i16_be(&vals[..2])], vec![i16_be(&vals[2..])]],
            None,
            vec![],
        );
        match read_tiled_image(&data, &hdu).unwrap() {
            ImageData::I16(v) => assert_eq!(v, vals),
            other => panic!("expected I16, got {other:?}"),
        }
    }

    #[test]
    fn test_empty_tile_falls_back_to_gzip_column() {
        let pixels: Vec<u8> = [5.0f32, 5.0].iter().flat_map(|v| v.to_be_bytes()).collect();
        let (data, hdu) = tiled_hdu(
            "RICE_1",
            -32,
            2,
            &[("COMPRESSED_DATA", "1PB"), ("GZIP_COMPRESSED_DATA", "1PB")],
            &[vec![vec![], gzip(&pixels)]],
            Some((0.0, 0.0)),
            vec![],
        );
        match read_tiled_image(&data, &hdu).unwrap() {
            ImageData::F32(v) => assert_eq!(v, [5.0, 5.0]),
            other => panic!("expected F32, got {other:?}"),
        }
    }

    #[test]
    fn test_empty_tile_falls_back_to_uncompressed_column() {
        let (data, hdu) = tiled_hdu(
            "RICE_1",
            16,
            2,
            &[("COMPRESSED_DATA", "1PB"), ("UNCOMPRESSED_DATA", "1PI")],
            &[vec![vec![], i16_be(&[-9, 12])]],
            None,
            vec![],
        );
        match read_tiled_image(&data, &hdu).unwrap() {
            ImageData::I16(v) => assert_eq!(v, [-9, 12]),
            other => panic!("expected I16, got {other:?}"),
        }
    }

    #[test]
    fn test_empty_tile_without_fallback_is_an_error() {
        let (data, hdu) = tiled_hdu(
            "RICE_1",
            16,
            2,
            &[("COMPRESSED_DATA", "1PB")],
            &[vec![vec![]]],
            None,
            vec![],
        );
        assert!(matches!(
            read_tiled_image(&data, &hdu),
            Err(Error::DecompressionError(_))
        ));
    }

    #[test]
    fn test_header_zblank_reads_as_nan() {
        let ints: Vec<u8> = [i32::MIN, 4].iter().flat_map(|v| v.to_be_bytes()).collect();
        let (data, hdu) = tiled_hdu(
            "GZIP_1",
            -32,
            2,
            &[("COMPRESSED_DATA", "1PB")],
            &[vec![gzip(&ints)]],
            Some((0.5, 10.0)),
            vec![integer_card("ZBLANK", i32::MIN as i64)],
        );
        match read_tiled_image(&data, &hdu).unwrap() {
            ImageData::F32(v) => {
                assert!(v[0].is_nan());
                assert_eq!(v[1], 12.0);
            }
            other => panic!("expected F32, got {other:?}"),
        }
    }

    #[test]
    fn test_unsupported_codecs_are_named() {
        for (zcmptype, msg) in [
            ("HCOMPRESS_1", "HCOMPRESS_1"),
            ("PLIO_1", "PLIO_1"),
            ("RICE_2", "unrecognized ZCMPTYPE"),
        ] {
            let (data, hdu) = tiled_hdu(
                zcmptype,
                16,
                1,
                &[("COMPRESSED_DATA", "1PB")],
                &[vec![vec![0, 1]]],
                None,
                vec![],
            );
            match read_tiled_image(&data, &hdu) {
                Err(Error::UnsupportedCompression(m)) => assert_eq!(m, msg),
                other => panic!("{zcmptype}: expected UnsupportedCompression, got {other:?}"),
            }
        }
    }
}
