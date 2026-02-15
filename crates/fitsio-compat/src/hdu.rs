use crate::errors::{Error, Result};
use crate::fitsfile::FitsFile;
use crate::headers::{ReadsKey, WritesKey};

/// Lightweight handle to one HDU within a FITS file.
///
/// This stores only the index; the underlying data is always accessed
/// through the `FitsFile`.
#[derive(Debug, Clone)]
pub struct FitsHdu {
    pub(crate) hdu_index: usize,
}

/// Describes the kind and shape of data stored in an HDU.
#[derive(Debug, Clone, PartialEq)]
pub enum HduInfo {
    ImageInfo {
        shape: Vec<usize>,
        image_type: crate::images::ImageType,
    },
    TableInfo {
        column_count: usize,
        row_count: usize,
    },
    AnyInfo,
}

impl FitsHdu {
    /// Read a header keyword value from this HDU.
    pub fn read_key<T: ReadsKey>(&self, file: &FitsFile, name: &str) -> Result<T> {
        T::read_key(file, self, name)
    }

    /// Write a header keyword value to this HDU.
    pub fn write_key<T: WritesKey>(
        &self,
        file: &mut FitsFile,
        name: &str,
        value: &T,
    ) -> Result<()> {
        T::write_key(file, self, name, value)
    }

    /// Return information about the type and shape of data in this HDU.
    pub fn info(&self, file: &FitsFile) -> Result<HduInfo> {
        let fits_data = fitsio_pure::hdu::parse_fits(file.data())?;
        let hdu = fits_data.get(self.hdu_index).ok_or(Error::Message(format!(
            "HDU index {} out of range",
            self.hdu_index
        )))?;

        match &hdu.info {
            fitsio_pure::hdu::HduInfo::Primary { bitpix, naxes } => {
                let image_type = crate::images::ImageType::from_bitpix(*bitpix)?;
                Ok(HduInfo::ImageInfo {
                    shape: naxes.clone(),
                    image_type,
                })
            }
            fitsio_pure::hdu::HduInfo::Image { bitpix, naxes } => {
                let image_type = crate::images::ImageType::from_bitpix(*bitpix)?;
                Ok(HduInfo::ImageInfo {
                    shape: naxes.clone(),
                    image_type,
                })
            }
            fitsio_pure::hdu::HduInfo::AsciiTable {
                naxis2, tfields, ..
            } => Ok(HduInfo::TableInfo {
                column_count: *tfields,
                row_count: *naxis2,
            }),
            fitsio_pure::hdu::HduInfo::BinaryTable {
                naxis2, tfields, ..
            } => Ok(HduInfo::TableInfo {
                column_count: *tfields,
                row_count: *naxis2,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fitsfile::FitsFile;

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
        hdu.write_key(&mut f, "TESTVAL", &42i64).unwrap();
        let val: i64 = hdu.read_key(&f, "TESTVAL").unwrap();
        assert_eq!(val, 42);
    }
}
