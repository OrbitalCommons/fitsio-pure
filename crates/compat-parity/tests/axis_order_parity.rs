//! Image axis order: compat must order `ImageDescription.dimensions` and
//! `info()` shapes as `fitsio` does (row-major, the reverse of `NAXISn`), and
//! `read_region` ranges as cfitsio does (`NAXIS1` first). Only non-square
//! shapes can tell the conventions apart.

use std::path::Path;

use fitsio::hdu::HduInfo as CHduInfo;
use fitsio::images::{ImageDescription as CImageDesc, ImageType as CImageType};
use fitsio::FitsFile as CFits;

use fitsio_pure::compat::fitsfile::FitsFile as PureFits;
use fitsio_pure::compat::hdu::HduInfo as PureHduInfo;
use fitsio_pure::compat::images::{
    ImageDescription as PureImageDesc, ImageType as PureImageType, ReadImage, WriteImage,
};

const SHAPES: [&[usize]; 3] = [&[5, 7], &[7, 5], &[2, 3, 4]];

fn pixels(dims: &[usize]) -> Vec<i32> {
    (0..dims.iter().product::<usize>() as i32).collect()
}

fn write_cfitsio(path: &Path, dims: &[usize]) {
    let mut f = CFits::create(path).open().unwrap();
    let desc = CImageDesc {
        data_type: CImageType::Long,
        dimensions: dims,
    };
    let hdu = f.create_image("IMG", &desc).unwrap();
    hdu.write_image(&mut f, &pixels(dims)).unwrap();
}

fn write_pure(path: &Path, dims: &[usize]) {
    let mut f = PureFits::create(path).open().unwrap();
    let desc = PureImageDesc {
        data_type: PureImageType::Long,
        dimensions: dims,
    };
    let hdu = f.create_image("IMG", &desc).unwrap();
    i32::write_image(&mut f, &hdu, &pixels(dims)).unwrap();
}

fn c_shape(path: &Path) -> Vec<usize> {
    let mut f = CFits::open(path).unwrap();
    match f.hdu("IMG").unwrap().info {
        CHduInfo::ImageInfo { shape, .. } => shape,
        other => panic!("not an image: {other:?}"),
    }
}

fn pure_shape(path: &Path) -> Vec<usize> {
    let f = PureFits::open(path).unwrap();
    match f.hdu("IMG").unwrap().info(&f).unwrap() {
        PureHduInfo::ImageInfo { shape, .. } => shape,
        other => panic!("not an image: {other:?}"),
    }
}

fn naxes(path: &Path, n: usize) -> Vec<i64> {
    let mut f = CFits::open(path).unwrap();
    let hdu = f.hdu("IMG").unwrap();
    (1..=n)
        .map(|i| hdu.read_key(&mut f, &format!("NAXIS{i}")).unwrap())
        .collect()
}

#[test]
fn dimensions_and_shapes_match_fitsio() {
    let dir = tempfile::tempdir().unwrap();
    for (i, dims) in SHAPES.into_iter().enumerate() {
        let by_c = dir.path().join(format!("c{i}.fits"));
        let by_pure = dir.path().join(format!("pure{i}.fits"));
        write_cfitsio(&by_c, dims);
        write_pure(&by_pure, dims);

        assert_eq!(c_shape(&by_c), dims, "fitsio's own shape for {dims:?}");
        assert_eq!(
            pure_shape(&by_c),
            dims,
            "fitsio-pure reading cfitsio's {dims:?} image"
        );
        assert_eq!(
            c_shape(&by_pure),
            dims,
            "fitsio reading fitsio-pure's {dims:?} image"
        );
        assert_eq!(
            naxes(&by_pure, dims.len()),
            naxes(&by_c, dims.len()),
            "NAXISn for {dims:?}"
        );

        let mut c = CFits::open(&by_pure).unwrap();
        let hdu = c.hdu("IMG").unwrap();
        let read: Vec<i32> = hdu.read_image(&mut c).unwrap();
        assert_eq!(read, pixels(dims), "pixels of fitsio-pure's {dims:?} image");
    }
}

#[test]
fn read_region_ranges_are_naxis1_first_like_cfitsio() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.fits");
    // 5 rows of 7 columns: NAXIS1 = 7, NAXIS2 = 5.
    write_cfitsio(&path, &[5, 7]);
    let (x, y) = (1..6, 2..4);

    let mut c = CFits::open(&path).unwrap();
    let hdu = c.hdu("IMG").unwrap();
    let expected: Vec<i32> = hdu.read_region(&mut c, &[&x, &y]).unwrap();

    let f = PureFits::open(&path).unwrap();
    let hdu = f.hdu("IMG").unwrap();
    let ours = i32::read_region(&f, &hdu, &[x, y]).unwrap();
    assert_eq!(ours, expected);
    // Rows 2..4 of a 7-wide image, columns 1..6.
    assert_eq!(&ours[..5], &[15, 16, 17, 18, 19]);
}

/// A custom primary image: the same description through each library gives
/// the same NAXISn, and each reads the other's pixels.
#[test]
fn custom_primary_matches_fitsio() {
    use fitsio_pure::compat::FitsFile as RootFits;

    let dir = tempfile::tempdir().unwrap();
    for (i, dims) in SHAPES.into_iter().enumerate() {
        let by_c = dir.path().join(format!("c{i}.fits"));
        let by_pure = dir.path().join(format!("pure{i}.fits"));
        {
            let desc = CImageDesc {
                data_type: CImageType::Long,
                dimensions: dims,
            };
            let mut f = CFits::create(&by_c)
                .with_custom_primary(&desc)
                .open()
                .unwrap();
            let hdu = f.primary_hdu().unwrap();
            hdu.write_image(&mut f, &pixels(dims)).unwrap();
        }
        {
            let desc = PureImageDesc {
                data_type: PureImageType::Long,
                dimensions: dims,
            };
            let mut f = RootFits::create(&by_pure)
                .with_custom_primary(&desc)
                .open()
                .unwrap();
            let hdu = f.primary_hdu().unwrap();
            i32::write_image(&mut f, &hdu, &pixels(dims)).unwrap();
        }

        let primary_naxes = |path: &Path| -> Vec<i64> {
            let mut f = CFits::open(path).unwrap();
            let hdu = f.primary_hdu().unwrap();
            (1..=dims.len())
                .map(|n| hdu.read_key(&mut f, &format!("NAXIS{n}")).unwrap())
                .collect()
        };
        assert_eq!(
            primary_naxes(&by_pure),
            primary_naxes(&by_c),
            "NAXISn for {dims:?}"
        );

        let mut c = CFits::open(&by_pure).unwrap();
        let hdu = c.primary_hdu().unwrap();
        let read: Vec<i32> = hdu.read_image(&mut c).unwrap();
        assert_eq!(
            read,
            pixels(dims),
            "cfitsio reading fitsio-pure's {dims:?} primary"
        );

        let p = PureFits::open(&by_c).unwrap();
        let hdu = p.primary_hdu().unwrap();
        assert_eq!(
            i32::read_image(&p, &hdu).unwrap(),
            pixels(dims),
            "fitsio-pure reading cfitsio's {dims:?} primary"
        );
        match hdu.info(&p).unwrap() {
            PureHduInfo::ImageInfo { shape, .. } => assert_eq!(shape, dims),
            other => panic!("not an image: {other:?}"),
        }
    }
}
