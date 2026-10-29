use super::errors::{Error, Result};
use super::fitsfile::FitsFile;
use super::hdu::FitsHdu;
use super::sys;
use crate::bintable::{BinaryColumnData, BinaryColumnType};

/// A column to create, built as in `fitsio`:
///
/// ```
/// use fitsio_pure::compat::tables::{ColumnDataType, ColumnDescription};
///
/// let col = ColumnDescription::new("FLUX")
///     .with_type(ColumnDataType::Float)
///     .that_repeats(4)
///     .create()?;
/// assert_eq!(col.data_type.repeat, 4);
/// # Ok::<(), fitsio_pure::compat::errors::Error>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnDescription {
    /// The column's name, its `TTYPEn`.
    pub name: String,
    /// Its data type; `None` until [`ColumnDescription::with_type`] is called.
    pub data_type: Option<ColumnDataDescription>,
}

/// A column with its data type known: what [`ColumnDescription::create`]
/// returns and [`FitsFile::create_table`] takes, and what
/// `HduInfo::TableInfo` lists for each column of a table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConcreteColumnDescription {
    /// The column's name, its `TTYPEn`.
    pub name: String,
    /// Its data type.
    pub data_type: ColumnDataDescription,
}

impl ColumnDescription {
    /// A column called `name`, with no type yet.
    pub fn new<T: Into<String>>(name: T) -> Self {
        ColumnDescription {
            name: name.into(),
            data_type: None,
        }
    }

    /// Give the column a type, as a scalar.
    pub fn with_type(&mut self, typ: ColumnDataType) -> &mut ColumnDescription {
        self.data_type = Some(ColumnDataDescription::scalar(typ));
        self
    }

    /// Make each cell hold `repeat` values. Does nothing before
    /// [`with_type`](Self::with_type).
    pub fn that_repeats(&mut self, repeat: usize) -> &mut ColumnDescription {
        if let Some(desc) = &mut self.data_type {
            desc.repeat = repeat;
        }
        self
    }

    /// Set the width of each string in a `Text` column of several, as
    /// `TFORMn = 'rAw'` does. Does nothing before
    /// [`with_type`](Self::with_type).
    pub fn with_width(&mut self, width: usize) -> &mut ColumnDescription {
        if let Some(desc) = &mut self.data_type {
            desc.width = width;
        }
        self
    }

    /// The finished column, or an error if it has no type.
    pub fn create(&self) -> Result<ConcreteColumnDescription> {
        match &self.data_type {
            Some(data_type) => Ok(ConcreteColumnDescription {
                name: self.name.clone(),
                data_type: data_type.clone(),
            }),
            None => Err(Error::Message(
                "No data type given. Ensure the `with_type` method has been called.".to_string(),
            )),
        }
    }
}

/// A column's data type and shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnDataDescription {
    /// Values per cell: the `r` in `TFORMn = 'rT'`. For a string column, its
    /// length; for a bit column, its number of bits.
    pub repeat: usize,
    /// For a `Text` column, the width of each string: the `w` in `rAw`.
    pub width: usize,
    /// The type of each value.
    pub typ: ColumnDataType,
}

impl ColumnDataDescription {
    /// A description from its parts.
    pub fn new(typ: ColumnDataType, repeat: usize, width: usize) -> Self {
        ColumnDataDescription { repeat, width, typ }
    }

    /// One value per cell.
    pub fn scalar(typ: ColumnDataType) -> Self {
        ColumnDataDescription::new(typ, 1, 1)
    }

    /// `repeat` values per cell.
    pub fn vector(typ: ColumnDataType, repeat: usize) -> Self {
        ColumnDataDescription::new(typ, repeat, 1)
    }
}

/// The `TFORMn` value, as `fitsio` renders it: `"4E"`, or `"60A12"` for a
/// string column with a width.
impl From<ColumnDataDescription> for String {
    fn from(orig: ColumnDataDescription) -> String {
        let letter = String::from(orig.typ);
        match orig.typ {
            ColumnDataType::Text if orig.width > 1 => {
                format!("{}{letter}{}", orig.repeat, orig.width)
            }
            _ => format!("{}{letter}", orig.repeat),
        }
    }
}

/// Parse a `TFORMn`-like value such as `"1J"`, `"E"` or `"60A12"`, as
/// `fitsio` does. The unsigned and signed-byte letters `U`, `V`, `W` and `S`
/// are accepted, as cfitsio accepts them.
impl std::str::FromStr for ColumnDataDescription {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let bad = || Error::Message(format!("unsupported column format {s:?}"));
        let letter_at = s.find(|c: char| !c.is_ascii_digit()).ok_or_else(bad)?;
        let number = |digits: &str| match digits {
            "" => Ok(1),
            digits => digits.parse::<usize>().map_err(|_| bad()),
        };
        let repeat = number(&s[..letter_at])?;
        let letter = s[letter_at..].chars().next().ok_or_else(bad)?;
        let rest = &s[letter_at + letter.len_utf8()..];
        let width_end = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        let width = number(&rest[..width_end])?;
        let typ = match letter {
            'L' => ColumnDataType::Logical,
            'X' => ColumnDataType::Bit,
            'B' => ColumnDataType::Byte,
            'S' => ColumnDataType::SignedByte,
            'I' => ColumnDataType::Short,
            'U' => ColumnDataType::UnsignedShort,
            'J' => ColumnDataType::Int,
            'V' => ColumnDataType::UnsignedLong,
            'K' => ColumnDataType::LongLong,
            'W' => ColumnDataType::UnsignedLongLong,
            'E' => ColumnDataType::Float,
            'D' => ColumnDataType::Double,
            'A' => ColumnDataType::String,
            _ => return Err(bad()),
        };
        Ok(ColumnDataDescription { repeat, width, typ })
    }
}

/// The types a column can hold, as in `fitsio`.
///
/// `Int` and `Long` are both 32-bit (`J`), and `Text` and `String` both
/// strings (`A`). The unsigned types and `SignedByte` are stored as cfitsio
/// stores them: as the signed (or unsigned, for bytes) type of the same
/// size, with a `TZEROn` offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnDataType {
    Logical,
    Bit,
    Byte,
    SignedByte,
    Short,
    UnsignedShort,
    Int,
    Long,
    UnsignedLong,
    Float,
    Text,
    Double,
    LongLong,
    UnsignedLongLong,
    String,
}

/// The type's `TFORMn` letter, as `fitsio` renders it: `"U"` for
/// `UnsignedShort`, `"J"` for `Int` and `Long`, and so on.
impl From<ColumnDataType> for String {
    fn from(orig: ColumnDataType) -> String {
        use ColumnDataType::*;
        match orig {
            Logical => "L",
            Bit => "X",
            Byte => "B",
            SignedByte => "S",
            Short => "I",
            UnsignedShort => "U",
            Int | Long => "J",
            UnsignedLong => "V",
            Float => "E",
            Text | String => "A",
            Double => "D",
            LongLong => "K",
            UnsignedLongLong => "W",
        }
        .to_string()
    }
}

impl ColumnDataType {
    /// How the column is stored: its binary-table type, and the `TZEROn`
    /// offset that makes the unsigned and signed-byte types, as cfitsio
    /// writes them.
    fn storage(self) -> (BinaryColumnType, Option<crate::value::Value>) {
        use crate::value::Value;
        use ColumnDataType::*;
        match self {
            Logical => (BinaryColumnType::Logical, None),
            Bit => (BinaryColumnType::Bit, None),
            Byte => (BinaryColumnType::Byte, None),
            SignedByte => (BinaryColumnType::Byte, Some(Value::Integer(-128))),
            Short => (BinaryColumnType::Short, None),
            UnsignedShort => (BinaryColumnType::Short, Some(Value::Integer(32_768))),
            Int | Long => (BinaryColumnType::Int, None),
            UnsignedLong => (BinaryColumnType::Int, Some(Value::Integer(2_147_483_648))),
            Float => (BinaryColumnType::Float, None),
            Double => (BinaryColumnType::Double, None),
            LongLong => (BinaryColumnType::Long, None),
            UnsignedLongLong => (
                BinaryColumnType::Long,
                Some(Value::Float(9_223_372_036_854_775_808.0)),
            ),
            Text | String => (BinaryColumnType::Ascii, None),
        }
    }
}

impl ConcreteColumnDescription {
    /// The description `fitsio` gives a column of an existing table, from
    /// cfitsio's `ffgbcl`: the stored type, whatever `TZEROn` is, the `TFORMn`
    /// repeat, and a width of 1.
    ///
    /// `fitsio` panics on complex and variable-length columns. Here a complex
    /// column is described as `Float` or `Double` pairs, and a variable-length
    /// one by its element type.
    pub(crate) fn of_column(column: &crate::bintable::BinaryColumnDescriptor) -> Self {
        let (typ, repeat) = match column.col_type {
            BinaryColumnType::Logical => (ColumnDataType::Logical, column.repeat),
            BinaryColumnType::Bit => (ColumnDataType::Bit, column.repeat),
            BinaryColumnType::Byte => (ColumnDataType::Byte, column.repeat),
            BinaryColumnType::Short => (ColumnDataType::Short, column.repeat),
            BinaryColumnType::Int => (ColumnDataType::Int, column.repeat),
            BinaryColumnType::Long => (ColumnDataType::LongLong, column.repeat),
            BinaryColumnType::Float => (ColumnDataType::Float, column.repeat),
            BinaryColumnType::Double => (ColumnDataType::Double, column.repeat),
            BinaryColumnType::ComplexFloat => (ColumnDataType::Float, 2 * column.repeat),
            BinaryColumnType::ComplexDouble => (ColumnDataType::Double, 2 * column.repeat),
            BinaryColumnType::Ascii => (ColumnDataType::String, column.repeat),
            BinaryColumnType::VarArrayP(element) | BinaryColumnType::VarArrayQ(element) => {
                let typ = element
                    .to_string()
                    .parse::<ColumnDataDescription>()
                    .map_or(ColumnDataType::Byte, |d| d.typ);
                (typ, column.repeat)
            }
        };
        ConcreteColumnDescription {
            name: column.name.clone().unwrap_or_default(),
            data_type: ColumnDataDescription::vector(typ, repeat),
        }
    }

    /// The description of an ASCII-table column, by the type cfitsio reads it
    /// as: `Aw` a string of `w` characters, `Iw` an `Int` (a `Short` for
    /// `w <= 4`), `Fw.d` and `Ew.d` a `Float`, `Dw.d` a `Double`.
    pub(crate) fn of_ascii_column(column: &crate::table::AsciiColumnDescriptor) -> Self {
        use crate::table::AsciiColumnFormat;
        let (typ, repeat) = match column.format {
            AsciiColumnFormat::Character(width) => (ColumnDataType::String, width),
            AsciiColumnFormat::Integer(width) if width <= 4 => (ColumnDataType::Short, 1),
            AsciiColumnFormat::Integer(_) => (ColumnDataType::Int, 1),
            AsciiColumnFormat::FloatF(..) | AsciiColumnFormat::FloatE(..) => {
                (ColumnDataType::Float, 1)
            }
            AsciiColumnFormat::DoubleE(..) => (ColumnDataType::Double, 1),
        };
        ConcreteColumnDescription {
            name: column.name.clone().unwrap_or_default(),
            data_type: ColumnDataDescription::vector(typ, repeat),
        }
    }

    /// The column as stored, with its `TZEROn` and, for a `Text` column with a
    /// width, its `rAw` `TFORMn`, as `fitsio` renders them.
    fn storage(
        &self,
    ) -> (
        crate::bintable::BinaryColumnDescriptor,
        Option<crate::value::Value>,
        Option<String>,
    ) {
        let (col_type, tzero) = self.data_type.typ.storage();
        let repeat = self.data_type.repeat;
        let byte_width = match col_type {
            BinaryColumnType::Bit => repeat.div_ceil(8),
            _ => repeat * crate::bintable::binary_type_byte_size(&col_type),
        };
        let tform = (self.data_type.typ == ColumnDataType::Text && self.data_type.width > 1)
            .then(|| String::from(self.data_type.clone()));
        let column = crate::bintable::BinaryColumnDescriptor {
            name: Some(self.name.clone()),
            repeat,
            col_type,
            byte_width,
            tdim: None,
        };
        (column, tzero, tform)
    }
}

/// The binary-table header cards for `columns`, without `EXTNAME`.
pub(crate) fn table_cards(
    columns: &[ConcreteColumnDescription],
) -> Result<Vec<crate::header::Card>> {
    let stored: Vec<_> = columns.iter().map(|c| c.storage()).collect();
    let descriptors: Vec<_> = stored.iter().map(|(d, _, _)| d.clone()).collect();
    let mut cards = crate::bintable::build_binary_table_cards(&descriptors, 0, 0)?;
    for (i, (_, tzero, tform)) in stored.into_iter().enumerate() {
        let n = i + 1;
        if let Some(tform) = tform {
            let keyword = format!("TFORM{n}");
            if let Some(card) = cards.iter_mut().find(|c| c.keyword_str() == keyword) {
                card.value = Some(crate::value::Value::String(tform));
            }
        }
        if let Some(tzero) = tzero {
            // As cfitsio does, right after the column's `TFORMn`.
            let after = format!("TFORM{n}");
            let at = cards
                .iter()
                .position(|c| c.keyword_str() == after)
                .map_or(cards.len(), |i| i + 1);
            cards.insert(at, crate::header::Card::new(&format!("TZERO{n}"), tzero)?);
        }
    }
    Ok(cards)
}

/// A typed column read from a table.
#[derive(Debug, Clone, PartialEq)]
pub enum Column {
    Int32(Vec<i32>),
    Int64(Vec<i64>),
    Float(Vec<f32>),
    Double(Vec<f64>),
    String(Vec<std::string::String>),
    Short(Vec<i16>),
    Byte(Vec<u8>),
    Logical(Vec<bool>),
}

fn validate_hdu_index(file: &FitsFile, hdu: &FitsHdu) -> Result<usize> {
    let fits_data = file.parsed()?;
    if hdu.number >= fits_data.len() {
        return Err(Error::status(sys::END_OF_FILE));
    }
    Ok(hdu.number)
}

/// Types a table column can be read as, whole or a range of rows at a time,
/// as `fitsio`'s `ReadsCol` reads them: the numeric types, `String` and
/// `bool`. Generic code bounds on this to call `read_col`, `read_col_range`
/// and `read_cell_value`.
pub trait ReadsCol: ReadsColRange {
    fn read_col(file: &FitsFile, hdu: &FitsHdu, name: &str) -> Result<Vec<Self>>;
}

/// Trait for types that can be written to a table column.
pub trait WritesCol: Sized {
    fn write_col(file: &mut FitsFile, hdu: &FitsHdu, name: &str, data: &[Self]) -> Result<()>;
}

/// The index of the column called `name`.
///
/// A column with exactly that name, as `fitsio` looks it up, comes first.
/// Failing that, `name` picks a column as cfitsio's `ffgcno` with
/// `CASEINSEN` picks it, which is how code calling cfitsio directly looks
/// columns up: ignoring case and trailing blanks, with wildcards (see
/// [`template_match`]). One exact match wins, then one wildcard match; more
/// than one is `COL_NOT_UNIQUE` (237). With no match, a `name` that is a
/// column number, such as `"3"`, picks that column; anything else is
/// `COL_NOT_FOUND` (219).
fn find_column_index(cards: &[crate::header::Card], name: &str, tfields: usize) -> Result<usize> {
    let ttypes: Vec<&str> = (1..=tfields)
        .map(|i| {
            let ttype_key = format!("TTYPE{i}");
            cards
                .iter()
                .find_map(|card| match &card.value {
                    Some(crate::value::Value::String(s)) if card.keyword_str() == ttype_key => {
                        Some(s.as_str())
                    }
                    _ => None,
                })
                .unwrap_or("")
        })
        .collect();
    if let Some(i) = ttypes.iter().position(|ttype| ttype.trim_end() == name) {
        return Ok(i);
    }
    let mut exact = None;
    let mut wildcard = None;
    let mut wildcards = 0;
    for (i, ttype) in ttypes.iter().enumerate() {
        match template_match(name, ttype) {
            Some(true) if exact.is_some() => return Err(Error::status(sys::COL_NOT_UNIQUE)),
            Some(true) => exact = Some(i),
            Some(false) => {
                wildcard.get_or_insert(i);
                wildcards += 1;
            }
            None => {}
        }
    }
    match (exact, wildcard) {
        (Some(i), _) => Ok(i),
        (None, Some(_)) if wildcards > 1 => Err(Error::status(sys::COL_NOT_UNIQUE)),
        (None, Some(i)) => Ok(i),
        (None, None) => match name.trim().parse::<usize>() {
            Ok(n) if (1..=tfields).contains(&n) => Ok(n - 1),
            _ => Err(Error::status(sys::COL_NOT_FOUND)),
        },
    }
}

/// Whether the column name `name` matches `template`, as cfitsio's `ffcmps`
/// decides it ignoring case: `Some(true)` for an exact match, `Some(false)`
/// for a match through wildcards, `None` for none.
///
/// Trailing blanks don't count. `?` matches any one character, `*` any run
/// of them, and `#` a run of digits. A wildcard character is first taken
/// literally, so `AB*DE` is an exact match for a column named `AB*DE`.
fn template_match(template: &str, name: &str) -> Option<bool> {
    let upper = |s: &str| s.trim_end_matches(' ').to_ascii_uppercase().into_bytes();
    let (t, s) = (upper(template), upper(name));
    if t == s {
        return Some(true);
    }
    // A port of the scan in `ffcmps`, backtracking to the last `*` when a
    // path through it fails.
    let (mut ti, mut si) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    loop {
        let (tc, sc) = (t.get(ti).copied(), s.get(si).copied());
        match (tc, sc) {
            (None, None) => return Some(false),
            (None, Some(_)) => {
                let (tsave, ssave) = star?;
                ti = tsave;
                si = ssave + 1;
                star = Some((tsave, ssave + 1));
                continue;
            }
            (Some(b'*'), None) if ti + 1 == t.len() => return Some(false),
            (Some(_), None) => return None,
            (Some(tc), Some(sc)) if tc == sc || tc == b'?' => {
                ti += 1;
                si += 1;
            }
            (Some(b'#'), Some(sc)) if sc.is_ascii_digit() => {
                ti += 1;
                si += 1;
                while s.get(si).is_some_and(u8::is_ascii_digit) {
                    si += 1;
                }
            }
            (Some(b'*'), Some(_)) => {
                star = Some((ti, si));
                ti += 1;
                if ti == t.len() || t[ti] == b' ' {
                    return Some(false);
                }
                let next = t[ti];
                let found = s[si..].iter().position(|&c| c == next)?;
                si += found + 1;
                ti += 1;
            }
            (Some(_), Some(_)) => {
                let (tsave, ssave) = star?;
                ti = tsave;
                si = ssave + 1;
                star = Some((tsave, ssave + 1));
            }
        }
    }
}

fn get_tfields(hdu: &crate::hdu::Hdu) -> Result<usize> {
    match &hdu.info {
        crate::hdu::HduInfo::BinaryTable { tfields, .. } => Ok(*tfields),
        crate::hdu::HduInfo::AsciiTable { tfields, .. } => Ok(*tfields),
        _ => Err(Error::Message("HDU is not a table".to_string())),
    }
}

/// A numeric Rust type that table columns convert to and from.
///
/// Conversions follow cfitsio: the physical value is `TZEROn + TSCALn * raw`,
/// integer targets truncate toward zero on read and round on write, and a value
/// that does not fit the target (or the column's storage type) is an error.
trait ColumnScalar: Copy {
    /// Exact conversion from an integer, `None` if out of range.
    fn from_int(v: i128) -> Option<Self>;
    /// Conversion from a physical value, `None` if out of range.
    fn from_f64(v: f64) -> Option<Self>;
    /// The exact integer value, `None` for floating-point types.
    fn to_int(self) -> Option<i128>;
    fn to_f64(self) -> f64;
}

macro_rules! impl_int_column_scalar {
    ($($t:ty),+) => {$(
        impl ColumnScalar for $t {
            fn from_int(v: i128) -> Option<Self> {
                Self::try_from(v).ok()
            }

            fn from_f64(v: f64) -> Option<Self> {
                if v.is_finite() {
                    Self::try_from(v.trunc() as i128).ok()
                } else {
                    None
                }
            }

            fn to_int(self) -> Option<i128> {
                Some(i128::from(self))
            }

            fn to_f64(self) -> f64 {
                self as f64
            }
        }
    )+};
}

impl_int_column_scalar!(u8, i8, i16, u16, i32, u32, i64, u64);

impl ColumnScalar for f32 {
    fn from_int(v: i128) -> Option<Self> {
        Some(v as f32)
    }

    fn from_f64(v: f64) -> Option<Self> {
        Some(v as f32)
    }

    fn to_int(self) -> Option<i128> {
        None
    }

    fn to_f64(self) -> f64 {
        f64::from(self)
    }
}

impl ColumnScalar for f64 {
    fn from_int(v: i128) -> Option<Self> {
        Some(v as f64)
    }

    fn from_f64(v: f64) -> Option<Self> {
        Some(v)
    }

    fn to_int(self) -> Option<i128> {
        None
    }

    fn to_f64(self) -> f64 {
        self
    }
}

/// `TZEROn` as an exact integer offset when the column is integer-scaled
/// (`TSCALn = 1`, integral `TZEROn`). This is how FITS stores unsigned and
/// signed-byte columns, and the exact path keeps 64-bit values precise.
fn integer_offset(tscal: f64, tzero: f64) -> Option<i128> {
    (tscal == 1.0 && tzero.fract() == 0.0 && tzero.abs() <= u64::MAX as f64)
        .then_some(tzero as i128)
}

fn physical(raw: f64, tscal: f64, tzero: f64) -> f64 {
    if tscal == 1.0 && tzero == 0.0 {
        raw
    } else {
        raw * tscal + tzero
    }
}

fn stored(value: f64, tscal: f64, tzero: f64) -> f64 {
    if tscal == 1.0 && tzero == 0.0 {
        value
    } else {
        (value - tzero) / tscal
    }
}

fn out_of_range() -> Error {
    Error::status(sys::NUM_OVERFLOW)
}

fn not_numeric(name: &str) -> Error {
    Error::Message(format!("column '{}' is not numeric type", name))
}

fn scale_ints<T: ColumnScalar>(
    raw: impl Iterator<Item = i128>,
    tscal: f64,
    tzero: f64,
) -> Option<Vec<T>> {
    match integer_offset(tscal, tzero) {
        Some(zero) => raw.map(|x| T::from_int(x + zero)).collect(),
        None => raw
            .map(|x| T::from_f64(physical(x as f64, tscal, tzero)))
            .collect(),
    }
}

fn scale_column<T: ColumnScalar>(
    data: BinaryColumnData,
    tscal: f64,
    tzero: f64,
    name: &str,
) -> Result<Vec<T>> {
    let converted = match data {
        BinaryColumnData::Byte(v) => scale_ints(v.into_iter().map(i128::from), tscal, tzero),
        BinaryColumnData::Short(v) => scale_ints(v.into_iter().map(i128::from), tscal, tzero),
        BinaryColumnData::Int(v) => scale_ints(v.into_iter().map(i128::from), tscal, tzero),
        BinaryColumnData::Long(v) => scale_ints(v.into_iter().map(i128::from), tscal, tzero),
        // As cfitsio reads them as numbers, a bit column's packed bytes.
        BinaryColumnData::Bit(rows) => {
            scale_ints(rows.into_iter().flatten().map(i128::from), tscal, tzero)
        }
        BinaryColumnData::Float(v) => v
            .into_iter()
            .map(|x| T::from_f64(physical(f64::from(x), tscal, tzero)))
            .collect(),
        BinaryColumnData::Double(v) => v
            .into_iter()
            .map(|x| T::from_f64(physical(x, tscal, tzero)))
            .collect(),
        _ => return Err(not_numeric(name)),
    };
    converted.ok_or_else(out_of_range)
}

fn narrow<T: Copy, S: TryFrom<i128>>(
    data: &[T],
    raw: impl Fn(T) -> Option<i128>,
) -> Option<Vec<S>> {
    data.iter()
        .map(|&v| raw(v).and_then(|r| S::try_from(r).ok()))
        .collect()
}

fn unscale_column<T: ColumnScalar>(
    data: &[T],
    column: &crate::bintable::BinaryColumnDescriptor,
    tscal: f64,
    tzero: f64,
    name: &str,
) -> Result<BinaryColumnData> {
    let col_type = &column.col_type;
    let offset = integer_offset(tscal, tzero);
    let raw_int = |v: T| match (v.to_int(), offset) {
        (Some(i), Some(zero)) => Some(i - zero),
        _ => {
            let raw = stored(v.to_f64(), tscal, tzero).round();
            raw.is_finite().then_some(raw as i128)
        }
    };
    let converted = match col_type {
        BinaryColumnType::Byte => narrow(data, raw_int).map(BinaryColumnData::Byte),
        BinaryColumnType::Short => narrow(data, raw_int).map(BinaryColumnData::Short),
        BinaryColumnType::Int => narrow(data, raw_int).map(BinaryColumnData::Int),
        BinaryColumnType::Long => narrow(data, raw_int).map(BinaryColumnData::Long),
        // As cfitsio writes numbers to a bit column: as its packed bytes,
        // `ceil(repeat / 8)` per row.
        BinaryColumnType::Bit => {
            let bytes_per_row = column.repeat.div_ceil(8);
            if bytes_per_row == 0 || !data.len().is_multiple_of(bytes_per_row) {
                return Err(crate::error::Error::InvalidValue.into());
            }
            narrow::<T, u8>(data, raw_int).map(|bytes| {
                BinaryColumnData::Bit(bytes.chunks(bytes_per_row).map(<[u8]>::to_vec).collect())
            })
        }
        BinaryColumnType::Float => Some(BinaryColumnData::Float(
            data.iter()
                .map(|&v| stored(v.to_f64(), tscal, tzero) as f32)
                .collect(),
        )),
        BinaryColumnType::Double => Some(BinaryColumnData::Double(
            data.iter()
                .map(|&v| stored(v.to_f64(), tscal, tzero))
                .collect(),
        )),
        _ => return Err(not_numeric(name)),
    };
    converted.ok_or_else(out_of_range)
}

fn read_scaled<T: ColumnScalar>(
    file: &FitsFile,
    hdu: &FitsHdu,
    name: &str,
    rows: Option<(usize, usize)>,
) -> Result<Vec<T>> {
    let (idx, col_idx) = resolve_column(file, hdu, name)?;
    let parsed = file.parsed()?;
    let core_hdu = &parsed.hdus[idx];
    let data = match rows {
        None => crate::bintable::read_binary_column(file.data()?, core_hdu, col_idx)?,
        Some((start_row, num_rows)) => crate::bintable::read_binary_column_range(
            file.data()?,
            core_hdu,
            col_idx,
            start_row,
            num_rows,
        )?,
    };
    let (tscal, tzero) = crate::bintable::extract_column_scaling(&core_hdu.cards, col_idx + 1);
    scale_column(data, tscal, tzero, name)
}

fn write_scaled<T: ColumnScalar>(
    file: &mut FitsFile,
    hdu: &FitsHdu,
    name: &str,
    data: &[T],
) -> Result<()> {
    let col_data = {
        let (idx, col_idx) = resolve_column(file, hdu, name)?;
        let parsed = file.parsed()?;
        let core_hdu = &parsed.hdus[idx];
        let columns =
            crate::bintable::parse_binary_table_columns(&core_hdu.cards, get_tfields(core_hdu)?)?;
        let (tscal, tzero) = crate::bintable::extract_column_scaling(&core_hdu.cards, col_idx + 1);
        unscale_column(data, &columns[col_idx], tscal, tzero, name)?
    };
    write_col_inner(file, hdu, name, &col_data)
}

macro_rules! impl_numeric_col {
    ($($t:ty),+) => {$(
        impl ReadsCol for $t {
            fn read_col(file: &FitsFile, hdu: &FitsHdu, name: &str) -> Result<Vec<Self>> {
                read_scaled(file, hdu, name, None)
            }
        }

        impl ReadsColRange for $t {
            fn read_col_range(
                file: &FitsFile,
                hdu: &FitsHdu,
                name: &str,
                start_row: usize,
                num_rows: usize,
            ) -> Result<Vec<Self>> {
                read_scaled(file, hdu, name, Some((start_row, num_rows)))
            }
        }

        impl WritesCol for $t {
            fn write_col(
                file: &mut FitsFile,
                hdu: &FitsHdu,
                name: &str,
                data: &[Self],
            ) -> Result<()> {
                write_scaled(file, hdu, name, data)
            }
        }
    )+};
}

impl_numeric_col!(u8, i8, i16, u16, i32, u32, i64, u64, f32, f64);

impl ReadsCol for String {
    fn read_col(file: &FitsFile, hdu: &FitsHdu, name: &str) -> Result<Vec<Self>> {
        let idx = validate_hdu_index(file, hdu)?;
        let parsed = file.parsed()?;
        let core_hdu = &parsed.hdus[idx];
        let tfields = get_tfields(core_hdu)?;
        let col_idx = find_column_index(&core_hdu.cards, name, tfields)?;
        let col_data = crate::bintable::read_binary_column(file.data()?, core_hdu, col_idx)?;
        match col_data {
            crate::bintable::BinaryColumnData::Ascii(v) => Ok(v),
            _ => Err(Error::Message("column is not string type".to_string())),
        }
    }
}

impl ReadsCol for bool {
    fn read_col(file: &FitsFile, hdu: &FitsHdu, name: &str) -> Result<Vec<Self>> {
        let idx = validate_hdu_index(file, hdu)?;
        let parsed = file.parsed()?;
        let core_hdu = &parsed.hdus[idx];
        let tfields = get_tfields(core_hdu)?;
        let col_idx = find_column_index(&core_hdu.cards, name, tfields)?;
        let col_data = crate::bintable::read_binary_column(file.data()?, core_hdu, col_idx)?;
        match col_data {
            crate::bintable::BinaryColumnData::Logical(v) => Ok(v),
            _ => Err(Error::Message("column is not boolean type".to_string())),
        }
    }
}

/// Trait for reading a range of rows from a table column.
pub trait ReadsColRange: Sized {
    fn read_col_range(
        file: &FitsFile,
        hdu: &FitsHdu,
        name: &str,
        start_row: usize,
        num_rows: usize,
    ) -> Result<Vec<Self>>;
}

fn resolve_column(file: &FitsFile, hdu: &FitsHdu, name: &str) -> Result<(usize, usize)> {
    let idx = validate_hdu_index(file, hdu)?;
    let parsed = file.parsed()?;
    let core_hdu = &parsed.hdus[idx];
    let tfields = get_tfields(core_hdu)?;
    let col_idx = find_column_index(&core_hdu.cards, name, tfields)?;
    Ok((idx, col_idx))
}

macro_rules! impl_reads_col_range {
    ($t:ty, $convert:expr) => {
        impl ReadsColRange for $t {
            fn read_col_range(
                file: &FitsFile,
                hdu: &FitsHdu,
                name: &str,
                start_row: usize,
                num_rows: usize,
            ) -> Result<Vec<Self>> {
                let (idx, col_idx) = resolve_column(file, hdu, name)?;
                let parsed = file.parsed()?;
                let core_hdu = &parsed.hdus[idx];
                let col_data = crate::bintable::read_binary_column_range(
                    file.data()?,
                    core_hdu,
                    col_idx,
                    start_row,
                    num_rows,
                )?;
                #[allow(clippy::redundant_closure_call)]
                $convert(col_data)
            }
        }
    };
}

impl_reads_col_range!(String, |col_data: crate::bintable::BinaryColumnData| {
    match col_data {
        crate::bintable::BinaryColumnData::Ascii(v) => Ok(v),
        _ => Err(Error::Message("column is not string type".to_string())),
    }
});

impl_reads_col_range!(bool, |col_data: crate::bintable::BinaryColumnData| {
    match col_data {
        crate::bintable::BinaryColumnData::Logical(v) => Ok(v),
        _ => Err(Error::Message("column is not boolean type".to_string())),
    }
});

// ---- WritesCol implementations ----

impl WritesCol for String {
    fn write_col(file: &mut FitsFile, hdu: &FitsHdu, name: &str, data: &[Self]) -> Result<()> {
        let col_data = crate::bintable::BinaryColumnData::Ascii(data.to_vec());
        write_col_inner(file, hdu, name, &col_data)
    }
}

fn write_col_inner(
    file: &mut FitsFile,
    hdu: &FitsHdu,
    name: &str,
    col_data: &crate::bintable::BinaryColumnData,
) -> Result<()> {
    file.check_writable()?;
    let (idx, col_idx, rows, naxis2) = {
        let idx = validate_hdu_index(file, hdu)?;
        let parsed = file.parsed()?;
        let core_hdu = &parsed.hdus[idx];
        let tfields = get_tfields(core_hdu)?;
        let col_idx = find_column_index(&core_hdu.cards, name, tfields)?;
        let columns = crate::bintable::parse_binary_table_columns(&core_hdu.cards, tfields)?;
        let rows = crate::bintable::column_data_rows(&columns[col_idx], col_data)?;
        let naxis2 = match core_hdu.info {
            crate::hdu::HduInfo::BinaryTable { naxis2, .. } => naxis2,
            _ => return Err(Error::Message("HDU is not a binary table".to_string())),
        };
        (idx, col_idx, rows, naxis2)
    };

    // As in cfitsio, writing past the last row extends the table.
    if rows > naxis2 {
        grow_table(file, idx, rows)?;
    }

    // Clone the HDU so nothing borrows `file` when we write back below.
    let core_hdu = file.parsed()?.hdus[idx].clone();

    let mut data = file.data()?.to_vec();
    crate::bintable::write_binary_column(&mut data, &core_hdu, col_idx, col_data)?;
    file.set_data(data);
    Ok(())
}

/// Extends binary table HDU `idx` to `rows` rows. The new rows are zeroed and
/// inserted after the last row, so a heap after the rows moves down with
/// `THEAP`, and `NAXIS2` records the new count.
fn grow_table(file: &mut FitsFile, idx: usize, rows: usize) -> Result<()> {
    let mut fits_data = crate::hdu::parse_fits(file.data()?)?;
    let hdu = &mut fits_data.hdus[idx];
    let (naxis1, naxis2) = match hdu.info {
        crate::hdu::HduInfo::BinaryTable { naxis1, naxis2, .. } => (naxis1, naxis2),
        _ => return Err(Error::Message("HDU is not a binary table".to_string())),
    };
    let added = (rows - naxis2) * naxis1;
    for card in &mut hdu.cards {
        match (card.keyword_str(), &card.value) {
            ("NAXIS2", _) => card.value = Some(crate::value::Value::Integer(rows as i64)),
            ("THEAP", Some(crate::value::Value::Integer(theap))) => {
                card.value = Some(crate::value::Value::Integer(theap + added as i64))
            }
            _ => {}
        }
    }

    let mut out = Vec::with_capacity(file.data()?.len() + added + crate::BLOCK_SIZE);
    for (i, hdu) in fits_data.hdus.iter().enumerate() {
        let cards: Vec<_> = hdu.cards.iter().filter(|c| !c.is_end()).cloned().collect();
        out.extend_from_slice(&crate::header::serialize_header(&cards)?);
        let raw = file
            .data()?
            .get(hdu.data_start..hdu.data_start + hdu.data_len)
            .ok_or_else(|| Error::Message("HDU data runs past the end of the file".to_string()))?;
        let start = out.len();
        if i == idx {
            let (table, heap) = raw.split_at(naxis1 * naxis2);
            out.extend_from_slice(table);
            out.resize(out.len() + added, 0);
            out.extend_from_slice(heap);
        } else {
            out.extend_from_slice(raw);
        }
        out.resize(start + crate::block::padded_byte_len(out.len() - start), 0);
    }
    file.set_data(out);
    Ok(())
}

/// Where a bit (`X`) or byte (`B`) column's bytes are, for bit reads and
/// writes.
struct BitColumn {
    /// HDU index.
    idx: usize,
    /// The byte where row 0 of the column starts.
    start: usize,
    /// The table's row length, `NAXIS1`.
    naxis1: usize,
    naxis2: usize,
    /// The column's bytes per row: `ceil(repeat / 8)` for `X`, `repeat` for `B`.
    bytes_per_row: usize,
}

impl BitColumn {
    fn find(file: &FitsFile, hdu: &FitsHdu, name: &str) -> Result<BitColumn> {
        let (idx, col_idx) = resolve_column(file, hdu, name)?;
        let core_hdu = &file.parsed()?.hdus[idx];
        let (naxis1, naxis2) = match core_hdu.info {
            crate::hdu::HduInfo::BinaryTable { naxis1, naxis2, .. } => (naxis1, naxis2),
            _ => return Err(Error::status(sys::NOT_BTABLE)),
        };
        let columns =
            crate::bintable::parse_binary_table_columns(&core_hdu.cards, get_tfields(core_hdu)?)?;
        let column = &columns[col_idx];
        let bytes_per_row = match column.col_type {
            BinaryColumnType::Bit => column.repeat.div_ceil(8),
            BinaryColumnType::Byte => column.repeat,
            _ => return Err(Error::status(sys::NOT_LOGICAL_COL)),
        };
        let offset = crate::bintable::column_offsets(&columns)[col_idx];
        Ok(BitColumn {
            idx,
            start: core_hdu.data_start + offset,
            naxis1,
            naxis2,
            bytes_per_row,
        })
    }

    /// The column bit `first_bit` of row `first_row` starts at, counting
    /// through each row's whole bytes, padding bits included, as cfitsio
    /// does. A first bit past the row's bytes is `BAD_ELEM_NUM`.
    fn first_bit(&self, first_row: usize, first_bit: usize) -> Result<usize> {
        if first_bit >= self.bytes_per_row * 8 {
            return Err(Error::status(sys::BAD_ELEM_NUM));
        }
        Ok(first_row * self.bytes_per_row * 8 + first_bit)
    }

    /// The file byte holding column bit `bit`, and its mask, most
    /// significant bit first.
    fn locate(&self, bit: usize) -> (usize, u8) {
        let byte = bit / 8;
        let (row, in_row) = (byte / self.bytes_per_row, byte % self.bytes_per_row);
        (self.start + row * self.naxis1 + in_row, 0x80 >> (bit % 8))
    }
}

/// Read `out.len()` bits from bit column `name`, from bit `first_bit` of row
/// `first_row` on (both 0-based), as cfitsio's `ffgcx` reads them: most
/// significant bit first, running on through the rest of the row's bytes,
/// then into the next row. A byte (`B`) column reads the same way.
pub(crate) fn read_bits(
    file: &FitsFile,
    hdu: &FitsHdu,
    name: &str,
    first_row: usize,
    first_bit: usize,
    out: &mut [bool],
) -> Result<()> {
    if out.is_empty() {
        return Ok(());
    }
    let column = BitColumn::find(file, hdu, name)?;
    let start = column.first_bit(first_row, first_bit)?;
    let end = start + out.len();
    if end.div_ceil(8).div_ceil(column.bytes_per_row) > column.naxis2 {
        return Err(Error::status(sys::BAD_ROW_NUM));
    }
    let data = file.data()?;
    for (bit, out) in (start..end).zip(out) {
        let (byte, mask) = column.locate(bit);
        *out = data[byte] & mask != 0;
    }
    Ok(())
}

/// Write `bits` to bit column `name` from bit `first_bit` of row `first_row`
/// on (both 0-based), as cfitsio's `ffpclx` writes them, laid out as
/// [`read_bits`] reads them. Other bits are left as they are, and writing
/// past the last row extends the table.
pub(crate) fn write_bits(
    file: &mut FitsFile,
    hdu: &FitsHdu,
    name: &str,
    first_row: usize,
    first_bit: usize,
    bits: &[bool],
) -> Result<()> {
    file.check_writable()?;
    if bits.is_empty() {
        return Ok(());
    }
    let column = BitColumn::find(file, hdu, name)?;
    let start = column.first_bit(first_row, first_bit)?;
    let end = start + bits.len();
    let rows = end.div_ceil(8).div_ceil(column.bytes_per_row);
    let column = if rows > column.naxis2 {
        grow_table(file, column.idx, rows)?;
        BitColumn::find(file, hdu, name)?
    } else {
        column
    };
    let data = file.data_mut()?;
    for (bit, &set) in (start..end).zip(bits) {
        let (byte, mask) = column.locate(bit);
        if set {
            data[byte] |= mask;
        } else {
            data[byte] &= !mask;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compat::fitsfile::FitsFile;

    /// A scalar column of `typ`, or a vector of `repeat` values.
    fn concrete(name: &str, typ: ColumnDataType, repeat: usize) -> ConcreteColumnDescription {
        ColumnDescription::new(name)
            .with_type(typ)
            .that_repeats(repeat)
            .create()
            .unwrap()
    }

    #[test]
    fn column_descriptions_build_as_in_fitsio() {
        let col = ColumnDescription::new("X")
            .with_type(ColumnDataType::Text)
            .that_repeats(60)
            .with_width(12)
            .create()
            .unwrap();
        assert_eq!(col.name, "X");
        assert_eq!(
            col.data_type,
            ColumnDataDescription::new(ColumnDataType::Text, 60, 12)
        );
        assert_eq!(String::from(col.data_type), "60A12");
        // As in `fitsio`, only `Text` renders its width.
        let string = ColumnDataDescription::new(ColumnDataType::String, 60, 12);
        assert_eq!(String::from(string), "60A");
        // Repeat and width are ignored until there is a type.
        let mut untyped = ColumnDescription::new("Y");
        untyped.that_repeats(3).with_width(2);
        assert_eq!(untyped.data_type, None);
        assert!(untyped.create().is_err());
    }

    #[test]
    fn column_data_descriptions_parse_and_render_as_in_fitsio() {
        for (text, typ, repeat, width) in [
            ("1J", ColumnDataType::Int, 1, 1),
            ("E", ColumnDataType::Float, 1, 1),
            ("16X", ColumnDataType::Bit, 16, 1),
            ("3U", ColumnDataType::UnsignedShort, 3, 1),
            ("60A12", ColumnDataType::String, 60, 12),
            ("2W", ColumnDataType::UnsignedLongLong, 2, 1),
        ] {
            let parsed: ColumnDataDescription = text.parse().unwrap();
            assert_eq!(
                parsed,
                ColumnDataDescription::new(typ, repeat, width),
                "{text}"
            );
        }
        assert!("2Q".parse::<ColumnDataDescription>().is_err());
        assert!("".parse::<ColumnDataDescription>().is_err());
        assert_eq!(String::from(ColumnDataType::Long), "J");
        assert_eq!(String::from(ColumnDataType::Text), "A");
        assert_eq!(
            String::from(ColumnDataDescription::vector(ColumnDataType::Double, 4)),
            "4D"
        );
    }

    #[test]
    fn create_table_writes_every_column_type_as_cfitsio_does() {
        use ColumnDataType::*;
        let mut f = FitsFile::create_in_memory().unwrap();
        let types = [
            Logical,
            Bit,
            Byte,
            SignedByte,
            Short,
            UnsignedShort,
            Int,
            Long,
            UnsignedLong,
            Float,
            Text,
            Double,
            LongLong,
            UnsignedLongLong,
            String,
        ];
        let columns: Vec<_> = types
            .iter()
            .enumerate()
            .map(|(i, &typ)| concrete(&format!("C{i}"), typ, 2))
            .collect();
        let hdu = f.create_table("ALL".to_string(), &columns).unwrap();
        let cards = &f.parsed().unwrap().hdus[1].cards;
        let value = |key: &str| {
            cards
                .iter()
                .find(|c| c.keyword_str() == key)
                .and_then(|c| c.value.clone())
        };
        use crate::value::Value;
        for (n, tform, tzero) in [
            (1, "2L", None),
            (2, "2X", None),
            (3, "2B", None),
            (4, "2B", Some(Value::Integer(-128))),
            (6, "2I", Some(Value::Integer(32768))),
            (9, "2J", Some(Value::Integer(2_147_483_648))),
            (14, "2K", Some(Value::Float(9_223_372_036_854_775_808.0))),
            (15, "2A", None),
        ] {
            assert_eq!(
                value(&format!("TFORM{n}")),
                Some(Value::String(tform.to_string()))
            );
            assert_eq!(value(&format!("TZERO{n}")), tzero, "TZERO{n}");
        }
        match hdu.info(&f).unwrap() {
            crate::compat::hdu::HduInfo::TableInfo {
                column_descriptions,
                num_rows,
            } => {
                assert_eq!(num_rows, 0);
                // Read back, a column is described by how it is stored, as
                // cfitsio's ffgbcl describes it.
                let stored: Vec<_> = column_descriptions
                    .iter()
                    .map(|c| c.data_type.typ)
                    .collect();
                assert_eq!(
                    stored,
                    [
                        Logical, Bit, Byte, Byte, Short, Short, Int, Int, Int, Float, String,
                        Double, LongLong, LongLong, String
                    ]
                );
                assert!(column_descriptions.iter().all(|c| c.data_type.repeat == 2));
            }
            other => panic!("expected TableInfo, got {other:?}"),
        }
    }

    #[test]
    fn unsigned_columns_round_trip_through_create_table() {
        let mut f = FitsFile::create_in_memory().unwrap();
        let columns = [
            concrete("U16", ColumnDataType::UnsignedShort, 1),
            concrete("U32", ColumnDataType::UnsignedLong, 1),
            concrete("I8", ColumnDataType::SignedByte, 1),
        ];
        let hdu = f.create_table("U", &columns).unwrap();
        hdu.write_col(&mut f, "U16", &[0u16, 65535]).unwrap();
        hdu.write_col(&mut f, "U32", &[0u32, 4_000_000_000])
            .unwrap();
        hdu.write_col(&mut f, "I8", &[-128i8, 127]).unwrap();
        assert_eq!(hdu.read_col::<u16>(&f, "U16").unwrap(), [0, 65535]);
        assert_eq!(hdu.read_col::<u32>(&f, "U32").unwrap(), [0, 4_000_000_000]);
        assert_eq!(hdu.read_col::<i8>(&f, "I8").unwrap(), [-128, 127]);
    }

    #[test]
    fn create_table_and_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("table.fits");
        let mut f = FitsFile::create(&path).open().unwrap();

        let columns = vec![
            crate::bintable::BinaryColumnDescriptor {
                name: Some("ID".to_string()),
                repeat: 1,
                col_type: crate::bintable::BinaryColumnType::Int,
                byte_width: 4,
                tdim: None,
            },
            crate::bintable::BinaryColumnDescriptor {
                name: Some("VAL".to_string()),
                repeat: 1,
                col_type: crate::bintable::BinaryColumnType::Double,
                byte_width: 8,
                tdim: None,
            },
        ];

        let col_data = vec![
            crate::bintable::BinaryColumnData::Int(vec![10, 20, 30]),
            crate::bintable::BinaryColumnData::Double(vec![1.5, 2.5, 3.5]),
        ];

        let hdu_bytes =
            crate::bintable::serialize_binary_table_hdu(&columns, &col_data, 3).unwrap();

        let mut data = f.data().unwrap().to_vec();
        data.extend_from_slice(&hdu_bytes);
        f.set_data(data);

        let hdu = f.hdu(1usize).unwrap();

        let ids: Vec<i32> = i32::read_col(&f, &hdu, "ID").unwrap();
        assert_eq!(ids, vec![10, 20, 30]);

        let vals: Vec<f64> = f64::read_col(&f, &hdu, "VAL").unwrap();
        assert!((vals[0] - 1.5).abs() < 1e-10);
        assert!((vals[1] - 2.5).abs() < 1e-10);
        assert!((vals[2] - 3.5).abs() < 1e-10);
    }

    #[test]
    fn read_missing_column_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("table.fits");
        let mut f = FitsFile::create(&path).open().unwrap();

        let columns = vec![crate::bintable::BinaryColumnDescriptor {
            name: Some("X".to_string()),
            repeat: 1,
            col_type: crate::bintable::BinaryColumnType::Int,
            byte_width: 4,
            tdim: None,
        }];

        let col_data = vec![crate::bintable::BinaryColumnData::Int(vec![1])];

        let hdu_bytes =
            crate::bintable::serialize_binary_table_hdu(&columns, &col_data, 1).unwrap();

        let mut data = f.data().unwrap().to_vec();
        data.extend_from_slice(&hdu_bytes);
        f.set_data(data);

        let hdu = f.hdu(1usize).unwrap();
        match i32::read_col(&f, &hdu, "MISSING") {
            Err(Error::Fits(e)) => assert_eq!(e.status, sys::COL_NOT_FOUND as i32),
            other => panic!("expected COL_NOT_FOUND, got {other:?}"),
        }
    }

    #[test]
    fn columns_are_found_as_ffgcno_finds_them() {
        let names = ["Corrections", "TILE1", "TILE22", "flag", "FLAG"];
        let ttypes: Vec<crate::header::Card> = names
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let value = crate::value::Value::String(name.to_string());
                crate::header::Card::new(&format!("TTYPE{}", i + 1), value).unwrap()
            })
            .collect();
        let find = |name| find_column_index(&ttypes, name, names.len());
        let status = |name| match find(name) {
            Err(Error::Fits(e)) => e.status as u32,
            other => panic!("{name:?}: expected a status, got {other:?}"),
        };
        assert_eq!(find("corrections").unwrap(), 0);
        assert_eq!(find("CORRECTIONS  ").unwrap(), 0);
        assert_eq!(find("Corr*ns").unwrap(), 0);
        assert_eq!(find("tile?").unwrap(), 1);
        assert_eq!(find("T*22").unwrap(), 2);
        assert_eq!(status("tile#"), sys::COL_NOT_UNIQUE);
        // An exact name wins, then only one case-insensitive match may.
        assert_eq!(find("flag").unwrap(), 3);
        assert_eq!(find("FLAG").unwrap(), 4);
        assert_eq!(status("Flag"), sys::COL_NOT_UNIQUE);
        assert_eq!(find("2").unwrap(), 1);
        assert_eq!(status("6"), sys::COL_NOT_FOUND);
        assert_eq!(status("missing"), sys::COL_NOT_FOUND);
    }

    #[test]
    fn templates_match_as_ffcmps_matches_them() {
        for (template, name, want) in [
            ("ab", "AB", Some(true)),
            ("AB*DE", "AB*DE", Some(true)),
            ("AB*DE", "ABCDE", Some(false)),
            ("a*b*c", "aXbYbZc", Some(false)),
            ("a*c", "abcbd", None),
            ("*", "anything", Some(false)),
            ("x*", "x", Some(false)),
            ("x?", "x", None),
            ("col#", "COL123", Some(false)),
            ("col#x", "COL12x", Some(false)),
            ("col#", "COLx", None),
            ("", "", Some(true)),
        ] {
            assert_eq!(
                template_match(template, name),
                want,
                "{template:?} {name:?}"
            );
        }
    }

    fn make_test_table(f: &mut FitsFile) -> FitsHdu {
        let columns = vec![
            crate::bintable::BinaryColumnDescriptor {
                name: Some("ID".to_string()),
                repeat: 1,
                col_type: crate::bintable::BinaryColumnType::Int,
                byte_width: 4,
                tdim: None,
            },
            crate::bintable::BinaryColumnDescriptor {
                name: Some("VAL".to_string()),
                repeat: 1,
                col_type: crate::bintable::BinaryColumnType::Double,
                byte_width: 8,
                tdim: None,
            },
        ];

        let col_data = vec![
            crate::bintable::BinaryColumnData::Int(vec![10, 20, 30, 40, 50]),
            crate::bintable::BinaryColumnData::Double(vec![1.5, 2.5, 3.5, 4.5, 5.5]),
        ];

        let hdu_bytes =
            crate::bintable::serialize_binary_table_hdu(&columns, &col_data, 5).unwrap();

        let mut data = f.data().unwrap().to_vec();
        data.extend_from_slice(&hdu_bytes);
        f.set_data(data);

        f.hdu(1usize).unwrap()
    }

    #[test]
    fn read_col_range_middle_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("table.fits");
        let mut f = FitsFile::create(&path).open().unwrap();
        let hdu = make_test_table(&mut f);

        let ids: Vec<i32> = i32::read_col_range(&f, &hdu, "ID", 1, 3).unwrap();
        assert_eq!(ids, vec![20, 30, 40]);

        let vals: Vec<f64> = f64::read_col_range(&f, &hdu, "VAL", 2, 2).unwrap();
        assert!((vals[0] - 3.5).abs() < 1e-10);
        assert!((vals[1] - 4.5).abs() < 1e-10);
    }

    #[test]
    fn read_col_range_first_row() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("table.fits");
        let mut f = FitsFile::create(&path).open().unwrap();
        let hdu = make_test_table(&mut f);

        let ids: Vec<i32> = i32::read_col_range(&f, &hdu, "ID", 0, 1).unwrap();
        assert_eq!(ids, vec![10]);
    }

    #[test]
    fn read_col_range_out_of_bounds() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("table.fits");
        let mut f = FitsFile::create(&path).open().unwrap();
        let hdu = make_test_table(&mut f);

        assert!(i32::read_col_range(&f, &hdu, "ID", 3, 5).is_err());
    }

    #[test]
    fn write_col_i32() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("table.fits");
        let mut f = FitsFile::create(&path).open().unwrap();
        let hdu = make_test_table(&mut f);

        let new_data = vec![100, 200, 300, 400, 500];
        i32::write_col(&mut f, &hdu, "ID", &new_data).unwrap();

        let read_back: Vec<i32> = i32::read_col(&f, &hdu, "ID").unwrap();
        assert_eq!(read_back, new_data);
    }

    #[test]
    fn write_col_f64() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("table.fits");
        let mut f = FitsFile::create(&path).open().unwrap();
        let hdu = make_test_table(&mut f);

        let new_data = vec![10.5, 20.5, 30.5, 40.5, 50.5];
        f64::write_col(&mut f, &hdu, "VAL", &new_data).unwrap();

        let read_back: Vec<f64> = f64::read_col(&f, &hdu, "VAL").unwrap();
        assert_eq!(read_back, new_data);
    }

    #[test]
    fn write_col_preserves_other_columns() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("table.fits");
        let mut f = FitsFile::create(&path).open().unwrap();
        let hdu = make_test_table(&mut f);

        // Write new values to ID column
        i32::write_col(&mut f, &hdu, "ID", &[100, 200, 300, 400, 500]).unwrap();

        // VAL column should be unchanged
        let vals: Vec<f64> = f64::read_col(&f, &hdu, "VAL").unwrap();
        assert!((vals[0] - 1.5).abs() < 1e-10);
        assert!((vals[4] - 5.5).abs() < 1e-10);
    }

    #[test]
    fn hdu_write_col_method() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("table.fits");
        let mut f = FitsFile::create(&path).open().unwrap();
        let hdu = make_test_table(&mut f);

        hdu.write_col(&mut f, "ID", &[99i32, 88, 77, 66, 55])
            .unwrap();
        let read_back: Vec<i32> = hdu.read_col(&f, "ID").unwrap();
        assert_eq!(read_back, vec![99, 88, 77, 66, 55]);
    }

    fn column(
        name: &str,
        col_type: BinaryColumnType,
        byte_width: usize,
    ) -> crate::bintable::BinaryColumnDescriptor {
        crate::bintable::BinaryColumnDescriptor {
            name: Some(name.to_string()),
            repeat: 1,
            col_type,
            byte_width,
            tdim: None,
        }
    }

    #[test]
    fn write_col_grows_a_new_table() {
        let mut f = FitsFile::create_in_memory().unwrap();
        let hdu = f
            .create_table(
                "T",
                &[
                    concrete("A", ColumnDataType::Int, 1),
                    concrete("B", ColumnDataType::Double, 1),
                ],
            )
            .unwrap();
        hdu.write_col(&mut f, "A", &[1i32, 2, 3, 4, 5]).unwrap();
        hdu.write_col(&mut f, "B", &[0.5f64, 1.5, 2.5]).unwrap();

        let a: Vec<i32> = hdu.read_col(&f, "A").unwrap();
        let b: Vec<f64> = hdu.read_col(&f, "B").unwrap();
        assert_eq!(a, vec![1, 2, 3, 4, 5]);
        assert_eq!(b, vec![0.5, 1.5, 2.5, 0.0, 0.0]);
        assert_eq!(
            hdu.info(&f).unwrap(),
            crate::compat::hdu::HduInfo::TableInfo {
                column_descriptions: vec![
                    concrete("A", ColumnDataType::Int, 1),
                    concrete("B", ColumnDataType::Double, 1)
                ],
                num_rows: 5
            }
        );
    }

    #[test]
    fn write_col_with_fewer_values_than_rows_leaves_later_rows() {
        let mut f = FitsFile::create_in_memory().unwrap();
        let hdu = make_test_table(&mut f);
        hdu.write_col(&mut f, "ID", &[7i32, 8]).unwrap();
        let ids: Vec<i32> = hdu.read_col(&f, "ID").unwrap();
        assert_eq!(ids, vec![7, 8, 30, 40, 50]);
    }

    #[test]
    fn write_col_rejects_a_partial_row() {
        let mut f = FitsFile::create_in_memory().unwrap();
        let wide = concrete("V", ColumnDataType::Int, 3);
        let hdu = f.create_table("T", &[wide]).unwrap();
        assert!(hdu.write_col(&mut f, "V", &[1i32, 2, 3, 4]).is_err());
        hdu.write_col(&mut f, "V", &[1i32, 2, 3, 4, 5, 6]).unwrap();
        let v: Vec<i32> = hdu.read_col(&f, "V").unwrap();
        assert_eq!(v, vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn growing_a_table_keeps_its_heap() {
        let mut f = FitsFile::create_in_memory().unwrap();
        let columns = vec![
            column("ID", BinaryColumnType::Int, 4),
            column("SPEC", BinaryColumnType::VarArrayP('J'), 8),
        ];
        // Two rows of (ID, P descriptor), then a heap holding [1, 2, 3] and [4].
        let rows: [(i32, i32, i32); 2] = [(10, 3, 0), (20, 1, 12)];
        let heap: Vec<u8> = [1i32, 2, 3, 4]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect();
        let mut table: Vec<u8> = rows
            .iter()
            .flat_map(|(id, count, offset)| [id, count, offset].map(|v| v.to_be_bytes()).concat())
            .collect();
        table.extend_from_slice(&heap);
        table.resize(crate::block::padded_byte_len(table.len()), 0);
        let cards = crate::bintable::build_binary_table_cards(&columns, 2, heap.len()).unwrap();
        let mut data = f.data().unwrap().to_vec();
        data.extend_from_slice(&crate::header::serialize_header(&cards).unwrap());
        data.extend_from_slice(&table);
        f.set_data(data);
        let hdu = f.hdu(1usize).unwrap();
        let parsed = f.parsed().unwrap();
        let spec =
            crate::bintable::read_binary_column_vla(f.data().unwrap(), &parsed.hdus[1], 1).unwrap();
        assert_eq!(spec, BinaryColumnData::VarInt(vec![vec![1, 2, 3], vec![4]]));

        hdu.write_col(&mut f, "ID", &[10i32, 20, 30, 40]).unwrap();

        let ids: Vec<i32> = hdu.read_col(&f, "ID").unwrap();
        assert_eq!(ids, vec![10, 20, 30, 40]);
        let parsed = f.parsed().unwrap();
        let spec =
            crate::bintable::read_binary_column_vla(f.data().unwrap(), &parsed.hdus[1], 1).unwrap();
        assert_eq!(
            spec,
            BinaryColumnData::VarInt(vec![vec![1, 2, 3], vec![4], vec![], vec![]])
        );
    }

    #[test]
    fn hdu_read_col_range_method() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("table.fits");
        let mut f = FitsFile::create(&path).open().unwrap();
        let hdu = make_test_table(&mut f);

        let vals: Vec<f64> = hdu.read_col_range(&f, "VAL", 0, 3).unwrap();
        assert_eq!(vals.len(), 3);
        assert!((vals[0] - 1.5).abs() < 1e-10);
        assert!((vals[2] - 3.5).abs() < 1e-10);
    }

    fn make_typed_table(f: &mut FitsFile) -> FitsHdu {
        let columns = vec![
            crate::bintable::BinaryColumnDescriptor {
                name: Some("FLAG".to_string()),
                repeat: 1,
                col_type: BinaryColumnType::Logical,
                byte_width: 1,
                tdim: None,
            },
            crate::bintable::BinaryColumnDescriptor {
                name: Some("SHORT".to_string()),
                repeat: 1,
                col_type: BinaryColumnType::Short,
                byte_width: 2,
                tdim: None,
            },
            crate::bintable::BinaryColumnDescriptor {
                name: Some("FLOAT".to_string()),
                repeat: 1,
                col_type: BinaryColumnType::Float,
                byte_width: 4,
                tdim: None,
            },
        ];

        let col_data = vec![
            BinaryColumnData::Logical(vec![true, false, true, true]),
            BinaryColumnData::Short(vec![-1, 0, 255, 300]),
            BinaryColumnData::Float(vec![1.5, -2.5, 2.4, 3.0e9]),
        ];

        let hdu_bytes =
            crate::bintable::serialize_binary_table_hdu(&columns, &col_data, 4).unwrap();

        let mut data = f.data().unwrap().to_vec();
        data.extend_from_slice(&hdu_bytes);
        f.set_data(data);

        f.hdu(1usize).unwrap()
    }

    #[test]
    fn read_bool_column() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = FitsFile::create(dir.path().join("t.fits")).open().unwrap();
        let hdu = make_typed_table(&mut f);

        let flags: Vec<bool> = hdu.read_col(&f, "FLAG").unwrap();
        assert_eq!(flags, vec![true, false, true, true]);

        let range: Vec<bool> = hdu.read_col_range(&f, "FLAG", 1, 2).unwrap();
        assert_eq!(range, vec![false, true]);

        assert!(bool::read_col(&f, &hdu, "SHORT").is_err());
    }

    #[test]
    fn read_integer_column_as_other_types() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = FitsFile::create(dir.path().join("t.fits")).open().unwrap();
        let hdu = make_typed_table(&mut f);

        let wide: Vec<i64> = hdu.read_col(&f, "SHORT").unwrap();
        assert_eq!(wide, vec![-1, 0, 255, 300]);

        let float: Vec<f32> = hdu.read_col(&f, "SHORT").unwrap();
        assert_eq!(float, vec![-1.0, 0.0, 255.0, 300.0]);

        // -1 and 300 do not fit in a u8; the middle two rows do.
        assert!(u8::read_col(&f, &hdu, "SHORT").is_err());
        let narrow: Vec<u8> = hdu.read_col_range(&f, "SHORT", 1, 2).unwrap();
        assert_eq!(narrow, vec![0, 255]);
    }

    #[test]
    fn read_float_column_as_integer_truncates() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = FitsFile::create(dir.path().join("t.fits")).open().unwrap();
        let hdu = make_typed_table(&mut f);

        let ints: Vec<i64> = hdu.read_col(&f, "FLOAT").unwrap();
        assert_eq!(ints, vec![1, -2, 2, 3_000_000_000]);

        assert!(i32::read_col(&f, &hdu, "FLOAT").is_err());
    }

    #[test]
    fn write_checks_storage_range() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = FitsFile::create(dir.path().join("t.fits")).open().unwrap();
        let hdu = make_typed_table(&mut f);

        assert!(i64::write_col(&mut f, &hdu, "SHORT", &[1, 2, 3, 40000]).is_err());
        let unchanged: Vec<i16> = hdu.read_col(&f, "SHORT").unwrap();
        assert_eq!(unchanged, vec![-1, 0, 255, 300]);

        u32::write_col(&mut f, &hdu, "SHORT", &[0, 1, 2, 32767]).unwrap();
        let read_back: Vec<i16> = hdu.read_col(&f, "SHORT").unwrap();
        assert_eq!(read_back, vec![0, 1, 2, 32767]);
    }

    #[test]
    fn write_float_into_integer_column_rounds() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = FitsFile::create(dir.path().join("t.fits")).open().unwrap();
        let hdu = make_typed_table(&mut f);

        f64::write_col(&mut f, &hdu, "SHORT", &[2.5, -2.5, 0.4, -0.6]).unwrap();
        let read_back: Vec<i16> = hdu.read_col(&f, "SHORT").unwrap();
        assert_eq!(read_back, vec![3, -3, 0, -1]);
    }

    /// A `TFORM I` column with `TZERO1 = 32768`: the FITS encoding of `u16`.
    fn make_unsigned_table(f: &mut FitsFile) -> FitsHdu {
        let columns = vec![crate::bintable::BinaryColumnDescriptor {
            name: Some("U16".to_string()),
            repeat: 1,
            col_type: BinaryColumnType::Short,
            byte_width: 2,
            tdim: None,
        }];
        let mut cards = crate::bintable::build_binary_table_cards(&columns, 3, 0).unwrap();
        let mut keyword = [b' '; 8];
        keyword[..6].copy_from_slice(b"TZERO1");
        cards.push(crate::header::Card {
            keyword,
            value: Some(crate::value::Value::Integer(32768)),
            comment: None,
        });

        let mut hdu_bytes = crate::header::serialize_header(&cards).unwrap();
        for raw in [-32768i16, -1, 32767] {
            hdu_bytes.extend_from_slice(&raw.to_be_bytes());
        }
        hdu_bytes.resize(hdu_bytes.len().div_ceil(2880) * 2880, 0);

        let mut data = f.data().unwrap().to_vec();
        data.extend_from_slice(&hdu_bytes);
        f.set_data(data);

        f.hdu(1usize).unwrap()
    }

    #[test]
    fn tzero_column_reads_and_writes_unsigned() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = FitsFile::create(dir.path().join("t.fits")).open().unwrap();
        let hdu = make_unsigned_table(&mut f);

        let values: Vec<u16> = hdu.read_col(&f, "U16").unwrap();
        assert_eq!(values, vec![0, 32767, 65535]);
        let wide: Vec<i64> = hdu.read_col(&f, "U16").unwrap();
        assert_eq!(wide, vec![0, 32767, 65535]);
        assert!(i16::read_col(&f, &hdu, "U16").is_err());

        u16::write_col(&mut f, &hdu, "U16", &[65535, 40000, 1]).unwrap();
        let parsed = f.parsed().unwrap();
        let raw =
            crate::bintable::read_binary_column(f.data().unwrap(), &parsed.hdus[1], 0).unwrap();
        assert_eq!(raw, BinaryColumnData::Short(vec![32767, 7232, -32767]));
    }

    fn status(err: Error) -> u32 {
        match err {
            Error::Fits(e) => e.status as u32,
            other => panic!("expected a cfitsio status, got {other:?}"),
        }
    }

    #[test]
    fn numbers_read_and_write_a_bit_column_as_its_bytes() {
        // mwa_hyperdrive's solutions `Flag` column is `1X`, written as `i32`s.
        let mut f = FitsFile::create_in_memory().unwrap();
        let hdu = f
            .create_table("TILES", &[concrete("Flag", ColumnDataType::Bit, 1)])
            .unwrap();
        hdu.write_col(&mut f, "Flag", &[0i32, 1, 1, 0]).unwrap();
        assert_eq!(hdu.read_col::<i32>(&f, "Flag").unwrap(), [0, 1, 1, 0]);
        assert_eq!(hdu.read_col::<u8>(&f, "Flag").unwrap(), [0, 1, 1, 0]);
        // As in cfitsio, the number is the byte: 1 sets the last bit, not the
        // first, and a number a byte can't hold doesn't fit.
        let mut first = [true];
        hdu.read_col_bit(&f, "Flag", 1, 0, &mut first).unwrap();
        assert_eq!(first, [false]);
        let err = hdu.write_col(&mut f, "Flag", &[256i32]).unwrap_err();
        assert_eq!(status(err), sys::NUM_OVERFLOW);
        // A 12-bit column is two bytes a row.
        let wide = f
            .create_table("W", &[concrete("B", ColumnDataType::Bit, 12)])
            .unwrap();
        assert!(wide.write_col(&mut f, "B", &[1u8, 2, 3]).is_err());
        wide.write_col(&mut f, "B", &[0xA5u8, 0x0F]).unwrap();
        let mut bits = [false; 12];
        wide.read_col_bit(&f, "B", 0, 0, &mut bits).unwrap();
        let want = [1, 0, 1, 0, 0, 1, 0, 1, 0, 0, 0, 0].map(|b| b == 1);
        assert_eq!(bits, want);
    }

    #[test]
    fn bits_are_written_and_read_as_ffpclx_and_ffgcx_do() {
        let mut f = FitsFile::create_in_memory().unwrap();
        let hdu = f
            .create_table("FLAGS", &[concrete("FLAGS", ColumnDataType::Bit, 12)])
            .unwrap();
        let pattern: Vec<bool> = (0..20).map(|i| i % 3 == 0).collect();
        // From bit 4 of row 5: 12 bits fill row 5's two bytes, padding bits
        // included, and the last 8 start row 6. The table grows to 7 rows.
        hdu.write_col_bit(&mut f, "FLAGS", 5, 4, &pattern).unwrap();
        let info = hdu.info(&f).unwrap();
        assert!(matches!(
            info,
            crate::compat::hdu::HduInfo::TableInfo { num_rows: 7, .. }
        ));
        assert_eq!(
            hdu.read_col::<u8>(&f, "FLAGS").unwrap()[10..],
            [0x09, 0x24, 0x92, 0x00]
        );
        let mut back = vec![false; 20];
        hdu.read_col_bit(&f, "FLAGS", 5, 4, &mut back).unwrap();
        assert_eq!(back, pattern);
        // Bits not written keep their values.
        hdu.write_col_bit(&mut f, "FLAGS", 5, 5, &[true]).unwrap();
        assert_eq!(hdu.read_col::<u8>(&f, "FLAGS").unwrap()[10], 0x0D);

        let mut one = [false];
        let err = hdu.read_col_bit(&f, "FLAGS", 0, 16, &mut one).unwrap_err();
        assert_eq!(status(err), sys::BAD_ELEM_NUM);
        let mut many = vec![false; 17];
        let err = hdu.read_col_bit(&f, "FLAGS", 6, 0, &mut many).unwrap_err();
        assert_eq!(status(err), sys::BAD_ROW_NUM);
        let ids = f
            .create_table("IDS", &[concrete("ID", ColumnDataType::Int, 1)])
            .unwrap();
        let err = ids.write_col_bit(&mut f, "ID", 0, 0, &[true]).unwrap_err();
        assert_eq!(status(err), sys::NOT_LOGICAL_COL);
    }
}
