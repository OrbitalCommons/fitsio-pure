//! fitsio-pure#101: upstream `fitsio`'s image-creation examples compile
//! unchanged against `fitsio_pure::compat`, and write the same HDUs.
//!
//! The macro holds upstream's `create_image` and `with_custom_primary`
//! rustdoc examples verbatim (plus a borrowed `&dims` call) and expands them
//! against each library. cfitsio then reads both sets of files.

use std::path::Path;

use fitsio::FitsFile as CFits;

macro_rules! upstream_image_examples {
    ($($lib:ident)::+, $extensions:expr, $primary:expr) => {{
        use $($lib)::+::images::{ImageDescription, ImageType};
        use $($lib)::+::FitsFile;

        let mut fptr = FitsFile::create($extensions).open().unwrap();
        let image_description = ImageDescription {
            data_type: ImageType::Float,
            dimensions: &[100, 100],
        };
        fptr.create_image("EXTNAME".to_string(), &image_description)
            .unwrap();
        let dims = vec![7usize, 5];
        let borrowed = ImageDescription {
            data_type: ImageType::Long,
            dimensions: &dims,
        };
        fptr.create_image("SECOND", &borrowed).unwrap();
        drop(fptr);

        let description = ImageDescription {
            data_type: ImageType::Double,
            dimensions: &[52, 103],
        };
        let fptr = FitsFile::create($primary)
            .with_custom_primary(&description)
            .open()
            .unwrap();
        drop(fptr);
    }};
}

/// `(EXTNAME, BITPIX, [NAXIS1, NAXIS2, ...])` for every HDU, read by cfitsio.
fn layout(path: &Path) -> Vec<(String, i64, Vec<i64>)> {
    let mut f = CFits::open(path).unwrap();
    let mut out = Vec::new();
    for i in 0..f.num_hdus().unwrap() {
        let hdu = f.hdu(i).unwrap();
        let name: String = hdu
            .read_key(&mut f, "EXTNAME")
            .unwrap_or_else(|_| String::new());
        let bitpix: i64 = hdu.read_key(&mut f, "BITPIX").unwrap();
        let naxis: i64 = hdu.read_key(&mut f, "NAXIS").unwrap();
        let axes = (1..=naxis)
            .map(|n| hdu.read_key::<i64>(&mut f, &format!("NAXIS{n}")).unwrap())
            .collect();
        out.push((name, bitpix, axes));
    }
    out
}

#[test]
fn upstream_image_creation_examples_match_cfitsio() {
    let dir = tempfile::tempdir().unwrap();
    let (c_ext, c_primary) = (
        dir.path().join("c_ext.fits"),
        dir.path().join("c_primary.fits"),
    );
    let (p_ext, p_primary) = (
        dir.path().join("p_ext.fits"),
        dir.path().join("p_primary.fits"),
    );
    upstream_image_examples!(fitsio, &c_ext, &c_primary);
    upstream_image_examples!(fitsio_pure::compat, &p_ext, &p_primary);

    let expected_ext = vec![
        (String::new(), 8, vec![]),
        ("EXTNAME".to_string(), -32, vec![100, 100]),
        ("SECOND".to_string(), 32, vec![5, 7]),
    ];
    assert_eq!(layout(&c_ext), expected_ext);
    assert_eq!(layout(&p_ext), expected_ext);

    // A row-major [52, 103] primary is NAXIS1 = 103, NAXIS2 = 52.
    let c = layout(&c_primary);
    let p = layout(&p_primary);
    assert_eq!((c[0].1, c[0].2.clone()), (-64, vec![103, 52]));
    assert_eq!((p[0].1, p[0].2.clone()), (c[0].1, c[0].2.clone()));
}
