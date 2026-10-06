//! In-place header updates: only header bytes change, and the result parses.

use std::io::Cursor;

use fitsio_pure::edit::{update_card, update_card_growing, update_card_in_file, CardUpdate};
use fitsio_pure::hdu::{parse_fits, FitsData};
use fitsio_pure::header::{serialize_header, Card};
use fitsio_pure::image::{read_image_data, ImageData};
use fitsio_pure::value::Value;
use fitsio_pure::{Error, BLOCK_SIZE, CARDS_PER_BLOCK};

fn kw(name: &str) -> [u8; 8] {
    let mut k = [b' '; 8];
    k[..name.len()].copy_from_slice(name.as_bytes());
    k
}

fn card(name: &str, value: Value) -> Card {
    Card {
        keyword: kw(name),
        value: Some(value),
        comment: None,
    }
}

fn commentary(name: &str, text: &str) -> Card {
    Card {
        keyword: kw(name),
        value: None,
        comment: Some(text.into()),
    }
}

fn data_unit(values: &[i16]) -> Vec<u8> {
    let mut data: Vec<u8> = values.iter().flat_map(|v| v.to_be_bytes()).collect();
    data.resize(data.len().div_ceil(BLOCK_SIZE) * BLOCK_SIZE, 0);
    data
}

/// A 3x2 primary image with `extra` cards after the mandatory ones, then a
/// 2x2 image extension.
fn file_with(extra: Vec<Card>) -> Vec<u8> {
    let mut cards = vec![
        card("SIMPLE", Value::Logical(true)),
        card("BITPIX", Value::Integer(16)),
        card("NAXIS", Value::Integer(2)),
        card("NAXIS1", Value::Integer(3)),
        card("NAXIS2", Value::Integer(2)),
        card("EXTEND", Value::Logical(true)),
    ];
    cards.extend(extra);
    let mut bytes = serialize_header(&cards).unwrap();
    bytes.extend(data_unit(&[1, 2, 3, -4, -5, -6]));
    bytes.extend(
        serialize_header(&[
            card("XTENSION", Value::String("IMAGE".into())),
            card("BITPIX", Value::Integer(16)),
            card("NAXIS", Value::Integer(2)),
            card("NAXIS1", Value::Integer(2)),
            card("NAXIS2", Value::Integer(2)),
            card("PCOUNT", Value::Integer(0)),
            card("GCOUNT", Value::Integer(1)),
        ])
        .unwrap(),
    );
    bytes.extend(data_unit(&[7, 8, 9, 10]));
    bytes
}

fn sample() -> Vec<u8> {
    file_with(vec![
        card("OBJECT", Value::String("M31".into())),
        card("EXPTIME", Value::Float(1.5)),
        Card {
            keyword: kw("FILTER"),
            value: Some(Value::Undefined),
            comment: Some("no filter".into()),
        },
    ])
}

fn value_of(fits: &FitsData, hdu: usize, name: &str) -> Option<Value> {
    fits.hdus[hdu]
        .cards
        .iter()
        .find(|c| c.keyword_str() == name)
        .and_then(|c| c.value.clone())
}

/// The bytes of every data unit, padding included.
fn data_units(bytes: &[u8]) -> Vec<Vec<u8>> {
    let fits = parse_fits(bytes).unwrap();
    fits.iter()
        .map(|h| {
            bytes[h.data_start..h.data_start + h.data_len.div_ceil(BLOCK_SIZE) * BLOCK_SIZE]
                .to_vec()
        })
        .collect()
}

fn hash(bytes: &[u8]) -> u64 {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

fn pixels(bytes: &[u8]) -> Vec<ImageData> {
    let fits = parse_fits(bytes).unwrap();
    fits.iter()
        .map(|h| read_image_data(bytes, h).unwrap())
        .collect()
}

#[test]
fn replace_and_insert_change_only_header_bytes() {
    let before = sample();
    let data_before: Vec<u64> = data_units(&before).iter().map(|d| hash(d)).collect();
    let mut file = Cursor::new(before.clone());

    let object = card("OBJECT", Value::String("Kite Cluster".into()));
    assert_eq!(
        update_card(&mut file, 0, &object).unwrap(),
        CardUpdate::Replaced
    );
    let gain = Card {
        comment: Some("sensor gain".into()),
        ..card("GAIN", Value::Integer(160))
    };
    assert_eq!(
        update_card(&mut file, 0, &gain).unwrap(),
        CardUpdate::Inserted
    );
    let filter = card("FILTER", Value::String("Ha".into()));
    assert_eq!(
        update_card(&mut file, 0, &filter).unwrap(),
        CardUpdate::Replaced
    );
    let ext = card("EXTNAME", Value::String("SCI".into()));
    assert_eq!(
        update_card(&mut file, 1, &ext).unwrap(),
        CardUpdate::Inserted
    );

    let after = file.into_inner();
    assert_eq!(after.len(), before.len());
    let data_after: Vec<u64> = data_units(&after).iter().map(|d| hash(d)).collect();
    assert_eq!(data_after, data_before);
    assert_eq!(pixels(&after), pixels(&before));

    let fits = parse_fits(&after).unwrap();
    assert_eq!(
        value_of(&fits, 0, "OBJECT"),
        Some(Value::String("Kite Cluster".into()))
    );
    assert_eq!(value_of(&fits, 0, "GAIN"), Some(Value::Integer(160)));
    assert_eq!(value_of(&fits, 0, "EXPTIME"), Some(Value::Float(1.5)));
    assert_eq!(
        value_of(&fits, 0, "FILTER"),
        Some(Value::String("Ha".into()))
    );
    assert_eq!(
        value_of(&fits, 1, "EXTNAME"),
        Some(Value::String("SCI".into()))
    );
    let gain = fits.hdus[0]
        .cards
        .iter()
        .find(|c| c.keyword_str() == "GAIN");
    assert_eq!(gain.unwrap().comment.as_deref(), Some("sensor gain"));
    // GAIN went where END was, END right after it.
    let names: Vec<&str> = fits.hdus[0].cards.iter().map(|c| c.keyword_str()).collect();
    assert_eq!(&names[names.len() - 2..], ["GAIN", "END"]);
}

#[test]
fn matches_keywords_ignoring_case() {
    let mut file = Cursor::new(sample());
    let object = card("object", Value::String("M33".into()));
    assert_eq!(
        update_card(&mut file, 0, &object).unwrap(),
        CardUpdate::Replaced
    );
}

#[test]
fn commentary_cards_are_always_inserted() {
    let mut file = Cursor::new(sample());
    let history = commentary("HISTORY", "calibrated");
    assert_eq!(
        update_card(&mut file, 0, &history).unwrap(),
        CardUpdate::Inserted
    );
    assert_eq!(
        update_card(&mut file, 0, &history).unwrap(),
        CardUpdate::Inserted
    );
    let fits = parse_fits(file.get_ref()).unwrap();
    let count = fits.hdus[0]
        .cards
        .iter()
        .filter(|c| c.keyword_str() == "HISTORY")
        .count();
    assert_eq!(count, 2);
}

#[test]
fn structural_and_unwritable_cards_are_refused() {
    let before = sample();
    let mut file = Cursor::new(before.clone());
    for name in [
        "SIMPLE", "BITPIX", "NAXIS", "NAXIS1", "NAXIS12", "END", "BZERO", "BSCALE", "BLANK",
        "PCOUNT", "GCOUNT", "GROUPS", "TFIELDS", "TFORM3", "TTYPE", "CHECKSUM", "DATASUM",
        "XTENSION", "ZNAXIS2", "ZIMAGE", "CONTINUE", "bitpix",
    ] {
        let result = update_card(&mut file, 0, &card(name, Value::Integer(1)));
        if name == "TTYPE" {
            // Not indexed, so an ordinary keyword.
            assert!(result.is_ok(), "{name}");
            file = Cursor::new(before.clone());
        } else {
            assert!(result.is_err(), "{name} was accepted");
        }
    }
    let long = card("OBSERVER", Value::String("x".repeat(69)));
    assert!(matches!(
        update_card(&mut file, 0, &long),
        Err(Error::InvalidValue)
    ));
    let quotes = card("OBSERVER", Value::String("'".repeat(35)));
    assert!(update_card(&mut file, 0, &quotes).is_err());
    let fits_68 = card("OBSERVER", Value::String("y".repeat(68)));
    let no_value = Card {
        value: None,
        ..card("OBSERVER", Value::Integer(0))
    };
    assert!(update_card(&mut file, 0, &no_value).is_err());
    assert!(update_card(&mut file, 2, &card("OBSERVER", Value::Integer(1))).is_err());
    assert_eq!(file.get_ref(), &before, "a refused update changed the file");

    assert_eq!(
        update_card(&mut file, 0, &fits_68).unwrap(),
        CardUpdate::Inserted
    );
    let fits = parse_fits(file.get_ref()).unwrap();
    assert_eq!(
        value_of(&fits, 0, "OBSERVER"),
        Some(Value::String("y".repeat(68)))
    );
}

#[test]
fn long_string_cards_are_not_replaced() {
    let before = file_with(vec![card("NOTE", Value::String("z".repeat(150)))]);
    let mut file = Cursor::new(before.clone());
    let result = update_card(&mut file, 0, &card("NOTE", Value::String("short".into())));
    assert!(result.is_err());
    assert_eq!(file.into_inner(), before);
}

/// A primary header whose END is the last card of its block.
fn full_header_file() -> Vec<u8> {
    let extra: Vec<Card> = (0..CARDS_PER_BLOCK - 7)
        .map(|i| card(&format!("KEY{i}"), Value::Integer(i as i64)))
        .collect();
    let bytes = file_with(extra);
    let fits = parse_fits(&bytes).unwrap();
    assert_eq!(fits.primary().data_start, BLOCK_SIZE);
    assert_eq!(fits.primary().cards.len(), CARDS_PER_BLOCK);
    bytes
}

#[test]
fn full_header_is_an_error_by_default() {
    let before = full_header_file();
    let mut file = Cursor::new(before.clone());
    let gain = card("GAIN", Value::Integer(160));
    assert!(update_card(&mut file, 0, &gain).is_err());
    assert_eq!(file.get_ref(), &before);
    // Replacing still works in a full header.
    let key = card("KEY3", Value::Integer(-3));
    assert_eq!(
        update_card(&mut file, 0, &key).unwrap(),
        CardUpdate::Replaced
    );
}

#[test]
fn growing_moves_the_data_unchanged() {
    let before = full_header_file();
    let data_before = data_units(&before);
    let mut file = Cursor::new(before.clone());
    let gain = card("GAIN", Value::Float(2.5));
    assert_eq!(
        update_card_growing(&mut file, 0, &gain).unwrap(),
        CardUpdate::Grown
    );
    let after = file.into_inner();
    assert_eq!(after.len(), before.len() + BLOCK_SIZE);
    assert_eq!(data_units(&after), data_before);
    assert_eq!(pixels(&after), pixels(&before));
    let fits = parse_fits(&after).unwrap();
    assert_eq!(fits.primary().data_start, 2 * BLOCK_SIZE);
    assert_eq!(value_of(&fits, 0, "GAIN"), Some(Value::Float(2.5)));
    assert_eq!(value_of(&fits, 0, "KEY27"), Some(Value::Integer(27)));
    // Growing only happens when needed.
    let mut file = Cursor::new(sample());
    assert_eq!(
        update_card_growing(&mut file, 0, &gain).unwrap(),
        CardUpdate::Inserted
    );
}

#[test]
fn updates_a_file_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("frame.fits");
    std::fs::write(&path, sample()).unwrap();
    let card = card("OBJECT", Value::String("NGC 6819".into()));
    assert_eq!(
        update_card_in_file(&path, 0, &card).unwrap(),
        CardUpdate::Replaced
    );
    let fits = parse_fits(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        value_of(&fits, 0, "OBJECT"),
        Some(Value::String("NGC 6819".into()))
    );
}
