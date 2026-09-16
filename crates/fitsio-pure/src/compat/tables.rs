use super::errors::{Error, Result};
use super::fitsfile::FitsFile;
use super::hdu::FitsHdu;
use crate::bintable::{BinaryColumnData, BinaryColumnType};

/// Describes one column in a table extension.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnDescription {
    pub name: String,
    pub data_type: ColumnDataDescription,
}

/// Describes the data type and repeat count for a column.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnDataDescription {
    pub data_type: ColumnDataType,
    pub repeat: usize,
    pub width: usize,
}

/// The supported column data types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnDataType {
    Int,
    Long,
    Float,
    Double,
    String,
    Short,
    Byte,
    Logical,
}

/// A concrete column descriptor with computed byte width.
#[derive(Debug, Clone, PartialEq)]
pub struct ConcreteColumnDescription {
    pub name: String,
    pub data_type: ColumnDataType,
    pub repeat: usize,
    pub width: usize,
}

impl ColumnDescription {
    /// Convert to a concrete descriptor.
    pub fn to_concrete(&self) -> ConcreteColumnDescription {
        ConcreteColumnDescription {
            name: self.name.clone(),
            data_type: self.data_type.data_type,
            repeat: self.data_type.repeat,
            width: self.data_type.width,
        }
    }
}

impl ColumnDataDescription {
    /// Create a new column data description with the given type and repeat=1, width=1.
    pub fn new(data_type: ColumnDataType) -> Self {
        ColumnDataDescription {
            data_type,
            repeat: 1,
            width: 1,
        }
    }

    /// Set the repeat count.
    pub fn with_repeat(mut self, repeat: usize) -> Self {
        self.repeat = repeat;
        self
    }

    /// Set the width (used for string columns).
    pub fn with_width(mut self, width: usize) -> Self {
        self.width = width;
        self
    }
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
    if hdu.hdu_index >= fits_data.len() {
        return Err(Error::Message(format!(
            "HDU index {} out of range",
            hdu.hdu_index
        )));
    }
    Ok(hdu.hdu_index)
}

/// Trait for types that can be read from a table column.
pub trait ReadsCol: Sized {
    fn read_col(file: &FitsFile, hdu: &FitsHdu, name: &str) -> Result<Vec<Self>>;
}

/// Trait for types that can be written to a table column.
pub trait WritesCol: Sized {
    fn write_col(file: &mut FitsFile, hdu: &FitsHdu, name: &str, data: &[Self]) -> Result<()>;
}

fn find_column_index(cards: &[crate::header::Card], name: &str, tfields: usize) -> Result<usize> {
    for i in 1..=tfields {
        let ttype_key = format!("TTYPE{}", i);
        for card in cards {
            if card.keyword_str() == ttype_key {
                if let Some(crate::value::Value::String(ref s)) = card.value {
                    if s.trim() == name {
                        return Ok(i - 1);
                    }
                }
            }
        }
    }
    Err(Error::Message(format!("column '{}' not found", name)))
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

fn out_of_range(name: &str) -> Error {
    Error::Message(format!("value out of range for column '{}'", name))
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
    converted.ok_or_else(|| out_of_range(name))
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
    col_type: &BinaryColumnType,
    tscal: f64,
    tzero: f64,
    name: &str,
) -> Result<BinaryColumnData> {
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
    converted.ok_or_else(|| out_of_range(name))
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
        None => crate::bintable::read_binary_column(file.data(), core_hdu, col_idx)?,
        Some((start_row, num_rows)) => crate::bintable::read_binary_column_range(
            file.data(),
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
        unscale_column(data, &columns[col_idx].col_type, tscal, tzero, name)?
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
        let col_data = crate::bintable::read_binary_column(file.data(), core_hdu, col_idx)?;
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
        let col_data = crate::bintable::read_binary_column(file.data(), core_hdu, col_idx)?;
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
                    file.data(),
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
    let (idx, col_idx) = {
        let idx = validate_hdu_index(file, hdu)?;
        let parsed = file.parsed()?;
        let core_hdu = &parsed.hdus[idx];
        let tfields = get_tfields(core_hdu)?;
        let col_idx = find_column_index(&core_hdu.cards, name, tfields)?;
        (idx, col_idx)
    };

    // Clone the HDU so nothing borrows `file` when we write back below.
    let core_hdu = file.parsed()?.hdus[idx].clone();

    let mut data = file.data().to_vec();
    crate::bintable::write_binary_column(&mut data, &core_hdu, col_idx, col_data)?;
    file.set_data(data);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compat::fitsfile::FitsFile;

    #[test]
    fn column_description_to_concrete() {
        let desc = ColumnDescription {
            name: "X".to_string(),
            data_type: ColumnDataDescription::new(ColumnDataType::Int),
        };
        let concrete = desc.to_concrete();
        assert_eq!(concrete.name, "X");
        assert_eq!(concrete.data_type, ColumnDataType::Int);
        assert_eq!(concrete.repeat, 1);
    }

    #[test]
    fn column_data_description_builder() {
        let desc = ColumnDataDescription::new(ColumnDataType::String)
            .with_repeat(20)
            .with_width(20);
        assert_eq!(desc.data_type, ColumnDataType::String);
        assert_eq!(desc.repeat, 20);
        assert_eq!(desc.width, 20);
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

        let mut data = f.data().to_vec();
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

        let mut data = f.data().to_vec();
        data.extend_from_slice(&hdu_bytes);
        f.set_data(data);

        let hdu = f.hdu(1usize).unwrap();
        assert!(i32::read_col(&f, &hdu, "MISSING").is_err());
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

        let mut data = f.data().to_vec();
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

        let mut data = f.data().to_vec();
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

        let mut data = f.data().to_vec();
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
        let raw = crate::bintable::read_binary_column(f.data(), &parsed.hdus[1], 0).unwrap();
        assert_eq!(raw, BinaryColumnData::Short(vec![32767, 7232, -32767]));
    }
}
