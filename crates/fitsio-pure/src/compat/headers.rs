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

    // Like cfitsio, match keywords ignoring case, and refuse to read an
    // undefined value as any type.
    for card in &core_hdu.cards {
        if card.keyword_str().eq_ignore_ascii_case(name) {
            match card.value {
                Some(crate::value::Value::Undefined) => {
                    return Err(Error::Message(format!(
                        "keyword '{name}' value is undefined"
                    )))
                }
                Some(ref v) => return Ok(v.clone()),
                None => {}
            }
        }
    }
    // Like cfitsio, find HIERARCH keywords by name ignoring case, with or
    // without the `HIERARCH ` prefix.
    let name = crate::header::strip_hierarch(name);
    for card in &core_hdu.cards {
        if let Some((key, value)) = crate::header::hierarch_entry(card) {
            if key.eq_ignore_ascii_case(name) {
                return Ok(value);
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

    if crate::header::needs_hierarch(name) {
        // A name a standard 8-byte keyword can't hold becomes a HIERARCH
        // card, as cfitsio writes it, instead of being truncated.
        let wanted = crate::header::strip_hierarch(name);
        let existing = core_hdu.cards.iter().enumerate().find_map(|(i, card)| {
            let (key, _) = crate::header::hierarch_entry(card)?;
            key.eq_ignore_ascii_case(wanted)
                .then(|| (i, key.to_string()))
        });
        // An existing key keeps the name as the file spells it.
        let key = existing.as_ref().map_or(wanted, |(_, key)| key.as_str());
        let card = crate::header::hierarch_card(key, &value).map_err(|_| {
            Error::Message(format!(
                "keyword '{name}' and its value don't fit on one card"
            ))
        })?;
        match existing {
            Some((i, _)) => core_hdu.cards[i] = card,
            None => insert_before_end(&mut core_hdu.cards, card),
        }
    } else if let Some(card) = core_hdu.cards.iter_mut().find(|c| c.keyword_str() == name) {
        card.value = Some(value);
    } else {
        let card = crate::header::Card {
            keyword: make_keyword(name),
            value: Some(value),
            comment: None,
        };
        insert_before_end(&mut core_hdu.cards, card);
    }

    rebuild_fits_data(file, &fits_data)
}

fn insert_before_end(cards: &mut Vec<crate::header::Card>, card: crate::header::Card) {
    match cards.iter().position(|c| c.is_end()) {
        Some(idx) => cards.insert(idx, card),
        None => cards.push(card),
    }
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

#[cfg(test)]
mod hierarch_tests {
    use super::*;
    use crate::compat::fitsfile::FitsFile;

    fn header_cards(f: FitsFile) -> Vec<String> {
        let bytes = f.into_bytes().unwrap();
        bytes[..2880]
            .chunks(80)
            .map(|c| String::from_utf8_lossy(c).trim_end().to_string())
            .filter(|c| c.starts_with("HIERARCH"))
            .collect()
    }

    // Expected cards are what fitsio 0.21 / cfitsio 4.3.1 write for the same
    // keys, except that floats keep fitsio-pure's usual formatting (cfitsio
    // writes `1.234`).
    #[test]
    fn long_keywords_are_written_as_hierarch_like_cfitsio() {
        let mut f = FitsFile::create_in_memory().unwrap();
        let hdu = f.primary_hdu().unwrap();
        String::write_key(&mut f, &hdu, "ESO DET CHIP1 ID", &"CCD-42".to_string()).unwrap();
        f64::write_key(&mut f, &hdu, "ESO TEL TEMP", &1.234).unwrap();
        i64::write_key(&mut f, &hdu, "ESO DET NDIT", &42).unwrap();
        i64::write_key(&mut f, &hdu, "LONGNAME9", &7).unwrap();
        assert_eq!(
            header_cards(f),
            [
                "HIERARCH ESO DET CHIP1 ID = 'CCD-42  '",
                "HIERARCH ESO TEL TEMP = 1.234000000000000E0",
                "HIERARCH ESO DET NDIT =     42",
                "HIERARCH LONGNAME9 =         7",
            ]
        );
    }

    #[test]
    fn hierarch_keys_are_found_by_any_spelling() {
        let mut f = FitsFile::create_in_memory().unwrap();
        let hdu = f.primary_hdu().unwrap();
        String::write_key(&mut f, &hdu, "ESO DET CHIP1 ID", &"CCD-42".to_string()).unwrap();
        f64::write_key(&mut f, &hdu, "HIERARCH ESO TEL TEMP", &1.234).unwrap();
        for name in [
            "ESO DET CHIP1 ID",
            "HIERARCH ESO DET CHIP1 ID",
            "eso det chip1 id",
        ] {
            assert_eq!(
                String::read_key(&f, &hdu, name).unwrap(),
                "CCD-42",
                "{name}"
            );
        }
        assert_eq!(f64::read_key(&f, &hdu, "ESO TEL TEMP").unwrap(), 1.234);
    }

    #[test]
    fn rewriting_a_hierarch_key_updates_it_in_place() {
        let mut f = FitsFile::create_in_memory().unwrap();
        let hdu = f.primary_hdu().unwrap();
        String::write_key(&mut f, &hdu, "ESO DET CHIP1 ID", &"CCD-42".to_string()).unwrap();
        String::write_key(&mut f, &hdu, "eso det chip1 id", &"CCD-43".to_string()).unwrap();
        assert_eq!(
            String::read_key(&f, &hdu, "ESO DET CHIP1 ID").unwrap(),
            "CCD-43"
        );
        assert_eq!(header_cards(f), ["HIERARCH ESO DET CHIP1 ID = 'CCD-43  '"]);
    }

    #[test]
    fn names_sharing_eight_characters_stay_distinct() {
        let mut f = FitsFile::create_in_memory().unwrap();
        let hdu = f.primary_hdu().unwrap();
        i64::write_key(&mut f, &hdu, "ESO DET CHIP1 NX", &2048).unwrap();
        i64::write_key(&mut f, &hdu, "ESO DET CHIP1 NY", &4096).unwrap();
        assert_eq!(i64::read_key(&f, &hdu, "ESO DET CHIP1 NX").unwrap(), 2048);
        assert_eq!(i64::read_key(&f, &hdu, "ESO DET CHIP1 NY").unwrap(), 4096);
    }

    #[test]
    fn a_hierarch_card_that_does_not_fit_is_an_error() {
        let mut f = FitsFile::create_in_memory().unwrap();
        let hdu = f.primary_hdu().unwrap();
        let value = "v".repeat(60);
        assert!(String::write_key(&mut f, &hdu, "ESO A VERY LONG KEYWORD NAME", &value).is_err());
    }

    #[test]
    fn eight_character_keywords_are_unchanged() {
        let mut f = FitsFile::create_in_memory().unwrap();
        let hdu = f.primary_hdu().unwrap();
        String::write_key(&mut f, &hdu, "OBSERVER", &"Edwin".to_string()).unwrap();
        assert_eq!(String::read_key(&f, &hdu, "OBSERVER").unwrap(), "Edwin");
        assert!(header_cards(f).is_empty());
    }
}
