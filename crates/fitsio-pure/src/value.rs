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
    /// An undefined value: a value indicator (`= `) with nothing after it
    /// but an optional comment. Distinct from the empty string `''`.
    Undefined,
}

/// Split a value field at the comment separator.
///
/// Returns `(value_part, optional_comment)`. The comment does not include the
/// leading separator.
///
/// The FITS standard uses ` / ` (space-slash-space) but real-world files
/// omit either space (e.g. `BITPIX = -32 /No. of bits per pixel` from IDL,
/// or `NAXIS = 2/comment`). A non-string value never holds a `/`, so, as
/// cfitsio does, the comment starts at the first one.
fn split_comment(field: &[u8]) -> (&[u8], Option<&str>) {
    // For string values the comment starts after the closing quote, so the
    // caller must handle strings separately.
    let Some(slash) = field.iter().position(|&b| b == b'/') else {
        return (field, None);
    };
    // Skip the slash; also skip one optional space after it.
    let mut comment_start = slash + 1;
    if field.get(comment_start) == Some(&b' ') {
        comment_start += 1;
    }
    let comment = str::from_utf8(&field[comment_start..])
        .ok()
        .map(|s| s.trim_end());
    (&field[..slash], comment.filter(|s| !s.is_empty()))
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
    // 1. String values: first non-space byte is a single quote. Free-format
    //    cards may put it after column 11, as cfitsio and astropy accept.
    let start = value_bytes.iter().position(|&b| b != b' ')?;
    if value_bytes[start] == b'\'' {
        return parse_string(&value_bytes[start..]);
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
            place_number(format_float(*f).as_bytes(), &mut buf);
        }
        Value::String(s) => {
            write_string(s, &mut buf);
        }
        Value::ComplexInt(re, im) => {
            let s = alloc::format!("({}, {})", re, im);
            right_justify(s.as_bytes(), &mut buf[..30]);
        }
        Value::ComplexFloat(re, im) => {
            let re_s = format_float(*re);
            let im_s = format_float(*im);
            let s = alloc::format!("({}, {})", re_s, im_s);
            right_justify(s.as_bytes(), &mut buf[..50]);
        }
        Value::Undefined => {}
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

/// `f` as cfitsio writes a floating-point card value (`ffd2e`/`ffr2e` with
/// `decim = -digits`): C's `%.{digits}G`, then a `.` added where there is
/// neither a `.` nor an exponent (`1.`, `0.`), and an exponent form without a
/// `.` rewritten as `%.1E` (`1.0E-20`). `digits` is 15 for `f64` cards, as
/// cfitsio and `fitsio` write them, and 7 for `f32`. Infinities and NaN,
/// which cfitsio refuses, are written as Rust formats them.
fn format_g(f: f64, digits: usize) -> alloc::string::String {
    use alloc::format;
    use alloc::string::String;
    if !f.is_finite() {
        return format!("{f}");
    }
    let mut out = if f == 0.0 {
        String::from(if f.is_sign_negative() { "-0" } else { "0" })
    } else {
        // %G: the exponent %E would print decides between %E and %f.
        let e = format!("{:.*e}", digits - 1, f);
        let exponent: i32 = e[e.find('e').expect("has an exponent") + 1..]
            .parse()
            .expect("an integer exponent");
        if exponent < -4 || exponent >= digits as i32 {
            let mantissa = strip_zeros(&e[..e.find('e').expect("has an exponent")]);
            c_exponent(mantissa, exponent)
        } else {
            let fixed = format!("{:.*}", (digits as i32 - 1 - exponent) as usize, f);
            String::from(strip_zeros(&fixed))
        }
    };
    if !out.contains('.') {
        if out.contains('E') {
            let e = format!("{:.1e}", f);
            let (mantissa, exponent) = e.split_once('e').expect("has an exponent");
            out = c_exponent(mantissa, exponent.parse().expect("an integer exponent"));
        } else {
            out.push('.');
        }
    }
    out
}

/// `f` as a card value without losing precision: laid out as cfitsio lays
/// out `%.15G` (see [`format_g`]), with 15 significant digits when they read
/// back as `f`, and otherwise the 16 or 17 that do. So a value 15 digits
/// hold is written byte for byte as cfitsio writes it, and any other keeps
/// the digits cfitsio would round off.
fn format_float(f: f64) -> alloc::string::String {
    let mut text = format_g(f, 15);
    for digits in [16, 17] {
        if text.parse::<f64>().ok() == Some(f) {
            break;
        }
        text = format_g(f, digits);
    }
    text
}

/// `v` rounded to the 15 significant digits cfitsio's `ffpkyd` (and so
/// `fitsio`'s `write_key::<f64>`) writes. A [`Value::Float`] of the result
/// is written exactly as cfitsio writes `v`, at the cost of the precision
/// cfitsio drops; plain `v` is written without losing any.
///
/// The one exception is a value within a rounding of `f64::MAX`, whose 15
/// digits (`1.79769313486232E+308`) are past `f64::MAX`: it is returned as
/// it is, rather than as infinity.
pub fn cfitsio_rounded(v: f64) -> f64 {
    format_g(v, 15)
        .parse()
        .ok()
        .filter(|r: &f64| r.is_finite() || !v.is_finite())
        .unwrap_or(v)
}

/// `text` without trailing zeros after its decimal point, nor a bare `.`.
fn strip_zeros(text: &str) -> &str {
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.')
    } else {
        text
    }
}

/// `mantissa` with C's exponent form: `E`, a sign, and at least two digits.
fn c_exponent(mantissa: &str, exponent: i32) -> alloc::string::String {
    let sign = if exponent < 0 { '-' } else { '+' };
    alloc::format!("{mantissa}E{sign}{:02}", exponent.unsigned_abs())
}

/// Put a number's text in the value field as cfitsio does: right-justified
/// to column 30 when it fits, otherwise from column 11.
fn place_number(text: &[u8], buf: &mut [u8; 70]) {
    if text.len() <= 20 {
        right_justify(text, &mut buf[..20]);
    } else {
        let len = text.len().min(buf.len());
        buf[..len].copy_from_slice(&text[..len]);
    }
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
    fn parse_string_after_column_eleven() {
        let field = make_field("               'M31' / target");
        let (val, comment) = parse_value(&field).unwrap();
        assert_eq!(val, Value::String(String::from("M31")));
        assert_eq!(comment.unwrap(), "target");

        let field = make_field("                  ''");
        let (val, _) = parse_value(&field).unwrap();
        assert_eq!(val, Value::String(String::new()));
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

    /// The value field of `f`, trimmed.
    fn float_text(f: f64) -> alloc::string::String {
        let field = format_value(&Value::Float(f));
        alloc::string::String::from(str::from_utf8(&field).unwrap().trim())
    }

    #[test]
    fn floats_are_laid_out_as_cfitsio_writes_them() {
        // cfitsio 4.3.1's ffukyd(…, -15) text for each value.
        for (value, cfitsio) in [
            (1.0, "1."),
            (0.0, "0."),
            (-0.0, "-0."),
            (229760000.0, "229760000."),
            (2458089.5, "2458089.5"),
            (1e-20, "1.0E-20"),
            (1.5e-7, "1.5E-07"),
            (0.1, "0.1"),
            (1e15, "1.0E+15"),
            (1e16, "1.0E+16"),
            (-1.25e-5, "-1.25E-05"),
            (6.02214076e23, "6.02214076E+23"),
            (1e100, "1.0E+100"),
            (1e-5, "1.0E-05"),
            (0.0001, "0.0001"),
            (100.0, "100."),
            (1234567.0, "1234567."),
        ] {
            assert_eq!(float_text(value), cfitsio, "{value:e}");
            assert_eq!(format_g(value, 15), cfitsio, "{value:e}");
        }
        // cfitsio rounds these to 15 digits; native keeps what reads back.
        for (value, cfitsio, native) in [
            (
                -2559454.0788030702,
                "-2559454.07880307",
                "-2559454.0788030704",
            ),
            (70.04089501307288, "70.0408950130729", "70.04089501307288"),
            (2.0 / 3.0, "0.666666666666667", "0.6666666666666666"),
            (
                123456789012345678.0,
                "1.23456789012346E+17",
                "1.2345678901234568E+17",
            ),
            (
                0.00012345678901234567,
                "0.000123456789012346",
                "0.00012345678901234567",
            ),
        ] {
            assert_eq!(float_text(value), native, "{value:e}");
            assert_eq!(float_text(cfitsio_rounded(value)), cfitsio, "{value:e}");
            assert_eq!(float_text(value).parse::<f64>().unwrap(), value);
        }
    }

    #[test]
    fn cfitsio_rounding_never_overflows() {
        assert_eq!(cfitsio_rounded(f64::MAX), f64::MAX);
        assert_eq!(cfitsio_rounded(-f64::MAX), -f64::MAX);
        assert!(cfitsio_rounded(f64::NAN).is_nan());
    }

    #[test]
    fn a_float_longer_than_its_field_starts_at_column_11() {
        let field = format_value(&Value::Float(-1.2345678901234567e-100));
        let text = str::from_utf8(&field).unwrap().trim_end();
        assert_eq!(text, "-1.2345678901234567E-100");
        assert_eq!(text.parse::<f64>().unwrap(), -1.2345678901234567e-100);
    }

    #[test]
    fn format_value_field_is_70_bytes() {
        let buf = format_value(&Value::Integer(1));
        assert_eq!(buf.len(), 70);
    }
}
