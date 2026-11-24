//! `wcs::WcsKeys` against astropy.wcs, on fits-test-cases' WCS fixtures:
//! `wcs/*` (astropy's parse of each WCS in `<name>.wcs.json`) and
//! `scirs/wcs_tan_*` (pixel <-> world truth). Skipped without the corpus.

use std::path::{Path, PathBuf};

use fitsio_pure::hdu::parse_fits;
use fitsio_pure::wcs::{LinearForm, WcsKeys};
use serde_json::Value as Json;

fn corpus_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("FITS_TEST_CASES") {
        return Some(PathBuf::from(dir));
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fits-test-cases");
    dir.join("wcs").is_dir().then_some(dir)
}

const FIXTURES: [&str; 4] = [
    "wcs_pc_cdelt",
    "wcs_crota2",
    "wcs_tan_sip",
    "wcs_sin_freq_cube",
];

/// The primary HDU's keys for `alt` in `wcs/<name>.fits`, and astropy's
/// parse of each WCS in it.
fn load(dir: &Path, name: &str) -> (fitsio_pure::hdu::Hdu, Vec<Json>) {
    let bytes = std::fs::read(dir.join(format!("wcs/{name}.fits"))).unwrap();
    let hdu = parse_fits(&bytes).unwrap().hdus.remove(0);
    let truth: Json =
        serde_json::from_slice(&std::fs::read(dir.join(format!("wcs/{name}.wcs.json"))).unwrap())
            .unwrap();
    (hdu, truth["wcs"].as_array().unwrap().clone())
}

fn floats(v: &Json) -> Vec<f64> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap())
        .collect()
}

fn close(a: f64, b: f64, rel: f64) -> bool {
    a == b || (a - b).abs() <= rel * a.abs().max(b.abs())
}

fn alt_of(entry: &Json) -> Option<char> {
    entry["alt"].as_str().unwrap().chars().next()
}

/// Every WCS in the `wcs/` fixtures parses as astropy parses it: axis
/// types and units, CRPIX/CRVAL, the CD matrix (astropy's pixel scale
/// matrix, from CD, PC × CDELT or CROTA2), the linear form, the celestial
/// axes, the frame and equinox, and every SIP coefficient.
#[test]
fn wcs_keys_match_astropy() {
    let Some(dir) = corpus_dir() else {
        return;
    };
    for name in FIXTURES {
        let (hdu, entries) = load(&dir, name);
        let mut alternates = Vec::new();
        for entry in &entries {
            let alt = alt_of(entry);
            alternates.extend(alt);
            let what = format!("{name} alt {alt:?}");
            let keys = WcsKeys::from_hdu(&hdu, alt).unwrap().expect(&what);
            let n = entry["naxis"].as_u64().unwrap() as usize;
            assert_eq!(keys.naxis(), n, "{what}");
            let ctype: Vec<&str> = entry["ctype"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect();
            assert_eq!(keys.ctype, ctype, "{what}");
            for (i, unit) in entry["cunit"].as_array().unwrap().iter().enumerate() {
                let unit = unit.as_str().unwrap();
                // astropy fills in `deg` for celestial axes without CUNIT.
                let defaulted = keys.cunit[i].is_empty() && unit == "deg";
                assert!(keys.cunit[i] == unit || defaulted, "{what} CUNIT{}", i + 1);
            }
            assert_eq!(keys.crpix, floats(&entry["crpix"]), "{what}");
            assert_eq!(keys.crval, floats(&entry["crval"]), "{what}");
            for (i, row) in entry["cd"].as_array().unwrap().iter().enumerate() {
                for (j, want) in floats(row).into_iter().enumerate() {
                    assert!(
                        close(keys.cd(i, j), want, 1e-14),
                        "{what} CD{}_{}: {} vs astropy {want}",
                        i + 1,
                        j + 1,
                        keys.cd(i, j)
                    );
                }
            }
            let form = match (
                entry["has_cd"].as_bool().unwrap(),
                entry["has_crota"].as_bool().unwrap(),
            ) {
                (true, _) => LinearForm::Cd,
                (false, true) => LinearForm::Crota,
                (false, false) => LinearForm::Pc,
            };
            assert_eq!(keys.form, form, "{what}");
            let axis = |key: &str| entry[key].as_i64().unwrap();
            match &keys.celestial {
                Some(c) => {
                    assert_eq!((c.lon as i64, c.lat as i64), (axis("lng"), axis("lat")));
                }
                None => assert_eq!((axis("lng"), axis("lat")), (-1, -1), "{what}"),
            }
            assert_eq!(
                keys.radesys_or_default().unwrap_or(""),
                entry["radesys"].as_str().unwrap(),
                "{what}"
            );
            assert_eq!(
                keys.equinox_or_default(),
                entry["equinox"].as_f64(),
                "{what}"
            );
            assert!(keys.ignored.is_empty(), "{what}: {:?}", keys.ignored);
            match (&keys.sip, entry["sip"].is_null()) {
                (None, true) => {}
                (Some(sip), false) => {
                    let s = &entry["sip"];
                    let polys = [
                        ("a", Some(&sip.a)),
                        ("b", Some(&sip.b)),
                        ("ap", sip.ap.as_ref()),
                        ("bp", sip.bp.as_ref()),
                    ];
                    for (key, poly) in polys {
                        let poly = poly.unwrap();
                        let order = s[format!("{key}_order")].as_u64().unwrap() as usize;
                        assert_eq!(poly.order, order, "{what} {key}");
                        let matrix = s[key].as_array().unwrap();
                        let mut terms = 0;
                        for (p, row) in matrix.iter().enumerate() {
                            for (q, want) in floats(row).into_iter().enumerate() {
                                assert_eq!(poly.coefficient(p, q), want, "{what} {key}_{p}_{q}");
                                terms += usize::from(want != 0.0);
                            }
                        }
                        // Every term up to the order, A_0_0 and A_1_0 included.
                        assert_eq!(poly.coefficients.len(), terms, "{what} {key}");
                        assert_eq!(terms, (order + 1) * (order + 2) / 2, "{what} {key}");
                    }
                }
                _ => panic!("{what}: SIP presence differs from astropy"),
            }
        }
        assert_eq!(
            WcsKeys::alternates(&hdu.cards),
            alternates,
            "{name} alternates"
        );
    }
}

/// Writing the keys and reading them back gives the same WCS, in each form
/// the reading allows.
#[test]
fn wcs_keys_round_trip_through_cards() {
    let Some(dir) = corpus_dir() else {
        return;
    };
    for name in FIXTURES {
        let (hdu, entries) = load(&dir, name);
        for entry in &entries {
            let alt = alt_of(entry);
            let keys = WcsKeys::from_hdu(&hdu, alt).unwrap().unwrap();
            let mut forms = vec![LinearForm::Cd, LinearForm::Pc];
            if keys.form == LinearForm::Crota {
                forms.push(LinearForm::Crota);
            } else {
                assert!(keys.to_cards_as(LinearForm::Crota).is_err());
            }
            for form in forms {
                let what = format!("{name} alt {alt:?} as {form:?}");
                let mut cards: Vec<_> = hdu
                    .cards
                    .iter()
                    .filter(|c| {
                        ["SIMPLE", "BITPIX"].contains(&c.keyword_str())
                            || c.keyword_str().starts_with("NAXIS")
                    })
                    .cloned()
                    .collect();
                cards.extend(keys.to_cards_as(form).unwrap());
                let back = WcsKeys::from_cards(&cards, alt).unwrap().expect(&what);
                assert_eq!(back.form, form, "{what}");
                assert_eq!(back.ctype, keys.ctype, "{what}");
                assert_eq!(back.crpix, keys.crpix, "{what}");
                assert_eq!(back.crval, keys.crval, "{what}");
                assert_eq!(back.sip, keys.sip, "{what}");
                assert_eq!(back.celestial, keys.celestial, "{what}");
                assert_eq!(
                    (back.radesys.as_deref(), back.equinox, back.lonpole),
                    (keys.radesys.as_deref(), keys.equinox, keys.lonpole),
                    "{what}"
                );
                for (a, b) in back.cd.iter().zip(&keys.cd) {
                    assert!(close(*a, *b, 1e-15), "{what}: CD {a} vs {b}");
                }
            }
        }
    }
}

/// Through the `wcs` crate, every truth point's pixel unprojects to the
/// world coordinates astropy gives for it, SIP distortion included. The
/// fixtures' frames are ICRS and FK5, which the `wcs` crate gives as they
/// are.
#[cfg(feature = "cds-wcs")]
#[test]
fn wcs_crate_unprojects_as_astropy() {
    use cds_wcs::{ImgXY, WCSParams, WCS};

    let Some(dir) = corpus_dir() else {
        return;
    };
    let check = |what: &str, keys: &WcsKeys, points: &[Json], world_key: &str| {
        let params = WCSParams::try_from(keys).unwrap();
        let wcs = WCS::new(&params).unwrap();
        for point in points {
            let pixel = floats(&point["pixel"]);
            let world = floats(&point[world_key]);
            // The wcs crate counts pixels from 1, as FITS and the truth do.
            let lonlat = wcs.unproj(&ImgXY::new(pixel[0], pixel[1])).unwrap();
            let (lon, lat) = (lonlat.lon().to_degrees(), lonlat.lat().to_degrees());
            let dlon = (lon - world[0] + 540.0).rem_euclid(360.0) - 180.0;
            assert!(
                dlon.abs() * world[1].to_radians().cos() < 1e-9 && (lat - world[1]).abs() < 1e-9,
                "{what} pixel {pixel:?}: ({lon}, {lat}) vs astropy {world:?}"
            );
        }
    };
    for name in FIXTURES {
        let (hdu, entries) = load(&dir, name);
        let entry = &entries[0];
        let keys = WcsKeys::from_hdu(&hdu, None).unwrap().unwrap();
        if keys.sip.is_some() {
            // The wcs crate 0.4 can't evaluate SIP correctly, so the
            // conversion refuses rather than give wrong positions.
            assert!(WCSParams::try_from(&keys).is_err(), "{name}");
            continue;
        }
        check(name, &keys, entry["points"].as_array().unwrap(), "world");
    }
    for name in ["wcs_tan_cd_hst", "wcs_tan_cd_wide", "wcs_tan_pc_cdelt"] {
        let bytes = std::fs::read(dir.join(format!("scirs/{name}.fits"))).unwrap();
        let hdu = parse_fits(&bytes).unwrap().hdus.remove(0);
        let mut keys = WcsKeys::from_hdu(&hdu, None).unwrap().unwrap();
        // Header-only files: any image size will do for unprojecting.
        keys.naxes = vec![1000, 800];
        let truth: Json = serde_json::from_slice(
            &std::fs::read(dir.join(format!("scirs/{name}.wcs.json"))).unwrap(),
        )
        .unwrap();
        check(name, &keys, truth["points"].as_array().unwrap(), "world");
    }
}
