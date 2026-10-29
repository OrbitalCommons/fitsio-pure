//! Call sites written for `fitsio` 0.21, from mwalib and mwa_hyperdrive,
//! compiled unchanged against compat through `use fitsio_pure::compat as
//! fitsio`.
#![cfg(feature = "compat")]

use std::fmt::Display;

use fitsio_pure::compat as fitsio;

use fitsio::errors::Error;
use fitsio::hdu::{DescribesHdu, FitsHdu, HduInfo};
use fitsio::images::{ImageDescription, ImageType};
use fitsio::tables::{ColumnDataType, ColumnDescription};
use fitsio::FitsFile;

/// mwalib `fits_read::_get_fits_image`.
fn get_fits_image<T: fitsio::images::ReadImage>(
    fits_fptr: &mut FitsFile,
    hdu: &FitsHdu,
) -> Result<T, Error> {
    match &hdu.info {
        HduInfo::ImageInfo { .. } => hdu.read_image(fits_fptr),
        _ => Err(Error::Message("not an image".to_string())),
    }
}

/// mwalib `fits_read::_get_fits_col`.
fn get_fits_col<T: fitsio::tables::ReadsCol>(
    fits_fptr: &mut FitsFile,
    hdu: &FitsHdu,
    keyword: &str,
) -> Result<Vec<T>, Error> {
    hdu.read_col(fits_fptr, keyword)
}

/// mwalib `fits_read::read_cell_value`.
fn read_cell_value<T: fitsio::tables::ReadsCol>(
    fits_fptr: &mut FitsFile,
    fits_tile_table_hdu: &fitsio::hdu::FitsHdu,
    col_name: &str,
    row: usize,
) -> Result<T, Error> {
    fits_tile_table_hdu.read_cell_value(fits_fptr, col_name, row)
}

/// mwa_hyperdrive `io::read::fits::fits_open_hdu`.
fn fits_open_hdu<T: DescribesHdu + Display + Copy>(
    fptr: &mut FitsFile,
    hdu_description: T,
) -> Result<FitsHdu, Error> {
    fptr.hdu(hdu_description)
}

#[test]
fn upstream_call_sites_compile_and_run() {
    let mut fptr = FitsFile::create_in_memory().unwrap();
    let image = ImageDescription {
        data_type: ImageType::Float,
        dimensions: &[2, 3],
    };
    let hdu = fptr.create_image("SCI".to_string(), &image).unwrap();
    hdu.write_image(&mut fptr, &[1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0])
        .unwrap();

    let cols = [
        ColumnDescription::new("NAME")
            .with_type(ColumnDataType::String)
            .that_repeats(8)
            .create()
            .unwrap(),
        ColumnDescription::new("RA")
            .with_type(ColumnDataType::Double)
            .create()
            .unwrap(),
    ];
    let table = fptr.create_table("SOURCES".to_string(), &cols).unwrap();
    // mwa_hyperdrive `srclist::fits::write`: names as `&str` and `String`.
    let names = vec!["a".to_string(), "b".to_string()];
    table.write_col(&mut fptr, "NAME", &names).unwrap();
    let axis = 'A';
    let table = table
        .write_col(&mut fptr, format!("R{axis}"), &[10.0f64, 20.0])
        .unwrap();
    match &table.info {
        HduInfo::TableInfo { num_rows, .. } => assert_eq!(*num_rows, 2),
        other => panic!("expected a table, got {other:?}"),
    }

    // mwalib `gpubox_files`: the HDU count.
    assert_eq!(fptr.iter().count(), 3);
    let last_hdu_index = fptr.iter().count();
    assert_eq!(last_hdu_index, 3);

    let sci = fits_open_hdu(&mut fptr, "SCI").unwrap();
    let pixels: Vec<f32> = get_fits_image(&mut fptr, &sci).unwrap();
    assert_eq!(pixels, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    let sources = fits_open_hdu(&mut fptr, 2usize).unwrap();
    let ra: Vec<f64> = get_fits_col(&mut fptr, &sources, "RA").unwrap();
    assert_eq!(ra, [10.0, 20.0]);
    let name: String = read_cell_value(&mut fptr, &sources, "NAME", 1).unwrap();
    assert_eq!(name, "b");
    let row: Vec<f32> = sci.read_row(&fptr, 1).unwrap();
    assert_eq!(row, [4.0, 5.0, 6.0]);
}
