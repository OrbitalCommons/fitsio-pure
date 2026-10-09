use super::errors::{Error, Result};
use super::fitsfile::FitsFile;
use super::headers::{ReadsKey, WritesKey};
use super::images::{ReadsImage, WriteImage};
use super::tables::{ReadsCol, ReadsColRange, WritesCol};
use std::ops::Range;

/// Handle to one HDU within a FITS file.
///
/// As in `fitsio`, it carries the HDU's position and a snapshot of its
/// [`HduInfo`] taken when the handle was fetched, so `hdu.number` and
/// `match &hdu.info { … }` work as upstream code writes them. The pixel and
/// table data are always read through the `FitsFile`; call
/// [`FitsHdu::info`](FitsHdu::info) for info that reflects later changes.
#[derive(Debug, Clone, PartialEq)]
pub struct FitsHdu {
    /// The HDU's kind and shape when the handle was fetched.
    pub info: HduInfo,
    /// The HDU's 0-based position in the file (0 is the primary).
    pub number: usize,
}

/// Describes the kind and shape of data stored in an HDU.
#[derive(Debug, Clone, PartialEq)]
pub enum HduInfo {
    ImageInfo {
        /// Axis lengths in row-major order, slowest axis first, as `fitsio`
        /// gives them: the reverse of the FITS `NAXISn` order.
        shape: Vec<usize>,
        image_type: super::images::ImageType,
    },
    TableInfo {
        column_count: usize,
        row_count: usize,
    },
    AnyInfo,
}

/// FITS `NAXISn` order (fastest axis first) reversed into the row-major order
/// `fitsio` uses for image shapes and dimensions.
pub(crate) fn row_major(naxes: &[usize]) -> Vec<usize> {
    naxes.iter().rev().copied().collect()
}

impl FitsHdu {
    /// Read a header keyword value from this HDU.
    pub fn read_key<T: ReadsKey>(&self, file: &FitsFile, name: &str) -> Result<T> {
        T::read_key(file, self, name)
    }

    /// Write a header keyword value to this HDU.
    ///
    /// Takes the value by value, as `fitsio` does, so upstream calls such as
    /// `hdu.write_key(&mut f, "TELESCOP", "MWA")` or
    /// `hdu.write_key(&mut f, "EXPTIME", (30.0, "seconds"))` compile unchanged.
    /// A reference to a value works too.
    pub fn write_key<T: WritesKey>(&self, file: &mut FitsFile, name: &str, value: T) -> Result<()> {
        T::write_key(file, self, name, &value)
    }

    /// Read a column from a binary table HDU.
    pub fn read_col<T: ReadsCol>(&self, file: &FitsFile, name: &str) -> Result<Vec<T>> {
        T::read_col(file, self, name)
    }

    /// Read a range of rows from a binary table column.
    pub fn read_col_range<T: ReadsColRange>(
        &self,
        file: &FitsFile,
        name: &str,
        start_row: usize,
        num_rows: usize,
    ) -> Result<Vec<T>> {
        T::read_col_range(file, self, name, start_row, num_rows)
    }

    /// Write data to a column in a binary table HDU.
    pub fn write_col<T: WritesCol>(
        &self,
        file: &mut FitsFile,
        name: &str,
        data: &[T],
    ) -> Result<()> {
        T::write_col(file, self, name, data)
    }

    /// Return information about the type and shape of data in this HDU as
    /// the file holds it now.
    pub fn info(&self, file: &FitsFile) -> Result<HduInfo> {
        hdu_info(file, self.number)
    }

    /// The HDU's `EXTNAME`, or an empty string when it has none, as `fitsio`
    /// returns it.
    pub fn name(&self, file: &FitsFile) -> Result<String> {
        Ok(self.read_key::<String>(file, "EXTNAME").unwrap_or_default())
    }

    /// Read the whole image into a `Vec<T>` or, with the `array` feature, an
    /// `ArrayD<T>`, as `hdu.read_image(&mut f)` does in `fitsio`.
    pub fn read_image<T: ReadsImage>(&self, file: &FitsFile) -> Result<T> {
        T::image(file, self)
    }

    /// Read the pixels `start..end` of the flat, `NAXIS1`-fastest pixel order.
    pub fn read_section<T: ReadsImage>(
        &self,
        file: &FitsFile,
        start: usize,
        end: usize,
    ) -> Result<T> {
        T::section(file, self, start..end)
    }

    /// Read `num_rows` rows (runs along `NAXIS1`) from `start_row` on.
    pub fn read_rows<T: ReadsImage>(
        &self,
        file: &FitsFile,
        start_row: usize,
        num_rows: usize,
    ) -> Result<T> {
        T::rows(file, self, start_row, num_rows)
    }

    /// Read a rectangular region, one range per axis, `NAXIS1` first, as
    /// `fitsio` passes them to cfitsio.
    pub fn read_region<T: ReadsImage>(
        &self,
        file: &FitsFile,
        ranges: &[&Range<usize>],
    ) -> Result<T> {
        let ranges: Vec<Range<usize>> = ranges.iter().map(|&r| r.clone()).collect();
        T::region(file, self, &ranges)
    }

    /// Write `data` from the first pixel on, converting it to the image's
    /// type. More values than the image has pixels is an error, as in
    /// `fitsio`.
    pub fn write_image<T: WriteImage>(&self, file: &mut FitsFile, data: &[T]) -> Result<()> {
        T::write_image(file, self, data)
    }

    /// Write `data` to the pixels `start..end` of the flat pixel order.
    pub fn write_section<T: WriteImage>(
        &self,
        file: &mut FitsFile,
        start: usize,
        end: usize,
        data: &[T],
    ) -> Result<()> {
        T::write_section(file, self, start..end, data)
    }

    /// Write `data` (`NAXIS1`-fastest) to a rectangular region, one range per
    /// axis, `NAXIS1` first.
    pub fn write_region<T: WriteImage>(
        &self,
        file: &mut FitsFile,
        ranges: &[&Range<usize>],
        data: &[T],
    ) -> Result<()> {
        let ranges: Vec<Range<usize>> = ranges.iter().map(|&r| r.clone()).collect();
        T::write_region(file, self, &ranges, data)
    }

    /// Read the value in row `idx` of table column `name`.
    pub fn read_cell_value<T: ReadsColRange>(
        &self,
        file: &FitsFile,
        name: &str,
        idx: usize,
    ) -> Result<T> {
        T::read_col_range(file, self, name, idx, 1)?
            .pop()
            .ok_or_else(|| Error::Message(format!("row {idx} of column '{name}' not found")))
    }
}

impl FitsHdu {
    /// The handle for HDU `number` of `file`, with its info filled in.
    pub(crate) fn at(file: &FitsFile, number: usize) -> Result<FitsHdu> {
        Ok(FitsHdu {
            info: hdu_info(file, number)?,
            number,
        })
    }
}

/// The compat [`HduInfo`] of HDU `number`.
fn hdu_info(file: &FitsFile, number: usize) -> Result<HduInfo> {
    let fits_data = file.parsed()?;
    let hdu = fits_data
        .get(number)
        .ok_or(Error::status(super::sys::END_OF_FILE))?;
    let (bscale, bzero) = crate::image::extract_bscale_bzero(&hdu.cards);

    match &hdu.info {
        crate::hdu::HduInfo::Primary { bitpix, naxes } => {
            let image_type = super::images::ImageType::equivalent(*bitpix, bscale, bzero)?;
            Ok(HduInfo::ImageInfo {
                shape: row_major(naxes),
                image_type,
            })
        }
        crate::hdu::HduInfo::Image { bitpix, naxes } => {
            let image_type = super::images::ImageType::equivalent(*bitpix, bscale, bzero)?;
            Ok(HduInfo::ImageInfo {
                shape: row_major(naxes),
                image_type,
            })
        }
        crate::hdu::HduInfo::AsciiTable {
            naxis2, tfields, ..
        } => Ok(HduInfo::TableInfo {
            column_count: *tfields,
            row_count: *naxis2,
        }),
        crate::hdu::HduInfo::BinaryTable {
            naxis2, tfields, ..
        } => Ok(HduInfo::TableInfo {
            column_count: *tfields,
            row_count: *naxis2,
        }),
        crate::hdu::HduInfo::RandomGroups { bitpix, naxes, .. } => {
            let image_type = super::images::ImageType::equivalent(*bitpix, bscale, bzero)?;
            Ok(HduInfo::ImageInfo {
                shape: row_major(naxes),
                image_type,
            })
        }
        crate::hdu::HduInfo::CompressedImage {
            zbitpix, znaxes, ..
        } => {
            let image_type = super::images::ImageType::equivalent(*zbitpix, bscale, bzero)?;
            Ok(HduInfo::ImageInfo {
                shape: row_major(znaxes),
                image_type,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compat::fitsfile::FitsFile;

    #[test]
    fn hdu_info_primary_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        let f = FitsFile::create(&path).open().unwrap();
        let hdu = f.primary_hdu().unwrap();
        let info = hdu.info(&f).unwrap();
        match info {
            HduInfo::ImageInfo { shape, .. } => {
                assert!(shape.is_empty());
            }
            _ => panic!("Expected ImageInfo"),
        }
    }

    #[test]
    fn hdu_read_write_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        let mut f = FitsFile::create(&path).open().unwrap();
        let hdu = f.primary_hdu().unwrap();
        hdu.write_key(&mut f, "TESTVAL", 42i64).unwrap();
        let val: i64 = hdu.read_key(&f, "TESTVAL").unwrap();
        assert_eq!(val, 42);
    }

    use crate::compat::images::{ImageDescription, ImageType};

    fn image(f: &mut FitsFile, data_type: ImageType, dims: &[usize]) -> FitsHdu {
        let desc = ImageDescription {
            data_type,
            dimensions: dims,
        };
        f.create_image("IMG", &desc).unwrap()
    }

    /// `number` and `info` are filled in when the handle is fetched, and
    /// `info` is a snapshot; the `info()` method reads the file as it is now.
    #[test]
    fn number_and_info_fields_are_filled_in() {
        let mut f = FitsFile::create_in_memory().unwrap();
        let hdu = image(&mut f, ImageType::Short, &[2, 3]);
        assert_eq!(hdu.number, 1);
        assert_eq!(
            hdu.info,
            HduInfo::ImageInfo {
                shape: vec![2, 3],
                image_type: ImageType::Short,
            }
        );
        assert_eq!(hdu.name(&f).unwrap(), "IMG");
        assert_eq!(f.primary_hdu().unwrap().name(&f).unwrap(), "");
        assert_eq!(f.hdu("IMG").unwrap(), hdu);
        assert_eq!(hdu.info(&f).unwrap(), hdu.info);
    }

    #[test]
    fn write_image_writes_a_prefix_and_refuses_too_much() {
        let mut f = FitsFile::create_in_memory().unwrap();
        let hdu = image(&mut f, ImageType::Long, &[2, 3]);
        hdu.write_image(&mut f, &[1i32, 2, 3, 4, 5, 6]).unwrap();
        hdu.write_image(&mut f, &[9i32, 8]).unwrap();
        let pixels: Vec<i32> = hdu.read_image(&f).unwrap();
        assert_eq!(pixels, [9, 8, 3, 4, 5, 6]);
        assert!(hdu.write_image(&mut f, &[0i32; 7]).is_err());
        // The file keeps its size: earlier releases replaced the data unit
        // with whatever was written.
        let len = f.data().unwrap().len();
        hdu.write_image(&mut f, &[1i32]).unwrap();
        assert_eq!(f.data().unwrap().len(), len);
    }

    /// Values are converted to the image's type through its BZERO/BSCALE, as
    /// cfitsio converts them, and a value that doesn't fit is an error.
    #[test]
    fn writes_convert_to_the_stored_type() {
        let mut f = FitsFile::create_in_memory().unwrap();
        let unsigned = image(&mut f, ImageType::UnsignedShort, &[1, 3]);
        unsigned
            .write_image(&mut f, &[0u16, 40_000, 65_535])
            .unwrap();
        let back: Vec<u16> = unsigned.read_image(&f).unwrap();
        assert_eq!(back, [0, 40_000, 65_535]);
        unsigned.write_section(&mut f, 1, 2, &[123.6f64]).unwrap();
        let back: Vec<u16> = unsigned.read_image(&f).unwrap();
        assert_eq!(back, [0, 124, 65_535]);
        assert!(unsigned.write_image(&mut f, &[70_000i32]).is_err());
        assert!(unsigned.write_image(&mut f, &[-1i32]).is_err());

        let bytes = image(&mut f, ImageType::UnsignedByte, &[1, 2]);
        assert!(bytes.write_image(&mut f, &[256u16]).is_err());
        assert!(bytes.write_image(&mut f, &[f64::NAN]).is_err());
    }

    #[test]
    fn regions_and_sections_are_bounds_checked() {
        let mut f = FitsFile::create_in_memory().unwrap();
        let hdu = image(&mut f, ImageType::Long, &[2, 3]);
        assert!(hdu.write_section(&mut f, 4, 7, &[1i32, 2, 3]).is_err());
        assert!(hdu.write_section(&mut f, 0, 3, &[1i32, 2]).is_err());
        assert!(hdu
            .write_region(&mut f, &[&(0..4), &(0..1)], &[0i32; 4])
            .is_err());
        assert!(hdu.write_region(&mut f, &[&(0..1)], &[0i32]).is_err());
        hdu.write_region(&mut f, &[&(1..3), &(1..2)], &[7i32, 8])
            .unwrap();
        let region: Vec<i32> = hdu.read_region(&f, &[&(1..3), &(1..2)]).unwrap();
        assert_eq!(region, [7, 8]);
        let pixels: Vec<i32> = hdu.read_image(&f).unwrap();
        assert_eq!(pixels, [0, 0, 0, 0, 7, 8]);
    }

    #[test]
    fn trailing_ranges_must_be_absent_axes() {
        let mut f = FitsFile::create_in_memory().unwrap();
        let hdu = image(&mut f, ImageType::Long, &[2, 3]);
        hdu.write_region(&mut f, &[&(1..3), &(1..2), &(0..1)], &[7i32, 8])
            .unwrap();
        let region: Vec<i32> = hdu
            .read_region(&f, &[&(1..3), &(1..2), &(0..1), &(0..1)])
            .unwrap();
        assert_eq!(region, [7, 8]);
        assert!(hdu
            .write_region(&mut f, &[&(1..3), &(1..2), &(1..2)], &[7i32, 8])
            .is_err());
        assert!(hdu
            .read_region::<Vec<i32>>(&f, &[&(1..3), &(1..2), &(0..2)])
            .is_err());
    }

    #[test]
    fn writing_to_a_table_is_an_error() {
        let mut f = FitsFile::create_in_memory().unwrap();
        let columns = [crate::bintable::BinaryColumnDescriptor {
            name: Some("ID".to_string()),
            repeat: 1,
            col_type: crate::bintable::BinaryColumnType::Int,
            byte_width: 4,
            tdim: None,
        }];
        let data = [crate::bintable::BinaryColumnData::Int(vec![10, 20, 30])];
        let table = crate::bintable::serialize_binary_table_hdu(&columns, &data, 3).unwrap();
        let mut bytes = f.data().unwrap().to_vec();
        bytes.extend_from_slice(&table);
        f.set_data(bytes);
        let hdu = f.hdu(1usize).unwrap();
        assert!(hdu.write_image(&mut f, &[1i32]).is_err());
        assert_eq!(hdu.read_cell_value::<i32>(&f, "ID", 1).unwrap(), 20);
        assert!(hdu.read_cell_value::<i32>(&f, "ID", 3).is_err());
    }

    #[cfg(feature = "array")]
    #[test]
    fn read_image_into_an_arrayd() {
        use ndarray::ArrayD;
        let mut f = FitsFile::create_in_memory().unwrap();
        let hdu = image(&mut f, ImageType::Long, &[2, 3]);
        hdu.write_image(&mut f, &[1i32, 2, 3, 4, 5, 6]).unwrap();
        let data: ArrayD<u32> = hdu.read_image(&f).unwrap();
        assert_eq!(data.shape(), &[2, 3]);
        assert_eq!(data[[1, 0]], 4);
        let rows: ArrayD<f32> = hdu.read_rows(&f, 1, 1).unwrap();
        assert_eq!(rows.as_slice().unwrap(), &[4.0, 5.0, 6.0]);
    }
}
