//! Random-groups reads through compat against cfitsio's `ffggpe` and
//! `ffgpve`, on a groups file cfitsio wrote.

use std::ffi::CString;
use std::path::Path;

use fitsio::FitsFile as CFits;
use fitsio_pure::compat::fitsfile::FitsFile as PureFits;

const PCOUNT: usize = 3;
const DATA_LEN: usize = 6;
const GCOUNT: usize = 5;

/// cfitsio writes `GCOUNT` groups of stored values `0..`, with
/// `BSCALE = 2`, `BZERO = 10` and a `PSCAL`/`PZERO` that reads must ignore.
fn cfitsio_writes(path: &Path) {
    let name = CString::new(path.to_str().unwrap()).unwrap();
    let key = |k: &str| CString::new(k).unwrap();
    let (mut status, mut fptr) = (0, std::ptr::null_mut());
    let mut naxes: [std::os::raw::c_long; 3] = [0, 2, 3];
    let mut values: Vec<f32> = (0..(PCOUNT + DATA_LEN) * GCOUNT)
        .map(|v| v as f32)
        .collect();
    // An unscaled parameter of -0.0 must stay negative.
    values[1] = -0.0;
    unsafe {
        fitsio::sys::ffinit(&mut fptr, name.as_ptr(), &mut status);
        fitsio::sys::ffphpr(
            fptr,
            1,
            -32,
            3,
            naxes.as_mut_ptr(),
            PCOUNT as _,
            GCOUNT as _,
            1,
            &mut status,
        );
        let null = std::ptr::null_mut();
        fitsio::sys::ffpkyd(fptr, key("BSCALE").as_ptr(), 2.0, -15, null, &mut status);
        fitsio::sys::ffpkyd(fptr, key("BZERO").as_ptr(), 10.0, -15, null, &mut status);
        fitsio::sys::ffpkyd(fptr, key("PSCAL1").as_ptr(), 3.0, -15, null, &mut status);
        fitsio::sys::ffpkyd(fptr, key("PZERO1").as_ptr(), 100.0, -15, null, &mut status);
        fitsio::sys::ffrdef(fptr, &mut status);
        // Store the values as given.
        fitsio::sys::ffpscl(fptr, 1.0, 0.0, &mut status);
        fitsio::sys::ffpgpe(
            fptr,
            1,
            1,
            values.len() as _,
            values.as_mut_ptr(),
            &mut status,
        );
        fitsio::sys::ffclos(fptr, &mut status);
    }
    assert_eq!(status, 0);
}

/// Every read window, `(group, first, count)`, within and across groups and
/// past the ends.
fn windows(part_len: usize) -> Vec<(usize, usize, usize)> {
    let mut windows = Vec::new();
    for group in 0..=GCOUNT {
        for first in 0..=part_len {
            for count in [1, 2, part_len, part_len + 1, 2 * part_len + 1] {
                windows.push((group, first, count));
            }
        }
    }
    windows
}

#[test]
fn group_reads_match_cfitsio() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("groups.fits");
    cfitsio_writes(&path);
    let mut c = CFits::open(&path).unwrap();
    let p = PureFits::open(&path).unwrap();
    let hdu = p.primary_hdu().unwrap();

    for (params, part_len) in [(true, PCOUNT), (false, DATA_LEN)] {
        for (group, first, count) in windows(part_len) {
            let mut want = vec![0f32; count];
            let mut status = 0;
            unsafe {
                if params {
                    fitsio::sys::ffggpe(
                        c.as_raw(),
                        group as i64 + 1,
                        first as i64 + 1,
                        count as _,
                        want.as_mut_ptr(),
                        &mut status,
                    );
                } else {
                    fitsio::sys::ffgpve(
                        c.as_raw(),
                        group as i64 + 1,
                        first as i64 + 1,
                        count as _,
                        0.0,
                        want.as_mut_ptr(),
                        &mut 0,
                        &mut status,
                    );
                }
            }
            let mut got = vec![0f32; count];
            let result = if params {
                hdu.read_group_params(&p, group, first, &mut got)
            } else {
                hdu.read_group_data(&p, group, first, &mut got)
            };
            let what = format!("params {params} group {group} first {first} count {count}");
            match result {
                Ok(()) => {
                    assert_eq!(status, 0, "{what}");
                    let bits = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
                    assert_eq!(bits(&got), bits(&want), "{what}");
                }
                Err(fitsio_pure::compat::errors::Error::Fits(e)) => {
                    assert_eq!(e.status, status, "{what}")
                }
                Err(other) => panic!("{what}: {other:?}"),
            }
        }
    }
}
