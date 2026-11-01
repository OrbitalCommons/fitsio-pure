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

/// hyperdrive's solutions file, shaped as `solutions/hyperdrive.rs` writes
/// it (4-D and 2-D `f64` images with data), then an image of every type,
/// written through `$lib`'s `create_image`.
macro_rules! solutions_file {
    ($($lib:ident)::+, $path:expr) => {{
        use $($lib)::+::images::{ImageDescription, ImageType};
        use $($lib)::+::FitsFile;

        let mut f = FitsFile::create($path).open().unwrap();
        let dims = [2usize, 3, 4, 8];
        let solutions = f
            .create_image(
                "SOLUTIONS",
                &ImageDescription {
                    data_type: ImageType::Double,
                    dimensions: &dims,
                },
            )
            .unwrap();
        let values: Vec<f64> = (0..dims.iter().product::<usize>())
            .map(|i| i as f64 * 0.25)
            .collect();
        solutions.write_image(&mut f, &values).unwrap();
        let results = f
            .create_image(
                "RESULTS",
                &ImageDescription {
                    data_type: ImageType::Double,
                    dimensions: &[2, 4],
                },
            )
            .unwrap();
        results.write_image(&mut f, &[1.5f64; 8]).unwrap();
        for (name, data_type) in [
            ("U8", ImageType::UnsignedByte),
            ("I8", ImageType::Byte),
            ("I16", ImageType::Short),
            ("U16", ImageType::UnsignedShort),
            ("I32", ImageType::Long),
            ("U32", ImageType::UnsignedLong),
            ("I64", ImageType::LongLong),
            ("F32", ImageType::Float),
        ] {
            let desc = ImageDescription {
                data_type,
                dimensions: &[3, 5],
            };
            f.create_image(name, &desc).unwrap();
        }
    }};
}

/// fitsio-pure#188: images `create_image` adds are byte-identical to
/// `fitsio`'s, header comments included.
#[test]
fn created_images_match_fitsio_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let c_path = dir.path().join("c.fits");
    let p_path = dir.path().join("p.fits");
    solutions_file!(fitsio, &c_path);
    solutions_file!(fitsio_pure::compat, &p_path);
    let (c_bytes, p_bytes) = (
        std::fs::read(&c_path).unwrap(),
        std::fs::read(&p_path).unwrap(),
    );
    let cards = |b: &[u8]| -> Vec<String> {
        b.chunks(80)
            .map(|c| String::from_utf8_lossy(c).into_owned())
            .collect()
    };
    assert_eq!(cards(&p_bytes), cards(&c_bytes));
    assert!(p_bytes == c_bytes);
}
