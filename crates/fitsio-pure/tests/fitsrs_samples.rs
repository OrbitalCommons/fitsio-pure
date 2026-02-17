//! Validation tests against the fitsrs test corpus.
//!
//! These tests require the fitsrs sample files to be downloaded:
//!   curl -L -o /tmp/fits-rs-test-files.tar \
//!     "https://alasky.cds.unistra.fr/Aladin-Lite-test-files/fits-rs-test-files.tar"
//!   mkdir -p reference/fitsrs-samples
//!   tar xf /tmp/fits-rs-test-files.tar -C reference/fitsrs-samples/

use std::path::{Path, PathBuf};

fn samples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/fitsrs-samples/samples")
}

fn try_parse(path: &Path) -> (String, Result<fitsio_pure::hdu::FitsData, String>) {
    let name = path
        .strip_prefix(samples_dir().parent().unwrap())
        .unwrap_or(path)
        .display()
        .to_string();

    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) => return (name, Err(format!("read error: {e}"))),
    };

    match fitsio_pure::hdu::parse_fits(&data) {
        Ok(fits) => (name, Ok(fits)),
        Err(e) => (name, Err(format!("{e}"))),
    }
}

#[test]
fn validate_fitsrs_corpus() {
    let dir = samples_dir();
    if !dir.exists() {
        eprintln!("Skipping: fitsrs samples not found at {}", dir.display());
        return;
    }

    let mut files: Vec<PathBuf> = Vec::new();
    collect_fits_files(&dir, &mut files);
    files.sort();

    assert!(
        !files.is_empty(),
        "No .fits files found in {}",
        dir.display()
    );

    let mut pass = 0usize;
    let mut fail = 0usize;
    let mut results: Vec<(String, String)> = Vec::new();

    for path in &files {
        let (name, result) = try_parse(path);
        match result {
            Ok(fits) => {
                let hdu_summary: Vec<String> = fits
                    .iter()
                    .map(|hdu| match &hdu.info {
                        fitsio_pure::hdu::HduInfo::Primary { bitpix, naxes } => {
                            format!("Primary(bitpix={bitpix}, naxes={naxes:?})")
                        }
                        fitsio_pure::hdu::HduInfo::Image { bitpix, naxes } => {
                            format!("Image(bitpix={bitpix}, naxes={naxes:?})")
                        }
                        fitsio_pure::hdu::HduInfo::AsciiTable {
                            naxis1,
                            naxis2,
                            tfields,
                        } => {
                            format!("AsciiTable({naxis1}x{naxis2}, {tfields} cols)")
                        }
                        fitsio_pure::hdu::HduInfo::BinaryTable {
                            naxis1,
                            naxis2,
                            pcount,
                            tfields,
                        } => {
                            format!("BinTable({naxis1}x{naxis2}, pcount={pcount}, {tfields} cols)")
                        }
                        fitsio_pure::hdu::HduInfo::RandomGroups {
                            bitpix,
                            naxes,
                            pcount,
                            gcount,
                        } => {
                            format!("RandomGroups(bitpix={bitpix}, naxes={naxes:?}, pcount={pcount}, gcount={gcount})")
                        }
                        fitsio_pure::hdu::HduInfo::CompressedImage {
                            zbitpix,
                            znaxes,
                            zcmptype,
                            ..
                        } => {
                            format!("CompressedImage(zbitpix={zbitpix}, znaxes={znaxes:?}, {zcmptype})")
                        }
                    })
                    .collect();
                results.push((
                    name,
                    format!("PASS  {} HDUs: {}", fits.len(), hdu_summary.join(", ")),
                ));
                pass += 1;
            }
            Err(e) => {
                results.push((name, format!("FAIL  {e}")));
                fail += 1;
            }
        }
    }

    eprintln!("\n=== fitsrs corpus validation ===");
    for (name, status) in &results {
        eprintln!("  {name}: {status}");
    }
    eprintln!(
        "\nTotal: {} files, {} pass, {} fail\n",
        files.len(),
        pass,
        fail
    );

    // Don't assert all pass — this is a diagnostic test.
    // Failures are expected for compressed images, random groups, etc.
}

fn collect_fits_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_fits_files(&path, out);
            } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                // Skip macOS resource forks and .gz files
                if name.starts_with("._") || name.ends_with(".gz") {
                    continue;
                }
                if name.ends_with(".fits") {
                    out.push(path);
                }
            }
        }
    }
}
