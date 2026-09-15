//! Differential image roundtrip parity between `fitsio-pure`'s compat shims and
//! the canonical cfitsio-backed `fitsio` crate.
//!
//! Each case writes an image with one library and reads it back with the other,
//! in *both* directions, asserting the recovered pixel buffer is bit-for-bit
//! identical to the source.
//!
//! Comparisons are made on the **flat pixel buffer** (FITS storage order,
//! NAXIS1-fastest). This is deliberate: it makes the check independent of how
//! either library maps FITS axes onto an n-dimensional shape, so it validates
//! on-disk format interoperability rather than any particular axis convention.
//! (The ndarray axis-order mapping is exercised separately in the fitsio-pure
//! unit tests.)

use fitsio::images::{ImageDescription as CImageDesc, ImageType as CImageType};
use fitsio::FitsFile as CFits;

use fitsio_pure::compat::fitsfile::FitsFile as PureFits;
use fitsio_pure::compat::images::{
    ImageDescription as PureImageDesc, ImageType as PureImageType, ReadImage as PureReadImage,
    WriteImage as PureWriteImage,
};

/// Generate one parity test covering both write/read directions for a scalar
/// pixel type, across a 1-D shape and a deliberately non-square 2-D shape.
macro_rules! image_parity_case {
    ($name:ident, $t:ty, $pure_ty:expr, $c_ty:expr) => {
        #[test]
        fn $name() {
            // 1-D and a non-square 2-D shape (transposition bugs only show up
            // when the axis lengths differ).
            for dims in [vec![7usize], vec![4usize, 3usize]] {
                let n: usize = dims.iter().product();
                // Deterministic, exactly-representable values for every type.
                let data: Vec<$t> = (0..n).map(|i| (i as i32 - 2) as $t).collect();

                roundtrip_pure_to_cfitsio::<$t>(&dims, &data, $pure_ty, $c_ty);
                roundtrip_cfitsio_to_pure::<$t>(&dims, &data, $pure_ty, $c_ty);
            }
        }
    };
}

fn roundtrip_pure_to_cfitsio<T>(
    dims: &[usize],
    data: &[T],
    pure_ty: PureImageType,
    _c_ty: CImageType,
) where
    T: PureWriteImage + Clone + PartialEq + std::fmt::Debug,
    Vec<T>: fitsio::images::ReadImage,
{
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pure_to_c.fits");

    {
        let mut f = PureFits::create(&path).open().unwrap();
        let desc = PureImageDesc {
            data_type: pure_ty,
            dimensions: dims.to_vec(),
        };
        let hdu = f.create_image("SCI", &desc).unwrap();
        <T as PureWriteImage>::write_image(&mut f, &hdu, data).unwrap();
        f.flush().unwrap();
    }

    let mut fptr = CFits::open(&path).expect("cfitsio failed to open pure-written file");
    let hdu = fptr.hdu("SCI").expect("cfitsio could not find SCI HDU");
    let read: Vec<T> = hdu.read_image(&mut fptr).unwrap();
    assert_eq!(
        read,
        data.to_vec(),
        "pure -> cfitsio flat mismatch for dims {:?}",
        dims
    );
}

fn roundtrip_cfitsio_to_pure<T>(
    dims: &[usize],
    data: &[T],
    _pure_ty: PureImageType,
    c_ty: CImageType,
) where
    T: PureReadImage + Clone + PartialEq + std::fmt::Debug,
    T: fitsio::images::WriteImage,
{
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c_to_pure.fits");

    {
        let mut fptr = CFits::create(&path).open().unwrap();
        let desc = CImageDesc {
            data_type: c_ty,
            dimensions: dims,
        };
        let hdu = fptr.create_image("SCI", &desc).unwrap();
        hdu.write_image(&mut fptr, data).unwrap();
    }

    let f = PureFits::open(&path).expect("fitsio-pure failed to open cfitsio-written file");
    let hdu = f.hdu("SCI").expect("fitsio-pure could not find SCI HDU");
    let read: Vec<T> = <T as PureReadImage>::read_image(&f, &hdu).unwrap();
    assert_eq!(
        read,
        data.to_vec(),
        "cfitsio -> pure flat mismatch for dims {:?}",
        dims
    );
}

image_parity_case!(
    parity_u8,
    u8,
    PureImageType::UnsignedByte,
    CImageType::UnsignedByte
);
image_parity_case!(parity_i16, i16, PureImageType::Short, CImageType::Short);
image_parity_case!(parity_i32, i32, PureImageType::Long, CImageType::Long);
image_parity_case!(
    parity_i64,
    i64,
    PureImageType::LongLong,
    CImageType::LongLong
);
image_parity_case!(parity_f32, f32, PureImageType::Float, CImageType::Float);
image_parity_case!(parity_f64, f64, PureImageType::Double, CImageType::Double);
