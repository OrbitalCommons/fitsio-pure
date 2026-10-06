//! The streaming reader against the in-memory parser: every file in the
//! fits-test-cases corpus must stream to the same HDUs and pixels.

use std::cell::Cell;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use fitsio_pure::hdu::{parse_fits, Hdu, HduInfo};
use fitsio_pure::header::{serialize_header, Card};
use fitsio_pure::image::{
    build_image_hdu, read_image_data, read_image_data_into_f32, read_image_data_into_f64,
    serialize_image, ImageData,
};
use fitsio_pure::primary::build_primary_header;
use fitsio_pure::stream::{read_header, read_primary_header, FitsReader};
use fitsio_pure::value::Value;
use fitsio_pure::{Error, BLOCK_SIZE};

fn corpus_files() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fits-test-cases");
    let mut out = Vec::new();
    if dir.is_dir() {
        collect(&dir, &mut out);
    }
    out.sort();
    out
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n != ".git") {
                collect(&path, out);
            }
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| matches!(e, "fits" | "fit" | "metafits" | "uvfits"))
        {
            out.push(path);
        }
    }
}

fn assert_same_hdu(streamed: &Hdu, parsed: &Hdu, what: &str) {
    assert_eq!(streamed.cards, parsed.cards, "{what}: cards");
    assert_eq!(streamed.info, parsed.info, "{what}: info");
    assert_eq!(
        streamed.header_start, parsed.header_start,
        "{what}: header_start"
    );
    assert_eq!(streamed.data_start, parsed.data_start, "{what}: data_start");
    assert_eq!(streamed.data_len, parsed.data_len, "{what}: data_len");
}

/// Bitwise pixel equality, so NaN pixels compare equal.
fn assert_same_pixels(got: &ImageData, want: &ImageData, what: &str) {
    assert_eq!(
        core::mem::discriminant(got),
        core::mem::discriminant(want),
        "{what}: pixel type"
    );
    assert!(
        serialize_image(got) == serialize_image(want),
        "{what}: pixels differ"
    );
}

fn is_image(hdu: &Hdu) -> bool {
    matches!(
        hdu.info,
        HduInfo::Primary { .. } | HduInfo::Image { .. } | HduInfo::CompressedImage { .. }
    )
}

/// A `Read` that only implements `Read`, so the reader takes its
/// read-and-discard path.
struct ReadOnly<R>(R);

impl<R: Read> Read for ReadOnly<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

/// Counts bytes read and the largest single read requested.
struct Counting<R> {
    inner: R,
    read: Rc<Cell<u64>>,
    largest: Rc<Cell<usize>>,
}

impl<R> Counting<R> {
    fn new(inner: R) -> (Self, Rc<Cell<u64>>, Rc<Cell<usize>>) {
        let read = Rc::new(Cell::new(0));
        let largest = Rc::new(Cell::new(0));
        (
            Self {
                inner,
                read: read.clone(),
                largest: largest.clone(),
            },
            read,
            largest,
        )
    }
}

impl<R: Read> Read for Counting<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.largest.set(self.largest.get().max(buf.len()));
        let n = self.inner.read(buf)?;
        self.read.set(self.read.get() + n as u64);
        Ok(n)
    }
}

/// Streams every HDU, decoding images, and checks each against `parse_fits`.
/// Returns (HDUs, images decoded).
fn check_stream<R: Read>(mut reader: FitsReader<R>, bytes: &[u8], name: &str) -> (usize, usize) {
    let parsed = parse_fits(bytes).unwrap();
    let mut index = 0;
    let mut images = 0;
    while let Some(hdu) = reader.next_hdu().unwrap() {
        let what = format!("{name} HDU {index}");
        let expected = parsed
            .get(index)
            .unwrap_or_else(|| panic!("{what}: extra HDU"));
        assert_same_hdu(hdu, expected, &what);
        // Decode every other image so the skip path is exercised too.
        if is_image(expected) && index % 2 == 0 {
            if let Ok(want) = read_image_data(bytes, expected) {
                assert_same_pixels(&reader.read_image().unwrap(), &want, &what);
                images += 1;
            }
        }
        index += 1;
    }
    assert_eq!(index, parsed.len(), "{name}: HDU count");
    (index, images)
}

#[test]
fn corpus_streams_like_parse_fits() {
    let files = corpus_files();
    if files.is_empty() {
        eprintln!("Skipping: fits-test-cases not checked out");
        return;
    }
    let (mut files_ok, mut hdus, mut images, mut errors) = (0, 0, 0, 0);
    for path in &files {
        let name = path.display().to_string();
        let bytes = std::fs::read(path).unwrap();
        if parse_fits(&bytes).is_err() {
            // The streaming reader must fail on the same input somewhere.
            let mut reader = FitsReader::from_slice(&bytes);
            let failed = loop {
                match reader.next_hdu() {
                    Ok(Some(_)) => {}
                    Ok(None) => break false,
                    Err(_) => break true,
                }
            };
            assert!(failed, "{name}: parse_fits fails but streaming succeeds");
            errors += 1;
            continue;
        }
        let (h, i) = check_stream(FitsReader::open(path).unwrap(), &bytes, &name);
        check_stream(FitsReader::from_slice(&bytes), &bytes, &name);
        check_stream(FitsReader::new(ReadOnly(&bytes[..])), &bytes, &name);
        files_ok += 1;
        hdus += h;
        images += i;
    }
    eprintln!("streamed {files_ok} files, {hdus} HDUs, {images} images; {errors} rejected");
    assert!(files_ok > 50, "only {files_ok} corpus files streamed");
}

#[test]
fn corpus_every_image_decodes_identically() {
    for path in corpus_files() {
        let bytes = std::fs::read(&path).unwrap();
        let Ok(parsed) = parse_fits(&bytes) else {
            continue;
        };
        for (index, expected) in parsed.iter().enumerate() {
            if !is_image(expected) {
                continue;
            }
            let what = format!("{} HDU {index}", path.display());
            let mut reader = FitsReader::new(ReadOnly(&bytes[..]));
            for _ in 0..=index {
                reader.next_hdu().unwrap().unwrap();
            }
            match read_image_data(&bytes, expected) {
                Ok(want) => assert_same_pixels(&reader.read_image().unwrap(), &want, &what),
                Err(_) => assert!(reader.read_image().is_err(), "{what}"),
            }
            if matches!(expected.info, HduInfo::CompressedImage { .. }) {
                continue;
            }
            let n = match &expected.info {
                HduInfo::Primary { naxes, .. } | HduInfo::Image { naxes, .. } => {
                    if naxes.is_empty() {
                        0
                    } else {
                        naxes.iter().product()
                    }
                }
                _ => unreachable!(),
            };
            let mut want = vec![0f32; n];
            read_image_data_into_f32(&bytes, expected, &mut want).unwrap();
            let mut got = vec![0f32; n];
            let mut reader = FitsReader::from_slice(&bytes);
            for _ in 0..=index {
                reader.next_hdu().unwrap().unwrap();
            }
            reader.read_image_into_f32(&mut got).unwrap();
            assert_eq!(
                got.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                want.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                "{what}: f32"
            );
        }
    }
}

fn card(keyword: &str, value: Value) -> Card {
    let mut kw = [b' '; 8];
    kw[..keyword.len()].copy_from_slice(keyword.as_bytes());
    Card {
        keyword: kw,
        value: Some(value),
        comment: None,
    }
}

/// A primary image of `width` x `height` f64 pixels followed by an image
/// extension of 16-bit pixels.
fn two_image_file(width: usize, height: usize) -> Vec<u8> {
    let pixels: Vec<f64> = (0..width * height).map(|i| i as f64 * 0.5).collect();
    let mut bytes = build_image_hdu(-64, &[width, height], &ImageData::F64(pixels)).unwrap();
    let ext_cards = vec![
        card("XTENSION", Value::String("IMAGE".into())),
        card("BITPIX", Value::Integer(16)),
        card("NAXIS", Value::Integer(2)),
        card("NAXIS1", Value::Integer(3)),
        card("NAXIS2", Value::Integer(2)),
        card("PCOUNT", Value::Integer(0)),
        card("GCOUNT", Value::Integer(1)),
    ];
    bytes.extend(serialize_header(&ext_cards).unwrap());
    let mut data: Vec<u8> = [1i16, -2, 3, -4, 5, -6]
        .iter()
        .flat_map(|v| v.to_be_bytes())
        .collect();
    data.resize(BLOCK_SIZE, 0);
    bytes.extend(data);
    bytes
}

#[test]
fn header_only_read_stops_at_end() {
    let bytes = two_image_file(100, 80);
    let header_len = parse_fits(&bytes).unwrap().primary().data_start as u64;

    let (counting, read, _) = Counting::new(&bytes[..]);
    let hdu = read_primary_header(counting).unwrap();
    assert_eq!(read.get(), header_len);
    assert_eq!(
        hdu.info,
        HduInfo::Primary {
            bitpix: -64,
            naxes: vec![100, 80]
        }
    );
    assert_eq!(read_header(&bytes[..]).unwrap(), hdu.cards);
}

#[test]
fn decode_reads_in_bounded_chunks_and_stops_before_padding() {
    // 3 MB of f64 pixels, not a whole number of blocks.
    let (width, height) = (701, 535);
    let bytes = two_image_file(width, height);
    let parsed = parse_fits(&bytes).unwrap();
    let primary = parsed.primary();
    assert_ne!(primary.data_len % BLOCK_SIZE, 0);

    let (counting, read, largest) = Counting::new(&bytes[..]);
    let mut reader = FitsReader::new(counting);
    reader.next_hdu().unwrap();
    let image = reader.read_image().unwrap();
    assert_eq!(image, read_image_data(&bytes, primary).unwrap());
    assert_eq!(read.get(), (primary.data_start + primary.data_len) as u64);
    assert!(
        largest.get() <= 1 << 20,
        "read {} bytes at once",
        largest.get()
    );

    // The next HDU is still reachable after a decode that left the padding.
    let ext = reader.next_hdu().unwrap().unwrap();
    assert_eq!(
        ext.info,
        HduInfo::Image {
            bitpix: 16,
            naxes: vec![3, 2]
        }
    );
    assert_eq!(
        reader.read_image().unwrap(),
        ImageData::I16(vec![1, -2, 3, -4, 5, -6])
    );
    assert!(reader.next_hdu().unwrap().is_none());
}

#[test]
fn skipping_reads_nothing_on_a_seekable_reader() {
    let bytes = two_image_file(701, 535);
    let parsed = parse_fits(&bytes).unwrap();
    let read = Rc::new(Cell::new(0));
    struct SeekCounting<'a>(Cursor<&'a [u8]>, Rc<Cell<u64>>);
    impl Read for SeekCounting<'_> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = self.0.read(buf)?;
            self.1.set(self.1.get() + n as u64);
            Ok(n)
        }
    }
    impl std::io::Seek for SeekCounting<'_> {
        fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
            self.0.seek(pos)
        }
    }
    let mut reader =
        FitsReader::seekable(SeekCounting(Cursor::new(&bytes[..]), read.clone())).unwrap();
    reader.next_hdu().unwrap();
    let ext = reader.next_hdu().unwrap().unwrap();
    assert_same_hdu(ext, &parsed.hdus[1], "extension");
    let headers = (parsed.hdus[0].data_start + BLOCK_SIZE) as u64;
    assert_eq!(read.get(), headers, "only header blocks were read");
}

#[test]
fn image_into_buffers_matches_in_memory() {
    let bytes = two_image_file(30, 20);
    let parsed = parse_fits(&bytes).unwrap();
    let mut want = vec![0f64; 600];
    read_image_data_into_f64(&bytes, parsed.primary(), &mut want).unwrap();
    let mut reader = FitsReader::from_slice(&bytes);
    reader.next_hdu().unwrap();
    let mut got = vec![0f64; 600];
    reader.read_image_into_f64(&mut got).unwrap();
    assert_eq!(got, want);

    let mut reader = FitsReader::from_slice(&bytes);
    reader.next_hdu().unwrap();
    assert!(matches!(
        reader.read_image_into_f64(&mut [0.0; 599]),
        Err(Error::InvalidValue)
    ));
}

#[test]
fn data_cannot_be_decoded_twice() {
    let bytes = two_image_file(4, 4);
    let mut reader = FitsReader::from_slice(&bytes);
    reader.next_hdu().unwrap();
    reader.read_image().unwrap();
    assert!(reader.read_image().is_err());

    let mut reader = FitsReader::from_slice(&bytes);
    reader.next_hdu().unwrap();
    reader.skip_data().unwrap();
    assert!(reader.read_image().is_err());
}

/// A header declaring an 8 TB image over a few blocks of input.
fn hostile_file() -> Vec<u8> {
    let cards = build_primary_header(-64, &[1_000_000, 1_000_000]).unwrap();
    let mut bytes = serialize_header(&cards).unwrap();
    bytes.extend(vec![0u8; BLOCK_SIZE]);
    bytes
}

#[test]
fn declared_size_past_known_length_fails_before_allocating() {
    let bytes = hostile_file();
    assert!(matches!(
        FitsReader::from_slice(&bytes).next_hdu(),
        Err(Error::UnexpectedEof)
    ));
    assert!(matches!(
        FitsReader::with_len(&bytes[..], bytes.len() as u64).next_hdu(),
        Err(Error::UnexpectedEof)
    ));
}

#[test]
fn truncated_data_of_unknown_length_is_an_error() {
    let bytes = hostile_file();
    // Unknown length: the header is accepted, and the decode fails at EOF
    // having reserved at most one chunk, not 8 TB.
    let mut reader = FitsReader::new(&bytes[..]);
    reader.next_hdu().unwrap();
    assert!(matches!(reader.read_image(), Err(Error::UnexpectedEof)));

    let mut reader = FitsReader::new(&bytes[..]);
    reader.next_hdu().unwrap();
    assert!(matches!(reader.next_hdu(), Err(Error::UnexpectedEof)));

    let full = two_image_file(30, 20);
    let cut = &full[..parse_fits(&full).unwrap().primary().data_start + 100];
    let mut reader = FitsReader::new(cut);
    reader.next_hdu().unwrap();
    let mut buf = vec![0f32; 600];
    assert!(matches!(
        reader.read_image_into_f32(&mut buf),
        Err(Error::UnexpectedEof)
    ));
}

#[test]
fn missing_trailing_padding_is_allowed() {
    let full = two_image_file(30, 20);
    let parsed = parse_fits(&full).unwrap();
    let ext = &parsed.hdus[1];
    let cut = &full[..ext.data_start + ext.data_len];
    let mut reader = FitsReader::new(cut);
    reader.next_hdu().unwrap();
    reader.next_hdu().unwrap().unwrap();
    assert!(reader.next_hdu().unwrap().is_none());
    let mut reader = FitsReader::from_slice(cut);
    reader.next_hdu().unwrap();
    reader.next_hdu().unwrap();
    assert_eq!(
        reader.read_image().unwrap(),
        ImageData::I16(vec![1, -2, 3, -4, 5, -6])
    );
    assert!(reader.next_hdu().unwrap().is_none());
}

#[test]
fn rejects_non_fits_and_short_input() {
    assert!(matches!(
        FitsReader::new(&[0u8; 100][..]).next_hdu(),
        Err(Error::UnexpectedEof)
    ));
    let mut not_primary = two_image_file(2, 2);
    not_primary.drain(..parse_fits(&not_primary).unwrap().hdus[1].header_start);
    assert!(FitsReader::new(&not_primary[..]).next_hdu().is_err());
}
