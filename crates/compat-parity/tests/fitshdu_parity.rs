//! fitsio-pure#92: upstream `fitsio`'s `FitsHdu` call shapes compile against
//! `fitsio_pure::compat` and give the same results.
//!
//! The macro is written the way upstream code and docs use `FitsHdu`: the
//! `info` and `number` fields, `name`, and the image reads and writes as
//! methods. It expands against each library, and both must agree. cfitsio
//! also reads the fitsio-pure file, so the written bytes are checked too.

use std::path::Path;

macro_rules! upstream_hdu_calls {
    ($($lib:ident)::+, $path:expr) => {{
        use $($lib)::+::hdu::HduInfo;
        use $($lib)::+::images::{ImageDescription, ImageType};
        use $($lib)::+::FitsFile;

        let mut fptr = FitsFile::create($path).open().unwrap();
        let desc = ImageDescription {
            data_type: ImageType::Long,
            dimensions: &[5, 7],
        };
        let hdu = fptr.create_image("IMG".to_string(), &desc).unwrap();
        let pixels: Vec<i32> = (0..35).collect();
        assert!(hdu.write_image(&mut fptr, &pixels).is_ok());
        let too_many: Vec<i32> = (0..36).collect();
        assert!(hdu.write_image(&mut fptr, &too_many).is_err());

        let desc = ImageDescription {
            data_type: ImageType::Float,
            dimensions: &[4, 6],
        };
        let fhdu = fptr.create_image("FLT".to_string(), &desc).unwrap();
        // f64 values into a Float image, as upstream's own doc example does.
        let data_to_write: Vec<f64> = vec![1.0, 2.0, 3.0];
        fhdu.write_section(&mut fptr, 0, data_to_write.len(), &data_to_write)
            .unwrap();
        let data_to_write: Vec<f64> = vec![9.0, 8.0, 7.0, 6.0, 5.0, 4.0];
        let ranges = [&(1..4), &(2..4)];
        fhdu.write_region(&mut fptr, &ranges, &data_to_write).unwrap();
        // Values past the region's size are ignored.
        fhdu.write_region(&mut fptr, &[&(5..6), &(0..1)], &[1.5f64, 99.0])
            .unwrap();
        drop(fptr);

        let mut fptr = FitsFile::open($path).unwrap();
        let hdu = fptr.hdu(1).unwrap();
        let shape = match &hdu.info {
            HduInfo::ImageInfo { shape, .. } => shape.clone(),
            _ => vec![],
        };
        let number = hdu.number;
        let name = hdu.name(&mut fptr).unwrap();
        let section: Vec<i32> = hdu.read_section(&mut fptr, 3, 12).unwrap();
        let xcoord = 1..4;
        let ycoord = 2..5;
        let region: Vec<i32> = hdu.read_region(&mut fptr, &[&xcoord, &ycoord]).unwrap();
        let rows: Vec<f32> = hdu.read_rows(&mut fptr, 1, 2).unwrap();
        let image: Vec<f64> = hdu.read_image(&mut fptr).unwrap();
        let fhdu = fptr.hdu("FLT").unwrap();
        let floats: Vec<f32> = fhdu.read_image(&mut fptr).unwrap();
        (shape, number, name, section, region, rows, image, floats)
    }};
}

/// The FLT image, read by cfitsio.
fn cfitsio_floats(path: &Path) -> Vec<f32> {
    let mut f = fitsio::FitsFile::open(path).unwrap();
    let hdu = f.hdu("FLT").unwrap();
    hdu.read_image(&mut f).unwrap()
}

#[test]
fn upstream_fitshdu_calls_match_cfitsio() {
    let dir = tempfile::tempdir().unwrap();
    let c_path = dir.path().join("c.fits");
    let p_path = dir.path().join("p.fits");
    let c = upstream_hdu_calls!(fitsio, &c_path);
    let p = upstream_hdu_calls!(fitsio_pure::compat, &p_path);
    assert_eq!(p, c);

    // Spot-check the cfitsio results themselves, so agreement isn't vacuous.
    let (shape, number, name, section, region, rows, image, floats) = &c;
    assert_eq!(
        (shape.as_slice(), *number, name.as_str()),
        (&[5, 7][..], 1, "IMG")
    );
    assert_eq!(section, &(3..12).collect::<Vec<i32>>());
    assert_eq!(region, &[15, 16, 17, 22, 23, 24, 29, 30, 31]);
    assert_eq!(rows.len(), 14);
    assert_eq!(image[34], 34.0);
    assert_eq!(&floats[..3], &[1.0, 2.0, 3.0]);
    assert_eq!(floats[5], 1.5);
    assert_eq!(&floats[13..16], &[9.0, 8.0, 7.0]);

    assert_eq!(cfitsio_floats(&p_path), cfitsio_floats(&c_path));
}
