use fitsio_pure::block::padded_byte_len;
use fitsio_pure::hdu::{FitsData, Hdu, HduInfo};
use fitsio_pure::header::{serialize_header, Card};
use fitsio_pure::primary::build_primary_header;
use fitsio_pure::value::Value;
use std::process;

const USAGE: &str = "\
Usage: fitsconv <subcommand> [args...]

Subcommands:
  extract <input.fits> <hdu_index> <output.fits>
      Extract a single HDU to a new FITS file.

  info <input.fits>
      Print HDU summary (same as fitsinfo).";

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
    }
    out
}

fn format_fits_info(fits: &FitsData) -> String {
    let mut out = String::new();
    for (i, hdu) in fits.hdus.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&format_hdu(i, hdu));
    }
    out
}

fn extract_hdu(file_data: &[u8], fits: &FitsData, hdu_index: usize) -> Result<Vec<u8>, String> {
    let hdu = fits.hdus.get(hdu_index).ok_or_else(|| {
        format!(
            "HDU index {} out of range (file has {} HDUs)",
            hdu_index,
            fits.hdus.len()
        )
    })?;

    if hdu_index == 0 {
        let header_bytes = serialize_header(&hdu.cards);
        let data_padded = padded_byte_len(hdu.data_len);
        let mut output = Vec::with_capacity(header_bytes.len() + data_padded);
        output.extend_from_slice(&header_bytes);
        if hdu.data_len > 0 {
            let data_slice = &file_data[hdu.data_start..hdu.data_start + hdu.data_len];
            output.extend_from_slice(data_slice);
            output.resize(header_bytes.len() + data_padded, 0u8);
        }
        return Ok(output);
    }

    let mut output = Vec::new();

    let primary_cards = build_primary_header(8, &[])
        .map_err(|e| format!("Failed to build primary header: {}", e))?;
    let primary_header = serialize_header(&primary_cards);
    output.extend_from_slice(&primary_header);

    let ext_header = serialize_header(&hdu.cards);
    output.extend_from_slice(&ext_header);

    if hdu.data_len > 0 {
        let data_padded = padded_byte_len(hdu.data_len);
        let data_slice = &file_data[hdu.data_start..hdu.data_start + hdu.data_len];
        output.extend_from_slice(data_slice);
        output.resize(primary_header.len() + ext_header.len() + data_padded, 0u8);
    }

    Ok(output)
}

fn run(args: &[String]) -> Result<String, String> {
    if args.is_empty() {
        return Err(USAGE.to_string());
    }

    match args[0].as_str() {
        "info" => {
            if args.len() != 2 {
                return Err("Usage: fitsconv info <input.fits>".to_string());
            }
            let path = &args[1];
            let data =
                std::fs::read(path).map_err(|e| format!("Error reading '{}': {}", path, e))?;
            let fits = fitsio_pure::hdu::parse_fits(&data)
                .map_err(|e| format!("Error parsing '{}': {}", path, e))?;
            Ok(format_fits_info(&fits))
        }
        "extract" => {
            if args.len() != 4 {
                return Err(
                    "Usage: fitsconv extract <input.fits> <hdu_index> <output.fits>".to_string(),
                );
            }
            let input_path = &args[1];
            let hdu_index: usize = args[2]
                .parse()
                .map_err(|_| format!("Invalid HDU index: '{}'", args[2]))?;
            let output_path = &args[3];

            let data = std::fs::read(input_path)
                .map_err(|e| format!("Error reading '{}': {}", input_path, e))?;
            let fits = fitsio_pure::hdu::parse_fits(&data)
                .map_err(|e| format!("Error parsing '{}': {}", input_path, e))?;

            let output_data = extract_hdu(&data, &fits, hdu_index)?;

            std::fs::write(output_path, &output_data)
                .map_err(|e| format!("Error writing '{}': {}", output_path, e))?;

            Ok(format!(
                "Extracted HDU {} to '{}' ({} bytes)\n",
                hdu_index,
                output_path,
                output_data.len()
            ))
        }
        other => Err(format!("Unknown subcommand: '{}'\n\n{}", other, USAGE)),
    }
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
    use fitsio_pure::header::Card;
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

    fn build_multi_ext_fits() -> Vec<u8> {
        let primary_cards = primary_header_naxis0();
        let ext1_cards = image_extension_header(-32, &[32, 32], Some("SCI"));
        let ext2_cards = bintable_extension_header(24, 100, 0, 3, Some("EVENTS"));

        let primary_header = serialize_header(&primary_cards);
        let ext1_header = serialize_header(&ext1_cards);
        let ext1_data_bytes = 32 * 32 * 4;
        let ext1_data_padded = padded_byte_len(ext1_data_bytes);
        let ext2_header = serialize_header(&ext2_cards);
        let ext2_data_bytes = 24 * 100;
        let ext2_data_padded = padded_byte_len(ext2_data_bytes);

        let mut data = Vec::new();
        data.extend_from_slice(&primary_header);
        data.extend_from_slice(&ext1_header);
        data.resize(data.len() + ext1_data_padded, 0u8);
        data.extend_from_slice(&ext2_header);
        data.resize(data.len() + ext2_data_padded, 0u8);
        data
    }

    #[test]
    fn extract_primary_hdu() {
        let cards = primary_header_image(16, &[64, 64]);
        let data_bytes = 64 * 64 * 2;
        let file_data = build_fits_bytes(&cards, data_bytes);
        let fits = fitsio_pure::hdu::parse_fits(&file_data).unwrap();

        let output = extract_hdu(&file_data, &fits, 0).unwrap();
        let re_parsed = fitsio_pure::hdu::parse_fits(&output).unwrap();
        assert_eq!(re_parsed.hdus.len(), 1);
        match &re_parsed.primary().info {
            HduInfo::Primary { bitpix, naxes } => {
                assert_eq!(*bitpix, 16);
                assert_eq!(naxes, &[64, 64]);
            }
            other => panic!("Expected Primary, got {:?}", other),
        }
    }

    #[test]
    fn extract_extension_hdu() {
        let file_data = build_multi_ext_fits();
        let fits = fitsio_pure::hdu::parse_fits(&file_data).unwrap();
        assert_eq!(fits.hdus.len(), 3);

        let output = extract_hdu(&file_data, &fits, 1).unwrap();
        let re_parsed = fitsio_pure::hdu::parse_fits(&output).unwrap();

        assert_eq!(re_parsed.hdus.len(), 2);
        match &re_parsed.primary().info {
            HduInfo::Primary { bitpix, naxes } => {
                assert_eq!(*bitpix, 8);
                assert!(naxes.is_empty());
            }
            other => panic!("Expected Primary, got {:?}", other),
        }
        match &re_parsed.hdus[1].info {
            HduInfo::Image { bitpix, naxes } => {
                assert_eq!(*bitpix, -32);
                assert_eq!(naxes, &[32, 32]);
            }
            other => panic!("Expected Image, got {:?}", other),
        }
    }

    #[test]
    fn extract_bintable_extension() {
        let file_data = build_multi_ext_fits();
        let fits = fitsio_pure::hdu::parse_fits(&file_data).unwrap();

        let output = extract_hdu(&file_data, &fits, 2).unwrap();
        let re_parsed = fitsio_pure::hdu::parse_fits(&output).unwrap();

        assert_eq!(re_parsed.hdus.len(), 2);
        match &re_parsed.hdus[1].info {
            HduInfo::BinaryTable {
                naxis1,
                naxis2,
                pcount,
                tfields,
            } => {
                assert_eq!(*naxis1, 24);
                assert_eq!(*naxis2, 100);
                assert_eq!(*pcount, 0);
                assert_eq!(*tfields, 3);
            }
            other => panic!("Expected BinaryTable, got {:?}", other),
        }
    }

    #[test]
    fn extract_out_of_range() {
        let cards = primary_header_naxis0();
        let file_data = build_fits_bytes(&cards, 0);
        let fits = fitsio_pure::hdu::parse_fits(&file_data).unwrap();

        let result = extract_hdu(&file_data, &fits, 5);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("out of range"));
    }

    #[test]
    fn info_subcommand_format() {
        let file_data = build_multi_ext_fits();
        let fits = fitsio_pure::hdu::parse_fits(&file_data).unwrap();
        let output = format_fits_info(&fits);

        assert!(output.contains("HDU 0: Primary"));
        assert!(output.contains("HDU 1: IMAGE extension (EXTNAME: SCI)"));
        assert!(output.contains("HDU 2: BINTABLE extension (EXTNAME: EVENTS)"));
    }

    #[test]
    fn run_no_args_shows_usage() {
        let args: Vec<String> = vec![];
        let result = run(&args);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Usage:"));
    }

    #[test]
    fn run_unknown_subcommand() {
        let args = vec!["bogus".to_string()];
        let result = run(&args);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown subcommand"));
    }

    #[test]
    fn run_extract_wrong_arg_count() {
        let args = vec!["extract".to_string(), "input.fits".to_string()];
        let result = run(&args);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Usage:"));
    }

    #[test]
    fn run_extract_invalid_index() {
        let args = vec![
            "extract".to_string(),
            "input.fits".to_string(),
            "abc".to_string(),
            "output.fits".to_string(),
        ];
        let result = run(&args);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid HDU index"));
    }

    #[test]
    fn run_info_wrong_arg_count() {
        let args = vec!["info".to_string()];
        let result = run(&args);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Usage:"));
    }

    #[test]
    fn run_info_file_not_found() {
        let args = vec!["info".to_string(), "nonexistent.fits".to_string()];
        let result = run(&args);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Error reading"));
    }

    #[test]
    fn run_extract_with_temp_files() {
        let file_data = build_multi_ext_fits();

        let dir = std::env::temp_dir().join("fitsconv_test_extract");
        std::fs::create_dir_all(&dir).unwrap();
        let input_path = dir.join("input.fits");
        let output_path = dir.join("output.fits");
        std::fs::write(&input_path, &file_data).unwrap();

        let args = vec![
            "extract".to_string(),
            input_path.to_str().unwrap().to_string(),
            "1".to_string(),
            output_path.to_str().unwrap().to_string(),
        ];
        let result = run(&args).unwrap();
        assert!(result.contains("Extracted HDU 1"));

        let output_data = std::fs::read(&output_path).unwrap();
        let re_parsed = fitsio_pure::hdu::parse_fits(&output_data).unwrap();
        assert_eq!(re_parsed.hdus.len(), 2);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn run_info_with_temp_file() {
        let file_data = build_multi_ext_fits();

        let dir = std::env::temp_dir().join("fitsconv_test_info");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.fits");
        std::fs::write(&path, &file_data).unwrap();

        let args = vec!["info".to_string(), path.to_str().unwrap().to_string()];
        let result = run(&args).unwrap();
        assert!(result.contains("HDU 0: Primary"));
        assert!(result.contains("HDU 1: IMAGE extension"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn extract_primary_no_data() {
        let cards = primary_header_naxis0();
        let file_data = build_fits_bytes(&cards, 0);
        let fits = fitsio_pure::hdu::parse_fits(&file_data).unwrap();

        let output = extract_hdu(&file_data, &fits, 0).unwrap();
        let re_parsed = fitsio_pure::hdu::parse_fits(&output).unwrap();
        assert_eq!(re_parsed.hdus.len(), 1);
        assert_eq!(re_parsed.primary().data_len, 0);
    }
}
