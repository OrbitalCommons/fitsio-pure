//! Tile-compressed image decompression for FITS.
//!
//! Supports RICE_1/RICE_ONE and GZIP_1 compression algorithms per the
//! FITS tiled image compression convention.

use alloc::string::String;
use alloc::vec::Vec;

use crate::endian::{read_f64_be, read_i32_be};
use crate::error::{Error, Result};
use crate::hdu::{Hdu, HduInfo};
use crate::header::Card;
use crate::image::ImageData;
use crate::value::Value;

// ---------------------------------------------------------------------------
// Column layout helpers
// ---------------------------------------------------------------------------

struct ColumnInfo {
    compressed_data_offset: usize,
    zscale_offset: Option<usize>,
    zzero_offset: Option<usize>,
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

/// Parse the binary table column layout to find COMPRESSED_DATA, ZSCALE, and ZZERO columns.
fn parse_column_layout(cards: &[Card], tfields: usize) -> Result<ColumnInfo> {
    let mut offsets = Vec::with_capacity(tfields);
    let mut compressed_data_col = None;
    let mut zscale_col = None;
    let mut zzero_col = None;

    // First pass: collect column names
    for i in 1..=tfields {
        let ttype_kw = alloc::format!("TTYPE{}", i);
        let name = card_string_value(cards, &ttype_kw).unwrap_or_default();
        if name == "COMPRESSED_DATA" {
            compressed_data_col = Some(i - 1);
        } else if name == "ZSCALE" {
            zscale_col = Some(i - 1);
        } else if name == "ZZERO" {
            zzero_col = Some(i - 1);
        }
    }

    // Second pass: compute byte offsets from TFORM values
    let mut offset = 0usize;
    for i in 1..=tfields {
        offsets.push(offset);
        let tform_kw = alloc::format!("TFORM{}", i);
        let tform = card_string_value(cards, &tform_kw)
            .ok_or(Error::InvalidHeader("missing TFORM in compressed image"))?;
        let (repeat, col_type) = crate::bintable::parse_tform_binary(&tform)?;
        let width = match col_type {
            crate::bintable::BinaryColumnType::Bit => repeat.div_ceil(8),
            crate::bintable::BinaryColumnType::VarArrayP(_) => 8 * repeat,
            crate::bintable::BinaryColumnType::VarArrayQ(_) => 16 * repeat,
            _ => repeat * crate::bintable::binary_type_byte_size(&col_type),
        };
        offset += width;
    }

    let compressed_idx =
        compressed_data_col.ok_or(Error::InvalidHeader("missing COMPRESSED_DATA column"))?;

    Ok(ColumnInfo {
        compressed_data_offset: offsets[compressed_idx],
        zscale_offset: zscale_col.map(|i| offsets[i]),
        zzero_offset: zzero_col.map(|i| offsets[i]),
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

/// Extract compressed tile bytes from the heap for a given row.
///
/// Returns `(data_slice, count)` where `count` is the number of compressed
/// bytes.  For Rice decompression the slice extends beyond `count` so that
/// the bit-stream reader can safely over-read by a few bytes, matching the
/// cfitsio behaviour.
fn extract_tile_bytes(
    fits_data: &[u8],
    data_start: usize,
    naxis1: usize,
    naxis2: usize,
    row: usize,
    col_offset: usize,
) -> Result<(&[u8], usize)> {
    let desc_pos = data_start + row * naxis1 + col_offset;
    if desc_pos + 8 > fits_data.len() {
        return Err(Error::UnexpectedEof);
    }
    let (count, heap_offset) = read_p_descriptor(&fits_data[desc_pos..]);
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

/// Strip the gzip header and trailer, returning the raw deflate payload.
fn strip_gzip_header(data: &[u8]) -> Result<&[u8]> {
    if data.len() < 18 || data[0] != 0x1f || data[1] != 0x8b || data[2] != 0x08 {
        return Err(Error::DecompressionError("invalid gzip header"));
    }
    let flg = data[3];
    let mut pos = 10usize;
    if flg & 0x04 != 0 {
        // FEXTRA
        if pos + 2 > data.len() {
            return Err(Error::DecompressionError("truncated gzip FEXTRA"));
        }
        let xlen = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2 + xlen;
    }
    if flg & 0x08 != 0 {
        // FNAME: skip null-terminated string
        while pos < data.len() && data[pos] != 0 {
            pos += 1;
        }
        pos += 1; // skip the null terminator
    }
    if flg & 0x10 != 0 {
        // FCOMMENT: skip null-terminated string
        while pos < data.len() && data[pos] != 0 {
            pos += 1;
        }
        pos += 1;
    }
    if flg & 0x02 != 0 {
        // FHCRC
        pos += 2;
    }
    if pos >= data.len() || data.len() < pos + 8 {
        return Err(Error::DecompressionError("truncated gzip data"));
    }
    // Strip the 8-byte trailer (CRC32 + ISIZE)
    Ok(&data[pos..data.len() - 8])
}

/// Decompress GZIP_1 compressed tile data.
fn gzip_decompress(compressed: &[u8]) -> Result<Vec<u8>> {
    // Try gzip format first (magic bytes 1f 8b), then zlib, then raw deflate.
    if compressed.len() >= 2 && compressed[0] == 0x1f && compressed[1] == 0x8b {
        let deflate_payload = strip_gzip_header(compressed)?;
        return miniz_oxide::inflate::decompress_to_vec(deflate_payload)
            .map_err(|_| Error::DecompressionError("gzip inflate failed"));
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

    let col_info = parse_column_layout(&hdu.cards, tfields)?;
    let is_rice = zcmptype.contains("RICE");
    let is_gzip = zcmptype.contains("GZIP");
    if !is_rice && !is_gzip {
        return Err(Error::UnsupportedCompression(
            "only RICE_1 and GZIP_1 supported",
        ));
    }

    let grid = TileGrid::new(znaxes, ztile);

    // For float types with quantization, we need ZSCALE/ZZERO
    let is_quantized = (zbitpix == -32 || zbitpix == -64)
        && col_info.zscale_offset.is_some()
        && col_info.zzero_offset.is_some();

    if is_rice {
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
    } else {
        decompress_gzip_tiles(
            fits_data,
            hdu,
            zbitpix,
            total_pixels,
            &grid,
            naxis1,
            naxis2,
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
    T: Copy,
    F: FnMut(usize, &[u8], usize, usize) -> Result<Vec<T>>,
{
    let mut output = alloc::vec![fill; total_pixels];
    for row in 0..grid.len().min(naxis2) {
        let (tile_data, tile_count) = extract_tile_bytes(
            fits_data,
            hdu.data_start,
            naxis1,
            naxis2,
            row,
            col_info.compressed_data_offset,
        )?;
        let vals = decode(row, tile_data, tile_count, grid.tile_pixels(row))?;
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
    // Per-tile ZSCALE/ZZERO, read from the tile's own binary-table row.
    let quant = |row: usize| {
        read_zscale_zzero(
            fits_data,
            hdu.data_start,
            naxis1,
            row,
            col_info.zscale_offset.unwrap(),
            col_info.zzero_offset.unwrap(),
        )
    };

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
            let (scale, zero) = quant(row);
            vals.iter()
                .map(|&iv| (zero + scale * iv as f64) as f32)
                .collect::<Vec<f32>>()
        })))
    } else if is_quantized && zbitpix == -64 {
        Ok(ImageData::F64(scatter!(0.0f64, |row, vals: Vec<i32>| {
            let (scale, zero) = quant(row);
            vals.iter()
                .map(|&iv| zero + scale * iv as f64)
                .collect::<Vec<f64>>()
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
fn decompress_gzip_tiles(
    fits_data: &[u8],
    hdu: &Hdu,
    zbitpix: i64,
    total_pixels: usize,
    grid: &TileGrid,
    naxis1: usize,
    naxis2: usize,
    col_info: &ColumnInfo,
    is_quantized: bool,
) -> Result<ImageData> {
    let quant = |row: usize| {
        read_zscale_zzero(
            fits_data,
            hdu.data_start,
            naxis1,
            row,
            col_info.zscale_offset.unwrap(),
            col_info.zzero_offset.unwrap(),
        )
    };

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
                    let raw = gzip_decompress(&tile_data[..tile_count])?;
                    Ok($conv(row, raw, pixels_in_tile))
                },
            )?
        };
    }

    if is_quantized && zbitpix == -32 {
        Ok(ImageData::F32(scatter!(
            0.0f32,
            |row, raw: Vec<u8>, _n: usize| {
                let (scale, zero) = quant(row);
                bytes_to_i32(&raw)
                    .iter()
                    .map(|&iv| (zero + scale * iv as f64) as f32)
                    .collect::<Vec<f32>>()
            }
        )))
    } else if is_quantized && zbitpix == -64 {
        Ok(ImageData::F64(scatter!(
            0.0f64,
            |row, raw: Vec<u8>, _n: usize| {
                let (scale, zero) = quant(row);
                bytes_to_i32(&raw)
                    .iter()
                    .map(|&iv| zero + scale * iv as f64)
                    .collect::<Vec<f64>>()
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

fn read_zscale_zzero(
    fits_data: &[u8],
    data_start: usize,
    naxis1: usize,
    row: usize,
    zscale_offset: usize,
    zzero_offset: usize,
) -> (f64, f64) {
    let row_start = data_start + row * naxis1;
    let scale = read_f64_be(&fits_data[row_start + zscale_offset..]);
    let zero = read_f64_be(&fits_data[row_start + zzero_offset..]);
    (scale, zero)
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
}
