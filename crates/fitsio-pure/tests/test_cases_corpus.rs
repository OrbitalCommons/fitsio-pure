//! Integration tests against the OrbitalCommons/fits-test-cases corpus.
//!
//! These tests require the `FITS_TEST_CASES` environment variable to point at
//! a checkout of <https://github.com/OrbitalCommons/fits-test-cases>.
//! When the variable is unset or the directory doesn't exist the tests are
//! silently skipped, so local builds without the corpus still pass.

use std::path::{Path, PathBuf};

use fitsio_pure::hdu::{parse_fits, FitsData, HduInfo};
use fitsio_pure::image::{extract_bscale_bzero, read_image_data, read_image_physical, ImageData};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn corpus_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var("FITS_TEST_CASES").ok()?);
    if dir.is_dir() {
        Some(dir)
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
            if name.ends_with(".fits") || name.ends_with(".fit") {
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
