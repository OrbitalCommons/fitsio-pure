//! Random-groups (UVFITS) files written by `fitsio_pure::groups::GroupsWriter`
//! against the same file written by cfitsio's `ffphpr` and `ffpgpe`, as Marlu
//! writes UVFITS.

use std::ffi::CString;
use std::io::BufWriter;
use std::path::Path;

use fitsio::FitsFile as CFits;
use fitsio_pure::bintable::{
    serialize_binary_table_hdu, BinaryColumnData, BinaryColumnDescriptor, BinaryColumnType,
};
use fitsio_pure::groups::{group_params, GroupParam, GroupsWriter};
use fitsio_pure::hdu::{parse_fits, HduInfo};

/// Marlu's parameters: the Julian date split over two `DATE`s, only the
/// first with a zero point.
const JD_ZERO: f64 = 2456528.5;

fn params() -> Vec<GroupParam> {
    let plain = |name: &str| GroupParam::new(name).with_scale(1.0).with_zero(0.0);
    vec![
        plain("UU"),
        plain("VV"),
        plain("WW"),
        plain("BASELINE"),
        GroupParam::new("DATE").with_scale(1.0).with_zero(JD_ZERO),
        GroupParam::new("DATE").with_scale(1.0),
    ]
}

/// `[complex, stokes, freq, ra, dec]`, `NAXIS2` onwards.
const NAXES: [usize; 5] = [3, 4, 5, 1, 1];
const GCOUNT: usize = 6;

/// What each group gets: a full group, a short one, and two groups' worth in
/// one call, then nothing, so the last groups are zero-filled.
fn writes() -> Vec<Vec<f32>> {
    let group_len = params().len() + NAXES.iter().product::<usize>();
    let values = |n: usize, k: f32| (0..n).map(|i| k + i as f32 * 0.5).collect::<Vec<_>>();
    vec![
        values(group_len, 1.0),
        values(10, 100.0),
        values(2 * group_len, -7.0),
    ]
}

fn cfitsio_writes(path: &Path) {
    let name = CString::new(path.to_str().unwrap()).unwrap();
    let mut status = 0;
    let mut fptr = std::ptr::null_mut();
    let mut naxes: Vec<std::os::raw::c_long> = std::iter::once(0)
        .chain(NAXES.iter().map(|&n| n as _))
        .collect();
    let group_len = params().len() + NAXES.iter().product::<usize>();
    unsafe {
        fitsio::sys::ffinit(&mut fptr, name.as_ptr(), &mut status);
        fitsio::sys::ffphpr(
            fptr,
            1,
            -32,
            naxes.len() as _,
            naxes.as_mut_ptr(),
            params().len() as _,
            GCOUNT as _,
            1,
            &mut status,
        );
        for (i, param) in params().iter().enumerate() {
            let n = i + 1;
            let key = |k: &str| CString::new(format!("{k}{n}")).unwrap();
            let ptype = CString::new(param.ptype.as_str()).unwrap();
            fitsio::sys::ffpkys(
                fptr,
                key("PTYPE").as_ptr(),
                ptype.as_ptr(),
                std::ptr::null_mut(),
                &mut status,
            );
            if let Some(pscal) = param.pscal {
                fitsio::sys::ffpkyd(
                    fptr,
                    key("PSCAL").as_ptr(),
                    pscal,
                    -15,
                    std::ptr::null_mut(),
                    &mut status,
                );
            }
            if let Some(pzero) = param.pzero {
                fitsio::sys::ffpkyd(
                    fptr,
                    key("PZERO").as_ptr(),
                    pzero,
                    -15,
                    std::ptr::null_mut(),
                    &mut status,
                );
            }
        }
        let mut group = 1;
        for mut values in writes() {
            fitsio::sys::ffpgpe(
                fptr,
                group,
                1,
                values.len() as _,
                values.as_mut_ptr(),
                &mut status,
            );
            group += values.len().div_ceil(group_len) as i64;
        }
        fitsio::sys::ffclos(fptr, &mut status);
    }
    assert_eq!(status, 0);
}

fn pure_writes(path: &Path) {
    let file = BufWriter::new(std::fs::File::create(path).unwrap());
    let mut writer = GroupsWriter::new(file, -32, &NAXES, &params(), GCOUNT, &[]).unwrap();
    for values in writes() {
        writer.write_group(&values).unwrap();
    }
    let mut file = writer.finish().unwrap();
    // A UVFITS antenna table follows, written to the same sink.
    let columns = [
        BinaryColumnDescriptor {
            name: Some("ANNAME".into()),
            repeat: 8,
            col_type: BinaryColumnType::Ascii,
            byte_width: 8,
            tdim: None,
        },
        BinaryColumnDescriptor {
            name: Some("NOSTA".into()),
            repeat: 1,
            col_type: BinaryColumnType::Int,
            byte_width: 4,
            tdim: None,
        },
    ];
    let data = [
        BinaryColumnData::Ascii(vec!["Tile011".into(), "Tile012".into()]),
        BinaryColumnData::Int(vec![1, 2]),
    ];
    let table = serialize_binary_table_hdu(&columns, &data, 2).unwrap();
    std::io::Write::write_all(&mut file, &table).unwrap();
}

/// The bytes of `path`'s data unit, and its header cards up to `END`.
fn split(path: &Path) -> (Vec<String>, Vec<u8>) {
    let bytes = std::fs::read(path).unwrap();
    let parsed = parse_fits(&bytes).unwrap();
    let hdu = &parsed.hdus[0];
    let header = bytes[..hdu.data_start]
        .chunks(80)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .take_while(|c| !c.starts_with("END "))
        .collect();
    (
        header,
        bytes[hdu.data_start..hdu.data_start + hdu.data_len].to_vec(),
    )
}

#[test]
fn groups_match_cfitsio() {
    let dir = tempfile::tempdir().unwrap();
    let c_path = dir.path().join("c.uvfits");
    let p_path = dir.path().join("p.uvfits");
    cfitsio_writes(&c_path);
    pure_writes(&p_path);

    let (c_header, c_data) = split(&c_path);
    let (p_header, p_data) = split(&p_path);
    // The groups data, short groups and unwritten groups included, is
    // byte-identical.
    assert_eq!(p_data, c_data);
    // So are the header cards up to the parameters, whose numbers cfitsio
    // formats its own way (`1.` rather than `1.0E0`).
    let fixed = c_header
        .iter()
        .position(|c| c.starts_with("PTYPE1"))
        .unwrap();
    assert_eq!(p_header[..fixed], c_header[..fixed]);

    // The parameters read back the same from both files.
    for path in [&c_path, &p_path] {
        let bytes = std::fs::read(path).unwrap();
        let parsed = parse_fits(&bytes).unwrap();
        let HduInfo::RandomGroups { pcount, .. } = parsed.hdus[0].info else {
            panic!("not random groups");
        };
        assert_eq!(
            group_params(&parsed.hdus[0].cards, pcount),
            params(),
            "{}",
            path.display()
        );
    }

    // cfitsio reads our groups, and the antenna table after them.
    let mut c = CFits::open(&p_path).unwrap();
    let mut status = 0;
    let (pcount, data_len) = (params().len(), NAXES.iter().product::<usize>());
    let mut group = vec![0f32; pcount + data_len];
    unsafe {
        let (params, data) = group.split_at_mut(pcount);
        fitsio::sys::ffggpe(
            c.as_raw(),
            3,
            1,
            pcount as _,
            params.as_mut_ptr(),
            &mut status,
        );
        fitsio::sys::ffgpve(
            c.as_raw(),
            3,
            1,
            data_len as _,
            0.0,
            data.as_mut_ptr(),
            &mut 0,
            &mut status,
        );
    }
    assert_eq!(status, 0);
    let want: Vec<f32> = writes()[2][..pcount + data_len].to_vec();
    assert_eq!(group, want);
    let an = c.hdu(1).unwrap();
    let names: Vec<String> = an.read_col(&mut c, "ANNAME").unwrap();
    assert_eq!(names, ["Tile011", "Tile012"]);
}
