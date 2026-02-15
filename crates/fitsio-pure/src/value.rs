use alloc::string::String;
use alloc::string::ToString;
use core::str;

/// A parsed FITS header value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// FITS logical value (`T` or `F`).
    Logical(bool),
    /// FITS integer value.
    Integer(i64),
    /// FITS floating-point value.
    Float(f64),
    /// FITS character string (content between single quotes).
    String(String),
    /// FITS complex integer `(real, imaginary)`.
    ComplexInt(i64, i64),
    /// FITS complex float `(real, imaginary)`.
    ComplexFloat(f64, f64),
}

/// Split a value field at the comment separator.
///
/// Returns `(value_part, optional_comment)`. The comment does not include the
/// leading separator.
///
/// The FITS standard uses ` / ` (space-slash-space) but real-world files
/// produced by IDL and other tools omit the trailing space (e.g.
/// `BITPIX = -32 /No. of bits per pixel`).  Both cfitsio and fitsrs accept
/// ` /` without requiring a trailing space, so we do the same.
fn split_comment(field: &[u8]) -> (&[u8], Option<&str>) {
    // For string values the comment starts after the closing quote, so the
    // caller must handle strings separately.  For non-string values we scan
    // for ` /` (space then slash).
    let len = field.len();
    let mut i = 0;
    while i + 1 < len {
        if field[i] == b' ' && field[i + 1] == b'/' {
            let value_part = &field[..i];
            // Skip the slash; also skip one optional space after it.
            let mut comment_start = i + 2;
            if comment_start < len && field[comment_start] == b' ' {
                comment_start += 1;
            }
            let comment = str::from_utf8(&field[comment_start..])
                .ok()
                .map(|s| s.trim_end());
            return (value_part, comment.filter(|s| !s.is_empty()));
        }
        i += 1;
    }
    (field, None)
}

/// Parse a FITS character-string value from the 70-byte value field.
///
/// String values begin with `'` at the first byte.  The string content
/// continues until the closing `'` (doubled single-quotes `''` inside the
/// string represent a literal `'`).  Everything after the closing quote is
/// either whitespace or a ` / ` comment separator followed by the comment.
fn parse_string(field: &[u8]) -> Option<(Value, Option<&str>)> {
    if field.is_empty() || field[0] != b'\'' {
        return None;
    }

    let mut value = String::new();
    let mut i = 1; // skip opening quote
    let len = field.len();

    loop {
        if i >= len {
            // Unterminated string — be lenient and accept what we have.
            break;
        }
        if field[i] == b'\'' {
            if i + 1 < len && field[i + 1] == b'\'' {
                // Doubled quote → literal single-quote.
                value.push('\'');
                i += 2;
            } else {
                // Closing quote.
                i += 1;
                break;
            }
        } else {
            value.push(field[i] as char);
            i += 1;
        }
    }

    // Trim trailing spaces from the string value (FITS pads to min 8 chars).
    let trimmed = value.trim_end().to_string();

    // Look for comment after the closing quote.
    let remainder = &field[i..];
    let comment = find_comment_in_remainder(remainder);

    Some((Value::String(trimmed), comment))
}

/// Given the bytes after a closing string quote, find the comment if present.
fn find_comment_in_remainder(remainder: &[u8]) -> Option<&str> {
    let len = remainder.len();
    let mut i = 0;
    while i + 1 < len {
        if remainder[i] == b' ' && remainder[i + 1] == b'/' {
            // Skip the slash; also skip one optional space after it.
            let mut comment_start = i + 2;
            if comment_start < len && remainder[comment_start] == b' ' {
                comment_start += 1;
            }
            let comment = str::from_utf8(&remainder[comment_start..])
                .ok()
                .map(|s| s.trim_end());
            return comment.filter(|s| !s.is_empty());
        }
        i += 1;
    }
    None
}

/// Try to parse a complex value `(real, imag)`.
fn parse_complex(text: &str) -> Option<Value> {
    let text = text.trim();
    if !text.starts_with('(') || !text.ends_with(')') {
        return None;
    }
    let inner = &text[1..text.len() - 1];
    let comma_pos = inner.find(',')?;
    let left = inner[..comma_pos].trim();
    let right = inner[comma_pos + 1..].trim();

    // Try integer complex first, then float complex.
    if let (Ok(re), Ok(im)) = (left.parse::<i64>(), right.parse::<i64>()) {
        // Only treat as integer complex if neither part looks like a float.
        if !left.contains('.') && !right.contains('.') {
            return Some(Value::ComplexInt(re, im));
        }
    }

    let re = parse_float_str(left)?;
    let im = parse_float_str(right)?;
    Some(Value::ComplexFloat(re, im))
}

/// Parse a float string, handling FITS `D` exponent notation.
fn parse_float_str(s: &str) -> Option<f64> {
    let normalized = s.replace('D', "E").replace('d', "e");
    normalized.parse::<f64>().ok()
}

/// Parse a FITS header value from the 70-byte value portion of an 80-byte
/// card (bytes 10..80).
///
/// Returns the parsed [`Value`] and an optional comment string.
///
/// The caller is responsible for checking that bytes 8..10 of the card are
/// `= ` (the value indicator) before calling this function.
pub fn parse_value(value_bytes: &[u8]) -> Option<(Value, Option<&str>)> {
    if value_bytes.is_empty() {
        return None;
    }

    // 1. String values: first non-space byte is a single quote.
    if value_bytes[0] == b'\'' {
        return parse_string(value_bytes);
    }

    // For all other types, split off the comment first.
    let (val_part, comment) = split_comment(value_bytes);

    let val_text = str::from_utf8(val_part).ok()?.trim();
    if val_text.is_empty() {
        return None;
    }

    // 2. Logical: `T` or `F` — standard puts it in byte 30 of the card
    //    (index 20 in the 70-byte field). We check both: if the trimmed text
    //    is exactly `T` or `F`, or the canonical position holds the value.
    if val_text == "T" {
        return Some((Value::Logical(true), comment));
    }
    if val_text == "F" {
        return Some((Value::Logical(false), comment));
    }

    // 3. Complex values: `(real, imag)`
    if val_text.starts_with('(') {
        if let Some(v) = parse_complex(val_text) {
            return Some((v, comment));
        }
    }

    // 4. Integer: no decimal point or exponent characters.
    if !val_text.contains('.')
        && !val_text.contains('E')
        && !val_text.contains('e')
        && !val_text.contains('D')
        && !val_text.contains('d')
    {
        if let Ok(n) = val_text.parse::<i64>() {
            return Some((Value::Integer(n), comment));
        }
    }

    // 5. Float.
    if let Some(f) = parse_float_str(val_text) {
        return Some((Value::Float(f), comment));
    }

    None
}

/// Serialize a [`Value`] into a 70-byte field suitable for bytes 10..80 of an
/// 80-byte FITS card.
///
/// Numeric and logical values are right-justified in the first 20 bytes
/// (columns 11-30 of the card).  String values start at byte 0 with a single
/// quote.
pub fn format_value(value: &Value) -> [u8; 70] {
    let mut buf = [b' '; 70];

    match value {
        Value::Logical(b) => {
            // Standard: logical value in column 30 = index 20 of value field.
            buf[19] = if *b { b'T' } else { b'F' };
        }
        Value::Integer(n) => {
            let s = format_integer(*n);
            right_justify(&s, &mut buf[..20]);
        }
        Value::Float(f) => {
            let s = format_float(*f);
            right_justify(&s, &mut buf[..20]);
        }
        Value::String(s) => {
            write_string(s, &mut buf);
        }
        Value::ComplexInt(re, im) => {
            let s = alloc::format!("({}, {})", re, im);
            right_justify(s.as_bytes(), &mut buf[..30]);
        }
        Value::ComplexFloat(re, im) => {
            let re_s = format_float_with_max(*re, 20);
            let im_s = format_float_with_max(*im, 20);
            let s = alloc::format!("({}, {})", re_s, im_s);
            right_justify(s.as_bytes(), &mut buf[..50]);
        }
    }

    buf
}

/// Right-justify `src` within `dest`, padding the left with spaces.
fn right_justify(src: &[u8], dest: &mut [u8]) {
    let len = src.len().min(dest.len());
    let start = dest.len() - len;
    // Fill with spaces first (already done by caller, but be safe).
    for b in dest.iter_mut() {
        *b = b' ';
    }
    dest[start..start + len].copy_from_slice(&src[..len]);
}

fn format_integer(n: i64) -> alloc::vec::Vec<u8> {
    use alloc::format;
    format!("{}", n).into_bytes()
}

fn format_float_raw(f: f64) -> alloc::string::String {
    format_float_with_max(f, 20)
}

fn format_float_with_max(f: f64, max_len: usize) -> alloc::string::String {
    use alloc::format;
    if f == 0.0 {
        return alloc::string::String::from("0.0");
    }
    // Start with high precision and reduce until the result fits.
    let mut precision = 15usize;
    loop {
        let s = format!("{:.prec$E}", f, prec = precision);
        if s.len() <= max_len || precision == 0 {
            return s;
        }
        precision -= 1;
    }
}

fn format_float(f: f64) -> alloc::vec::Vec<u8> {
    format_float_raw(f).into_bytes()
}

fn write_string(s: &str, buf: &mut [u8; 70]) {
    let mut pos = 0;
    buf[pos] = b'\'';
    pos += 1;

    for ch in s.bytes() {
        if pos >= 69 {
            break; // Leave room for closing quote.
        }
        if ch == b'\'' {
            if pos + 1 >= 69 {
                break;
            }
            buf[pos] = b'\'';
            buf[pos + 1] = b'\'';
            pos += 2;
        } else {
            buf[pos] = ch;
            pos += 1;
        }
    }

    // Pad to minimum 8 characters between quotes (so closing quote at >= index 9).
    while pos < 9 {
        buf[pos] = b' ';
        pos += 1;
    }

    if pos < 70 {
        buf[pos] = b'\'';
        // Remaining bytes stay as spaces.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a 70-byte field from a string, right-padded with spaces.
    fn make_field(s: &str) -> [u8; 70] {
        let mut buf = [b' '; 70];
        let bytes = s.as_bytes();
        let len = bytes.len().min(70);
        buf[..len].copy_from_slice(&bytes[..len]);
        buf
    }

    // ---- Logical ----

    #[test]
    fn parse_logical_true() {
        let field = make_field("                   T");
        let (val, comment) = parse_value(&field).unwrap();
        assert_eq!(val, Value::Logical(true));
        assert!(comment.is_none());
    }

    #[test]
    fn parse_logical_false() {
        let field = make_field("                   F");
        let (val, comment) = parse_value(&field).unwrap();
        assert_eq!(val, Value::Logical(false));
        assert!(comment.is_none());
    }

    #[test]
    fn parse_logical_with_comment() {
        let field = make_field("                   T / this is a flag");
        let (val, comment) = parse_value(&field).unwrap();
        assert_eq!(val, Value::Logical(true));
        assert_eq!(comment.unwrap(), "this is a flag");
    }

    // ---- Integer ----

    #[test]
    fn parse_integer_positive() {
        let field = make_field("                  42");
        let (val, comment) = parse_value(&field).unwrap();
        assert_eq!(val, Value::Integer(42));
        assert!(comment.is_none());
    }

    #[test]
    fn parse_integer_negative() {
        let field = make_field("                 -99");
        let (val, comment) = parse_value(&field).unwrap();
        assert_eq!(val, Value::Integer(-99));
        assert!(comment.is_none());
    }

    #[test]
    fn parse_integer_with_comment() {
        let field = make_field("                1024 / block count");
        let (val, comment) = parse_value(&field).unwrap();
        assert_eq!(val, Value::Integer(1024));
        assert_eq!(comment.unwrap(), "block count");
    }

    #[test]
    fn parse_integer_zero() {
        let field = make_field("                   0");
        let (val, _) = parse_value(&field).unwrap();
        assert_eq!(val, Value::Integer(0));
    }

    // ---- Float ----

    #[test]
    fn parse_float_simple() {
        let field = make_field("             9.80665");
        let (val, _) = parse_value(&field).unwrap();
        match val {
            Value::Float(f) => assert!((f - 9.80665).abs() < 1e-10),
            other => panic!("Expected Float, got {:?}", other),
        }
    }

    #[test]
    fn parse_float_scientific_e() {
        let field = make_field("           1.234E+05");
        let (val, _) = parse_value(&field).unwrap();
        match val {
            Value::Float(f) => assert!((f - 1.234e5).abs() < 1e-5),
            other => panic!("Expected Float, got {:?}", other),
        }
    }

    #[test]
    fn parse_float_d_exponent() {
        let field = make_field("           1.234D+05");
        let (val, _) = parse_value(&field).unwrap();
        match val {
            Value::Float(f) => assert!((f - 1.234e5).abs() < 1e-5),
            other => panic!("Expected Float, got {:?}", other),
        }
    }

    #[test]
    fn parse_float_negative_exponent() {
        let field = make_field("          -2.5D-03");
        let (val, _) = parse_value(&field).unwrap();
        match val {
            Value::Float(f) => assert!((f - (-2.5e-3)).abs() < 1e-15),
            other => panic!("Expected Float, got {:?}", other),
        }
    }

    #[test]
    fn parse_float_with_comment() {
        let field = make_field("               0.5 / scale factor");
        let (val, comment) = parse_value(&field).unwrap();
        match val {
            Value::Float(f) => assert!((f - 0.5).abs() < 1e-10),
            other => panic!("Expected Float, got {:?}", other),
        }
        assert_eq!(comment.unwrap(), "scale factor");
    }

    // ---- String ----

    #[test]
    fn parse_string_simple() {
        let field = make_field("'SIMPLE  '");
        let (val, _) = parse_value(&field).unwrap();
        assert_eq!(val, Value::String(String::from("SIMPLE")));
    }

    #[test]
    fn parse_string_with_comment() {
        let field = make_field("'IMAGE   '           / image type");
        let (val, comment) = parse_value(&field).unwrap();
        assert_eq!(val, Value::String(String::from("IMAGE")));
        assert_eq!(comment.unwrap(), "image type");
    }

    #[test]
    fn parse_string_embedded_quotes() {
        let field = make_field("'it''s ok'");
        let (val, _) = parse_value(&field).unwrap();
        assert_eq!(val, Value::String(String::from("it's ok")));
    }

    #[test]
    fn parse_string_empty() {
        let field = make_field("'        '");
        let (val, _) = parse_value(&field).unwrap();
        assert_eq!(val, Value::String(String::from("")));
    }

    #[test]
    fn parse_string_min_padding() {
        // String shorter than 8 chars, padded to 8.
        let field = make_field("'AB      '");
        let (val, _) = parse_value(&field).unwrap();
        assert_eq!(val, Value::String(String::from("AB")));
    }

    // ---- Complex Integer ----

    #[test]
    fn parse_complex_int() {
        let field = make_field("            (42, -7)");
        let (val, _) = parse_value(&field).unwrap();
        assert_eq!(val, Value::ComplexInt(42, -7));
    }

    #[test]
    fn parse_complex_int_with_comment() {
        let field = make_field("            (1, 2) / impedance");
        let (val, comment) = parse_value(&field).unwrap();
        assert_eq!(val, Value::ComplexInt(1, 2));
        assert_eq!(comment.unwrap(), "impedance");
    }

    // ---- Complex Float ----

    #[test]
    fn parse_complex_float() {
        let field = make_field("       (1.5, -3.25)");
        let (val, _) = parse_value(&field).unwrap();
        match val {
            Value::ComplexFloat(re, im) => {
                assert!((re - 1.5).abs() < 1e-10);
                assert!((im - (-3.25)).abs() < 1e-10);
            }
            other => panic!("Expected ComplexFloat, got {:?}", other),
        }
    }

    // ---- Round-trip tests ----

    #[test]
    fn roundtrip_logical() {
        for &b in &[true, false] {
            let v = Value::Logical(b);
            let buf = format_value(&v);
            let (parsed, _) = parse_value(&buf).unwrap();
            assert_eq!(parsed, v);
        }
    }

    #[test]
    fn roundtrip_integer() {
        for &n in &[0i64, 1, -1, 42, -9999, i64::MAX, i64::MIN] {
            let v = Value::Integer(n);
            let buf = format_value(&v);
            let (parsed, _) = parse_value(&buf).unwrap();
            assert_eq!(parsed, v, "round-trip failed for {}", n);
        }
    }

    #[test]
    fn roundtrip_float() {
        for &f in &[0.0f64, 1.0, -1.0, 9.80665, 1.23e10, -4.56e-20] {
            let v = Value::Float(f);
            let buf = format_value(&v);
            let (parsed, _) = parse_value(&buf).unwrap();
            match parsed {
                Value::Float(pf) => {
                    if f == 0.0 {
                        assert_eq!(pf, 0.0);
                    } else {
                        let rel_err = ((pf - f) / f).abs();
                        assert!(
                            rel_err < 1e-10,
                            "round-trip float failed: {} vs {} (rel err {})",
                            f,
                            pf,
                            rel_err
                        );
                    }
                }
                other => panic!("Expected Float, got {:?}", other),
            }
        }
    }

    #[test]
    fn roundtrip_string() {
        for s in &["HELLO", "", "it's here", "X", "A long string value"] {
            let v = Value::String(String::from(*s));
            let buf = format_value(&v);
            let (parsed, _) = parse_value(&buf).unwrap();
            assert_eq!(parsed, v, "round-trip failed for {:?}", s);
        }
    }

    #[test]
    fn roundtrip_complex_int() {
        let v = Value::ComplexInt(10, -20);
        let buf = format_value(&v);
        let (parsed, _) = parse_value(&buf).unwrap();
        assert_eq!(parsed, v);
    }

    #[test]
    fn roundtrip_complex_float() {
        let v = Value::ComplexFloat(1.5, -2.5);
        let buf = format_value(&v);
        let (parsed, _) = parse_value(&buf).unwrap();
        match parsed {
            Value::ComplexFloat(re, im) => {
                assert!((re - 1.5).abs() < 1e-10);
                assert!((im - (-2.5)).abs() < 1e-10);
            }
            other => panic!("Expected ComplexFloat, got {:?}", other),
        }
    }

    // ---- Format tests ----

    #[test]
    fn format_logical_position() {
        let buf = format_value(&Value::Logical(true));
        // Logical should be at index 19 (column 30 of card).
        assert_eq!(buf[19], b'T');
        // Everything else should be spaces.
        for (i, &b) in buf.iter().enumerate() {
            if i != 19 {
                assert_eq!(b, b' ', "non-space at index {}", i);
            }
        }
    }

    #[test]
    fn format_integer_right_justified() {
        let buf = format_value(&Value::Integer(42));
        let first20 = core::str::from_utf8(&buf[..20]).unwrap();
        assert_eq!(first20.trim(), "42");
        // Check right-justification.
        assert_eq!(buf[19], b'2');
        assert_eq!(buf[18], b'4');
    }

    #[test]
    fn format_string_quotes_and_padding() {
        let buf = format_value(&Value::String(String::from("AB")));
        // Should start with quote.
        assert_eq!(buf[0], b'\'');
        // Content.
        assert_eq!(buf[1], b'A');
        assert_eq!(buf[2], b'B');
        // Padded to 8 chars, closing quote at index 9.
        assert_eq!(buf[9], b'\'');
    }

    #[test]
    fn format_string_embedded_quotes() {
        let buf = format_value(&Value::String(String::from("it's")));
        let s = core::str::from_utf8(&buf).unwrap();
        // Should contain doubled quotes.
        assert!(s.contains("it''s"), "Expected doubled quote in: {}", s);
    }

    // ---- Edge cases ----

    #[test]
    fn parse_empty_field_returns_none() {
        assert!(parse_value(b"").is_none());
    }

    #[test]
    fn parse_all_spaces_returns_none() {
        let field = make_field("");
        assert!(parse_value(&field).is_none());
    }

    #[test]
    fn parse_integer_comment_no_trailing_space() {
        // Real-world: "BITPIX  =                  -32 /No.Bits per pixel"
        let field = make_field("                 -32 /No.Bits per pixel");
        let (val, comment) = parse_value(&field).unwrap();
        assert_eq!(val, Value::Integer(-32));
        assert_eq!(comment.unwrap(), "No.Bits per pixel");
    }

    #[test]
    fn parse_float_comment_no_trailing_space() {
        let field = make_field("               0.5 /scale");
        let (val, comment) = parse_value(&field).unwrap();
        match val {
            Value::Float(f) => assert!((f - 0.5).abs() < 1e-10),
            other => panic!("Expected Float, got {:?}", other),
        }
        assert_eq!(comment.unwrap(), "scale");
    }

    #[test]
    fn parse_string_comment_no_trailing_space() {
        let field = make_field("'IMAGE   '           /image type");
        let (val, comment) = parse_value(&field).unwrap();
        assert_eq!(val, Value::String(String::from("IMAGE")));
        assert_eq!(comment.unwrap(), "image type");
    }

    #[test]
    fn parse_large_integer() {
        let field = make_field("       9999999999999");
        let (val, _) = parse_value(&field).unwrap();
        assert_eq!(val, Value::Integer(9999999999999));
    }

    #[test]
    fn format_value_field_is_70_bytes() {
        let buf = format_value(&Value::Integer(1));
        assert_eq!(buf.len(), 70);
    }
}
