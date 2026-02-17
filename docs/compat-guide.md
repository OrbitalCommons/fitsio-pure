# Compat API: drop-in replacement for fitsio

Enable the `compat` feature to get an API that mirrors the [`fitsio`](https://github.com/simonrw/rust-fitsio) crate. In most cases, switching only requires changing your dependency and `use` paths.

## Cargo.toml

Replace your `fitsio` dependency:

```toml
# Before
[dependencies]
fitsio = "0.21"

# After
[dependencies]
fitsio-pure = { git = "https://github.com/OrbitalCommons/fitsio-pure", features = ["compat"] }
```

## Import changes

```rust
// Before (fitsio)
use fitsio::FitsFile;
use fitsio::images::{ImageDescription, ImageType, ReadImage, WriteImage};
use fitsio::hdu::HduInfo;
use fitsio::headers::ReadsKey;
use fitsio::tables::{ColumnDescription, ColumnDataDescription, ColumnDataType, ReadsCol};

// After (fitsio-pure with compat feature)
use fitsio_pure::compat::fitsfile::FitsFile;
use fitsio_pure::compat::images::{ImageDescription, ImageType, ReadImage, WriteImage};
use fitsio_pure::compat::hdu::HduInfo;
use fitsio_pure::compat::headers::ReadsKey;
use fitsio_pure::compat::tables::{ColumnDescription, ColumnDataDescription, ColumnDataType, ReadsCol};
```

## Examples

### Creating a FITS file and writing an image

```rust
use fitsio_pure::compat::fitsfile::FitsFile;
use fitsio_pure::compat::images::{ImageDescription, ImageType, WriteImage};

let mut fitsfile = FitsFile::create("output.fits")
    .overwrite()
    .open()
    .unwrap();

let description = ImageDescription {
    data_type: ImageType::Float,
    dimensions: vec![100, 100],
};

let hdu = fitsfile.create_image("SCI", &description).unwrap();

let pixels: Vec<f32> = vec![0.0; 100 * 100];
f32::write_image(&mut fitsfile, &hdu, &pixels).unwrap();
```

### Reading image data

```rust
use fitsio_pure::compat::fitsfile::FitsFile;
use fitsio_pure::compat::images::ReadImage;

let fitsfile = FitsFile::open("input.fits").unwrap();
let hdu = fitsfile.hdu("SCI").unwrap();

let pixels: Vec<f32> = f32::read_image(&fitsfile, &hdu).unwrap();
```

### Reading and writing header keywords

```rust
use fitsio_pure::compat::fitsfile::FitsFile;
use fitsio_pure::compat::headers::{ReadsKey, WritesKey};

let mut fitsfile = FitsFile::edit("data.fits").unwrap();
let hdu = fitsfile.primary_hdu().unwrap();

// Write a keyword
hdu.write_key(&mut fitsfile, "OBJECT", &"NGC 1234".to_string()).unwrap();

// Read it back
let object: String = hdu.read_key(&fitsfile, "OBJECT").unwrap();
```

### Reading table columns

```rust
use fitsio_pure::compat::fitsfile::FitsFile;
use fitsio_pure::compat::tables::ReadsCol;

let fitsfile = FitsFile::open("catalog.fits").unwrap();
let hdu = fitsfile.hdu("CATALOG").unwrap();

let ra: Vec<f64> = f64::read_col(&fitsfile, &hdu, "RA").unwrap();
let dec: Vec<f64> = f64::read_col(&fitsfile, &hdu, "DEC").unwrap();
```

## API comparison: fitsio vs fitsio-pure compat

| Operation | fitsio | fitsio-pure compat |
|---|---|---|
| Open file | `FitsFile::open(path)?` | `FitsFile::open(path)?` |
| Edit file | `FitsFile::edit(path)?` | `FitsFile::edit(path)?` |
| Create file | `FitsFile::create(path).open()?` | `FitsFile::create(path).open()?` |
| Overwrite | `FitsFile::create(path).overwrite().open()?` | `FitsFile::create(path).overwrite().open()?` |
| Get primary HDU | `f.primary_hdu()?` | `f.primary_hdu()?` |
| Get HDU by name | `f.hdu("SCI")?` | `f.hdu("SCI")?` |
| Get HDU by index | `f.hdu(0)?` | `f.hdu(0usize)?` |
| Read header key | `hdu.read_key::<T>(&mut f, name)?` | `hdu.read_key::<T>(&f, name)?` |
| Write header key | `hdu.write_key(&mut f, name, val)?` | `hdu.write_key(&mut f, name, val)?` |
| Read image | `hdu.read_image(&mut f)?` | `T::read_image(&f, &hdu)?` |
| Write image | `hdu.write_image(&mut f, &data)?` | `T::write_image(&mut f, &hdu, &data)?` |
| Read column | `hdu.read_col::<T>(&mut f, name)?` | `T::read_col(&f, &hdu, name)?` |
| HDU info | `hdu.info` | `hdu.info(&f)?` |
| Number of HDUs | `f.num_hdus()?` | `f.num_hdus()?` |

### Key differences from fitsio

- No mutable borrow required for read operations (reads take `&FitsFile`, not `&mut FitsFile`).
- Image and column read/write use associated functions (`T::read_image(...)`) instead of methods on the HDU.
- HDU info requires a reference to the file since there is no cached C-side state.
