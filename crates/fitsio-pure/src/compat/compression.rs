//! cfitsio's `"file.fits[compress …]"` output-file syntax.
//!
//! A file created with a compression spec after its name is held and edited
//! uncompressed in memory, and its images are tile-compressed when it is
//! saved, as cfitsio writes them: a primary image moves to the first
//! extension, behind an empty primary HDU, and is marked with `ZSIMPLE`.

use std::path::{Path, PathBuf};

use super::errors::{Error, Result};
use crate::compress::{Algorithm, Dither, Quantize, TileCompression};
use crate::header::Card;
use crate::image::ImageData;

/// A parsed `[compress …]` spec.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CompressSpec {
    algorithm: Algorithm,
    /// Tile dimensions, `NAXIS1` first, as given; 0 or missing means the
    /// whole first axis and one pixel along the others, as in cfitsio.
    tiles: Vec<usize>,
    quantize: Quantize,
}

/// Split a trailing `[compress …]` off `path`, if it has one.
pub(crate) fn split_path(path: &Path) -> Result<(PathBuf, Option<CompressSpec>)> {
    let Some(text) = path.to_str() else {
        return Ok((path.to_path_buf(), None));
    };
    let Some(open) = text.rfind('[').filter(|_| text.ends_with(']')) else {
        return Ok((path.to_path_buf(), None));
    };
    let spec = text[open + 1..text.len() - 1].trim_start();
    if !(spec.starts_with("compress") || spec.starts_with("COMPRESS")) {
        return Ok((path.to_path_buf(), None));
    }
    Ok((PathBuf::from(&text[..open]), Some(parse(&spec[8..])?)))
}

/// Parse what follows `compress`, as cfitsio's `ffparsecompspec` does:
/// an algorithm (`R` for `RICE_1`, `G` for `GZIP_1`), tile dimensions such as
/// `100,100`, then `; q LEVEL` (or `q0 LEVEL`, without dithering).
fn parse(spec: &str) -> Result<CompressSpec> {
    let unsupported = |what: &str| Error::Message(format!("unsupported compression spec: {what}"));
    let mut rest = spec.trim_start();
    let word_end = rest.find([' ', ';']).unwrap_or(rest.len());
    let algorithm = match rest.as_bytes().first().map(u8::to_ascii_uppercase) {
        None | Some(b';') => Algorithm::Rice,
        Some(b'R') => Algorithm::Rice,
        Some(b'G') => Algorithm::Gzip,
        Some(_) => return Err(unsupported(&rest[..word_end])),
    };
    if !rest.starts_with(';') {
        rest = &rest[word_end..];
    }

    let (tiles_text, options) = match rest.split_once(';') {
        Some((tiles, options)) => (tiles, Some(options)),
        None => (rest, None),
    };
    let tiles = tiles_text
        .split([',', ' '])
        .filter(|t| !t.is_empty())
        .map(|t| t.parse().map_err(|_| unsupported(tiles_text)))
        .collect::<Result<Vec<usize>>>()?;

    let mut quantize = Quantize::new();
    if let Some(options) = options {
        for option in options.split(',').map(str::trim).filter(|o| !o.is_empty()) {
            let Some(value) = option.strip_prefix(['q', 'Q']) else {
                return Err(unsupported(option));
            };
            // `q0` quantizes without dithering.
            let (dither, value) = match value.strip_prefix('0') {
                Some(level) if level.starts_with(' ') || level.is_empty() => (Dither::None, level),
                _ => (Dither::Subtractive, value),
            };
            if value.starts_with(['z', 'Z']) {
                return Err(unsupported("SUBTRACTIVE_DITHER_2 (qz)"));
            }
            let level: f32 = match value.trim() {
                "" => 0.0,
                level => level.parse().map_err(|_| unsupported(option))?,
            };
            // A level of 0 means cfitsio's default: 4, or 16 without dithering.
            let level = match (level, dither) {
                (0.0, Dither::None) => 16.0,
                (0.0, Dither::Subtractive) => 4.0,
                (level, _) => level,
            };
            quantize = quantize.dither(dither);
            quantize = quantize.level(level);
        }
    }
    Ok(CompressSpec {
        algorithm,
        tiles,
        quantize,
    })
}

impl CompressSpec {
    /// The settings for an image with axes `naxes`.
    fn for_image(&self, naxes: &[usize]) -> TileCompression {
        let tiles: Vec<usize> = naxes
            .iter()
            .enumerate()
            .map(
                |(axis, &len)| match self.tiles.get(axis).copied().unwrap_or(0) {
                    0 if axis == 0 => len,
                    0 => 1,
                    n => n,
                },
            )
            .collect();
        TileCompression::new(self.algorithm)
            .tile_dims(&tiles)
            .quantize(self.quantize)
    }
}

/// Keywords an image header has that the compressed header replaces.
fn is_image_structure(keyword: &str) -> bool {
    matches!(
        keyword,
        "SIMPLE" | "XTENSION" | "BITPIX" | "PCOUNT" | "GCOUNT" | "EXTEND"
    ) || keyword
        .strip_prefix("NAXIS")
        .is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit()))
}

/// `data`, a whole file, with every non-empty image HDU tile-compressed.
pub(crate) fn compress_images(data: &[u8], spec: &CompressSpec) -> Result<Vec<u8>> {
    use crate::compress::{compress_image_hdu, compress_primary_image_hdu};
    use crate::hdu::HduInfo;

    let parsed = crate::hdu::parse_fits(data)?;
    let mut out = Vec::with_capacity(data.len() / 2);
    for (index, hdu) in parsed.hdus.iter().enumerate() {
        let image = match &hdu.info {
            HduInfo::Primary { bitpix, naxes } | HduInfo::Image { bitpix, naxes }
                if !naxes.is_empty() && !naxes.contains(&0) =>
            {
                Some((*bitpix, naxes))
            }
            _ => None,
        };
        let Some((bitpix, naxes)) = image else {
            // Copied as it is, padding included.
            let end = hdu.data_start + crate::block::padded_byte_len(hdu.data_len);
            let start = out.len();
            out.extend_from_slice(&data[hdu.header_start..end.min(data.len())]);
            out.resize(start + (end - hdu.header_start), 0);
            continue;
        };
        let primary = index == 0;
        if primary {
            // An empty primary HDU keeping the image's BITPIX, as cfitsio
            // writes it.
            let mut cards = crate::primary::build_primary_header(bitpix, &[])?;
            cards.push(Card {
                keyword: *b"EXTEND  ",
                value: Some(crate::value::Value::Logical(true)),
                comment: Some("FITS dataset may contain extensions".into()),
            });
            out.extend(crate::header::serialize_header(&cards)?);
        }
        let extra: Vec<Card> = hdu
            .cards
            .iter()
            .filter(|c| !c.is_end() && !is_image_structure(c.keyword_str()))
            .cloned()
            .collect();
        let opts = spec.for_image(naxes);
        macro_rules! compress {
            ($pixels:expr) => {
                if primary {
                    compress_primary_image_hdu(naxes, $pixels, &opts, &extra)?
                } else {
                    compress_image_hdu(naxes, $pixels, &opts, &extra)?
                }
            };
        }
        let bytes = match crate::image::read_image_data(data, hdu)? {
            ImageData::U8(v) => compress!(&v),
            ImageData::I16(v) => compress!(&v),
            ImageData::I32(v) => compress!(&v),
            ImageData::F32(v) => compress!(&v),
            ImageData::F64(v) => compress!(&v),
            ImageData::I64(_) => {
                return Err(Error::Message(
                    "64-bit integer images can't be tile-compressed".to_string(),
                ))
            }
        };
        out.extend(bytes);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs_parse_as_cfitsio_parses_them() {
        let rice = Quantize::new();
        let cases = [
            ("", Algorithm::Rice, vec![], rice),
            (" R", Algorithm::Rice, vec![], rice),
            (" RICE 100,100", Algorithm::Rice, vec![100, 100], rice),
            (" G", Algorithm::Gzip, vec![], rice),
            (" GZIP_1 64, 32", Algorithm::Gzip, vec![64, 32], rice),
            ("; q 8", Algorithm::Rice, vec![], rice.level(8.0)),
            (
                " R 10,5; q0 16",
                Algorithm::Rice,
                vec![10, 5],
                rice.level(16.0).dither(Dither::None),
            ),
            (" G; q -0.5", Algorithm::Gzip, vec![], rice.level(-0.5)),
        ];
        for (spec, algorithm, tiles, quantize) in cases {
            assert_eq!(
                parse(spec).unwrap(),
                CompressSpec {
                    algorithm,
                    tiles,
                    quantize
                },
                "{spec:?}"
            );
        }
        assert_eq!(
            parse("; q0").unwrap().quantize,
            rice.level(16.0).dither(Dither::None)
        );
        for bad in [" H", " P", "; qz 4", "; s 2", " R x,y"] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn paths_split_off_only_a_compress_spec() {
        let (path, spec) = split_path(Path::new("dir/a.fits[compress G 16,8]")).unwrap();
        assert_eq!(path, Path::new("dir/a.fits"));
        assert_eq!(spec.unwrap().tiles, vec![16, 8]);
        let (path, spec) = split_path(Path::new("dir/a.fits[1]")).unwrap();
        assert_eq!(path, Path::new("dir/a.fits[1]"));
        assert!(spec.is_none());
        assert!(split_path(Path::new("a.fits[compress H]")).is_err());
    }
}
