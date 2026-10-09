use super::errors::{Error, Result};
use super::fitsfile::FitsFile;
use super::hdu::FitsHdu;
use super::sys;

/// A header value with an optional comment.
#[derive(Debug, Clone, PartialEq)]
pub struct HeaderValue<T> {
    pub value: T,
    pub comment: Option<String>,
}

/// Trait for types that can be read from a FITS header card.
///
/// Implemented, as in `fitsio`, for `i32`, `i64`, `f32`, `f64`, `bool` and
/// `String`, and for [`HeaderValue`] of each, which also returns the card's
/// comment.
pub trait ReadsKey: Sized {
    fn read_key(file: &FitsFile, hdu: &FitsHdu, name: &str) -> Result<Self>;
}

/// Trait for types that can be written to a FITS header card.
///
/// Implemented, as in `fitsio`, for every integer type from `i8` to `u64`,
/// `f32`, `f64`, `bool`, `&str` and `String`, and for each of those paired
/// with a comment as `(value, &str)` or `(value, String)`. References to any
/// of these work too, so `write_key` accepts a value or a borrow of one.
pub trait WritesKey {
    fn write_key(file: &mut FitsFile, hdu: &FitsHdu, name: &str, value: &Self) -> Result<()>;
}

/// The value and comment of the first card named `name` in `hdu`.
fn find_card(
    file: &FitsFile,
    hdu: &FitsHdu,
    name: &str,
) -> Result<(crate::value::Value, Option<String>)> {
    // cfitsio ignores blanks around the name, so a name padded to 8 bytes
    // as it sits on the card still matches.
    let name = name.trim();
    let fits_data = file.parsed()?;
    let core_hdu = fits_data
        .get(hdu.number)
        .ok_or(Error::status(sys::END_OF_FILE))?;

    // Like cfitsio, match keywords ignoring case, and refuse to read an
    // undefined value as any type.
    for card in &core_hdu.cards {
        if card.keyword_str().eq_ignore_ascii_case(name) {
            match card.value {
                Some(crate::value::Value::Undefined) => {
                    return Err(Error::status(sys::VALUE_UNDEFINED))
                }
                Some(ref v) => return Ok((v.clone(), card.comment.clone())),
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
                return Ok((value, hierarch_comment(card)));
            }
        }
    }
    Err(Error::status(sys::KEY_NO_EXIST))
}

/// The comment after the value of a `HIERARCH` card, if it has one.
fn hierarch_comment(card: &crate::header::Card) -> Option<String> {
    let text = card.comment.as_deref()?;
    let eq = text.find('=')?;
    let (_, comment) = crate::value::parse_value(text[eq + 1..].trim_start().as_bytes())?;
    comment.map(String::from)
}

/// Conversion of a card value to a Rust type, as `read_key` does it.
trait FromCardValue: Sized {
    fn from_card_value(value: crate::value::Value) -> Result<Self>;
}

impl FromCardValue for i64 {
    fn from_card_value(value: crate::value::Value) -> Result<Self> {
        match value {
            crate::value::Value::Integer(n) => Ok(n),
            _ => Err(Error::status(sys::BAD_C2D)),
        }
    }
}

impl FromCardValue for i32 {
    fn from_card_value(value: crate::value::Value) -> Result<Self> {
        let n = i64::from_card_value(value)?;
        i32::try_from(n).map_err(|_| Error::status(sys::NUM_OVERFLOW))
    }
}

impl FromCardValue for f64 {
    fn from_card_value(value: crate::value::Value) -> Result<Self> {
        match value {
            crate::value::Value::Float(f) => Ok(f),
            crate::value::Value::Integer(n) => Ok(n as f64),
            _ => Err(Error::status(sys::BAD_C2D)),
        }
    }
}

impl FromCardValue for f32 {
    fn from_card_value(value: crate::value::Value) -> Result<Self> {
        match value {
            crate::value::Value::Float(f) => Ok(f as f32),
            crate::value::Value::Integer(n) => Ok(n as f32),
            _ => Err(Error::status(sys::BAD_C2F)),
        }
    }
}

impl FromCardValue for bool {
    fn from_card_value(value: crate::value::Value) -> Result<Self> {
        match value {
            crate::value::Value::Logical(b) => Ok(b),
            _ => Err(Error::status(sys::BAD_C2D)),
        }
    }
}

impl FromCardValue for String {
    fn from_card_value(value: crate::value::Value) -> Result<Self> {
        match value {
            crate::value::Value::String(s) => Ok(s.trim().to_string()),
            _ => Err(Error::Message(format!(
                "keyword value {value:?} is not a string"
            ))),
        }
    }
}

macro_rules! reads_key_impl {
    ($($t:ty),*) => {$(
        impl ReadsKey for $t {
            fn read_key(file: &FitsFile, hdu: &FitsHdu, name: &str) -> Result<Self> {
                let (value, _) = find_card(file, hdu, name)?;
                <$t>::from_card_value(value)
            }
        }

        impl ReadsKey for HeaderValue<$t> {
            fn read_key(file: &FitsFile, hdu: &FitsHdu, name: &str) -> Result<Self> {
                let (value, comment) = find_card(file, hdu, name)?;
                Ok(HeaderValue {
                    value: <$t>::from_card_value(value)?,
                    comment,
                })
            }
        }
    )*};
}

reads_key_impl!(i32, i64, f32, f64, bool, String);

fn make_keyword(name: &str) -> [u8; 8] {
    let mut kw = [b' '; 8];
    let bytes = name.as_bytes();
    let len = bytes.len().min(8);
    kw[..len].copy_from_slice(&bytes[..len]);
    kw
}

/// Append ` / comment` to a `HIERARCH` card's text, cut to fit the card, as
/// cfitsio does.
fn add_hierarch_comment(card: &mut crate::header::Card, comment: &str) {
    if let Some(text) = card.comment.as_mut() {
        let room = crate::CARD_SIZE.saturating_sub(8 + text.len() + 3);
        if room > 0 {
            text.push_str(" / ");
            text.extend(comment.chars().take(room));
        }
    }
}

/// Set `name` to `value` in `hdu`, replacing the comment when one is given.
/// An existing card keeps its comment when `comment` is `None`.
fn write_key_to_file(
    file: &mut FitsFile,
    hdu: &FitsHdu,
    name: &str,
    value: crate::value::Value,
    comment: Option<&str>,
) -> Result<()> {
    file.check_writable()?;
    // As cfitsio does, ignore blanks around the name and write it in upper
    // case.
    let name = &name.trim().to_ascii_uppercase();
    if !valid_keyword(name) {
        return Err(Error::status(sys::BAD_KEYCHAR));
    }

    let mut fits_data = crate::hdu::parse_fits(file.data())?;
    let core_hdu = fits_data
        .hdus
        .get_mut(hdu.number)
        .ok_or(Error::status(sys::END_OF_FILE))?;

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
        // cfitsio rejects a name too long to fit as BAD_KEYCHAR too.
        let mut card = crate::header::hierarch_card(key, &value)
            .map_err(|_| Error::status(sys::BAD_KEYCHAR))?;
        if let Some(comment) = comment {
            add_hierarch_comment(&mut card, comment);
        }
        match existing {
            Some((i, _)) => core_hdu.cards[i] = card,
            None => insert_before_end(&mut core_hdu.cards, card),
        }
    } else if let Some(card) = core_hdu.cards.iter_mut().find(|c| c.keyword_str() == name) {
        card.value = Some(value);
        if let Some(comment) = comment {
            card.comment = Some(comment.to_string());
        }
    } else {
        let card = crate::header::Card {
            keyword: make_keyword(name),
            value: Some(value),
            comment: comment.map(String::from),
        };
        insert_before_end(&mut core_hdu.cards, card);
    }

    rebuild_fits_data(file, &fits_data)
}

/// Whether cfitsio accepts `name`, upper-cased, as a keyword: digits, upper
/// case letters, `-` and `_`, or for a `HIERARCH` name, any printable
/// characters but `=`.
fn valid_keyword(name: &str) -> bool {
    if crate::header::needs_hierarch(name) {
        name.bytes()
            .all(|b| (b' '..=b'~').contains(&b) && b != b'=')
    } else {
        !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
    }
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

/// Conversion of a Rust value to the card value `write_key` writes.
trait ToCardValue {
    fn to_card_value(&self, name: &str) -> Result<crate::value::Value>;
}

macro_rules! int_to_card_value {
    ($($t:ty),*) => {$(
        impl ToCardValue for $t {
            fn to_card_value(&self, name: &str) -> Result<crate::value::Value> {
                // FITS has one integer type; narrow integers widen to it.
                i64::try_from(*self)
                    .map(crate::value::Value::Integer)
                    .map_err(|_| {
                        Error::Message(format!(
                            "keyword '{name}' value {self} does not fit in a FITS integer card"
                        ))
                    })
            }
        }
    )*};
}

int_to_card_value!(i8, i16, i32, i64, u8, u16, u32, u64);

impl ToCardValue for f64 {
    fn to_card_value(&self, _name: &str) -> Result<crate::value::Value> {
        Ok(crate::value::Value::Float(*self))
    }
}

impl ToCardValue for f32 {
    fn to_card_value(&self, _name: &str) -> Result<crate::value::Value> {
        // Write the shortest decimal that reads back as this f32, so `0.1f32`
        // is written as 0.1 rather than as the f64 0.10000000149011612.
        let shortest = self.to_string().parse::<f64>().unwrap_or(f64::from(*self));
        Ok(crate::value::Value::Float(shortest))
    }
}

impl ToCardValue for bool {
    fn to_card_value(&self, _name: &str) -> Result<crate::value::Value> {
        Ok(crate::value::Value::Logical(*self))
    }
}

impl ToCardValue for str {
    fn to_card_value(&self, _name: &str) -> Result<crate::value::Value> {
        Ok(crate::value::Value::String(self.to_string()))
    }
}

impl ToCardValue for String {
    fn to_card_value(&self, name: &str) -> Result<crate::value::Value> {
        self.as_str().to_card_value(name)
    }
}

macro_rules! writes_key_impl {
    ($($t:ty),*) => {$(
        impl WritesKey for $t {
            fn write_key(file: &mut FitsFile, hdu: &FitsHdu, name: &str, value: &Self) -> Result<()> {
                let value = value.to_card_value(name)?;
                write_key_to_file(file, hdu, name, value, None)
            }
        }
    )*};
}

// `&str` itself is covered by `str` and the impl for references below.
writes_key_impl!(i8, i16, i32, i64, u8, u16, u32, u64, f32, f64, bool, String, str);

macro_rules! writes_key_with_comment_impl {
    ($($t:ty),*) => {$(
        impl WritesKey for ($t, &str) {
            fn write_key(file: &mut FitsFile, hdu: &FitsHdu, name: &str, value: &Self) -> Result<()> {
                let card_value = value.0.to_card_value(name)?;
                write_key_to_file(file, hdu, name, card_value, Some(value.1))
            }
        }

        impl WritesKey for ($t, String) {
            fn write_key(file: &mut FitsFile, hdu: &FitsHdu, name: &str, value: &Self) -> Result<()> {
                let card_value = value.0.to_card_value(name)?;
                write_key_to_file(file, hdu, name, card_value, Some(&value.1))
            }
        }
    )*};
}

writes_key_with_comment_impl!(i8, i16, i32, i64, u8, u16, u32, u64, f32, f64, bool, String, &str);

/// A reference writes what it points to, so callers written against earlier
/// releases, which passed `&value`, still compile.
impl<T: WritesKey + ?Sized> WritesKey for &T {
    fn write_key(file: &mut FitsFile, hdu: &FitsHdu, name: &str, value: &Self) -> Result<()> {
        T::write_key(file, hdu, name, value)
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

    fn card_text(f: FitsFile, keyword: &str) -> String {
        let bytes = f.into_bytes().unwrap();
        bytes[..2880]
            .chunks(80)
            .map(|c| String::from_utf8_lossy(c).trim_end().to_string())
            .find(|c| c.starts_with(keyword))
            .unwrap()
    }

    /// Every value type `fitsio` writes, passed by value as upstream callers
    /// pass it, reads back as written.
    #[test]
    fn write_key_takes_upstream_value_types_by_value() {
        let mut f = FitsFile::create_in_memory().unwrap();
        let hdu = f.primary_hdu().unwrap();
        hdu.write_key(&mut f, "I8", -8i8).unwrap();
        hdu.write_key(&mut f, "I16", -16i16).unwrap();
        hdu.write_key(&mut f, "I32", -32i32).unwrap();
        hdu.write_key(&mut f, "I64", -64i64).unwrap();
        hdu.write_key(&mut f, "U8", 8u8).unwrap();
        hdu.write_key(&mut f, "U16", 16u16).unwrap();
        hdu.write_key(&mut f, "U32", 32u32).unwrap();
        hdu.write_key(&mut f, "U64", 64u64).unwrap();
        hdu.write_key(&mut f, "F32", 0.1f32).unwrap();
        hdu.write_key(&mut f, "F64", 2.5f64).unwrap();
        hdu.write_key(&mut f, "INT", 7).unwrap();
        hdu.write_key(&mut f, "TELESCOP", "MWA").unwrap();
        hdu.write_key(&mut f, "OBSERVER", String::from("Edwin"))
            .unwrap();

        for (key, want) in [
            ("I8", -8),
            ("I16", -16),
            ("I32", -32),
            ("I64", -64),
            ("U8", 8),
            ("U16", 16),
            ("U32", 32),
            ("U64", 64),
            ("INT", 7),
        ] {
            assert_eq!(hdu.read_key::<i64>(&f, key).unwrap(), want, "{key}");
        }
        assert_eq!(hdu.read_key::<i32>(&f, "I32").unwrap(), -32);
        assert_eq!(hdu.read_key::<f32>(&f, "F32").unwrap(), 0.1f32);
        assert_eq!(hdu.read_key::<f64>(&f, "F64").unwrap(), 2.5);
        assert_eq!(hdu.read_key::<String>(&f, "TELESCOP").unwrap(), "MWA");
        assert_eq!(hdu.read_key::<String>(&f, "OBSERVER").unwrap(), "Edwin");
    }

    /// Callers written against earlier releases passed `&value`; they still
    /// compile and write the same cards.
    #[test]
    #[allow(clippy::needless_borrows_for_generic_args)]
    fn write_key_still_accepts_references() {
        let mut f = FitsFile::create_in_memory().unwrap();
        let hdu = f.primary_hdu().unwrap();
        hdu.write_key(&mut f, "EXPTIME", &42i64).unwrap();
        hdu.write_key(&mut f, "OBJECT", &"M31".to_string()).unwrap();
        hdu.write_key(&mut f, "FILTER", &"R").unwrap();
        assert_eq!(hdu.read_key::<i64>(&f, "EXPTIME").unwrap(), 42);
        assert_eq!(hdu.read_key::<String>(&f, "OBJECT").unwrap(), "M31");
        assert_eq!(hdu.read_key::<String>(&f, "FILTER").unwrap(), "R");
    }

    /// `(value, comment)` writes `KEY = value / comment`, and `HeaderValue`
    /// reads the comment back.
    #[test]
    fn comment_tuples_round_trip_through_header_value() {
        let mut f = FitsFile::create_in_memory().unwrap();
        let hdu = f.primary_hdu().unwrap();
        hdu.write_key(&mut f, "EXPTIME", (30.0f64, "seconds"))
            .unwrap();
        hdu.write_key(&mut f, "NPOLS", (4u32, String::from("polarisations")))
            .unwrap();
        hdu.write_key(&mut f, "TELESCOP", ("MWA", "telescope"))
            .unwrap();
        hdu.write_key(&mut f, "OBSERVER", (String::from("Edwin"), "who"))
            .unwrap();

        let exptime: HeaderValue<f64> = hdu.read_key(&f, "EXPTIME").unwrap();
        assert_eq!(exptime.value, 30.0);
        assert_eq!(exptime.comment.as_deref(), Some("seconds"));
        let npols: HeaderValue<i32> = hdu.read_key(&f, "NPOLS").unwrap();
        assert_eq!(
            (npols.value, npols.comment.as_deref()),
            (4, Some("polarisations"))
        );
        let telescop: HeaderValue<String> = hdu.read_key(&f, "TELESCOP").unwrap();
        assert_eq!(
            (telescop.value.as_str(), telescop.comment.as_deref()),
            ("MWA", Some("telescope"))
        );
        let observer: HeaderValue<String> = hdu.read_key(&f, "OBSERVER").unwrap();
        assert_eq!(observer.comment.as_deref(), Some("who"));
    }

    /// Rewriting a key without a comment keeps the card's comment; writing
    /// one with a comment replaces it.
    #[test]
    fn rewriting_a_key_keeps_or_replaces_its_comment() {
        let mut f = FitsFile::create_in_memory().unwrap();
        let hdu = f.primary_hdu().unwrap();
        hdu.write_key(&mut f, "GAIN", (100i64, "e-/ADU")).unwrap();
        hdu.write_key(&mut f, "GAIN", 120i64).unwrap();
        let gain: HeaderValue<i64> = hdu.read_key(&f, "GAIN").unwrap();
        assert_eq!((gain.value, gain.comment.as_deref()), (120, Some("e-/ADU")));
        hdu.write_key(&mut f, "GAIN", (140i64, "electrons per ADU"))
            .unwrap();
        let gain: HeaderValue<i64> = hdu.read_key(&f, "GAIN").unwrap();
        assert_eq!(gain.comment.as_deref(), Some("electrons per ADU"));
    }

    /// An f32 is written as the shortest decimal that reads back as it, not
    /// as its widened f64.
    #[test]
    fn f32_values_are_written_as_their_shortest_decimal() {
        let mut f = FitsFile::create_in_memory().unwrap();
        let hdu = f.primary_hdu().unwrap();
        hdu.write_key(&mut f, "SCALE", 0.1f32).unwrap();
        // The widened f64 would be written 1.000000014901161E-1.
        assert_eq!(card_text(f, "SCALE"), "SCALE   = 1.000000000000000E-1");
    }

    #[test]
    fn values_that_do_not_fit_are_errors() {
        let mut f = FitsFile::create_in_memory().unwrap();
        let hdu = f.primary_hdu().unwrap();
        assert!(hdu.write_key(&mut f, "BIG", u64::MAX).is_err());
        hdu.write_key(&mut f, "WIDE", 1i64 << 40).unwrap();
        assert!(hdu.read_key::<i32>(&f, "WIDE").is_err());
        assert_eq!(hdu.read_key::<i64>(&f, "WIDE").unwrap(), 1 << 40);
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
    fn hierarch_keys_carry_comments() {
        let mut f = FitsFile::create_in_memory().unwrap();
        let hdu = f.primary_hdu().unwrap();
        hdu.write_key(&mut f, "ESO TEL TEMP", (1.5f64, "degC"))
            .unwrap();
        let temp: HeaderValue<f64> = hdu.read_key(&f, "ESO TEL TEMP").unwrap();
        assert_eq!((temp.value, temp.comment.as_deref()), (1.5, Some("degC")));
        assert_eq!(
            header_cards(f),
            ["HIERARCH ESO TEL TEMP = 1.500000000000000E0 / degC"]
        );
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
