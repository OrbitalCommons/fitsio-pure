//! `tiled::uncompressed_header` against cfitsio's `fits_img_decompress_header`
//! for every tile-compressed image in fits-test-cases.

use std::ffi::CString;
use std::path::{Path, PathBuf};

use fitsio_pure::hdu::{parse_fits, HduInfo};
use fitsio_pure::header::{parse_header_blocks, Card};
use fitsio_pure::tiled::uncompressed_header;

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fits-test-cases")
}

fn fits_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            fits_files(&path, out);
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("fits" | "fit" | "fz")
        ) {
            out.push(path);
        }
    }
}

/// The header cfitsio makes for HDU `hdu` (0-based) of `path` when it
/// decompresses it, parsed as `parse_fits` parses a header (long strings
/// joined), without `END`.
fn cfitsio_header(path: &Path, hdu: usize) -> Vec<Card> {
    let name = CString::new(path.to_str().unwrap()).unwrap();
    let mem = CString::new("mem://").unwrap();
    let (mut input, mut output) = (std::ptr::null_mut(), std::ptr::null_mut());
    let (mut status, mut hdutype, mut nkeys) = (0, 0, 0);
    let mut header = Vec::new();
    unsafe {
        fitsio::sys::ffopen(&mut input, name.as_ptr(), 0, &mut status);
        fitsio::sys::ffmahd(input, hdu as i32 + 1, &mut hdutype, &mut status);
        fitsio::sys::ffinit(&mut output, mem.as_ptr(), &mut status);
        fitsio::sys::fits_img_decompress_header(input, output, &mut status);
        fitsio::sys::ffghsp(output, &mut nkeys, std::ptr::null_mut(), &mut status);
        for i in 1..=nkeys {
            let mut record = [0 as std::os::raw::c_char; 81];
            fitsio::sys::ffgrec(output, i, record.as_mut_ptr(), &mut status);
            let text = std::ffi::CStr::from_ptr(record.as_ptr()).to_bytes();
            let mut card = [b' '; 80];
            card[..text.len()].copy_from_slice(text);
            header.extend_from_slice(&card);
        }
        let mut ignored = 0;
        fitsio::sys::ffclos(output, &mut ignored);
        fitsio::sys::ffclos(input, &mut ignored);
    }
    assert_eq!(status, 0, "{} HDU {hdu}", path.display());
    header.extend_from_slice(&format!("{:<80}", "END").into_bytes());
    header.resize(header.len().div_ceil(2880) * 2880, b' ');
    let mut cards = parse_header_blocks(&header).unwrap();
    cards.retain(|c| c.keyword_str() != "END");
    cards
}

#[test]
fn uncompressed_headers_match_cfitsio() {
    let mut files = Vec::new();
    fits_files(&corpus(), &mut files);
    files.sort();
    let mut checked = 0;
    for path in files {
        let bytes = std::fs::read(&path).unwrap();
        let Ok(parsed) = parse_fits(&bytes) else {
            continue;
        };
        for (i, hdu) in parsed.hdus.iter().enumerate() {
            if !matches!(hdu.info, HduInfo::CompressedImage { .. }) {
                continue;
            }
            let ours = uncompressed_header(hdu).unwrap();
            let theirs = cfitsio_header(&path, i);
            if let Some(at) =
                (0..ours.len().max(theirs.len())).find(|&k| ours.get(k) != theirs.get(k))
            {
                panic!(
                    "{} HDU {i}: card {at} is {:?}, cfitsio's {:?} ({} vs {} cards)",
                    path.display(),
                    ours.get(at),
                    theirs.get(at),
                    ours.len(),
                    theirs.len()
                );
            }
            checked += 1;
        }
    }
    eprintln!("{checked} compressed images");
    assert!(
        checked > 40,
        "only {checked} compressed images in the corpus"
    );
}
