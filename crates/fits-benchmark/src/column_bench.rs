use std::path::Path;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Backend trait -- each FITS library implements this
// ---------------------------------------------------------------------------

trait ColumnBackend {
    fn name() -> &'static str;

    fn write_f32_column(path: &Path, col_name: &str, data: &[f32]);
    fn write_f64_column(path: &Path, col_name: &str, data: &[f64]);
    fn write_i32_column(path: &Path, col_name: &str, data: &[i32]);
    fn write_i64_column(path: &Path, col_name: &str, data: &[i64]);

    fn read_f32_column(path: &Path, col_name: &str) -> Vec<f32>;
    fn read_f64_column(path: &Path, col_name: &str) -> Vec<f64>;
    fn read_i32_column(path: &Path, col_name: &str) -> Vec<i32>;
    fn read_i64_column(path: &Path, col_name: &str) -> Vec<i64>;
}

// ---------------------------------------------------------------------------
// fitsio-pure compat backend
// ---------------------------------------------------------------------------

#[cfg(feature = "pure")]
mod pure_backend {
    use super::*;
    use fitsio_pure::bintable::{BinaryColumnData, BinaryColumnDescriptor, BinaryColumnType};
    use fitsio_pure::compat::fitsfile::FitsFile;
    use fitsio_pure::compat::tables::ReadsCol;

    pub struct PureBackend;

    impl ColumnBackend for PureBackend {
        fn name() -> &'static str {
            "fitsio-pure"
        }

        fn write_f32_column(path: &Path, col_name: &str, data: &[f32]) {
            let mut f = FitsFile::create(path).overwrite().open().unwrap();
            let columns = vec![BinaryColumnDescriptor {
                name: Some(col_name.to_string()),
                repeat: 1,
                col_type: BinaryColumnType::Float,
                byte_width: 4,
            }];
            let col_data = vec![BinaryColumnData::Float(data.to_vec())];
            let nrows = data.len();
            let hdu_bytes =
                fitsio_pure::bintable::serialize_binary_table_hdu(&columns, &col_data, nrows)
                    .unwrap();
            let mut file_data = f.data().to_vec();
            file_data.extend_from_slice(&hdu_bytes);
            f.set_data(file_data);
        }

        fn write_f64_column(path: &Path, col_name: &str, data: &[f64]) {
            let mut f = FitsFile::create(path).overwrite().open().unwrap();
            let columns = vec![BinaryColumnDescriptor {
                name: Some(col_name.to_string()),
                repeat: 1,
                col_type: BinaryColumnType::Double,
                byte_width: 8,
            }];
            let col_data = vec![BinaryColumnData::Double(data.to_vec())];
            let nrows = data.len();
            let hdu_bytes =
                fitsio_pure::bintable::serialize_binary_table_hdu(&columns, &col_data, nrows)
                    .unwrap();
            let mut file_data = f.data().to_vec();
            file_data.extend_from_slice(&hdu_bytes);
            f.set_data(file_data);
        }

        fn write_i32_column(path: &Path, col_name: &str, data: &[i32]) {
            let mut f = FitsFile::create(path).overwrite().open().unwrap();
            let columns = vec![BinaryColumnDescriptor {
                name: Some(col_name.to_string()),
                repeat: 1,
                col_type: BinaryColumnType::Int,
                byte_width: 4,
            }];
            let col_data = vec![BinaryColumnData::Int(data.to_vec())];
            let nrows = data.len();
            let hdu_bytes =
                fitsio_pure::bintable::serialize_binary_table_hdu(&columns, &col_data, nrows)
                    .unwrap();
            let mut file_data = f.data().to_vec();
            file_data.extend_from_slice(&hdu_bytes);
            f.set_data(file_data);
        }

        fn write_i64_column(path: &Path, col_name: &str, data: &[i64]) {
            let mut f = FitsFile::create(path).overwrite().open().unwrap();
            let columns = vec![BinaryColumnDescriptor {
                name: Some(col_name.to_string()),
                repeat: 1,
                col_type: BinaryColumnType::Long,
                byte_width: 8,
            }];
            let col_data = vec![BinaryColumnData::Long(data.to_vec())];
            let nrows = data.len();
            let hdu_bytes =
                fitsio_pure::bintable::serialize_binary_table_hdu(&columns, &col_data, nrows)
                    .unwrap();
            let mut file_data = f.data().to_vec();
            file_data.extend_from_slice(&hdu_bytes);
            f.set_data(file_data);
        }

        fn read_f32_column(path: &Path, col_name: &str) -> Vec<f32> {
            let f = FitsFile::open(path).unwrap();
            let hdu = f.hdu(1usize).unwrap();
            f32::read_col(&f, &hdu, col_name).unwrap()
        }

        fn read_f64_column(path: &Path, col_name: &str) -> Vec<f64> {
            let f = FitsFile::open(path).unwrap();
            let hdu = f.hdu(1usize).unwrap();
            f64::read_col(&f, &hdu, col_name).unwrap()
        }

        fn read_i32_column(path: &Path, col_name: &str) -> Vec<i32> {
            let f = FitsFile::open(path).unwrap();
            let hdu = f.hdu(1usize).unwrap();
            i32::read_col(&f, &hdu, col_name).unwrap()
        }

        fn read_i64_column(path: &Path, col_name: &str) -> Vec<i64> {
            let f = FitsFile::open(path).unwrap();
            let hdu = f.hdu(1usize).unwrap();
            i64::read_col(&f, &hdu, col_name).unwrap()
        }
    }
}

// ---------------------------------------------------------------------------
// fitsio (cfitsio) backend
// ---------------------------------------------------------------------------

#[cfg(feature = "cfitsio")]
mod cfitsio_backend {
    use super::*;
    use fitsio::tables::{ColumnDataDescription, ColumnDataType};
    use fitsio::FitsFile;

    pub struct CfitsioBackend;

    fn create_and_write_col<T: fitsio::tables::WritesCol>(
        path: &Path,
        col_name: &str,
        data_type: ColumnDataType,
        data: &[T],
    ) {
        let col_desc = fitsio::tables::ConcreteColumnDescription {
            name: col_name.to_string(),
            data_type: ColumnDataDescription {
                repeat: 1,
                width: 1,
                typ: data_type,
            },
        };
        let mut f = FitsFile::create(path).overwrite().open().unwrap();
        let hdu = f.create_table("DATA".to_string(), &[col_desc]).unwrap();
        hdu.write_col(&mut f, col_name, data).unwrap();
    }

    impl ColumnBackend for CfitsioBackend {
        fn name() -> &'static str {
            "fitsio (cfitsio)"
        }

        fn write_f32_column(path: &Path, col_name: &str, data: &[f32]) {
            create_and_write_col(path, col_name, ColumnDataType::Float, data);
        }

        fn write_f64_column(path: &Path, col_name: &str, data: &[f64]) {
            create_and_write_col(path, col_name, ColumnDataType::Double, data);
        }

        fn write_i32_column(path: &Path, col_name: &str, data: &[i32]) {
            create_and_write_col(path, col_name, ColumnDataType::Int, data);
        }

        fn write_i64_column(path: &Path, col_name: &str, data: &[i64]) {
            create_and_write_col(path, col_name, ColumnDataType::LongLong, data);
        }

        fn read_f32_column(path: &Path, col_name: &str) -> Vec<f32> {
            let mut f = FitsFile::open(path).unwrap();
            let hdu = f.hdu(1).unwrap();
            hdu.read_col(&mut f, col_name).unwrap()
        }

        fn read_f64_column(path: &Path, col_name: &str) -> Vec<f64> {
            let mut f = FitsFile::open(path).unwrap();
            let hdu = f.hdu(1).unwrap();
            hdu.read_col(&mut f, col_name).unwrap()
        }

        fn read_i32_column(path: &Path, col_name: &str) -> Vec<i32> {
            let mut f = FitsFile::open(path).unwrap();
            let hdu = f.hdu(1).unwrap();
            hdu.read_col(&mut f, col_name).unwrap()
        }

        fn read_i64_column(path: &Path, col_name: &str) -> Vec<i64> {
            let mut f = FitsFile::open(path).unwrap();
            let hdu = f.hdu(1).unwrap();
            hdu.read_col(&mut f, col_name).unwrap()
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
    write_mrow_per_sec: f64,
    read_mrow_per_sec: f64,
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

fn generate_i64(n: usize) -> Vec<i64> {
    let mut data = Vec::with_capacity(n);
    let mut state: u64 = 0xdeadbeef;
    for _ in 0..n {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        data.push(state as i64);
    }
    data
}

fn bench_column(
    dir: &Path,
    label: &str,
    iterations: usize,
    nrows: usize,
    write: impl Fn(&Path),
    read: impl Fn(&Path),
) -> BenchResult {
    let path = dir.join("bench_col.fits");
    let megarows = nrows as f64 / 1_000_000.0;

    // Warmup
    write(&path);

    let write_ms = time_iterations(|| write(&path), iterations);
    let read_ms = time_iterations(|| read(&path), iterations);

    let _ = std::fs::remove_file(&path);

    BenchResult {
        label: label.to_string(),
        write_ms,
        read_ms,
        write_mrow_per_sec: megarows / (write_ms / 1000.0),
        read_mrow_per_sec: megarows / (read_ms / 1000.0),
    }
}

fn run_column_benchmarks<B: ColumnBackend>(dir: &Path) -> Vec<BenchResult> {
    let mut results = Vec::new();

    let sizes: &[(&str, usize, usize)] = &[
        ("1K rows", 1_000, 100),
        ("10K rows", 10_000, 50),
        ("100K rows", 100_000, 20),
        ("1M rows", 1_000_000, 5),
    ];

    for &(size_label, nrows, iterations) in sizes {
        eprint!("  {}: {} ...", B::name(), size_label);

        let f32_data = generate_f32(nrows);
        results.push(bench_column(
            dir,
            &format!("f32 {size_label}"),
            iterations,
            nrows,
            |p| B::write_f32_column(p, "DATA", &f32_data),
            |p| drop(B::read_f32_column(p, "DATA")),
        ));

        let f64_data = generate_f64(nrows);
        results.push(bench_column(
            dir,
            &format!("f64 {size_label}"),
            iterations,
            nrows,
            |p| B::write_f64_column(p, "DATA", &f64_data),
            |p| drop(B::read_f64_column(p, "DATA")),
        ));

        let i32_data = generate_i32(nrows);
        results.push(bench_column(
            dir,
            &format!("i32 {size_label}"),
            iterations,
            nrows,
            |p| B::write_i32_column(p, "DATA", &i32_data),
            |p| drop(B::read_i32_column(p, "DATA")),
        ));

        let i64_data = generate_i64(nrows);
        results.push(bench_column(
            dir,
            &format!("i64 {size_label}"),
            iterations,
            nrows,
            |p| B::write_i64_column(p, "DATA", &i64_data),
            |p| drop(B::read_i64_column(p, "DATA")),
        ));

        eprintln!(" done");
    }

    results
}

fn print_results(backend_name: &str, results: &[BenchResult]) {
    println!("\n### {backend_name}\n");
    println!(
        "| {:22} | {:>10} | {:>12} | {:>10} | {:>12} |",
        "Test", "Write ms", "Write MR/s", "Read ms", "Read MR/s"
    );
    println!(
        "|{:-<24}|{:->12}|{:->14}|{:->12}|{:->14}|",
        "", "", "", "", ""
    );
    for r in results {
        println!(
            "| {:22} | {:>10.2} | {:>12.1} | {:>10.2} | {:>12.1} |",
            r.label, r.write_ms, r.write_mrow_per_sec, r.read_ms, r.read_mrow_per_sec
        );
    }
}

fn main() {
    let dir = std::env::temp_dir().join("fits-column-benchmark");
    std::fs::create_dir_all(&dir).unwrap();

    println!("# FITS Column I/O Benchmark\n");
    println!("Measuring write and read throughput for binary table columns.");
    println!("Each test writes/reads the column multiple times and reports the average.");
    println!("MR/s = megarows per second.\n");

    #[cfg(feature = "pure")]
    {
        use pure_backend::PureBackend;
        let results = run_column_benchmarks::<PureBackend>(&dir);
        print_results(PureBackend::name(), &results);
    }

    #[cfg(feature = "cfitsio")]
    {
        use cfitsio_backend::CfitsioBackend;
        let results = run_column_benchmarks::<CfitsioBackend>(&dir);
        print_results(CfitsioBackend::name(), &results);
    }

    let _ = std::fs::remove_dir_all(&dir);
}
