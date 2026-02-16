use std::path::Path;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Backend trait -- each FITS library implements this
// ---------------------------------------------------------------------------

trait FitsBackend {
    fn name() -> &'static str;

    fn write_f32_image(path: &Path, shape: &[usize], data: &[f32]);
    fn write_f64_image(path: &Path, shape: &[usize], data: &[f64]);
    fn write_i32_image(path: &Path, shape: &[usize], data: &[i32]);

    fn read_f32_image(path: &Path) -> Vec<f32>;
    fn read_f64_image(path: &Path) -> Vec<f64>;
    fn read_i32_image(path: &Path) -> Vec<i32>;
}

// ---------------------------------------------------------------------------
// fitsio-pure compat backend
// ---------------------------------------------------------------------------

#[cfg(feature = "pure")]
mod pure_backend {
    use super::*;
    use fitsio_pure::compat::fitsfile::FitsFile;
    use fitsio_pure::compat::images::{ImageDescription, ImageType, ReadImage, WriteImage};

    pub struct PureBackend;

    impl FitsBackend for PureBackend {
        fn name() -> &'static str {
            "fitsio-pure"
        }

        fn write_f32_image(path: &Path, shape: &[usize], data: &[f32]) {
            let mut f = FitsFile::create(path).overwrite().open().unwrap();
            let desc = ImageDescription {
                data_type: ImageType::Float,
                dimensions: shape.to_vec(),
            };
            let hdu = f.create_image("DATA", &desc).unwrap();
            f32::write_image(&mut f, &hdu, data).unwrap();
        }

        fn write_f64_image(path: &Path, shape: &[usize], data: &[f64]) {
            let mut f = FitsFile::create(path).overwrite().open().unwrap();
            let desc = ImageDescription {
                data_type: ImageType::Double,
                dimensions: shape.to_vec(),
            };
            let hdu = f.create_image("DATA", &desc).unwrap();
            f64::write_image(&mut f, &hdu, data).unwrap();
        }

        fn write_i32_image(path: &Path, shape: &[usize], data: &[i32]) {
            let mut f = FitsFile::create(path).overwrite().open().unwrap();
            let desc = ImageDescription {
                data_type: ImageType::Long,
                dimensions: shape.to_vec(),
            };
            let hdu = f.create_image("DATA", &desc).unwrap();
            i32::write_image(&mut f, &hdu, data).unwrap();
        }

        fn read_f32_image(path: &Path) -> Vec<f32> {
            let f = FitsFile::open(path).unwrap();
            let hdu = f.hdu(1).unwrap();
            f32::read_image(&f, &hdu).unwrap()
        }

        fn read_f64_image(path: &Path) -> Vec<f64> {
            let f = FitsFile::open(path).unwrap();
            let hdu = f.hdu(1).unwrap();
            f64::read_image(&f, &hdu).unwrap()
        }

        fn read_i32_image(path: &Path) -> Vec<i32> {
            let f = FitsFile::open(path).unwrap();
            let hdu = f.hdu(1).unwrap();
            i32::read_image(&f, &hdu).unwrap()
        }
    }
}

// ---------------------------------------------------------------------------
// fitsio-pure core backend (no compat layer)
// ---------------------------------------------------------------------------

#[cfg(feature = "pure")]
mod pure_core_backend {
    use super::*;
    use fitsio_pure::extension::{build_extension_header, ExtensionType};
    use fitsio_pure::hdu::parse_fits;
    use fitsio_pure::header::{serialize_header, Card};
    use fitsio_pure::image::{read_image_data, ImageData};
    use fitsio_pure::primary::build_primary_header;
    use fitsio_pure::value::Value;

    pub struct PureCoreBackend;

    fn make_keyword(name: &str) -> [u8; 8] {
        let mut kw = [b' '; 8];
        let bytes = name.as_bytes();
        let len = bytes.len().min(8);
        kw[..len].copy_from_slice(&bytes[..len]);
        kw
    }

    fn write_image_file(path: &Path, shape: &[usize], bitpix: i64, data_bytes: &[u8]) {
        // Build primary HDU (empty)
        let primary_cards = build_primary_header(8, &[]).unwrap();
        let primary_header = serialize_header(&primary_cards);

        // Build extension image HDU header
        let mut ext_cards =
            build_extension_header(ExtensionType::Image, bitpix, shape, 0, 1).unwrap();
        ext_cards.push(Card {
            keyword: make_keyword("EXTNAME"),
            value: Some(Value::String("DATA".to_string())),
            comment: None,
        });
        let ext_header = serialize_header(&ext_cards);

        let padded_data_len = fitsio_pure::block::padded_byte_len(data_bytes.len());
        let total = primary_header.len() + ext_header.len() + padded_data_len;
        let mut buf = Vec::with_capacity(total);
        buf.extend_from_slice(&primary_header);
        buf.extend_from_slice(&ext_header);
        buf.extend_from_slice(data_bytes);
        buf.resize(total, 0);

        std::fs::write(path, &buf).unwrap();
    }

    impl FitsBackend for PureCoreBackend {
        fn name() -> &'static str {
            "fitsio-pure (core)"
        }

        fn write_f32_image(path: &Path, shape: &[usize], data: &[f32]) {
            let bytes = fitsio_pure::image::serialize_image_f32(data);
            write_image_file(path, shape, -32, &bytes);
        }

        fn write_f64_image(path: &Path, shape: &[usize], data: &[f64]) {
            let bytes = fitsio_pure::image::serialize_image_f64(data);
            write_image_file(path, shape, -64, &bytes);
        }

        fn write_i32_image(path: &Path, shape: &[usize], data: &[i32]) {
            let bytes = fitsio_pure::image::serialize_image_i32(data);
            write_image_file(path, shape, 32, &bytes);
        }

        fn read_f32_image(path: &Path) -> Vec<f32> {
            let data = std::fs::read(path).unwrap();
            let parsed = parse_fits(&data).unwrap();
            let hdu = &parsed.hdus[1];
            match read_image_data(&data, hdu).unwrap() {
                ImageData::F32(v) => v,
                _ => panic!("expected f32"),
            }
        }

        fn read_f64_image(path: &Path) -> Vec<f64> {
            let data = std::fs::read(path).unwrap();
            let parsed = parse_fits(&data).unwrap();
            let hdu = &parsed.hdus[1];
            match read_image_data(&data, hdu).unwrap() {
                ImageData::F64(v) => v,
                _ => panic!("expected f64"),
            }
        }

        fn read_i32_image(path: &Path) -> Vec<i32> {
            let data = std::fs::read(path).unwrap();
            let parsed = parse_fits(&data).unwrap();
            let hdu = &parsed.hdus[1];
            match read_image_data(&data, hdu).unwrap() {
                ImageData::I32(v) => v,
                _ => panic!("expected i32"),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// fitsio (cfitsio) backend
// ---------------------------------------------------------------------------

#[cfg(feature = "cfitsio")]
mod cfitsio_backend {
    use super::*;
    use fitsio::images::{ImageDescription, ImageType};
    use fitsio::FitsFile;

    pub struct CfitsioBackend;

    impl FitsBackend for CfitsioBackend {
        fn name() -> &'static str {
            "fitsio (cfitsio)"
        }

        fn write_f32_image(path: &Path, shape: &[usize], data: &[f32]) {
            let mut f = FitsFile::create(path).overwrite().open().unwrap();
            let desc = ImageDescription {
                data_type: ImageType::Float,
                dimensions: shape,
            };
            let hdu = f.create_image("DATA", &desc).unwrap();
            hdu.write_image(&mut f, data).unwrap();
        }

        fn write_f64_image(path: &Path, shape: &[usize], data: &[f64]) {
            let mut f = FitsFile::create(path).overwrite().open().unwrap();
            let desc = ImageDescription {
                data_type: ImageType::Double,
                dimensions: shape,
            };
            let hdu = f.create_image("DATA", &desc).unwrap();
            hdu.write_image(&mut f, data).unwrap();
        }

        fn write_i32_image(path: &Path, shape: &[usize], data: &[i32]) {
            let mut f = FitsFile::create(path).overwrite().open().unwrap();
            let desc = ImageDescription {
                data_type: ImageType::Long,
                dimensions: shape,
            };
            let hdu = f.create_image("DATA", &desc).unwrap();
            hdu.write_image(&mut f, data).unwrap();
        }

        fn read_f32_image(path: &Path) -> Vec<f32> {
            let mut f = FitsFile::open(path).unwrap();
            let hdu = f.hdu(1).unwrap();
            hdu.read_image(&mut f).unwrap()
        }

        fn read_f64_image(path: &Path) -> Vec<f64> {
            let mut f = FitsFile::open(path).unwrap();
            let hdu = f.hdu(1).unwrap();
            hdu.read_image(&mut f).unwrap()
        }

        fn read_i32_image(path: &Path) -> Vec<i32> {
            let mut f = FitsFile::open(path).unwrap();
            let hdu = f.hdu(1).unwrap();
            hdu.read_image(&mut f).unwrap()
        }
    }
}

// ---------------------------------------------------------------------------
// Benchmark harness
// ---------------------------------------------------------------------------

struct BenchResult {
    label: String,
    write_ms: f64,
    read_ms: f64,
    write_mpx_per_sec: f64,
    read_mpx_per_sec: f64,
}

fn time_iterations<F: FnMut()>(mut f: F, iterations: usize) -> f64 {
    let start = Instant::now();
    for _ in 0..iterations {
        f();
    }
    start.elapsed().as_secs_f64() * 1000.0 / iterations as f64
}

fn generate_f32(n: usize) -> Vec<f32> {
    let mut data = Vec::with_capacity(n);
    let mut state: u64 = 0xdeadbeef;
    for _ in 0..n {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        data.push((state >> 40) as f32 * 0.001);
    }
    data
}

fn generate_f64(n: usize) -> Vec<f64> {
    let mut data = Vec::with_capacity(n);
    let mut state: u64 = 0xdeadbeef;
    for _ in 0..n {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        data.push((state >> 32) as f64 * 0.001);
    }
    data
}

fn generate_i32(n: usize) -> Vec<i32> {
    let mut data = Vec::with_capacity(n);
    let mut state: u64 = 0xdeadbeef;
    for _ in 0..n {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        data.push((state >> 32) as i32);
    }
    data
}

fn bench_type(
    dir: &Path,
    label: &str,
    iterations: usize,
    total: usize,
    write: impl Fn(&Path),
    read: impl Fn(&Path),
) -> BenchResult {
    let path = dir.join("bench.fits");
    let megapixels = total as f64 / 1_000_000.0;

    // Warmup
    write(&path);

    let write_ms = time_iterations(|| write(&path), iterations);
    let read_ms = time_iterations(|| read(&path), iterations);

    let _ = std::fs::remove_file(&path);

    BenchResult {
        label: label.to_string(),
        write_ms,
        read_ms,
        write_mpx_per_sec: megapixels / (write_ms / 1000.0),
        read_mpx_per_sec: megapixels / (read_ms / 1000.0),
    }
}

fn run_benchmarks<B: FitsBackend>(dir: &Path) -> Vec<BenchResult> {
    let mut results = Vec::new();

    let sizes: &[(&str, &[usize], usize)] = &[
        ("256x256", &[256, 256], 50),
        ("1024x1024", &[1024, 1024], 20),
        ("4096x4096", &[4096, 4096], 5),
        ("512x512x100", &[512, 512, 100], 3),
    ];

    for &(size_label, shape, iterations) in sizes {
        let total: usize = shape.iter().product();

        eprint!("  {}: {} ...", B::name(), size_label);

        let f32_data = generate_f32(total);
        let label = format!("f32 {size_label}");
        results.push(bench_type(
            dir,
            &label,
            iterations,
            total,
            |p| B::write_f32_image(p, shape, &f32_data),
            |p| drop(B::read_f32_image(p)),
        ));

        let f64_data = generate_f64(total);
        let label = format!("f64 {size_label}");
        results.push(bench_type(
            dir,
            &label,
            iterations,
            total,
            |p| B::write_f64_image(p, shape, &f64_data),
            |p| drop(B::read_f64_image(p)),
        ));

        let i32_data = generate_i32(total);
        let label = format!("i32 {size_label}");
        results.push(bench_type(
            dir,
            &label,
            iterations,
            total,
            |p| B::write_i32_image(p, shape, &i32_data),
            |p| drop(B::read_i32_image(p)),
        ));

        eprintln!(" done");
    }

    results
}

fn print_results(backend_name: &str, results: &[BenchResult]) {
    println!("\n### {backend_name}\n");
    println!(
        "| {:22} | {:>10} | {:>12} | {:>10} | {:>12} |",
        "Test", "Write ms", "Write MP/s", "Read ms", "Read MP/s"
    );
    println!(
        "|{:-<24}|{:->12}|{:->14}|{:->12}|{:->14}|",
        "", "", "", "", ""
    );
    for r in results {
        println!(
            "| {:22} | {:>10.2} | {:>12.1} | {:>10.2} | {:>12.1} |",
            r.label, r.write_ms, r.write_mpx_per_sec, r.read_ms, r.read_mpx_per_sec
        );
    }
}

fn main() {
    let dir = std::env::temp_dir().join("fits-benchmark");
    std::fs::create_dir_all(&dir).unwrap();

    println!("# FITS I/O Benchmark\n");
    println!("Measuring write and read throughput for large image arrays.");
    println!("Each test writes/reads the array multiple times and reports the average.");
    println!("MP/s = megapixels per second.\n");

    #[cfg(feature = "pure")]
    {
        use pure_core_backend::PureCoreBackend;
        let results = run_benchmarks::<PureCoreBackend>(&dir);
        print_results(PureCoreBackend::name(), &results);
    }

    #[cfg(feature = "pure")]
    {
        use pure_backend::PureBackend;
        let results = run_benchmarks::<PureBackend>(&dir);
        print_results(PureBackend::name(), &results);
    }

    #[cfg(feature = "cfitsio")]
    {
        use cfitsio_backend::CfitsioBackend;
        let results = run_benchmarks::<CfitsioBackend>(&dir);
        print_results(CfitsioBackend::name(), &results);
    }

    let _ = std::fs::remove_dir_all(&dir);
}
