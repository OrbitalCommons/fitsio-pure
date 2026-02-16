use fitsio_pure::hdu::{FitsData, Hdu, HduInfo};
use fitsio_pure::header::Card;
use fitsio_pure::value::Value;
use std::process;

fn card_string_value(cards: &[Card], keyword: &str) -> Option<String> {
    cards.iter().find_map(|c| {
        if c.keyword_str() == keyword {
            match &c.value {
                Some(Value::String(s)) => Some(s.trim().to_string()),
                _ => None,
            }
        } else {
            None
        }
    })
}

fn format_hdu(index: usize, hdu: &Hdu) -> String {
    let mut out = String::new();
    match &hdu.info {
        HduInfo::Primary { bitpix, naxes } => {
            out.push_str(&format!("HDU {}: Primary\n", index));
            out.push_str(&format!("  BITPIX: {}\n", bitpix));
            out.push_str(&format!("  NAXIS: {}\n", naxes.len()));
            if !naxes.is_empty() {
                out.push_str(&format!("  Dimensions: {:?}\n", naxes));
            }
            out.push_str(&format!("  Data size: {} bytes\n", hdu.data_len));
        }
        HduInfo::Image { bitpix, naxes } => {
            let extname = card_string_value(&hdu.cards, "EXTNAME");
            let ext_label = match extname {
                Some(name) => format!(" (EXTNAME: {})", name),
                None => String::new(),
            };
            out.push_str(&format!("HDU {}: IMAGE extension{}\n", index, ext_label));
            out.push_str(&format!("  BITPIX: {}\n", bitpix));
            out.push_str(&format!("  NAXIS: {}\n", naxes.len()));
            if !naxes.is_empty() {
                out.push_str(&format!("  Dimensions: {:?}\n", naxes));
            }
            out.push_str(&format!("  Data size: {} bytes\n", hdu.data_len));
        }
        HduInfo::AsciiTable {
            naxis1,
            naxis2,
            tfields,
        } => {
            let extname = card_string_value(&hdu.cards, "EXTNAME");
            let ext_label = match extname {
                Some(name) => format!(" (EXTNAME: {})", name),
                None => String::new(),
            };
            out.push_str(&format!("HDU {}: TABLE extension{}\n", index, ext_label));
            out.push_str(&format!("  Columns: {}\n", tfields));
            out.push_str(&format!("  Rows: {}\n", naxis2));
            out.push_str(&format!("  Row width: {} bytes\n", naxis1));
            out.push_str(&format!("  Data size: {} bytes\n", hdu.data_len));
        }
        HduInfo::BinaryTable {
            naxis1,
            naxis2,
            pcount,
            tfields,
        } => {
            let extname = card_string_value(&hdu.cards, "EXTNAME");
            let ext_label = match extname {
                Some(name) => format!(" (EXTNAME: {})", name),
                None => String::new(),
            };
            out.push_str(&format!("HDU {}: BINTABLE extension{}\n", index, ext_label));
            out.push_str(&format!("  Columns: {}\n", tfields));
            out.push_str(&format!("  Rows: {}\n", naxis2));
            out.push_str(&format!("  Row width: {} bytes\n", naxis1));
            out.push_str(&format!("  Data size: {} bytes\n", naxis1 * naxis2));
            if *pcount > 0 {
                out.push_str(&format!("  Heap size: {} bytes\n", pcount));
            }
        }
        HduInfo::RandomGroups {
            bitpix,
            naxes,
            pcount,
            gcount,
        } => {
            out.push_str(&format!("HDU {}: Random Groups\n", index));
            out.push_str(&format!("  BITPIX: {}\n", bitpix));
            out.push_str(&format!("  Axes: {:?}\n", naxes));
            out.push_str(&format!("  PCOUNT: {}\n", pcount));
            out.push_str(&format!("  GCOUNT: {}\n", gcount));
            out.push_str(&format!("  Data size: {} bytes\n", hdu.data_len));
        }
    }
    out
}

fn format_verbose_cards(cards: &[Card]) -> String {
    let mut out = String::new();
    out.push_str("  Header cards:\n");
    for card in cards {
        if card.is_end() {
            continue;
        }
        let kw = card.keyword_str();
        match (&card.value, &card.comment) {
            (Some(val), Some(comment)) => {
                out.push_str(&format!("    {} = {:?} / {}\n", kw, val, comment));
            }
            (Some(val), None) => {
                out.push_str(&format!("    {} = {:?}\n", kw, val));
            }
            (None, Some(comment)) => {
                out.push_str(&format!("    {} {}\n", kw, comment));
            }
            (None, None) => {
                if !card.is_blank() {
                    out.push_str(&format!("    {}\n", kw));
                }
            }
        }
    }
    out
}

fn format_fits_info(fits: &FitsData, verbose: bool) -> String {
    let mut out = String::new();
    for (i, hdu) in fits.hdus.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&format_hdu(i, hdu));
        if verbose {
            out.push_str(&format_verbose_cards(&hdu.cards));
        }
    }
    out
}

fn run(args: &[String]) -> Result<String, String> {
    let mut verbose = false;
    let mut file_path = None;

    for arg in args {
        if arg == "-v" || arg == "--verbose" {
            verbose = true;
        } else if arg.starts_with('-') {
            return Err(format!("Unknown option: {}", arg));
        } else {
            if file_path.is_some() {
                return Err("Too many arguments".to_string());
            }
            file_path = Some(arg.as_str());
        }
    }

    let path = file_path.ok_or_else(|| {
        "Usage: fitsinfo [-v] <file.fits>\n\nPrint HDU summary for a FITS file.".to_string()
    })?;

    let data = std::fs::read(path).map_err(|e| format!("Error reading '{}': {}", path, e))?;

    let fits = fitsio_pure::hdu::parse_fits(&data)
        .map_err(|e| format!("Error parsing '{}': {}", path, e))?;

    Ok(format_fits_info(&fits, verbose))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(output) => print!("{}", output),
        Err(msg) => {
            eprintln!("{}", msg);
            process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fitsio_pure::block::padded_byte_len;
    use fitsio_pure::header::{serialize_header, Card};
    use fitsio_pure::value::Value;

    fn make_keyword(name: &str) -> [u8; 8] {
        let mut k = [b' '; 8];
        let bytes = name.as_bytes();
        let len = bytes.len().min(8);
        k[..len].copy_from_slice(&bytes[..len]);
        k
    }

    fn card(keyword: &str, value: Value) -> Card {
        Card {
            keyword: make_keyword(keyword),
            value: Some(value),
            comment: None,
        }
    }

    fn primary_header_naxis0() -> Vec<Card> {
        vec![
            card("SIMPLE", Value::Logical(true)),
            card("BITPIX", Value::Integer(8)),
            card("NAXIS", Value::Integer(0)),
        ]
    }

    fn primary_header_image(bitpix: i64, dims: &[usize]) -> Vec<Card> {
        let mut cards = vec![
            card("SIMPLE", Value::Logical(true)),
            card("BITPIX", Value::Integer(bitpix)),
            card("NAXIS", Value::Integer(dims.len() as i64)),
        ];
        for (i, &d) in dims.iter().enumerate() {
            let kw = format!("NAXIS{}", i + 1);
            cards.push(card(&kw, Value::Integer(d as i64)));
        }
        cards
    }

    fn image_extension_header(bitpix: i64, dims: &[usize], extname: Option<&str>) -> Vec<Card> {
        let mut cards = vec![
            card("XTENSION", Value::String(String::from("IMAGE"))),
            card("BITPIX", Value::Integer(bitpix)),
            card("NAXIS", Value::Integer(dims.len() as i64)),
        ];
        for (i, &d) in dims.iter().enumerate() {
            let kw = format!("NAXIS{}", i + 1);
            cards.push(card(&kw, Value::Integer(d as i64)));
        }
        cards.push(card("PCOUNT", Value::Integer(0)));
        cards.push(card("GCOUNT", Value::Integer(1)));
        if let Some(name) = extname {
            cards.push(card("EXTNAME", Value::String(String::from(name))));
        }
        cards
    }

    fn bintable_extension_header(
        naxis1: usize,
        naxis2: usize,
        pcount: usize,
        tfields: usize,
        extname: Option<&str>,
    ) -> Vec<Card> {
        let mut cards = vec![
            card("XTENSION", Value::String(String::from("BINTABLE"))),
            card("BITPIX", Value::Integer(8)),
            card("NAXIS", Value::Integer(2)),
            card("NAXIS1", Value::Integer(naxis1 as i64)),
            card("NAXIS2", Value::Integer(naxis2 as i64)),
            card("PCOUNT", Value::Integer(pcount as i64)),
            card("GCOUNT", Value::Integer(1)),
            card("TFIELDS", Value::Integer(tfields as i64)),
        ];
        if let Some(name) = extname {
            cards.push(card("EXTNAME", Value::String(String::from(name))));
        }
        cards
    }

    fn build_fits_bytes(header_cards: &[Card], data_bytes: usize) -> Vec<u8> {
        let header = serialize_header(header_cards);
        let padded_data = padded_byte_len(data_bytes);
        let mut result = Vec::with_capacity(header.len() + padded_data);
        result.extend_from_slice(&header);
        result.resize(header.len() + padded_data, 0u8);
        result
    }

    #[test]
    fn format_primary_naxis0() {
        let cards = primary_header_naxis0();
        let data = build_fits_bytes(&cards, 0);
        let fits = fitsio_pure::hdu::parse_fits(&data).unwrap();
        let output = format_fits_info(&fits, false);

        assert!(output.contains("HDU 0: Primary"));
        assert!(output.contains("BITPIX: 8"));
        assert!(output.contains("NAXIS: 0"));
        assert!(output.contains("Data size: 0 bytes"));
    }

    #[test]
    fn format_primary_with_image() {
        let cards = primary_header_image(16, &[1024, 1024]);
        let data_bytes = 1024 * 1024 * 2;
        let data = build_fits_bytes(&cards, data_bytes);
        let fits = fitsio_pure::hdu::parse_fits(&data).unwrap();
        let output = format_fits_info(&fits, false);

        assert!(output.contains("HDU 0: Primary"));
        assert!(output.contains("BITPIX: 16"));
        assert!(output.contains("NAXIS: 2"));
        assert!(output.contains("Dimensions: [1024, 1024]"));
        assert!(output.contains("Data size: 2097152 bytes"));
    }

    #[test]
    fn format_multi_extension() {
        let primary_cards = primary_header_naxis0();
        let ext_cards = image_extension_header(-32, &[512, 512], Some("SCI"));

        let primary_header = serialize_header(&primary_cards);
        let ext_header = serialize_header(&ext_cards);
        let ext_data_bytes = 512 * 512 * 4;
        let ext_data_padded = padded_byte_len(ext_data_bytes);

        let mut data = Vec::new();
        data.extend_from_slice(&primary_header);
        data.extend_from_slice(&ext_header);
        data.resize(data.len() + ext_data_padded, 0u8);

        let fits = fitsio_pure::hdu::parse_fits(&data).unwrap();
        let output = format_fits_info(&fits, false);

        assert!(output.contains("HDU 0: Primary"));
        assert!(output.contains("HDU 1: IMAGE extension (EXTNAME: SCI)"));
        assert!(output.contains("BITPIX: -32"));
        assert!(output.contains("Dimensions: [512, 512]"));
        assert!(output.contains("Data size: 1048576 bytes"));
    }

    #[test]
    fn format_bintable_extension() {
        let primary_cards = primary_header_naxis0();
        let ext_cards = bintable_extension_header(40, 10000, 1234, 5, Some("EVENTS"));

        let primary_header = serialize_header(&primary_cards);
        let ext_header = serialize_header(&ext_cards);
        let ext_data_bytes = 40 * 10000 + 1234;
        let ext_data_padded = padded_byte_len(ext_data_bytes);

        let mut data = Vec::new();
        data.extend_from_slice(&primary_header);
        data.extend_from_slice(&ext_header);
        data.resize(data.len() + ext_data_padded, 0u8);

        let fits = fitsio_pure::hdu::parse_fits(&data).unwrap();
        let output = format_fits_info(&fits, false);

        assert!(output.contains("HDU 1: BINTABLE extension (EXTNAME: EVENTS)"));
        assert!(output.contains("Columns: 5"));
        assert!(output.contains("Rows: 10000"));
        assert!(output.contains("Row width: 40 bytes"));
        assert!(output.contains("Data size: 400000 bytes"));
        assert!(output.contains("Heap size: 1234 bytes"));
    }

    #[test]
    fn format_bintable_no_heap() {
        let primary_cards = primary_header_naxis0();
        let ext_cards = bintable_extension_header(24, 100, 0, 3, None);

        let primary_header = serialize_header(&primary_cards);
        let ext_header = serialize_header(&ext_cards);
        let ext_data_bytes = 24 * 100;
        let ext_data_padded = padded_byte_len(ext_data_bytes);

        let mut data = Vec::new();
        data.extend_from_slice(&primary_header);
        data.extend_from_slice(&ext_header);
        data.resize(data.len() + ext_data_padded, 0u8);

        let fits = fitsio_pure::hdu::parse_fits(&data).unwrap();
        let output = format_fits_info(&fits, false);

        assert!(output.contains("HDU 1: BINTABLE extension"));
        assert!(!output.contains("Heap size:"));
    }

    #[test]
    fn verbose_shows_header_cards() {
        let cards = primary_header_image(16, &[100, 200]);
        let data_bytes = 100 * 200 * 2;
        let data = build_fits_bytes(&cards, data_bytes);
        let fits = fitsio_pure::hdu::parse_fits(&data).unwrap();
        let output = format_fits_info(&fits, true);

        assert!(output.contains("Header cards:"));
        assert!(output.contains("SIMPLE"));
        assert!(output.contains("BITPIX"));
        assert!(output.contains("NAXIS"));
    }

    #[test]
    fn run_missing_file() {
        let args = vec!["nonexistent.fits".to_string()];
        let result = run(&args);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Error reading"));
    }

    #[test]
    fn run_no_args() {
        let args: Vec<String> = vec![];
        let result = run(&args);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Usage:"));
    }

    #[test]
    fn run_unknown_option() {
        let args = vec!["--foo".to_string()];
        let result = run(&args);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown option"));
    }

    #[test]
    fn run_too_many_args() {
        let args = vec!["a.fits".to_string(), "b.fits".to_string()];
        let result = run(&args);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Too many arguments"));
    }

    #[test]
    fn run_with_temp_file() {
        let cards = primary_header_image(16, &[64, 64]);
        let data_bytes = 64 * 64 * 2;
        let fits_data = build_fits_bytes(&cards, data_bytes);

        let dir = std::env::temp_dir().join("fitsinfo_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.fits");
        std::fs::write(&path, &fits_data).unwrap();

        let args = vec![path.to_str().unwrap().to_string()];
        let result = run(&args).unwrap();
        assert!(result.contains("HDU 0: Primary"));
        assert!(result.contains("BITPIX: 16"));
        assert!(result.contains("Dimensions: [64, 64]"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn run_verbose_with_temp_file() {
        let cards = primary_header_naxis0();
        let fits_data = build_fits_bytes(&cards, 0);

        let dir = std::env::temp_dir().join("fitsinfo_test_verbose");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.fits");
        std::fs::write(&path, &fits_data).unwrap();

        let args = vec!["-v".to_string(), path.to_str().unwrap().to_string()];
        let result = run(&args).unwrap();
        assert!(result.contains("Header cards:"));
        assert!(result.contains("SIMPLE"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
