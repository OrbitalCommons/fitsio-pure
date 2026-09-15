use ndarray::{Array, ArrayBase, ArrayD, Data, Dimension};

use super::errors::Result;
use super::fitsfile::FitsFile;
use super::hdu::{FitsHdu, HduInfo};
use super::images::{ReadImage, WriteImage};

/// Write an n-dimensional array directly to an image HDU (requires the `array`
/// feature).
///
/// The array is flattened in row-major (C) order — which is FITS storage order
/// (NAXIS1 fastest-varying) — and written via the element type's
/// [`WriteImage`] impl. The target HDU must already be created with a matching
/// total pixel count; per `create_image`'s convention the `dimensions` are
/// given in FITS axis order (NAXIS1 first), i.e. the reverse of the array's
/// `(.., NAXIS2, NAXIS1)` shape.
pub trait WriteImageArray {
    /// Flatten and write this array's elements into `hdu`.
    fn write_image_array(&self, file: &mut FitsFile, hdu: &FitsHdu) -> Result<()>;
}

impl<S, D> WriteImageArray for ArrayBase<S, D>
where
    S: Data,
    S::Elem: WriteImage + Clone,
    D: Dimension,
{
    fn write_image_array(&self, file: &mut FitsFile, hdu: &FitsHdu) -> Result<()> {
        // `iter()` yields elements in logical row-major order regardless of the
        // array's physical memory layout, giving a NAXIS1-fastest buffer.
        let flat: Vec<S::Elem> = self.iter().cloned().collect();
        <S::Elem as WriteImage>::write_image(file, hdu, &flat)
    }
}

impl<T> ReadImage for ArrayD<T>
where
    T: Clone + ReadImage,
{
    fn read_image(file: &FitsFile, hdu: &FitsHdu) -> Result<Vec<Self>> {
        let data: Vec<T> = T::read_image(file, hdu)?;
        // FITS stores NAXIS1 as the fastest-varying axis, but ndarray is
        // row-major (last axis fastest). Reverse the FITS axis order so the
        // flat NAXIS1-fastest buffer maps to a (.., NAXIS2, NAXIS1) shape;
        // without this, any non-square image is transposed/mis-strided.
        let mut shape = image_shape(file, hdu)?;
        shape.reverse();
        let arr = Array::from_shape_vec(shape, data)
            .map_err(|e| super::errors::Error::Message(e.to_string()))?;
        Ok(vec![arr])
    }

    fn read_section(
        file: &FitsFile,
        hdu: &FitsHdu,
        range: std::ops::Range<usize>,
    ) -> Result<Vec<Self>> {
        let data: Vec<T> = T::read_section(file, hdu, range)?;
        let shape = vec![data.len()];
        let arr = Array::from_shape_vec(shape, data)
            .map_err(|e| super::errors::Error::Message(e.to_string()))?;
        Ok(vec![arr])
    }

    fn read_rows(
        file: &FitsFile,
        hdu: &FitsHdu,
        start_row: usize,
        num_rows: usize,
    ) -> Result<Vec<Self>> {
        let data: Vec<T> = T::read_rows(file, hdu, start_row, num_rows)?;
        let full_shape = image_shape(file, hdu)?;
        // A "row" in FITS is one slice along NAXIS1 (the first/fastest axis).
        // read_image_rows returns num_rows * naxes[0] elements.
        let row_length = if !full_shape.is_empty() {
            full_shape[0]
        } else {
            data.len() / num_rows
        };
        let shape = if num_rows == 1 {
            vec![row_length]
        } else {
            vec![num_rows, row_length]
        };
        let arr = Array::from_shape_vec(shape, data)
            .map_err(|e| super::errors::Error::Message(e.to_string()))?;
        Ok(vec![arr])
    }

    fn read_region(
        file: &FitsFile,
        hdu: &FitsHdu,
        ranges: &[std::ops::Range<usize>],
    ) -> Result<Vec<Self>> {
        let data: Vec<T> = T::read_region(file, hdu, ranges)?;
        // `ranges` are in FITS axis order (NAXIS1 first) and the returned
        // buffer is NAXIS1-fastest, so reverse to row-major shape to match
        // `read_image`.
        let mut shape: Vec<usize> = ranges.iter().map(|r| r.end - r.start).collect();
        shape.reverse();
        let arr = Array::from_shape_vec(shape, data)
            .map_err(|e| super::errors::Error::Message(e.to_string()))?;
        Ok(vec![arr])
    }
}

fn image_shape(file: &FitsFile, hdu: &FitsHdu) -> Result<Vec<usize>> {
    match hdu.info(file)? {
        HduInfo::ImageInfo { shape, .. } => Ok(shape),
        _ => Err(super::errors::Error::Message(
            "HDU is not an image".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compat::images::{ImageDescription, ImageType, WriteImage};
    use ndarray::{arr2, Array};

    #[test]
    fn write_image_array_2d_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("write2d.fits");
        let mut f = FitsFile::create(&path).open().unwrap();

        // Logical 4-row x 3-col array; values encode row*10 + col.
        let arr = Array::from_shape_fn((4, 3), |(r, c)| (r * 10 + c) as f32);

        // create_image takes FITS axis order (NAXIS1 first): cols then rows.
        let desc = ImageDescription {
            data_type: ImageType::Float,
            dimensions: vec![3, 4],
        };
        let hdu = f.create_image("SCI", &desc).unwrap();
        arr.write_image_array(&mut f, &hdu).unwrap();

        // Scalar read returns the flat storage buffer (NAXIS1-fastest), which
        // must equal the array's row-major flatten.
        let flat: Vec<f32> = f32::read_image(&f, &hdu).unwrap();
        let expected: Vec<f32> = arr.iter().copied().collect();
        assert_eq!(flat, expected);
        assert_eq!(flat[0], 0.0); // (0,0)
        assert_eq!(flat[11], 32.0); // (3,2)
    }

    #[test]
    fn write_image_array_1d_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("write1d.fits");
        let mut f = FitsFile::create(&path).open().unwrap();

        let arr = Array::from_vec(vec![10i32, 20, 30, 40, 50]);
        let desc = ImageDescription {
            data_type: ImageType::Long,
            dimensions: vec![5],
        };
        let hdu = f.create_image("DATA", &desc).unwrap();
        arr.write_image_array(&mut f, &hdu).unwrap();

        let flat: Vec<i32> = i32::read_image(&f, &hdu).unwrap();
        assert_eq!(flat, vec![10, 20, 30, 40, 50]);
    }

    #[test]
    fn write_image_array_non_contiguous_uses_logical_order() {
        // A transposed view is Fortran-ordered in memory; `write_image_array`
        // must still emit logical row-major order.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("write_t.fits");
        let mut f = FitsFile::create(&path).open().unwrap();

        let base = arr2(&[[1.0f64, 2.0, 3.0], [4.0, 5.0, 6.0]]); // shape (2, 3)
        let view = base.t(); // shape (3, 2), non-contiguous

        // view logical order: [1,4, 2,5, 3,6]
        let desc = ImageDescription {
            data_type: ImageType::Double,
            dimensions: vec![2, 3], // NAXIS1=2, NAXIS2=3 to match view shape (3, 2)
        };
        let hdu = f.create_image("T", &desc).unwrap();
        view.write_image_array(&mut f, &hdu).unwrap();

        let flat: Vec<f64> = f64::read_image(&f, &hdu).unwrap();
        assert_eq!(flat, vec![1.0, 4.0, 2.0, 5.0, 3.0, 6.0]);
    }

    #[test]
    fn read_image_2d_f32() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("img.fits");
        let mut f = FitsFile::create(&path).open().unwrap();

        let desc = ImageDescription {
            data_type: ImageType::Float,
            dimensions: vec![3, 4],
        };
        let hdu = f.create_image("SCI", &desc).unwrap();
        let pixels: Vec<f32> = (0..12).map(|i| i as f32).collect();
        f32::write_image(&mut f, &hdu, &pixels).unwrap();

        let result: Vec<ArrayD<f32>> = ArrayD::<f32>::read_image(&f, &hdu).unwrap();
        assert_eq!(result.len(), 1);
        let arr = &result[0];
        // dimensions [3, 4] => NAXIS1=3, NAXIS2=4; shape is (NAXIS2, NAXIS1).
        assert_eq!(arr.shape(), &[4, 3]);
        assert_eq!(arr[[0, 0]], 0.0);
        assert_eq!(arr[[3, 2]], 11.0);
    }

    #[test]
    fn read_image_3d_f64() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("img3d.fits");
        let mut f = FitsFile::create(&path).open().unwrap();

        let desc = ImageDescription {
            data_type: ImageType::Double,
            dimensions: vec![2, 3, 4],
        };
        let hdu = f.create_image("CUBE", &desc).unwrap();
        let pixels: Vec<f64> = (0..24).map(|i| i as f64).collect();
        f64::write_image(&mut f, &hdu, &pixels).unwrap();

        let result: Vec<ArrayD<f64>> = ArrayD::<f64>::read_image(&f, &hdu).unwrap();
        assert_eq!(result.len(), 1);
        let arr = &result[0];
        // dimensions [2, 3, 4] => NAXIS1=2, NAXIS2=3, NAXIS3=4; shape reverses
        // to (NAXIS3, NAXIS2, NAXIS1).
        assert_eq!(arr.shape(), &[4, 3, 2]);
        assert_eq!(arr[[0, 0, 0]], 0.0);
        assert_eq!(arr[[3, 2, 1]], 23.0);
    }

    #[test]
    fn read_section_returns_1d() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sec.fits");
        let mut f = FitsFile::create(&path).open().unwrap();

        let desc = ImageDescription {
            data_type: ImageType::Float,
            dimensions: vec![3, 4],
        };
        let hdu = f.create_image("SCI", &desc).unwrap();
        let pixels: Vec<f32> = (0..12).map(|i| i as f32).collect();
        f32::write_image(&mut f, &hdu, &pixels).unwrap();

        let result: Vec<ArrayD<f32>> = ArrayD::<f32>::read_section(&f, &hdu, 4..8).unwrap();
        assert_eq!(result.len(), 1);
        let arr = &result[0];
        assert_eq!(arr.shape(), &[4]);
        assert_eq!(arr[0], 4.0);
        assert_eq!(arr[3], 7.0);
    }

    #[test]
    fn read_rows_2d() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rows.fits");
        let mut f = FitsFile::create(&path).open().unwrap();

        // dimensions [5, 4] means NAXIS1=5 (columns), NAXIS2=4 (rows)
        let desc = ImageDescription {
            data_type: ImageType::Float,
            dimensions: vec![5, 4],
        };
        let hdu = f.create_image("SCI", &desc).unwrap();
        let pixels: Vec<f32> = (0..20).map(|i| i as f32).collect();
        f32::write_image(&mut f, &hdu, &pixels).unwrap();

        // Reading 2 rows starting at row 1 gives 2*5=10 elements
        let result: Vec<ArrayD<f32>> = ArrayD::<f32>::read_rows(&f, &hdu, 1, 2).unwrap();
        assert_eq!(result.len(), 1);
        let arr = &result[0];
        assert_eq!(arr.shape(), &[2, 5]);
        assert_eq!(arr[[0, 0]], 5.0); // row 1, col 0
        assert_eq!(arr[[1, 4]], 14.0); // row 2, col 4
    }

    #[test]
    fn read_region_2d() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("region.fits");
        let mut f = FitsFile::create(&path).open().unwrap();

        let desc = ImageDescription {
            data_type: ImageType::Float,
            dimensions: vec![5, 4],
        };
        let hdu = f.create_image("SCI", &desc).unwrap();
        let pixels: Vec<f32> = (0..20).map(|i| i as f32).collect();
        f32::write_image(&mut f, &hdu, &pixels).unwrap();

        let result: Vec<ArrayD<f32>> = ArrayD::<f32>::read_region(&f, &hdu, &[1..3, 0..2]).unwrap();
        assert_eq!(result.len(), 1);
        let arr = &result[0];
        assert_eq!(arr.shape(), &[2, 2]);
    }

    #[test]
    fn read_image_1d() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("img1d.fits");
        let mut f = FitsFile::create(&path).open().unwrap();

        let desc = ImageDescription {
            data_type: ImageType::Long,
            dimensions: vec![6],
        };
        let hdu = f.create_image("DATA", &desc).unwrap();
        let pixels: Vec<i32> = vec![10, 20, 30, 40, 50, 60];
        i32::write_image(&mut f, &hdu, &pixels).unwrap();

        let result: Vec<ArrayD<i32>> = ArrayD::<i32>::read_image(&f, &hdu).unwrap();
        assert_eq!(result.len(), 1);
        let arr = &result[0];
        assert_eq!(arr.shape(), &[6]);
        assert_eq!(arr[0], 10);
        assert_eq!(arr[5], 60);
    }

    #[test]
    fn read_image_type_conversion() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conv.fits");
        let mut f = FitsFile::create(&path).open().unwrap();

        let desc = ImageDescription {
            data_type: ImageType::Short,
            dimensions: vec![2, 3],
        };
        let hdu = f.create_image("SCI", &desc).unwrap();
        let pixels: Vec<i16> = vec![1, 2, 3, 4, 5, 6];
        i16::write_image(&mut f, &hdu, &pixels).unwrap();

        // Read i16 data as f32 array
        let result: Vec<ArrayD<f32>> = ArrayD::<f32>::read_image(&f, &hdu).unwrap();
        assert_eq!(result.len(), 1);
        let arr = &result[0];
        // dimensions [2, 3] => NAXIS1=2, NAXIS2=3; shape is (NAXIS2, NAXIS1).
        assert_eq!(arr.shape(), &[3, 2]);
        assert_eq!(arr[[0, 0]], 1.0);
        assert_eq!(arr[[2, 1]], 6.0);
    }

    #[test]
    fn read_image_nonsquare_preserves_layout() {
        // Regression test for the NAXIS axis-order bug: a logical 4-row x 3-col
        // image must round-trip to shape [4, 3] with element positions intact.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonsquare.fits");
        let mut f = FitsFile::create(&path).open().unwrap();

        // NAXIS1 = 3 (columns, fastest), NAXIS2 = 4 (rows).
        let desc = ImageDescription {
            data_type: ImageType::Float,
            dimensions: vec![3, 4],
        };
        let hdu = f.create_image("SCI", &desc).unwrap();

        // Flat buffer in FITS storage order (NAXIS1-fastest): pixel at logical
        // (row, col) is stored at row * 3 + col, with value encoded as row*10+col.
        let mut pixels = vec![0.0f32; 12];
        for row in 0..4 {
            for col in 0..3 {
                pixels[row * 3 + col] = (row * 10 + col) as f32;
            }
        }
        f32::write_image(&mut f, &hdu, &pixels).unwrap();

        let result: Vec<ArrayD<f32>> = ArrayD::<f32>::read_image(&f, &hdu).unwrap();
        let arr = &result[0];
        assert_eq!(arr.shape(), &[4, 3], "shape must be (rows, cols)");
        for row in 0..4 {
            for col in 0..3 {
                assert_eq!(
                    arr[[row, col]],
                    (row * 10 + col) as f32,
                    "scrambled element at ({row}, {col})"
                );
            }
        }
    }
}
