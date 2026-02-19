//! Integration tests against the OrbitalCommons/fits-test-cases corpus.
//!
//! These tests require the `FITS_TEST_CASES` environment variable to point at
//! a checkout of <https://github.com/OrbitalCommons/fits-test-cases>.
//! When the variable is unset or the directory doesn't exist the tests are
//! silently skipped, so local builds without the corpus still pass.

use std::path::{Path, PathBuf};

use fitsio_pure::bintable::{parse_binary_table_columns, read_binary_column, BinaryColumnData};
use fitsio_pure::hdu::{parse_fits, FitsData, HduInfo};
use fitsio_pure::image::{extract_bscale_bzero, read_image_data, read_image_physical, ImageData};
use fitsio_pure::value::Value;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn find_keyword_str<'a>(cards: &'a [fitsio_pure::header::Card], keyword: &str) -> Option<&'a str> {
    cards.iter().find_map(|c| {
        if c.keyword_str() == keyword {
            match &c.value {
                Some(Value::String(s)) => Some(s.as_str()),
                _ => None,
            }
        } else {
            None
        }
    })
}

fn find_keyword_int(cards: &[fitsio_pure::header::Card], keyword: &str) -> Option<i64> {
    cards.iter().find_map(|c| {
        if c.keyword_str() == keyword {
            match &c.value {
                Some(Value::Integer(n)) => Some(*n),
                _ => None,
            }
        } else {
            None
        }
    })
}

fn corpus_dir() -> Option<PathBuf> {
    if let Ok(val) = std::env::var("FITS_TEST_CASES") {
        let dir = PathBuf::from(val);
        if dir.is_dir() {
            return Some(dir);
        }
    }
    // Fall back to submodule path
    let submod = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fits-test-cases");
    if submod.is_dir() {
        Some(submod)
    } else {
        None
    }
}

fn corpus_file(rel: &str) -> Option<PathBuf> {
    let path = corpus_dir()?.join(rel);
    if path.is_file() {
        Some(path)
    } else {
        panic!("Corpus file missing: {}", path.display());
    }
}

fn load(rel: &str) -> Option<(Vec<u8>, FitsData)> {
    let path = corpus_file(rel)?;
    let bytes =
        std::fs::read(&path).unwrap_or_else(|e| panic!("Failed to read {}: {e}", path.display()));
    let fits =
        parse_fits(&bytes).unwrap_or_else(|e| panic!("Failed to parse {}: {e}", path.display()));
    Some((bytes, fits))
}

fn collect_fits_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().map(|n| n == ".git").unwrap_or(false) {
                continue;
            }
            collect_fits_files(&path, &mut *out);
        } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.ends_with(".fits")
                || name.ends_with(".fit")
                || name.ends_with(".metafits")
                || name.ends_with(".uvfits")
            {
                out.push(path);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Parse every file in the corpus
// ---------------------------------------------------------------------------

#[test]
fn all_files_parse() {
    let dir = match corpus_dir() {
        Some(d) => d,
        None => {
            eprintln!("Skipping: FITS_TEST_CASES not set");
            return;
        }
    };

    let mut files = Vec::new();
    collect_fits_files(&dir, &mut files);
    files.sort();

    assert!(
        !files.is_empty(),
        "No FITS files found in {}",
        dir.display()
    );

    let mut failures = Vec::new();

    for path in &files {
        let data = match std::fs::read(path) {
            Ok(d) => d,
            Err(e) => {
                failures.push(format!("{}: read error: {e}", path.display()));
                continue;
            }
        };
        if let Err(e) = parse_fits(&data) {
            failures.push(format!("{}: {e}", path.display()));
        }
    }

    assert!(
        failures.is_empty(),
        "Failed to parse {} of {} files:\n  {}",
        failures.len(),
        files.len(),
        failures.join("\n  ")
    );
}

// ---------------------------------------------------------------------------
// Random Groups
// ---------------------------------------------------------------------------

#[test]
fn random_groups_metadata() {
    let (_, fits) = match load("nasa-samples/Random_Groups.fits") {
        Some(v) => v,
        None => return,
    };

    assert_eq!(fits.len(), 2); // Random groups primary + AIPS AN binary table
    match &fits.primary().info {
        HduInfo::RandomGroups {
            bitpix,
            naxes,
            pcount,
            gcount,
        } => {
            assert_eq!(*bitpix, -32);
            assert_eq!(naxes, &[3, 4, 1, 1, 1]);
            assert_eq!(*pcount, 6);
            assert_eq!(*gcount, 7956);
        }
        other => panic!("Expected RandomGroups, got {:?}", other),
    }
    assert_eq!(fits.primary().data_len, 572832);
}

// ---------------------------------------------------------------------------
// Multi-extension images (HST NICMOS)
// ---------------------------------------------------------------------------

#[test]
fn hst_nicmos_multi_extension() {
    let (_, fits) = match load("nasa-samples/HST_NICMOS.fits") {
        Some(v) => v,
        None => return,
    };

    // Empty primary + 5 image extensions: SCI, ERR, DQ, SAMP, TIME
    assert_eq!(fits.len(), 6);

    // Primary is empty
    match &fits.primary().info {
        HduInfo::Primary { naxes, .. } => assert!(naxes.is_empty()),
        other => panic!("Expected Primary, got {:?}", other),
    }

    // SCI extension: BITPIX=-32, 270x263
    let sci = fits.find_by_name("SCI").unwrap();
    match &sci.info {
        HduInfo::Image { bitpix, naxes } => {
            assert_eq!(*bitpix, -32);
            assert_eq!(naxes, &[270, 263]);
        }
        other => panic!("Expected Image for SCI, got {:?}", other),
    }

    // DQ extension: BITPIX=16
    let dq = fits.find_by_name("DQ").unwrap();
    match &dq.info {
        HduInfo::Image { bitpix, .. } => assert_eq!(*bitpix, 16),
        other => panic!("Expected Image for DQ, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// ASCII table (HST FOS)
// ---------------------------------------------------------------------------

#[test]
fn hst_fos_ascii_table() {
    let (_, fits) = match load("nasa-samples/HST_FOS.fits") {
        Some(v) => v,
        None => return,
    };

    assert_eq!(fits.len(), 2);

    // Primary: image data
    match &fits.primary().info {
        HduInfo::Primary { bitpix, naxes } => {
            assert_eq!(*bitpix, -32);
            assert_eq!(naxes, &[2064, 2]);
        }
        other => panic!("Expected Primary, got {:?}", other),
    }

    // Extension: ASCII table with 19 columns, 2 rows
    match &fits.get(1).unwrap().info {
        HduInfo::AsciiTable {
            tfields, naxis2, ..
        } => {
            assert_eq!(*tfields, 19);
            assert_eq!(*naxis2, 2);
        }
        other => panic!("Expected AsciiTable, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Binary tables (EUVE, vizier, astrometry, hipsgen)
// ---------------------------------------------------------------------------

#[test]
fn euve_binary_tables() {
    let (_, fits) = match load("nasa-samples/EUVE.fits") {
        Some(v) => v,
        None => return,
    };

    // 9 HDUs: empty primary, 4 images, 4 binary tables
    assert_eq!(fits.len(), 9);

    let bintable_count = fits
        .iter()
        .filter(|h| matches!(&h.info, HduInfo::BinaryTable { .. }))
        .count();
    assert_eq!(bintable_count, 4);

    // ds_limits table
    let ds_limits = fits.find_by_name("ds_limits").unwrap();
    match &ds_limits.info {
        HduInfo::BinaryTable {
            tfields, naxis2, ..
        } => {
            assert_eq!(*tfields, 3);
            assert_eq!(*naxis2, 3);
        }
        other => panic!("Expected BinaryTable, got {:?}", other),
    }
}

#[test]
fn vizier_binary_table() {
    let (_, fits) = match load("vizier/II_278_transit.fits") {
        Some(v) => v,
        None => return,
    };

    assert_eq!(fits.len(), 2);
    match &fits.get(1).unwrap().info {
        HduInfo::BinaryTable {
            tfields, naxis2, ..
        } => {
            assert_eq!(*tfields, 11);
            assert_eq!(*naxis2, 177);
        }
        other => panic!("Expected BinaryTable, got {:?}", other),
    }
}

#[test]
fn astrometry_corr_table() {
    let (_, fits) = match load("astrometry/corr.fits") {
        Some(v) => v,
        None => return,
    };

    assert_eq!(fits.len(), 2);
    match &fits.get(1).unwrap().info {
        HduInfo::BinaryTable {
            tfields, naxis2, ..
        } => {
            assert_eq!(*tfields, 28);
            assert_eq!(*naxis2, 52);
        }
        other => panic!("Expected BinaryTable, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// 3D and 4D+ images
// ---------------------------------------------------------------------------

#[test]
fn cube_3d_image() {
    let (_, fits) = match load("rust-fitsio/cube.fits") {
        Some(v) => v,
        None => return,
    };

    assert_eq!(fits.len(), 1);
    match &fits.primary().info {
        HduInfo::Primary { bitpix, naxes } => {
            assert_eq!(*bitpix, 64);
            assert_eq!(naxes.len(), 3);
            assert_eq!(naxes, &[6, 3, 2]);
        }
        other => panic!("Expected Primary, got {:?}", other),
    }
}

#[test]
fn hyper_4d_image() {
    let (_, fits) = match load("rust-fitsio/hyper.fits") {
        Some(v) => v,
        None => return,
    };

    assert_eq!(fits.len(), 1);
    match &fits.primary().info {
        HduInfo::Primary { bitpix, naxes } => {
            assert_eq!(*bitpix, 64);
            assert_eq!(naxes.len(), 4);
            assert_eq!(naxes, &[2, 3, 3, 2]);
        }
        other => panic!("Expected Primary, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Boolean columns
// ---------------------------------------------------------------------------

#[test]
fn boolean_columns_table() {
    let (_, fits) = match load("rust-fitsio/boolean_columns.fits") {
        Some(v) => v,
        None => return,
    };

    assert_eq!(fits.len(), 2);
    match &fits.get(1).unwrap().info {
        HduInfo::BinaryTable {
            tfields, naxis2, ..
        } => {
            assert_eq!(*tfields, 21);
            assert_eq!(*naxis2, 256);
        }
        other => panic!("Expected BinaryTable, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Multi-extension with image + binary table (full_example)
// ---------------------------------------------------------------------------

#[test]
fn full_example_mixed_extensions() {
    let (bytes, fits) = match load("rust-fitsio/full_example.fits") {
        Some(v) => v,
        None => return,
    };

    assert_eq!(fits.len(), 2);

    // Primary: 100x100 i32 image
    match &fits.primary().info {
        HduInfo::Primary { bitpix, naxes } => {
            assert_eq!(*bitpix, 32);
            assert_eq!(naxes, &[100, 100]);
        }
        other => panic!("Expected Primary, got {:?}", other),
    }

    // Read image data to verify pixel count
    let img = read_image_data(&bytes, fits.primary()).unwrap();
    match img {
        ImageData::I32(v) => assert_eq!(v.len(), 10000),
        other => panic!("Expected I32, got {:?}", other),
    }

    // TESTEXT binary table: 4 columns, 50 rows
    let ext = fits.find_by_name("TESTEXT").unwrap();
    match &ext.info {
        HduInfo::BinaryTable {
            tfields, naxis2, ..
        } => {
            assert_eq!(*tfields, 4);
            assert_eq!(*naxis2, 50);
        }
        other => panic!("Expected BinaryTable, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Unsigned 16-bit via BZERO
// ---------------------------------------------------------------------------

#[test]
fn ushort_bzero_pattern() {
    let (bytes, fits) = match load("rust-fitsio/ushort.fits") {
        Some(v) => v,
        None => return,
    };

    assert_eq!(fits.len(), 1);
    match &fits.primary().info {
        HduInfo::Primary { bitpix, naxes } => {
            assert_eq!(*bitpix, 16);
            assert_eq!(naxes, &[1024, 1024]);
        }
        other => panic!("Expected Primary, got {:?}", other),
    }

    let (bscale, bzero) = extract_bscale_bzero(&fits.primary().cards);
    assert_eq!(bscale, 1.0);
    assert_eq!(bzero, 32768.0);

    // Physical values should all be non-negative (unsigned interpretation)
    let physical = read_image_physical(&bytes, fits.primary()).unwrap();
    assert_eq!(physical.len(), 1024 * 1024);
    assert!(
        physical.iter().all(|&v| v >= 0.0),
        "Expected all physical values >= 0 for unsigned 16-bit"
    );
}

// ---------------------------------------------------------------------------
// cfitsio iter_image: image data readable
// ---------------------------------------------------------------------------

#[test]
fn cfitsio_iter_image() {
    let (bytes, fits) = match load("cfitsio/iter_image.fit") {
        Some(v) => v,
        None => return,
    };

    assert_eq!(fits.len(), 1);
    match &fits.primary().info {
        HduInfo::Primary { bitpix, naxes } => {
            assert_eq!(*bitpix, 32);
            assert_eq!(naxes, &[113, 91]);
        }
        other => panic!("Expected Primary, got {:?}", other),
    }

    let img = read_image_data(&bytes, fits.primary()).unwrap();
    match img {
        ImageData::I32(v) => assert_eq!(v.len(), 113 * 91),
        other => panic!("Expected I32, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// meter-sim: mixed data types
// ---------------------------------------------------------------------------

#[test]
fn meter_sim_mixed_types() {
    let (_, fits) = match load("meter-sim/mixed_types_u8_i32_f32.fits") {
        Some(v) => v,
        None => return,
    };

    assert_eq!(fits.len(), 4); // empty primary + 3 image extensions

    let uint8 = fits.find_by_name("UINT8").unwrap();
    match &uint8.info {
        HduInfo::Image { bitpix, naxes } => {
            assert_eq!(*bitpix, 8);
            assert_eq!(naxes, &[2, 2]);
        }
        other => panic!("Expected Image for UINT8, got {:?}", other),
    }

    let float32 = fits.find_by_name("FLOAT32").unwrap();
    match &float32.info {
        HduInfo::Image { bitpix, naxes } => {
            assert_eq!(*bitpix, -32);
            assert_eq!(naxes, &[4, 4]);
        }
        other => panic!("Expected Image for FLOAT32, got {:?}", other),
    }

    let int32 = fits.find_by_name("INT32").unwrap();
    match &int32.info {
        HduInfo::Image { bitpix, naxes } => {
            assert_eq!(*bitpix, 32);
            assert_eq!(naxes, &[3, 3]);
        }
        other => panic!("Expected Image for INT32, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// meter-sim: gradient orientations with EXTNAME lookup
// ---------------------------------------------------------------------------

#[test]
fn meter_sim_gradient_extnames() {
    let (_, fits) = match load("meter-sim/gradients_all_orientations_labeled.fits") {
        Some(v) => v,
        None => return,
    };

    assert_eq!(fits.len(), 5); // empty primary + 4 gradient extensions

    for name in &[
        "VERT_BRIGHT_BOT_DARK_TOP",
        "HORIZ_BRIGHT_LEFT_DARK_RIGHT",
        "VERT_DARK_BOT_BRIGHT_TOP",
        "HORIZ_DARK_LEFT_BRIGHT_RIGHT",
    ] {
        let hdu = fits
            .find_by_name(name)
            .unwrap_or_else(|| panic!("Missing EXTNAME: {name}"));
        match &hdu.info {
            HduInfo::Image { bitpix, .. } => assert_eq!(*bitpix, -32),
            other => panic!("Expected Image for {name}, got {:?}", other),
        }
    }
}

// ---------------------------------------------------------------------------
// meter-sim: f64 round-trip with value verification
// ---------------------------------------------------------------------------

#[test]
fn meter_sim_roundtrip_f64_values() {
    let (bytes, fits) = match load("meter-sim/roundtrip_f64_multi_hdu.fits") {
        Some(v) => v,
        None => return,
    };

    assert_eq!(fits.len(), 3);

    let image1 = fits.find_by_name("IMAGE1").unwrap();
    match &image1.info {
        HduInfo::Image { bitpix, naxes } => {
            assert_eq!(*bitpix, -64);
            assert_eq!(naxes, &[4, 3]);
        }
        other => panic!("Expected Image, got {:?}", other),
    }

    // Read pixel data — all pixels in IMAGE1 should be 1.5
    let img = read_image_data(&bytes, image1).unwrap();
    match img {
        ImageData::F64(v) => {
            assert_eq!(v.len(), 12);
            for (i, &val) in v.iter().enumerate() {
                assert!(
                    (val - 1.5).abs() < 1e-10,
                    "IMAGE1 pixel {i}: expected 1.5, got {val}"
                );
            }
        }
        other => panic!("Expected F64, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// meter-sim: extreme aspect ratios with near-degenerate dimensions
// ---------------------------------------------------------------------------

#[test]
fn meter_sim_extreme_aspect_ratios() {
    let (_, fits) = match load("meter-sim/extreme_ratios_panorama_column_1d.fits") {
        Some(v) => v,
        None => return,
    };

    assert_eq!(fits.len(), 5);

    // ROW: 500 x 1 (single row)
    let row = fits.find_by_name("ROW").unwrap();
    match &row.info {
        HduInfo::Image { naxes, .. } => assert_eq!(naxes, &[500, 1]),
        other => panic!("Expected Image for ROW, got {:?}", other),
    }

    // COL: 1 x 500 (single column)
    let col = fits.find_by_name("COL").unwrap();
    match &col.info {
        HduInfo::Image { naxes, .. } => assert_eq!(naxes, &[1, 500]),
        other => panic!("Expected Image for COL, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Tile-compressed images
// ---------------------------------------------------------------------------

#[test]
fn compressed_rice_m13() {
    let (bytes, fits) = match load("nasa-samples/m13_rice.fits") {
        Some(v) => v,
        None => return,
    };

    assert_eq!(fits.len(), 2);
    match &fits.get(1).unwrap().info {
        HduInfo::CompressedImage {
            zbitpix,
            znaxes,
            zcmptype,
            ..
        } => {
            assert_eq!(*zbitpix, 16);
            assert_eq!(znaxes, &[300, 300]);
            assert!(zcmptype.contains("RICE"), "Expected RICE, got {zcmptype}");
        }
        other => panic!("Expected CompressedImage, got {:?}", other),
    }

    let hdu = fits.get(1).unwrap();
    let img = read_image_data(&bytes, hdu).unwrap();
    match img {
        ImageData::I16(v) => assert_eq!(v.len(), 90000),
        other => panic!("Expected I16, got {:?}", other),
    }
}

#[test]
fn compressed_gzip_m13() {
    let (bytes, fits) = match load("nasa-samples/m13_gzip.fits") {
        Some(v) => v,
        None => return,
    };

    assert_eq!(fits.len(), 2);
    match &fits.get(1).unwrap().info {
        HduInfo::CompressedImage {
            zbitpix,
            znaxes,
            zcmptype,
            ..
        } => {
            assert_eq!(*zbitpix, 16);
            assert_eq!(znaxes, &[300, 300]);
            assert!(zcmptype.contains("GZIP"), "Expected GZIP, got {zcmptype}");
        }
        other => panic!("Expected CompressedImage, got {:?}", other),
    }

    let hdu = fits.get(1).unwrap();
    let img = read_image_data(&bytes, hdu).unwrap();
    match img {
        ImageData::I16(v) => assert_eq!(v.len(), 90000),
        other => panic!("Expected I16, got {:?}", other),
    }
}

#[test]
fn compressed_rice_vs_gzip_identical_pixels() {
    let (rice_bytes, rice_fits) = match load("nasa-samples/m13_rice.fits") {
        Some(v) => v,
        None => return,
    };
    let (gzip_bytes, gzip_fits) = match load("nasa-samples/m13_gzip.fits") {
        Some(v) => v,
        None => return,
    };

    let rice_img = read_image_data(&rice_bytes, rice_fits.get(1).unwrap()).unwrap();
    let gzip_img = read_image_data(&gzip_bytes, gzip_fits.get(1).unwrap()).unwrap();

    match (&rice_img, &gzip_img) {
        (ImageData::I16(r), ImageData::I16(g)) => {
            assert_eq!(r.len(), g.len());
            assert_eq!(r, g, "Rice and GZIP should produce identical pixels");
        }
        _ => panic!("Expected I16 from both"),
    }
}

#[test]
fn compressed_rice_comp_fits() {
    let (bytes, fits) = match load("fitsio-pure-fixtures/comp.fits") {
        Some(v) => v,
        None => return,
    };

    assert_eq!(fits.len(), 2);
    match &fits.get(1).unwrap().info {
        HduInfo::CompressedImage {
            zbitpix, znaxes, ..
        } => {
            assert_eq!(*zbitpix, 16);
            assert_eq!(znaxes, &[440, 300]);
        }
        other => panic!("Expected CompressedImage, got {:?}", other),
    }

    let hdu = fits.get(1).unwrap();
    let img = read_image_data(&bytes, hdu).unwrap();
    match img {
        ImageData::I16(v) => assert_eq!(v.len(), 132000),
        other => panic!("Expected I16, got {:?}", other),
    }
}

#[test]
fn compressed_rice_float_quantized() {
    let (bytes, fits) = match load("nasa-samples/m13real_rice.fits") {
        Some(v) => v,
        None => return,
    };

    assert_eq!(fits.len(), 2);
    match &fits.get(1).unwrap().info {
        HduInfo::CompressedImage {
            zbitpix, znaxes, ..
        } => {
            assert_eq!(*zbitpix, -32);
            assert_eq!(znaxes, &[300, 300]);
        }
        other => panic!("Expected CompressedImage, got {:?}", other),
    }

    let hdu = fits.get(1).unwrap();
    let img = read_image_data(&bytes, hdu).unwrap();
    match img {
        ImageData::F32(v) => {
            assert_eq!(v.len(), 90000);
            assert!(
                v.iter().all(|x| x.is_finite()),
                "All float pixels should be finite"
            );
        }
        other => panic!("Expected F32, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// HEALPix tile (hipsgen)
// ---------------------------------------------------------------------------

#[test]
fn hipsgen_healpix_tile() {
    let (_, fits) = match load("hipsgen/Npix8.fits") {
        Some(v) => v,
        None => return,
    };

    assert_eq!(fits.len(), 1);
    match &fits.primary().info {
        HduInfo::Primary { bitpix, naxes } => {
            assert_eq!(*bitpix, 16);
            assert_eq!(naxes, &[512, 512]);
        }
        other => panic!("Expected Primary, got {:?}", other),
    }
}

// ===========================================================================
// MWA telescope test data
// ===========================================================================

// ---------------------------------------------------------------------------
// MWA metafits: minimal MWAX (1297526432)
// ---------------------------------------------------------------------------

#[test]
fn mwa_metafits_minimal_structure() {
    let (_, fits) = match load("mwa/1297526432.metafits") {
        Some(v) => v,
        None => return,
    };

    assert_eq!(fits.len(), 2);

    // Primary: empty with metadata
    match &fits.primary().info {
        HduInfo::Primary { naxes, .. } => assert!(naxes.is_empty()),
        other => panic!("Expected Primary, got {:?}", other),
    }

    // Check key header values
    assert_eq!(
        find_keyword_int(&fits.primary().cards, "GPSTIME"),
        Some(1297526432)
    );
    assert_eq!(find_keyword_int(&fits.primary().cards, "EXPOSURE"), Some(2));
    assert_eq!(
        find_keyword_str(&fits.primary().cards, "TELESCOP"),
        Some("MWA")
    );
    assert_eq!(find_keyword_int(&fits.primary().cards, "NINPUTS"), Some(4));
    assert_eq!(find_keyword_int(&fits.primary().cards, "NCHANS"), Some(48));

    // TILEDATA table: 17 columns, 4 rows
    let tiledata = fits.find_by_name("TILEDATA").unwrap();
    match &tiledata.info {
        HduInfo::BinaryTable {
            tfields, naxis2, ..
        } => {
            assert_eq!(*tfields, 17);
            assert_eq!(*naxis2, 4);
        }
        other => panic!("Expected BinaryTable, got {:?}", other),
    }
}

#[test]
fn mwa_metafits_minimal_column_data() {
    let (bytes, fits) = match load("mwa/1297526432.metafits") {
        Some(v) => v,
        None => return,
    };

    let tiledata = fits.find_by_name("TILEDATA").unwrap();
    let tfields = match &tiledata.info {
        HduInfo::BinaryTable { tfields, .. } => *tfields,
        _ => panic!("Expected BinaryTable"),
    };
    let cols = parse_binary_table_columns(&tiledata.cards, tfields).unwrap();

    // Find TileName column by name
    let tilename_idx = cols
        .iter()
        .position(|c| c.name.as_deref() == Some("TileName"))
        .unwrap();
    let tilename_data = read_binary_column(&bytes, tiledata, tilename_idx).unwrap();
    match &tilename_data {
        BinaryColumnData::Ascii(v) => {
            assert_eq!(v.len(), 4);
            assert_eq!(
                v[0].trim_matches(|c: char| c == ' ' || c == '\0'),
                "Tile052"
            );
        }
        other => panic!("Expected Ascii for TileName, got {:?}", other),
    }

    // Find Pol column by name
    let pol_idx = cols
        .iter()
        .position(|c| c.name.as_deref() == Some("Pol"))
        .unwrap();
    let pol_data = read_binary_column(&bytes, tiledata, pol_idx).unwrap();
    match &pol_data {
        BinaryColumnData::Ascii(v) => {
            assert_eq!(v.len(), 4);
            assert_eq!(v[0].trim_matches(|c: char| c == ' ' || c == '\0'), "Y");
        }
        other => panic!("Expected Ascii for Pol, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// MWA metafits: full 128-tile with CONTINUE long strings (1244973688)
// ---------------------------------------------------------------------------

#[test]
fn mwa_metafits_full_structure() {
    let (_, fits) = match load("mwa/1244973688.metafits") {
        Some(v) => v,
        None => return,
    };

    assert_eq!(fits.len(), 2);

    assert_eq!(
        find_keyword_int(&fits.primary().cards, "GPSTIME"),
        Some(1244973688)
    );
    assert_eq!(
        find_keyword_int(&fits.primary().cards, "EXPOSURE"),
        Some(120)
    );
    assert_eq!(
        find_keyword_str(&fits.primary().cards, "TELESCOP"),
        Some("MWA")
    );
    assert_eq!(
        find_keyword_int(&fits.primary().cards, "NINPUTS"),
        Some(256)
    );
    assert_eq!(
        find_keyword_int(&fits.primary().cards, "NCHANS"),
        Some(3072)
    );

    // TILEDATA: 20 columns, 256 rows
    let tiledata = fits.find_by_name("TILEDATA").unwrap();
    match &tiledata.info {
        HduInfo::BinaryTable {
            tfields, naxis2, ..
        } => {
            assert_eq!(*tfields, 20);
            assert_eq!(*naxis2, 256);
        }
        other => panic!("Expected BinaryTable, got {:?}", other),
    }
}

#[test]
fn mwa_metafits_continue_long_string() {
    let (_, fits) = match load("mwa/1244973688.metafits") {
        Some(v) => v,
        None => return,
    };

    // CHANNELS should be a long string assembled from CONTINUE cards
    let channels = find_keyword_str(&fits.primary().cards, "CHANNELS").unwrap();
    assert_eq!(
        channels,
        "104,105,106,107,108,109,110,111,112,113,114,115,116,117,118,119,120,121,122,123,124,125,126,127"
    );
    // 24 channels listed
    assert_eq!(channels.split(',').count(), 24);
}

#[test]
fn mwa_metafits_full_column_data() {
    let (bytes, fits) = match load("mwa/1244973688.metafits") {
        Some(v) => v,
        None => return,
    };

    let tiledata = fits.find_by_name("TILEDATA").unwrap();
    let tfields = match &tiledata.info {
        HduInfo::BinaryTable { tfields, .. } => *tfields,
        _ => panic!("Expected BinaryTable"),
    };
    let cols = parse_binary_table_columns(&tiledata.cards, tfields).unwrap();

    // TileName first row
    let tilename_idx = cols
        .iter()
        .position(|c| c.name.as_deref() == Some("TileName"))
        .unwrap();
    let tilename_data = read_binary_column(&bytes, tiledata, tilename_idx).unwrap();
    match &tilename_data {
        BinaryColumnData::Ascii(v) => {
            assert_eq!(v.len(), 256);
            assert_eq!(
                v[0].trim_matches(|c: char| c == ' ' || c == '\0'),
                "Tile104"
            );
        }
        other => panic!("Expected Ascii for TileName, got {:?}", other),
    }

    // Gains column: 24I per row (array-in-cell)
    let gains_idx = cols
        .iter()
        .position(|c| c.name.as_deref() == Some("Gains"))
        .unwrap();
    assert_eq!(cols[gains_idx].repeat, 24);
    let gains_data = read_binary_column(&bytes, tiledata, gains_idx).unwrap();
    match &gains_data {
        BinaryColumnData::Short(v) => {
            // 256 rows x 24 elements = 6144 total values
            assert_eq!(v.len(), 256 * 24);
        }
        other => panic!("Expected Short for Gains, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// MWA metafits: signal chain correction (1096952256)
// ---------------------------------------------------------------------------

#[test]
fn mwa_metafits_signal_chain() {
    let (_, fits) = match load("mwa/1096952256_metafits.fits") {
        Some(v) => v,
        None => return,
    };

    assert_eq!(fits.len(), 3);
    assert_eq!(
        find_keyword_int(&fits.primary().cards, "GPSTIME"),
        Some(1096952256)
    );

    // TILEDATA: 21 columns, 256 rows
    let tiledata = fits.find_by_name("TILEDATA").unwrap();
    match &tiledata.info {
        HduInfo::BinaryTable {
            tfields, naxis2, ..
        } => {
            assert_eq!(*tfields, 21);
            assert_eq!(*naxis2, 256);
        }
        other => panic!("Expected BinaryTable for TILEDATA, got {:?}", other),
    }

    // SIGCHAINDATA: 3 columns, 8 rows
    let sigchain = fits.find_by_name("SIGCHAINDATA").unwrap();
    match &sigchain.info {
        HduInfo::BinaryTable {
            tfields, naxis2, ..
        } => {
            assert_eq!(*tfields, 3);
            assert_eq!(*naxis2, 8);
        }
        other => panic!("Expected BinaryTable for SIGCHAINDATA, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// MWA metafits: calibration data (1111842752)
// ---------------------------------------------------------------------------

#[test]
fn mwa_metafits_calibration_data() {
    let (_, fits) = match load("mwa/1111842752_metafits.fits") {
        Some(v) => v,
        None => return,
    };

    assert_eq!(fits.len(), 3);
    assert_eq!(
        find_keyword_int(&fits.primary().cards, "GPSTIME"),
        Some(1111842752)
    );

    // TILEDATA: 21 columns, 256 rows
    let tiledata = fits.find_by_name("TILEDATA").unwrap();
    match &tiledata.info {
        HduInfo::BinaryTable {
            tfields, naxis2, ..
        } => {
            assert_eq!(*tfields, 21);
            assert_eq!(*naxis2, 256);
        }
        other => panic!("Expected BinaryTable for TILEDATA, got {:?}", other),
    }

    // CALIBDATA: 6 columns, 256 rows
    let calibdata = fits.find_by_name("CALIBDATA").unwrap();
    match &calibdata.info {
        HduInfo::BinaryTable {
            tfields, naxis2, ..
        } => {
            assert_eq!(*tfields, 6);
            assert_eq!(*naxis2, 256);
        }
        other => panic!("Expected BinaryTable for CALIBDATA, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// MWA GLEAM sky model catalog
// ---------------------------------------------------------------------------

#[test]
fn mwa_gleam_catalog_structure() {
    let (_, fits) = match load("mwa/gleam.fits") {
        Some(v) => v,
        None => return,
    };

    assert_eq!(fits.len(), 2);

    let table = fits.get(1).unwrap();
    match &table.info {
        HduInfo::BinaryTable {
            tfields, naxis2, ..
        } => {
            assert_eq!(*tfields, 9);
            assert_eq!(*naxis2, 4);
        }
        other => panic!("Expected BinaryTable, got {:?}", other),
    }

    // Verify column names match expected sky model schema
    let cols = parse_binary_table_columns(&table.cards, 9).unwrap();
    let names: Vec<&str> = cols.iter().filter_map(|c| c.name.as_deref()).collect();
    assert_eq!(
        names,
        &["Name", "RAJ2000", "DEJ2000", "S_200", "alpha", "beta", "a", "b", "pa"]
    );
}

#[test]
fn mwa_gleam_catalog_values() {
    let (bytes, fits) = match load("mwa/gleam.fits") {
        Some(v) => v,
        None => return,
    };

    let table = fits.get(1).unwrap();
    let tfields = match &table.info {
        HduInfo::BinaryTable { tfields, .. } => *tfields,
        _ => panic!("Expected BinaryTable"),
    };
    let cols = parse_binary_table_columns(&table.cards, tfields).unwrap();

    // Name column: first row should be "point-pl"
    let name_idx = cols
        .iter()
        .position(|c| c.name.as_deref() == Some("Name"))
        .unwrap();
    let name_data = read_binary_column(&bytes, table, name_idx).unwrap();
    match &name_data {
        BinaryColumnData::Ascii(v) => {
            assert_eq!(v.len(), 4);
            assert_eq!(
                v[0].trim_matches(|c: char| c == ' ' || c == '\0'),
                "point-pl"
            );
        }
        other => panic!("Expected Ascii for Name, got {:?}", other),
    }

    // RAJ2000 first row = 1.0
    let ra_idx = cols
        .iter()
        .position(|c| c.name.as_deref() == Some("RAJ2000"))
        .unwrap();
    let ra_data = read_binary_column(&bytes, table, ra_idx).unwrap();
    match &ra_data {
        BinaryColumnData::Double(v) => {
            assert_eq!(v.len(), 4);
            assert!(
                (v[0] - 1.0).abs() < 1e-10,
                "RAJ2000[0] = {}, expected 1.0",
                v[0]
            );
        }
        other => panic!("Expected Double for RAJ2000, got {:?}", other),
    }

    // DEJ2000 first row = 2.0
    let dec_idx = cols
        .iter()
        .position(|c| c.name.as_deref() == Some("DEJ2000"))
        .unwrap();
    let dec_data = read_binary_column(&bytes, table, dec_idx).unwrap();
    match &dec_data {
        BinaryColumnData::Double(v) => {
            assert_eq!(v.len(), 4);
            assert!(
                (v[0] - 2.0).abs() < 1e-10,
                "DEJ2000[0] = {}, expected 2.0",
                v[0]
            );
        }
        other => panic!("Expected Double for DEJ2000, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// MWA Hyperdrive jack.fits: multi-table source model
// ---------------------------------------------------------------------------

#[test]
fn mwa_jack_multi_table() {
    let (_, fits) = match load("mwa/jack.fits") {
        Some(v) => v,
        None => return,
    };

    assert_eq!(fits.len(), 3);

    // Extension 1: component table, 17 columns, 8 rows
    match &fits.get(1).unwrap().info {
        HduInfo::BinaryTable {
            tfields, naxis2, ..
        } => {
            assert_eq!(*tfields, 17);
            assert_eq!(*naxis2, 8);
        }
        other => panic!("Expected BinaryTable for ext 1, got {:?}", other),
    }

    // Extension 2: source mapping table, 4 columns, 4 rows
    match &fits.get(2).unwrap().info {
        HduInfo::BinaryTable {
            tfields, naxis2, ..
        } => {
            assert_eq!(*tfields, 4);
            assert_eq!(*naxis2, 4);
        }
        other => panic!("Expected BinaryTable for ext 2, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// MWA SDC3 UVFITS: random groups format
// ---------------------------------------------------------------------------

#[test]
fn mwa_sdc3_uvfits_random_groups() {
    let (_, fits) = match load("mwa/sdc3_0000.uvfits") {
        Some(v) => v,
        None => return,
    };

    assert_eq!(fits.len(), 3);

    // Primary: random groups with 9 parameters, 1 group
    match &fits.primary().info {
        HduInfo::RandomGroups {
            bitpix,
            naxes,
            pcount,
            gcount,
        } => {
            assert_eq!(*bitpix, -32);
            assert_eq!(naxes, &[3, 1, 1, 1, 1, 1]);
            assert_eq!(*pcount, 9);
            assert_eq!(*gcount, 1);
        }
        other => panic!("Expected RandomGroups, got {:?}", other),
    }

    // AIPS FQ table: 5 columns, 1 row
    let fq = fits.find_by_name("AIPS FQ").unwrap();
    match &fq.info {
        HduInfo::BinaryTable {
            tfields, naxis2, ..
        } => {
            assert_eq!(*tfields, 5);
            assert_eq!(*naxis2, 1);
        }
        other => panic!("Expected BinaryTable for AIPS FQ, got {:?}", other),
    }

    // AIPS AN table: 13 columns, 512 rows
    let an = fits.find_by_name("AIPS AN").unwrap();
    match &an.info {
        HduInfo::BinaryTable {
            tfields, naxis2, ..
        } => {
            assert_eq!(*tfields, 13);
            assert_eq!(*naxis2, 512);
        }
        other => panic!("Expected BinaryTable for AIPS AN, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// MWA MWAX gpubox: alternating visibility + weight images
// ---------------------------------------------------------------------------

#[test]
fn mwa_gpubox_image_structure() {
    let (_, fits) = match load("mwa/1297526432_gpubox_ch117.fits") {
        Some(v) => v,
        None => return,
    };

    // Empty primary + 4 image extensions (2 timesteps x 2 HDUs each)
    assert_eq!(fits.len(), 5);

    match &fits.primary().info {
        HduInfo::Primary { naxes, .. } => assert!(naxes.is_empty()),
        other => panic!("Expected Primary, got {:?}", other),
    }

    // HDU 1: visibility data (BITPIX=32, 16x3)
    match &fits.get(1).unwrap().info {
        HduInfo::Image { bitpix, naxes } => {
            assert_eq!(*bitpix, 32);
            assert_eq!(naxes, &[16, 3]);
        }
        other => panic!("Expected Image for HDU 1, got {:?}", other),
    }

    // HDU 2: weights (BITPIX=-32, 4x3)
    match &fits.get(2).unwrap().info {
        HduInfo::Image { bitpix, naxes } => {
            assert_eq!(*bitpix, -32);
            assert_eq!(naxes, &[4, 3]);
        }
        other => panic!("Expected Image for HDU 2, got {:?}", other),
    }

    // HDU 3: same pattern repeats for timestep 2
    match &fits.get(3).unwrap().info {
        HduInfo::Image { bitpix, naxes } => {
            assert_eq!(*bitpix, 32);
            assert_eq!(naxes, &[16, 3]);
        }
        other => panic!("Expected Image for HDU 3, got {:?}", other),
    }

    // HDU 4: weights for timestep 2
    match &fits.get(4).unwrap().info {
        HduInfo::Image { bitpix, naxes } => {
            assert_eq!(*bitpix, -32);
            assert_eq!(naxes, &[4, 3]);
        }
        other => panic!("Expected Image for HDU 4, got {:?}", other),
    }
}

#[test]
fn mwa_gpubox_pixel_data() {
    let (bytes, fits) = match load("mwa/1297526432_gpubox_ch117.fits") {
        Some(v) => v,
        None => return,
    };

    // Read visibility data from HDU 1 (32-bit int, 16x3 = 48 pixels)
    let vis = read_image_data(&bytes, fits.get(1).unwrap()).unwrap();
    match vis {
        ImageData::I32(v) => assert_eq!(v.len(), 48),
        other => panic!("Expected I32 for visibility, got {:?}", other),
    }

    // Read weight data from HDU 2 (32-bit float, 4x3 = 12 pixels)
    let wgt = read_image_data(&bytes, fits.get(2).unwrap()).unwrap();
    match wgt {
        ImageData::F32(v) => {
            assert_eq!(v.len(), 12);
            assert!(
                v.iter().all(|x| x.is_finite()),
                "All weights should be finite"
            );
        }
        other => panic!("Expected F32 for weights, got {:?}", other),
    }
}
