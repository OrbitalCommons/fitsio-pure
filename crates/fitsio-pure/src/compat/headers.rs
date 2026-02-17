use super::errors::{Error, Result};
use super::fitsfile::FitsFile;
use super::hdu::FitsHdu;

/// A header value with an optional comment.
#[derive(Debug, Clone, PartialEq)]
pub struct HeaderValue<T> {
    pub value: T,
    pub comment: Option<String>,
}

/// Trait for types that can be read from a FITS header card.
pub trait ReadsKey: Sized {
    fn read_key(file: &FitsFile, hdu: &FitsHdu, name: &str) -> Result<Self>;
}

/// Trait for types that can be written to a FITS header card.
pub trait WritesKey {
    fn write_key(file: &mut FitsFile, hdu: &FitsHdu, name: &str, value: &Self) -> Result<()>;
}

fn find_card_value(file: &FitsFile, hdu: &FitsHdu, name: &str) -> Result<crate::value::Value> {
    let fits_data = file.parsed()?;
    let core_hdu = fits_data.get(hdu.hdu_index).ok_or(Error::Message(format!(
        "HDU index {} not found",
        hdu.hdu_index
    )))?;

    for card in &core_hdu.cards {
        if card.keyword_str() == name {
            if let Some(ref v) = card.value {
                return Ok(v.clone());
            }
        }
    }
    Err(Error::Message(format!("keyword '{name}' not found")))
}

impl ReadsKey for i64 {
    fn read_key(file: &FitsFile, hdu: &FitsHdu, name: &str) -> Result<Self> {
        match find_card_value(file, hdu, name)? {
            crate::value::Value::Integer(n) => Ok(n),
            _ => Err(Error::Message(format!(
                "keyword '{name}' is not an integer"
            ))),
        }
    }
}

impl ReadsKey for f64 {
    fn read_key(file: &FitsFile, hdu: &FitsHdu, name: &str) -> Result<Self> {
        match find_card_value(file, hdu, name)? {
            crate::value::Value::Float(f) => Ok(f),
            crate::value::Value::Integer(n) => Ok(n as f64),
            _ => Err(Error::Message(format!("keyword '{name}' is not a float"))),
        }
    }
}

impl ReadsKey for bool {
    fn read_key(file: &FitsFile, hdu: &FitsHdu, name: &str) -> Result<Self> {
        match find_card_value(file, hdu, name)? {
            crate::value::Value::Logical(b) => Ok(b),
            _ => Err(Error::Message(format!("keyword '{name}' is not a logical"))),
        }
    }
}

impl ReadsKey for String {
    fn read_key(file: &FitsFile, hdu: &FitsHdu, name: &str) -> Result<Self> {
        match find_card_value(file, hdu, name)? {
            crate::value::Value::String(s) => Ok(s.trim().to_string()),
            _ => Err(Error::Message(format!("keyword '{name}' is not a string"))),
        }
    }
}

fn make_keyword(name: &str) -> [u8; 8] {
    let mut kw = [b' '; 8];
    let bytes = name.as_bytes();
    let len = bytes.len().min(8);
    kw[..len].copy_from_slice(&bytes[..len]);
    kw
}

fn write_key_to_file(
    file: &mut FitsFile,
    hdu: &FitsHdu,
    name: &str,
    value: crate::value::Value,
) -> Result<()> {
    let mut fits_data = crate::hdu::parse_fits(file.data())?;
    let core_hdu = fits_data
        .hdus
        .get_mut(hdu.hdu_index)
        .ok_or(Error::Message(format!(
            "HDU index {} not found",
            hdu.hdu_index
        )))?;

    let keyword = make_keyword(name);
    let mut found = false;
    for card in &mut core_hdu.cards {
        if card.keyword_str() == name {
            card.value = Some(value.clone());
            found = true;
            break;
        }
    }

    if !found {
        let end_idx = core_hdu.cards.iter().position(|c| c.is_end());
        let new_card = crate::header::Card {
            keyword,
            value: Some(value),
            comment: None,
        };
        if let Some(idx) = end_idx {
            core_hdu.cards.insert(idx, new_card);
        } else {
            core_hdu.cards.push(new_card);
        }
    }

    rebuild_fits_data(file, &fits_data)
}

fn rebuild_fits_data(file: &mut FitsFile, fits_data: &crate::hdu::FitsData) -> Result<()> {
    let mut new_data = Vec::new();

    for (i, hdu) in fits_data.hdus.iter().enumerate() {
        let cards_without_end: Vec<_> = hdu.cards.iter().filter(|c| !c.is_end()).cloned().collect();
        let header_bytes = crate::header::serialize_header(&cards_without_end)?;
        new_data.extend_from_slice(&header_bytes);

        if hdu.data_len > 0 {
            let data_end = hdu.data_start + hdu.data_len;
            if data_end <= file.data().len() {
                let raw = &file.data()[hdu.data_start..data_end];
                let padded_len = crate::block::padded_byte_len(raw.len());
                new_data.extend_from_slice(raw);
                new_data.resize(new_data.len() + (padded_len - raw.len()), 0);
            }
        }

        let _ = i;
    }

    file.set_data(new_data);
    Ok(())
}

impl WritesKey for i64 {
    fn write_key(file: &mut FitsFile, hdu: &FitsHdu, name: &str, value: &Self) -> Result<()> {
        write_key_to_file(file, hdu, name, crate::value::Value::Integer(*value))
    }
}

impl WritesKey for f64 {
    fn write_key(file: &mut FitsFile, hdu: &FitsHdu, name: &str, value: &Self) -> Result<()> {
        write_key_to_file(file, hdu, name, crate::value::Value::Float(*value))
    }
}

impl WritesKey for bool {
    fn write_key(file: &mut FitsFile, hdu: &FitsHdu, name: &str, value: &Self) -> Result<()> {
        write_key_to_file(file, hdu, name, crate::value::Value::Logical(*value))
    }
}

impl WritesKey for String {
    fn write_key(file: &mut FitsFile, hdu: &FitsHdu, name: &str, value: &Self) -> Result<()> {
        write_key_to_file(file, hdu, name, crate::value::Value::String(value.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compat::fitsfile::FitsFile;

    #[test]
    fn read_write_integer_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        let mut f = FitsFile::create(&path).open().unwrap();
        let hdu = f.primary_hdu().unwrap();
        i64::write_key(&mut f, &hdu, "TESTKEY", &42).unwrap();
        let val = i64::read_key(&f, &hdu, "TESTKEY").unwrap();
        assert_eq!(val, 42);
    }

    #[test]
    fn read_write_float_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        let mut f = FitsFile::create(&path).open().unwrap();
        let hdu = f.primary_hdu().unwrap();
        f64::write_key(&mut f, &hdu, "FLTKEY", &3.125).unwrap();
        let val = f64::read_key(&f, &hdu, "FLTKEY").unwrap();
        assert!((val - 3.125).abs() < 1e-10);
    }

    #[test]
    fn read_write_bool_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        let mut f = FitsFile::create(&path).open().unwrap();
        let hdu = f.primary_hdu().unwrap();
        bool::write_key(&mut f, &hdu, "FLAG", &true).unwrap();
        let val = bool::read_key(&f, &hdu, "FLAG").unwrap();
        assert!(val);
    }

    #[test]
    fn read_write_string_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        let mut f = FitsFile::create(&path).open().unwrap();
        let hdu = f.primary_hdu().unwrap();
        String::write_key(&mut f, &hdu, "OBJECT", &"NGC 1234".to_string()).unwrap();
        let val = String::read_key(&f, &hdu, "OBJECT").unwrap();
        assert_eq!(val, "NGC 1234");
    }

    #[test]
    fn read_missing_key_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.fits");
        let f = FitsFile::create(&path).open().unwrap();
        let hdu = f.primary_hdu().unwrap();
        assert!(i64::read_key(&f, &hdu, "MISSING").is_err());
    }

    #[test]
    fn header_value_struct() {
        let hv = HeaderValue {
            value: 42i64,
            comment: Some("the answer".to_string()),
        };
        assert_eq!(hv.value, 42);
        assert_eq!(hv.comment.as_deref(), Some("the answer"));
    }
}
