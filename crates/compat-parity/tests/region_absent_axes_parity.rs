//! Region ranges for axes the image doesn't have. cfitsio reads only the
//! first `NAXIS` ranges, so upstream code can pass a `0..1` per degenerate
//! Stokes and frequency axis to a 2-D image. FastFitsCutter does this when the
//! header carries `CTYPE3`/`CTYPE4` but the image is 2-D.

use std::path::Path;

macro_rules! absent_axis_calls {
    ($($lib:ident)::+, $path:expr) => {{
        use $($lib)::+::images::{ImageDescription, ImageType};
        use $($lib)::+::FitsFile;

        let desc = ImageDescription {
            data_type: ImageType::Float,
            dimensions: &[5, 6],
        };
        let mut fptr = FitsFile::create($path)
            .with_custom_primary(&desc)
            .open()
            .unwrap();
        let hdu = fptr.primary_hdu().unwrap();
        let pixels: Vec<f32> = (0..30).map(|v| v as f32).collect();
        hdu.write_image(&mut fptr, &pixels).unwrap();
        hdu.write_region(&mut fptr, &[&(1..3), &(2..4), &(0..1), &(0..1)], &[-1.0f32, -2.0, -3.0, -4.0])
            .unwrap();
        drop(fptr);

        let mut fptr = FitsFile::open($path).unwrap();
        let hdu = fptr.primary_hdu().unwrap();
        let three: Vec<f32> = hdu.read_region(&mut fptr, &[&(0..4), &(1..3), &(0..1)]).unwrap();
        let four: Vec<f32> = hdu
            .read_region(&mut fptr, &[&(0..4), &(1..3), &(0..1), &(0..1)])
            .unwrap();
        let image: Vec<f32> = hdu.read_image(&mut fptr).unwrap();
        (three, four, image)
    }};
}

fn cfitsio_image(path: &Path) -> Vec<f32> {
    let mut f = fitsio::FitsFile::open(path).unwrap();
    let hdu = f.primary_hdu().unwrap();
    hdu.read_image(&mut f).unwrap()
}

#[test]
fn trailing_unit_ranges_match_cfitsio() {
    let dir = tempfile::tempdir().unwrap();
    let c_path = dir.path().join("c.fits");
    let p_path = dir.path().join("p.fits");
    let c = absent_axis_calls!(fitsio, &c_path);
    let p = absent_axis_calls!(fitsio_pure::compat, &p_path);
    assert_eq!(p, c);

    let (three, four, image) = &c;
    assert_eq!(three, &[6.0, 7.0, 8.0, 9.0, 12.0, -1.0, -2.0, 15.0]);
    assert_eq!(three, four);
    assert_eq!(&image[13..15], &[-1.0, -2.0]);
    assert_eq!(&image[19..21], &[-3.0, -4.0]);

    assert_eq!(cfitsio_image(&p_path), cfitsio_image(&c_path));
}
