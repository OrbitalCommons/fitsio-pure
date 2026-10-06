//! In-place header updates by `fitsio_pure::edit` on files cfitsio wrote, read
//! back by cfitsio: the new values must read correctly and the pixels and data
//! bytes must be unchanged.

use fitsio::images::{ImageDescription, ImageType};
use fitsio::FitsFile as CFits;

use fitsio_pure::edit::{update_card_growing, update_card_in_file, CardUpdate};
use fitsio_pure::hdu::parse_fits;
use fitsio_pure::header::Card;
use fitsio_pure::value::Value;
use fitsio_pure::{BLOCK_SIZE, CARDS_PER_BLOCK};

fn card(name: &str, value: Value) -> Card {
    let mut keyword = [b' '; 8];
    keyword[..name.len()].copy_from_slice(name.as_bytes());
    Card {
        keyword,
        value: Some(value),
        comment: Some("set in place".into()),
    }
}

fn pixels() -> Vec<f32> {
    (0..600).map(|i| i as f32 * 0.25 - 7.0).collect()
}

/// A cfitsio file with an empty primary and a 30x20 f32 `SCI` image carrying
/// a string, a float and an integer key.
fn cfitsio_file(path: &std::path::Path) {
    let mut fptr = CFits::create(path).open().unwrap();
    let desc = ImageDescription {
        data_type: ImageType::Float,
        dimensions: &[20, 30],
    };
    let hdu = fptr.create_image("SCI", &desc).unwrap();
    hdu.write_image(&mut fptr, &pixels()).unwrap();
    hdu.write_key(&mut fptr, "OBJECT", "M31").unwrap();
    hdu.write_key(&mut fptr, "EXPTIME", 1.5f64).unwrap();
    hdu.write_key(&mut fptr, "GAIN", 100i64).unwrap();
}

/// The data unit bytes (padding included) of every HDU.
fn data_units(bytes: &[u8]) -> Vec<Vec<u8>> {
    parse_fits(bytes)
        .unwrap()
        .iter()
        .map(|h| {
            bytes[h.data_start..h.data_start + h.data_len.div_ceil(BLOCK_SIZE) * BLOCK_SIZE]
                .to_vec()
        })
        .collect()
}

#[test]
fn cfitsio_reads_cards_updated_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("frame.fits");
    cfitsio_file(&path);
    let before = std::fs::read(&path).unwrap();

    let updates = [
        (
            card("OBJECT", Value::String("Kite Cluster".into())),
            CardUpdate::Replaced,
        ),
        (card("EXPTIME", Value::Float(-0.125)), CardUpdate::Replaced),
        (card("GAIN", Value::Integer(-160)), CardUpdate::Replaced),
        (
            card("OBSERVER", Value::String("O'Brien".into())),
            CardUpdate::Inserted,
        ),
        (card("CCDTEMP", Value::Float(-12.75)), CardUpdate::Inserted),
        (card("BINNING", Value::Integer(2)), CardUpdate::Inserted),
    ];
    for (card, expected) in &updates {
        assert_eq!(update_card_in_file(&path, 1, card).unwrap(), *expected);
    }

    let after = std::fs::read(&path).unwrap();
    assert_eq!(after.len(), before.len());
    assert_eq!(data_units(&after), data_units(&before));

    let mut fptr = CFits::open(&path).unwrap();
    let hdu = fptr.hdu("SCI").unwrap();
    let object: String = hdu.read_key(&mut fptr, "OBJECT").unwrap();
    let exptime: f64 = hdu.read_key(&mut fptr, "EXPTIME").unwrap();
    let gain: i64 = hdu.read_key(&mut fptr, "GAIN").unwrap();
    let observer: String = hdu.read_key(&mut fptr, "OBSERVER").unwrap();
    let ccdtemp: f64 = hdu.read_key(&mut fptr, "CCDTEMP").unwrap();
    let binning: i64 = hdu.read_key(&mut fptr, "BINNING").unwrap();
    assert_eq!(object, "Kite Cluster");
    assert_eq!(exptime, -0.125);
    assert_eq!(gain, -160);
    assert_eq!(observer, "O'Brien");
    assert_eq!(ccdtemp, -12.75);
    assert_eq!(binning, 2);
    let read: Vec<f32> = hdu.read_image(&mut fptr).unwrap();
    assert_eq!(read, pixels());
}

#[test]
fn cfitsio_reads_a_header_grown_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("full.fits");
    cfitsio_file(&path);

    // Fill the SCI header so END is its last card.
    let used = parse_fits(&std::fs::read(&path).unwrap()).unwrap().hdus[1]
        .cards
        .len();
    {
        let mut fptr = CFits::edit(&path).unwrap();
        let hdu = fptr.hdu("SCI").unwrap();
        for i in 0..CARDS_PER_BLOCK - used {
            hdu.write_key(&mut fptr, &format!("FILL{i}"), i as i64)
                .unwrap();
        }
    }
    let before = std::fs::read(&path).unwrap();
    let sci = &parse_fits(&before).unwrap().hdus[1];
    assert_eq!(sci.cards.len(), CARDS_PER_BLOCK);
    assert_eq!(sci.data_start - sci.header_start, BLOCK_SIZE);

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    let filter = card("FILTER", Value::String("Ha".into()));
    assert_eq!(
        update_card_growing(&mut file, 1, &filter).unwrap(),
        CardUpdate::Grown
    );
    drop(file);

    let after = std::fs::read(&path).unwrap();
    assert_eq!(after.len(), before.len() + BLOCK_SIZE);
    assert_eq!(data_units(&after), data_units(&before));

    let mut fptr = CFits::open(&path).unwrap();
    let hdu = fptr.hdu("SCI").unwrap();
    let filter: String = hdu.read_key(&mut fptr, "FILTER").unwrap();
    assert_eq!(filter, "Ha");
    let object: String = hdu.read_key(&mut fptr, "OBJECT").unwrap();
    assert_eq!(object, "M31");
    let read: Vec<f32> = hdu.read_image(&mut fptr).unwrap();
    assert_eq!(read, pixels());
}
