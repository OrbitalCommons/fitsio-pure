use std::path::{Path, PathBuf};

use super::errors::{Error, Result};
use super::hdu::FitsHdu;
use super::images::ImageDescription;

/// Whether a file is opened for reading or writing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOpenMode {
    ReadOnly,
    ReadWrite,
}

/// An in-memory representation of an open FITS file.
#[derive(Debug)]
pub struct FitsFile {
    data: Vec<u8>,
    filename: PathBuf,
    mode: FileOpenMode,
}

/// Builder for creating a new FITS file.
pub struct NewFitsFile {
    path: PathBuf,
    overwrite: bool,
}

/// Trait for types that can identify an HDU (by index or name).
pub trait DescribesHdu {
    fn get_hdu<'a>(
        &self,
        fits_data: &'a crate::hdu::FitsData,
    ) -> Option<(usize, &'a crate::hdu::Hdu)>;
}

impl DescribesHdu for usize {
    fn get_hdu<'a>(
        &self,
        fits_data: &'a crate::hdu::FitsData,
    ) -> Option<(usize, &'a crate::hdu::Hdu)> {
        fits_data.get(*self).map(|hdu| (*self, hdu))
    }
}

impl DescribesHdu for &str {
    fn get_hdu<'a>(
        &self,
        fits_data: &'a crate::hdu::FitsData,
    ) -> Option<(usize, &'a crate::hdu::Hdu)> {
        for (i, hdu) in fits_data.iter().enumerate() {
            for card in &hdu.cards {
                if card.keyword_str() == "EXTNAME" {
                    if let Some(crate::value::Value::String(ref s)) = card.value {
                        if s.trim() == *self {
                            return Some((i, hdu));
                        }
                    }
                }
            }
        }
        None
    }
}

impl DescribesHdu for String {
    fn get_hdu<'a>(
        &self,
        fits_data: &'a crate::hdu::FitsData,
    ) -> Option<(usize, &'a crate::hdu::Hdu)> {
        self.as_str().get_hdu(fits_data)
    }
}

impl FitsFile {
    /// Open an existing FITS file in read-only mode.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let data = std::fs::read(path.as_ref())?;
        Ok(FitsFile {
            data,
            filename: path.as_ref().to_path_buf(),
            mode: FileOpenMode::ReadOnly,
        })
    }

    /// Open an existing FITS file for editing.
    pub fn edit<P: AsRef<Path>>(path: P) -> Result<Self> {
        let data = std::fs::read(path.as_ref())?;
        Ok(FitsFile {
            data,
            filename: path.as_ref().to_path_buf(),
            mode: FileOpenMode::ReadWrite,
        })
    }

    /// Return a builder for creating a new FITS file.
    pub fn create<P: AsRef<Path>>(path: P) -> NewFitsFile {
        NewFitsFile {
            path: path.as_ref().to_path_buf(),
            overwrite: false,
        }
    }

    /// Return a handle to the primary HDU (index 0).
    pub fn primary_hdu(&self) -> Result<FitsHdu> {
        Ok(FitsHdu { hdu_index: 0 })
    }

    /// Return a handle to the HDU described by `desc` (index or name).
    pub fn hdu<D: DescribesHdu>(&self, desc: D) -> Result<FitsHdu> {
        let fits_data = crate::hdu::parse_fits(&self.data)?;
        let (idx, _) = desc
            .get_hdu(&fits_data)
            .ok_or(Error::Message("HDU not found".to_string()))?;
        Ok(FitsHdu { hdu_index: idx })
    }

    /// Return the number of HDUs in this file.
    pub fn num_hdus(&self) -> Result<usize> {
        let fits_data = crate::hdu::parse_fits(&self.data)?;
        Ok(fits_data.len())
    }

    /// Return handles to all HDUs in the file.
    pub fn iter(&self) -> Result<Vec<FitsHdu>> {
        let fits_data = crate::hdu::parse_fits(&self.data)?;
        Ok((0..fits_data.len())
            .map(|i| FitsHdu { hdu_index: i })
            .collect())
    }

    /// Create a new image extension HDU with the given name and description.
    pub fn create_image(&mut self, extname: &str, desc: &ImageDescription) -> Result<FitsHdu> {
        let bitpix = desc.data_type.to_bitpix();
        let naxes = &desc.dimensions;

        let mut cards = crate::extension::build_extension_header(
            crate::extension::ExtensionType::Image,
            bitpix,
            naxes,
            0,
            1,
        )?;

        let extname_card = crate::header::Card {
            keyword: make_keyword("EXTNAME"),
            value: Some(crate::value::Value::String(extname.to_string())),
            comment: None,
        };
        cards.push(extname_card);

        let header_bytes = crate::header::serialize_header(&cards);

        let data_bytes = desc.dimensions.iter().copied().product::<usize>()
            * ((bitpix.unsigned_abs() as usize) / 8);
        let padded_data = crate::block::padded_byte_len(data_bytes);

        self.data.extend_from_slice(&header_bytes);
        self.data.resize(self.data.len() + padded_data, 0u8);

        let fits_data = crate::hdu::parse_fits(&self.data)?;
        let idx = fits_data.len() - 1;
        Ok(FitsHdu { hdu_index: idx })
    }

    /// Create a new binary table extension HDU.
    pub fn create_table(
        &mut self,
        extname: &str,
        columns: &[crate::bintable::BinaryColumnDescriptor],
    ) -> Result<FitsHdu> {
        let mut cards = crate::bintable::build_binary_table_cards(columns, 0, 0)?;

        let extname_card = crate::header::Card {
            keyword: make_keyword("EXTNAME"),
            value: Some(crate::value::Value::String(extname.to_string())),
            comment: None,
        };
        cards.push(extname_card);

        let header_bytes = crate::header::serialize_header(&cards);
        self.data.extend_from_slice(&header_bytes);

        let fits_data = crate::hdu::parse_fits(&self.data)?;
        let idx = fits_data.len() - 1;
        Ok(FitsHdu { hdu_index: idx })
    }

    /// Return a reference to the in-memory FITS bytes.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Replace the in-memory FITS bytes (used by write operations).
    pub fn set_data(&mut self, data: Vec<u8>) {
        self.data = data;
    }

    /// Flush the in-memory data to disk if opened for writing.
    pub fn flush(&self) -> Result<()> {
        if self.mode == FileOpenMode::ReadWrite {
            std::fs::write(&self.filename, &self.data)?;
        }
        Ok(())
    }

    /// Return the file path.
    pub fn filename(&self) -> &Path {
        &self.filename
    }

    /// Return the open mode.
    pub fn mode(&self) -> FileOpenMode {
        self.mode
    }
}

impl Drop for FitsFile {
    fn drop(&mut self) {
        if self.mode == FileOpenMode::ReadWrite {
            let _ = std::fs::write(&self.filename, &self.data);
        }
    }
}

impl NewFitsFile {
    /// Set whether to overwrite an existing file.
    pub fn overwrite(mut self) -> Self {
        self.overwrite = true;
        self
    }

    /// Finalize creation: write a minimal primary HDU and return an open `FitsFile`.
    pub fn open(self) -> Result<FitsFile> {
        if !self.overwrite && self.path.exists() {
            return Err(Error::Message(format!(
                "file already exists: {}",
                self.path.display()
            )));
        }

        let cards = crate::primary::build_primary_header(8, &[])?;
        let header_bytes = crate::header::serialize_header(&cards);

        std::fs::write(&self.path, &header_bytes)?;

        Ok(FitsFile {
            data: header_bytes,
            filename: self.path,
            mode: FileOpenMode::ReadWrite,
        })
    }
}

fn make_keyword(name: &str) -> [u8; 8] {
    let mut kw = [b' '; 8];
    let bytes = name.as_bytes();
    let len = bytes.len().min(8);
    kw[..len].copy_from_slice(&bytes[..len]);
    kw
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compat::images::ImageType;

    #[test]
    fn create_and_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        let f = FitsFile::create(&path).open().unwrap();
        assert_eq!(f.mode(), FileOpenMode::ReadWrite);
        assert!(f.data().len() >= 2880);
    }

    #[test]
    fn create_exists_without_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        FitsFile::create(&path).open().unwrap();
        assert!(FitsFile::create(&path).open().is_err());
    }

    #[test]
    fn create_with_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        FitsFile::create(&path).open().unwrap();
        FitsFile::create(&path).overwrite().open().unwrap();
    }

    #[test]
    fn open_readonly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        FitsFile::create(&path).open().unwrap();
        let f = FitsFile::open(&path).unwrap();
        assert_eq!(f.mode(), FileOpenMode::ReadOnly);
    }

    #[test]
    fn edit_mode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        FitsFile::create(&path).open().unwrap();
        let f = FitsFile::edit(&path).unwrap();
        assert_eq!(f.mode(), FileOpenMode::ReadWrite);
    }

    #[test]
    fn primary_hdu() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        let f = FitsFile::create(&path).open().unwrap();
        let hdu = f.primary_hdu().unwrap();
        assert_eq!(hdu.hdu_index, 0);
    }

    #[test]
    fn num_hdus() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        let f = FitsFile::create(&path).open().unwrap();
        assert_eq!(f.num_hdus().unwrap(), 1);
    }

    #[test]
    fn create_image_extension() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        let mut f = FitsFile::create(&path).open().unwrap();
        let desc = ImageDescription {
            data_type: ImageType::Float,
            dimensions: vec![10, 10],
        };
        let hdu = f.create_image("SCI", &desc).unwrap();
        assert_eq!(hdu.hdu_index, 1);
        assert_eq!(f.num_hdus().unwrap(), 2);
    }

    #[test]
    fn hdu_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        let mut f = FitsFile::create(&path).open().unwrap();
        let desc = ImageDescription {
            data_type: ImageType::Float,
            dimensions: vec![10],
        };
        f.create_image("SCI", &desc).unwrap();
        let hdu = f.hdu("SCI").unwrap();
        assert_eq!(hdu.hdu_index, 1);
    }

    #[test]
    fn hdu_by_index() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        let f = FitsFile::create(&path).open().unwrap();
        let hdu = f.hdu(0usize).unwrap();
        assert_eq!(hdu.hdu_index, 0);
    }

    #[test]
    fn hdu_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        let f = FitsFile::create(&path).open().unwrap();
        assert!(f.hdu("MISSING").is_err());
    }

    #[test]
    fn iter_hdus() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        let mut f = FitsFile::create(&path).open().unwrap();
        let desc = ImageDescription {
            data_type: ImageType::Short,
            dimensions: vec![5],
        };
        f.create_image("EXT1", &desc).unwrap();
        f.create_image("EXT2", &desc).unwrap();
        let hdus = f.iter().unwrap();
        assert_eq!(hdus.len(), 3);
    }
}
