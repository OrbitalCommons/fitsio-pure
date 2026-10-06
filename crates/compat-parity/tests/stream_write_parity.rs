//! cfitsio reads files written by `fitsio-pure`'s streaming `ImageWriter`
//! through an `AtomicFile`: every HDU's shape and pixels must match exactly.

use fitsio::hdu::HduInfo;
use fitsio::images::ReadImage as CReadImage;
use fitsio::FitsFile as CFits;

use fitsio_pure::header::Card;
use fitsio_pure::image_writer::{ImageWriter, Sample};
use fitsio_pure::io::AtomicFile;
use fitsio_pure::value::Value;

fn extname(name: &str) -> Card {
    Card {
        keyword: *b"EXTNAME ",
        value: Some(Value::String(name.into())),
        comment: None,
    }
}

/// Stream `data` as an image extension in pieces of `piece` samples.
fn extension<T: Sample>(
    file: AtomicFile,
    bitpix: i64,
    naxes: &[usize],
    name: &str,
    data: &[T],
    piece: usize,
) -> AtomicFile {
    let mut w = ImageWriter::with_chunk_size(file, &ext_cards(bitpix, naxes, name), 4096).unwrap();
    for chunk in data.chunks(piece) {
        w.write_samples(chunk).unwrap();
    }
    w.finish().unwrap()
}

fn ext_cards(bitpix: i64, naxes: &[usize], name: &str) -> Vec<Card> {
    let mut cards = fitsio_pure::extension::build_extension_header(
        fitsio_pure::extension::ExtensionType::Image,
        bitpix,
        naxes,
        0,
        1,
    )
    .unwrap();
    cards.push(extname(name));
    cards
}

fn check<T>(fptr: &mut CFits, name: &str, naxes: &[usize], expected: &[T])
where
    T: PartialEq + std::fmt::Debug,
    Vec<T>: CReadImage,
{
    let hdu = fptr.hdu(name).unwrap();
    match &hdu.info {
        HduInfo::ImageInfo { shape, .. } => {
            let fits_order: Vec<usize> = shape.iter().rev().copied().collect();
            assert_eq!(fits_order, naxes, "{name} shape");
        }
        other => panic!("{name}: expected an image, got {other:?}"),
    }
    let read: Vec<T> = hdu.read_image(fptr).unwrap();
    assert_eq!(read, expected, "{name} pixels");
}

#[test]
fn cfitsio_reads_streamed_multi_hdu_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("streamed.fits");

    let p_naxes = [7usize, 5];
    let primary: Vec<f32> = (0..35).map(|i| i as f32 * -1.25 + 0.5).collect();
    let u8_naxes = [5usize, 3, 4];
    let u8_data: Vec<u8> = (0..60).map(|i| (i * 13) as u8).collect();
    let i16_naxes = [3usize, 11];
    let i16_data: Vec<i16> = (0..33).map(|i| (i as i16 - 16) * 1021).collect();
    let i32_naxes = [9usize, 2];
    let i32_data: Vec<i32> = (0..18).map(|i| (i - 9) * 123_456_789 / 7).collect();
    let i64_naxes = [4usize, 3, 2];
    let i64_data: Vec<i64> = (0..24).map(|i| (i as i64 - 12) << 40).collect();
    let f64_naxes = [1000usize, 3];
    let f64_data: Vec<f64> = (0..3000).map(|i| i as f64 / 7.0 - 100.0).collect();

    let mut w = ImageWriter::primary(AtomicFile::new(&path).unwrap(), -32, &p_naxes, &[]).unwrap();
    w.write_samples(&primary[..3]).unwrap();
    w.write_samples(&primary[3..]).unwrap();
    let file = w.finish().unwrap();
    let file = extension(file, 8, &u8_naxes, "U8", &u8_data, 7);
    let file = extension(file, 16, &i16_naxes, "I16", &i16_data, 5);
    let file = extension(file, 32, &i32_naxes, "I32", &i32_data, 4);
    let file = extension(file, 64, &i64_naxes, "I64", &i64_data, 3);
    let file = extension(file, -64, &f64_naxes, "F64", &f64_data, 999);
    file.commit().unwrap();

    let mut fptr = CFits::open(&path).unwrap();
    let hdu = fptr.primary_hdu().unwrap();
    let read: Vec<f32> = hdu.read_image(&mut fptr).unwrap();
    assert_eq!(read, primary);
    check(&mut fptr, "U8", &u8_naxes, &u8_data);
    check(&mut fptr, "I16", &i16_naxes, &i16_data);
    check(&mut fptr, "I32", &i32_naxes, &i32_data);
    check(&mut fptr, "I64", &i64_naxes, &i64_data);
    check(&mut fptr, "F64", &f64_naxes, &f64_data);
}
