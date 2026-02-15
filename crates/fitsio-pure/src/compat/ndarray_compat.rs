use ndarray::{Array, ArrayD};

use super::errors::Result;
use super::fitsfile::FitsFile;
use super::hdu::{FitsHdu, HduInfo};
use super::images::ReadImage;

impl<T> ReadImage for ArrayD<T>
where
    T: Clone + ReadImage,
{
    fn read_image(file: &FitsFile, hdu: &FitsHdu) -> Result<Vec<Self>> {
        let data: Vec<T> = T::read_image(file, hdu)?;
        let shape = image_shape(file, hdu)?;
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
        let shape: Vec<usize> = ranges.iter().map(|r| r.end - r.start).collect();
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
        assert_eq!(arr.shape(), &[3, 4]);
        assert_eq!(arr[[0, 0]], 0.0);
        assert_eq!(arr[[2, 3]], 11.0);
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
        assert_eq!(arr.shape(), &[2, 3, 4]);
        assert_eq!(arr[[0, 0, 0]], 0.0);
        assert_eq!(arr[[1, 2, 3]], 23.0);
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
        assert_eq!(arr.shape(), &[2, 3]);
        assert_eq!(arr[[0, 0]], 1.0);
        assert_eq!(arr[[1, 2]], 6.0);
    }
}
