use crate::errors::{Error, Result};
use crate::fitsfile::FitsFile;
use crate::hdu::FitsHdu;

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

fn get_core_hdu(file: &FitsFile, hdu: &FitsHdu) -> Result<(fitsio_pure::hdu::FitsData, usize)> {
    let fits_data = fitsio_pure::hdu::parse_fits(file.data())?;
    if hdu.hdu_index >= fits_data.len() {
        return Err(Error::Message(format!(
            "HDU index {} out of range",
            hdu.hdu_index
        )));
    }
    Ok((fits_data, hdu.hdu_index))
}

/// Trait for types that can be read from a table column.
pub trait ReadsCol: Sized {
    fn read_col(file: &FitsFile, hdu: &FitsHdu, name: &str) -> Result<Vec<Self>>;
}

/// Trait for types that can be written to a table column.
pub trait WritesCol: Sized {
    fn write_col(file: &mut FitsFile, hdu: &FitsHdu, name: &str, data: &[Self]) -> Result<()>;
}

fn find_column_index(
    cards: &[fitsio_pure::header::Card],
    name: &str,
    tfields: usize,
) -> Result<usize> {
    for i in 1..=tfields {
        let ttype_key = format!("TTYPE{}", i);
        for card in cards {
            if card.keyword_str() == ttype_key {
                if let Some(fitsio_pure::value::Value::String(ref s)) = card.value {
                    if s.trim() == name {
                        return Ok(i - 1);
                    }
                }
            }
        }
    }
    Err(Error::Message(format!("column '{}' not found", name)))
}

fn get_tfields(hdu: &fitsio_pure::hdu::Hdu) -> Result<usize> {
    match &hdu.info {
        fitsio_pure::hdu::HduInfo::BinaryTable { tfields, .. } => Ok(*tfields),
        fitsio_pure::hdu::HduInfo::AsciiTable { tfields, .. } => Ok(*tfields),
        _ => Err(Error::Message("HDU is not a table".to_string())),
    }
}

impl ReadsCol for i32 {
    fn read_col(file: &FitsFile, hdu: &FitsHdu, name: &str) -> Result<Vec<Self>> {
        let (fits_data, idx) = get_core_hdu(file, hdu)?;
        let core_hdu = &fits_data.hdus[idx];
        let tfields = get_tfields(core_hdu)?;
        let col_idx = find_column_index(&core_hdu.cards, name, tfields)?;
        let col_data = fitsio_pure::bintable::read_binary_column(file.data(), core_hdu, col_idx)?;
        match col_data {
            fitsio_pure::bintable::BinaryColumnData::Int(v) => Ok(v),
            fitsio_pure::bintable::BinaryColumnData::Short(v) => {
                Ok(v.iter().map(|&x| x as i32).collect())
            }
            fitsio_pure::bintable::BinaryColumnData::Long(v) => {
                Ok(v.iter().map(|&x| x as i32).collect())
            }
            _ => Err(Error::Message("column is not integer type".to_string())),
        }
    }
}

impl ReadsCol for i64 {
    fn read_col(file: &FitsFile, hdu: &FitsHdu, name: &str) -> Result<Vec<Self>> {
        let (fits_data, idx) = get_core_hdu(file, hdu)?;
        let core_hdu = &fits_data.hdus[idx];
        let tfields = get_tfields(core_hdu)?;
        let col_idx = find_column_index(&core_hdu.cards, name, tfields)?;
        let col_data = fitsio_pure::bintable::read_binary_column(file.data(), core_hdu, col_idx)?;
        match col_data {
            fitsio_pure::bintable::BinaryColumnData::Long(v) => Ok(v),
            fitsio_pure::bintable::BinaryColumnData::Int(v) => {
                Ok(v.iter().map(|&x| x as i64).collect())
            }
            fitsio_pure::bintable::BinaryColumnData::Short(v) => {
                Ok(v.iter().map(|&x| x as i64).collect())
            }
            _ => Err(Error::Message("column is not integer type".to_string())),
        }
    }
}

impl ReadsCol for f32 {
    fn read_col(file: &FitsFile, hdu: &FitsHdu, name: &str) -> Result<Vec<Self>> {
        let (fits_data, idx) = get_core_hdu(file, hdu)?;
        let core_hdu = &fits_data.hdus[idx];
        let tfields = get_tfields(core_hdu)?;
        let col_idx = find_column_index(&core_hdu.cards, name, tfields)?;
        let col_data = fitsio_pure::bintable::read_binary_column(file.data(), core_hdu, col_idx)?;
        match col_data {
            fitsio_pure::bintable::BinaryColumnData::Float(v) => Ok(v),
            fitsio_pure::bintable::BinaryColumnData::Double(v) => {
                Ok(v.iter().map(|&x| x as f32).collect())
            }
            _ => Err(Error::Message("column is not float type".to_string())),
        }
    }
}

impl ReadsCol for f64 {
    fn read_col(file: &FitsFile, hdu: &FitsHdu, name: &str) -> Result<Vec<Self>> {
        let (fits_data, idx) = get_core_hdu(file, hdu)?;
        let core_hdu = &fits_data.hdus[idx];
        let tfields = get_tfields(core_hdu)?;
        let col_idx = find_column_index(&core_hdu.cards, name, tfields)?;
        let col_data = fitsio_pure::bintable::read_binary_column(file.data(), core_hdu, col_idx)?;
        match col_data {
            fitsio_pure::bintable::BinaryColumnData::Double(v) => Ok(v),
            fitsio_pure::bintable::BinaryColumnData::Float(v) => {
                Ok(v.iter().map(|&x| x as f64).collect())
            }
            fitsio_pure::bintable::BinaryColumnData::Int(v) => {
                Ok(v.iter().map(|&x| x as f64).collect())
            }
            fitsio_pure::bintable::BinaryColumnData::Long(v) => {
                Ok(v.iter().map(|&x| x as f64).collect())
            }
            _ => Err(Error::Message("column is not numeric type".to_string())),
        }
    }
}

impl ReadsCol for String {
    fn read_col(file: &FitsFile, hdu: &FitsHdu, name: &str) -> Result<Vec<Self>> {
        let (fits_data, idx) = get_core_hdu(file, hdu)?;
        let core_hdu = &fits_data.hdus[idx];
        let tfields = get_tfields(core_hdu)?;
        let col_idx = find_column_index(&core_hdu.cards, name, tfields)?;
        let col_data = fitsio_pure::bintable::read_binary_column(file.data(), core_hdu, col_idx)?;
        match col_data {
            fitsio_pure::bintable::BinaryColumnData::Ascii(v) => Ok(v),
            _ => Err(Error::Message("column is not string type".to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fitsfile::FitsFile;

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
            fitsio_pure::bintable::BinaryColumnDescriptor {
                name: Some("ID".to_string()),
                repeat: 1,
                col_type: fitsio_pure::bintable::BinaryColumnType::Int,
                byte_width: 4,
            },
            fitsio_pure::bintable::BinaryColumnDescriptor {
                name: Some("VAL".to_string()),
                repeat: 1,
                col_type: fitsio_pure::bintable::BinaryColumnType::Double,
                byte_width: 8,
            },
        ];

        let col_data = vec![
            fitsio_pure::bintable::BinaryColumnData::Int(vec![10, 20, 30]),
            fitsio_pure::bintable::BinaryColumnData::Double(vec![1.5, 2.5, 3.5]),
        ];

        let hdu_bytes =
            fitsio_pure::bintable::serialize_binary_table_hdu(&columns, &col_data, 3).unwrap();

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

        let columns = vec![fitsio_pure::bintable::BinaryColumnDescriptor {
            name: Some("X".to_string()),
            repeat: 1,
            col_type: fitsio_pure::bintable::BinaryColumnType::Int,
            byte_width: 4,
        }];

        let col_data = vec![fitsio_pure::bintable::BinaryColumnData::Int(vec![1])];

        let hdu_bytes =
            fitsio_pure::bintable::serialize_binary_table_hdu(&columns, &col_data, 1).unwrap();

        let mut data = f.data().to_vec();
        data.extend_from_slice(&hdu_bytes);
        f.set_data(data);

        let hdu = f.hdu(1usize).unwrap();
        assert!(i32::read_col(&f, &hdu, "MISSING").is_err());
    }
}
