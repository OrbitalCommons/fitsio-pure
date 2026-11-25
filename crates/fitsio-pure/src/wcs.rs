//! World Coordinate System keywords, typed: the cards of FITS WCS Papers I
//! and II and the SIP convention, read from a header and written back.
//!
//! [`WcsKeys`] is the keyword layer only. It reads which axes are celestial
//! and the projection code, `CRPIXi`/`CRVALi`/`CUNITi`, the linear part as
//! one CD matrix, `LONPOLE`/`LATPOLE`/`PVi_m`, `RADESYS`/`EQUINOX` and SIP
//! distortion coefficients. It does no projection maths; with the `cds-wcs`
//! feature it converts into the `wcs` crate's `WCSParams`, which does.
//!
//! Axis numbers in keywords are 1-based (`CRPIX1`); indices into the vectors
//! here are 0-based (`crpix[0]`).

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::error::{Error, Result};
use crate::hdu::{Hdu, HduInfo};
use crate::header::Card;
use crate::value::Value;

/// The keywords a header gave its linear transformation in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinearForm {
    /// `CDi_j`.
    Cd,
    /// `PCi_j` and `CDELTi`, either of which may be absent (identity and 1).
    Pc,
    /// `CDELTi` and the deprecated `CROTAi` of the latitude axis.
    Crota,
}

/// The celestial axes of a WCS: 0-based indices of its longitude and
/// latitude axes, and their projection code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Celestial {
    /// The longitude axis (`RA`, `GLON`, `ELON`, …).
    pub lon: usize,
    /// The latitude axis (`DEC`, `GLAT`, `ELAT`, …).
    pub lat: usize,
    /// The projection code, such as `TAN` or `SIN`.
    pub projection: String,
}

/// One SIP polynomial: its order (`A_ORDER`) and every coefficient the
/// header gives, `(p, q, value)` for `A_p_q`, sorted, the low-order ones
/// (`A_0_0`, `A_1_0`, `A_0_1`) included.
#[derive(Debug, Clone, PartialEq)]
pub struct SipPolynomial {
    /// `A_ORDER` (or `B_ORDER`, `AP_ORDER`, `BP_ORDER`).
    pub order: usize,
    /// `(p, q, coefficient)` for each `A_p_q` card with `p + q <= order`.
    pub coefficients: Vec<(usize, usize, f64)>,
}

impl SipPolynomial {
    /// The coefficient of `u^p v^q`: 0 when the header doesn't give it.
    pub fn coefficient(&self, p: usize, q: usize) -> f64 {
        self.coefficients
            .iter()
            .find(|&&(pp, qq, _)| (pp, qq) == (p, q))
            .map_or(0.0, |&(_, _, v)| v)
    }
}

/// SIP distortion (Shupe et al. 2005): the forward `A`/`B` polynomials and
/// the optional inverse `AP`/`BP`.
#[derive(Debug, Clone, PartialEq)]
pub struct Sip {
    /// `A_ORDER` and `A_p_q`: the forward distortion of the first axis.
    pub a: SipPolynomial,
    /// `B_ORDER` and `B_p_q`: the forward distortion of the second axis.
    pub b: SipPolynomial,
    /// `AP_ORDER` and `AP_p_q`: the inverse of `a`, when given.
    pub ap: Option<SipPolynomial>,
    /// `BP_ORDER` and `BP_p_q`: the inverse of `b`, when given.
    pub bp: Option<SipPolynomial>,
}

/// A `PVi_m` card: projection parameter `m` of axis `i` (1-based, as in
/// the keyword).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pv {
    /// `i`: the axis, 1-based.
    pub axis: usize,
    /// `m`: the parameter number.
    pub m: usize,
    /// The parameter's value.
    pub value: f64,
}

/// The WCS keywords of one header: the primary WCS or an alternate (`A`–`Z`).
///
/// The linear part is normalised to one CD matrix, `cd`, following WCS
/// Papers I and II:
/// - **`CDi_j`** if any is present; missing ones are 0. `PCi_j`, `CDELTi`
///   and `CROTAi` are then ignored, as Paper I tells new readers to. (Paper
///   I prohibits mixing CD and PC; wcslib, and so astropy, reads such a
///   header as PC × CDELT instead.)
/// - Otherwise **`PCi_j` × `CDELTi`**, `PCi_j` defaulting to the identity and
///   `CDELTi` to 1, if any `PCi_j` is present or no `CROTAi` applies. A
///   `CROTAi` alongside `PCi_j` is ignored, as wcslib ignores it.
/// - Otherwise **`CDELTi` and `CROTAi`** of the latitude axis (Paper II
///   eq. 189), as wcslib reads them; a `CROTAi` on another axis is ignored.
///   Without celestial axes, `CROTA2` rotates axes 1 and 2. Alternate WCSs
///   have no `CROTA`.
///
/// `form`, `cdelt`, `pc` and `crota` keep what the header used, so
/// [`WcsKeys::to_cards_as`] can write it back that way; [`ignored`] lists
/// the keywords the precedence set aside.
///
/// [`ignored`]: WcsKeys::ignored
#[derive(Debug, Clone, PartialEq)]
pub struct WcsKeys {
    /// `None` for the primary WCS, `Some('A')`… for an alternate.
    pub alt: Option<char>,
    /// `WCSNAMEa`.
    pub wcsname: Option<String>,
    /// The image's axis lengths, `NAXISn`.
    pub naxes: Vec<usize>,
    /// `CTYPEi`, blank when absent (a linear axis).
    pub ctype: Vec<String>,
    /// `CUNITi`, blank when absent.
    pub cunit: Vec<String>,
    /// `CRPIXi`, 0 when absent.
    pub crpix: Vec<f64>,
    /// `CRVALi`, 0 when absent.
    pub crval: Vec<f64>,
    /// The linear transformation as one CD matrix, row-major: `cd[i * n + j]`
    /// is `CD(i+1)_(j+1)`. See [`WcsKeys::cd`].
    pub cd: Vec<f64>,
    /// The keywords the header gave the linear part in.
    pub form: LinearForm,
    /// `CDELTi` as read, 1 when absent.
    pub cdelt: Vec<f64>,
    /// `PCi_j` as read, row-major, the identity when absent.
    pub pc: Vec<f64>,
    /// The `CROTAi` that applies, in degrees, when `form` is `Crota`.
    pub crota: Option<f64>,
    /// The longitude and latitude axes and their projection, if the WCS has
    /// exactly one of each with the same projection code.
    pub celestial: Option<Celestial>,
    /// `LONPOLEa` as given; its default depends on the projection.
    pub lonpole: Option<f64>,
    /// `LATPOLEa` as given.
    pub latpole: Option<f64>,
    /// `PVi_ma`, sorted.
    pub pv: Vec<Pv>,
    /// `RADESYSa` (or the older `RADECSYS`) as given; see
    /// [`WcsKeys::radesys_or_default`].
    pub radesys: Option<String>,
    /// `EQUINOXa` (or the deprecated `EPOCH`) as given; see
    /// [`WcsKeys::equinox_or_default`].
    pub equinox: Option<f64>,
    /// SIP distortion, when `A_ORDER` and `B_ORDER` are given (primary WCS
    /// only).
    pub sip: Option<Sip>,
    /// Keywords present but set aside by the rules above.
    pub ignored: Vec<String>,
}

/// A header's cards by keyword, the first of each.
struct Keys<'a>(BTreeMap<&'a str, &'a Value>);

impl<'a> Keys<'a> {
    fn new(cards: &'a [Card]) -> Self {
        let mut map = BTreeMap::new();
        for card in cards {
            if let Some(value) = &card.value {
                map.entry(card.keyword_str()).or_insert(value);
            }
        }
        Keys(map)
    }

    fn float(&self, key: &str) -> Option<f64> {
        match self.0.get(key)? {
            Value::Float(v) => Some(*v),
            Value::Integer(v) => Some(*v as f64),
            _ => None,
        }
    }

    fn int(&self, key: &str) -> Option<i64> {
        match self.0.get(key)? {
            Value::Integer(v) => Some(*v),
            _ => None,
        }
    }

    fn string(&self, key: &str) -> Option<String> {
        match self.0.get(key)? {
            Value::String(s) => Some(s.trim_end().to_string()),
            _ => None,
        }
    }

    fn has(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }
}

/// `key` as `prefix`, digits, then `suffix`: the digits' value.
fn index1(key: &str, prefix: &str, suffix: &str) -> Option<usize> {
    let digits = key.strip_prefix(prefix)?.strip_suffix(suffix)?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// `key` as `prefix`, digits, `_`, digits, then `suffix`.
fn index2(key: &str, prefix: &str, suffix: &str) -> Option<(usize, usize)> {
    let rest = key.strip_prefix(prefix)?.strip_suffix(suffix)?;
    let (i, j) = rest.split_once('_')?;
    let number = |s: &str| {
        (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            .then(|| s.parse().ok())
            .flatten()
    };
    Some((number(i)?, number(j)?))
}

/// The axis type of a `CTYPEi`: its first four characters without the `-`
/// padding.
fn axis_type(ctype: &str) -> &str {
    ctype.get(..4).unwrap_or(ctype).trim_end_matches('-')
}

fn is_longitude(ctype: &str) -> bool {
    let t = axis_type(ctype);
    t == "RA" || (t.len() == 4 && (t.ends_with("LON") || t.ends_with("LN")))
}

fn is_latitude(ctype: &str) -> bool {
    let t = axis_type(ctype);
    t == "DEC" || (t.len() == 4 && (t.ends_with("LAT") || t.ends_with("LT")))
}

/// The projection code of a celestial `CTYPEi`, as in `RA---TAN-SIP`.
fn projection_code(ctype: &str) -> Option<&str> {
    (ctype.len() >= 8 && ctype.as_bytes()[4] == b'-').then(|| ctype[5..8].trim_end_matches('-'))
}

impl WcsKeys {
    /// The WCS of HDU `hdu`, the primary one (`alt` `None`) or alternate
    /// `alt`. For a tile-compressed image, the keywords come from the image's
    /// header ([`crate::tiled::uncompressed_header`]). `None` when the
    /// header has no WCS keywords for `alt`.
    pub fn from_hdu(hdu: &Hdu, alt: Option<char>) -> Result<Option<WcsKeys>> {
        match hdu.info {
            HduInfo::CompressedImage { .. } => {
                Self::from_cards(&crate::tiled::uncompressed_header(hdu)?, alt)
            }
            _ => Self::from_cards(&hdu.cards, alt),
        }
    }

    /// The alternate WCS letters, `A`–`Z`, that `cards` has keywords for.
    pub fn alternates(cards: &[Card]) -> Vec<char> {
        ('A'..='Z')
            .filter(|&a| matches!(Self::from_cards(cards, Some(a)), Ok(Some(_))))
            .collect()
    }

    /// The WCS in `cards`, the primary one (`alt` `None`) or alternate `alt`
    /// (`A`–`Z`). `None` when there are no WCS keywords for it.
    pub fn from_cards(cards: &[Card], alt: Option<char>) -> Result<Option<WcsKeys>> {
        if alt.is_some_and(|a| !a.is_ascii_uppercase()) {
            return Err(Error::InvalidValue);
        }
        let keys = Keys::new(cards);
        let mut buf = [0u8; 4];
        let s: &str = match alt {
            Some(a) => a.encode_utf8(&mut buf),
            None => "",
        };

        let naxis = keys.int("NAXIS").unwrap_or(0).max(0) as usize;
        let naxes: Vec<usize> = (1..=naxis)
            .map(|i| keys.int(&format!("NAXIS{i}")).unwrap_or(0).max(0) as usize)
            .collect();

        // The number of WCS axes: WCSAXESa, or the most of NAXIS and any
        // axis a keyword names.
        let mut any = keys.has(&format!("WCSAXES{s}"));
        let mut highest = 0;
        let mut has_cd = false;
        let mut has_pc = false;
        for &key in keys.0.keys() {
            let one = ["CTYPE", "CUNIT", "CRPIX", "CRVAL", "CDELT"]
                .iter()
                .find_map(|p| index1(key, p, s));
            let two = ["CD", "PC", "PV"]
                .iter()
                .find_map(|p| index2(key, p, s).map(|ij| (*p, ij)));
            if let Some(i) = one {
                any = true;
                highest = highest.max(i);
            }
            if let Some((p, (i, j))) = two {
                any = true;
                highest = highest.max(i);
                match p {
                    "CD" => {
                        highest = highest.max(j);
                        has_cd = true;
                    }
                    "PC" => {
                        highest = highest.max(j);
                        has_pc = true;
                    }
                    _ => {}
                }
            }
            if alt.is_none() && index1(key, "CROTA", "").is_some() {
                any = true;
            }
        }
        if !any {
            return Ok(None);
        }
        let n = match keys.int(&format!("WCSAXES{s}")) {
            Some(n) if n > 0 => n as usize,
            _ => naxis.max(highest),
        };

        let per_axis_f = |p: &str, default: f64| -> Vec<f64> {
            (1..=n)
                .map(|i| keys.float(&format!("{p}{i}{s}")).unwrap_or(default))
                .collect()
        };
        let per_axis_s = |p: &str| -> Vec<String> {
            (1..=n)
                .map(|i| keys.string(&format!("{p}{i}{s}")).unwrap_or_default())
                .collect()
        };
        let ctype = per_axis_s("CTYPE");
        let cunit = per_axis_s("CUNIT");
        let crpix = per_axis_f("CRPIX", 0.0);
        let crval = per_axis_f("CRVAL", 0.0);
        let cdelt = per_axis_f("CDELT", 1.0);
        let matrix = |p: &str, diagonal: f64| -> Vec<f64> {
            let mut m = vec![0.0; n * n];
            for i in 0..n {
                for j in 0..n {
                    let default = if i == j { diagonal } else { 0.0 };
                    m[i * n + j] = keys
                        .float(&format!("{p}{}_{}{s}", i + 1, j + 1))
                        .unwrap_or(default);
                }
            }
            m
        };
        let pc = matrix("PC", 1.0);

        let celestial = {
            let lon: Vec<usize> = (0..n).filter(|&i| is_longitude(&ctype[i])).collect();
            let lat: Vec<usize> = (0..n).filter(|&i| is_latitude(&ctype[i])).collect();
            match (lon.as_slice(), lat.as_slice()) {
                (&[lon], &[lat]) => {
                    match (projection_code(&ctype[lon]), projection_code(&ctype[lat])) {
                        (Some(a), Some(b)) if a == b => Some(Celestial {
                            lon,
                            lat,
                            projection: a.to_string(),
                        }),
                        _ => None,
                    }
                }
                _ => None,
            }
        };

        let mut ignored = Vec::new();
        let mut note = |key: String| {
            if keys.has(&key) {
                ignored.push(key);
            }
        };
        // The CROTA that applies: the latitude axis's, or CROTA2 without
        // celestial axes. Alternate WCSs have none.
        let (rot_a, rot_b) = match &celestial {
            Some(c) => (c.lon, c.lat),
            None => (0, 1),
        };
        let crota_key = format!("CROTA{}", rot_b + 1);
        let crota_value = if alt.is_none() && n >= 2 {
            keys.float(&crota_key)
        } else {
            None
        };

        let (form, cd, crota) = if has_cd {
            for i in 1..=n {
                note(format!("CDELT{i}{s}"));
                for j in 1..=n {
                    note(format!("PC{i}_{j}{s}"));
                }
                if alt.is_none() {
                    note(format!("CROTA{i}"));
                }
            }
            (LinearForm::Cd, matrix("CD", 0.0), None)
        } else if has_pc || crota_value.is_none() {
            if alt.is_none() {
                for i in 1..=n {
                    note(format!("CROTA{i}"));
                }
            }
            let mut cd = pc.clone();
            for i in 0..n {
                for j in 0..n {
                    cd[i * n + j] *= cdelt[i];
                }
            }
            (LinearForm::Pc, cd, None)
        } else {
            let rho = crota_value.expect("checked above");
            for i in 1..=n {
                if i != rot_b + 1 {
                    note(format!("CROTA{i}"));
                }
            }
            let mut cd = vec![0.0; n * n];
            for i in 0..n {
                cd[i * n + i] = cdelt[i];
            }
            let (sin, cos) = (libm::sin(rho.to_radians()), libm::cos(rho.to_radians()));
            let (l, b) = (rot_a, rot_b);
            cd[l * n + l] = cdelt[l] * cos;
            cd[l * n + b] = -cdelt[b] * sin;
            cd[b * n + l] = cdelt[l] * sin;
            cd[b * n + b] = cdelt[b] * cos;
            (LinearForm::Crota, cd, Some(rho))
        };

        let mut pv: Vec<Pv> = keys
            .0
            .iter()
            .filter_map(|(&key, _)| {
                let (axis, m) = index2(key, "PV", s)?;
                Some(Pv {
                    axis,
                    m,
                    value: keys.float(key)?,
                })
            })
            .collect();
        pv.sort_by_key(|p| (p.axis, p.m));

        let radesys = keys
            .string(&format!("RADESYS{s}"))
            .or_else(|| alt.is_none().then(|| keys.string("RADECSYS")).flatten());
        let equinox = keys
            .float(&format!("EQUINOX{s}"))
            .or_else(|| alt.is_none().then(|| keys.float("EPOCH")).flatten());

        let sip = if alt.is_none() {
            let poly = |name: &str, ignored: &mut Vec<String>| -> Option<SipPolynomial> {
                let order = keys.int(&format!("{name}_ORDER"))?.max(0) as usize;
                let prefix = format!("{name}_");
                let mut coefficients = Vec::new();
                for &key in keys.0.keys() {
                    let Some((p, q)) = index2(key, &prefix, "") else {
                        continue;
                    };
                    match keys.float(key) {
                        Some(v) if p + q <= order => coefficients.push((p, q, v)),
                        _ => ignored.push(key.to_string()),
                    }
                }
                coefficients.sort_by_key(|&(p, q, _)| (p, q));
                Some(SipPolynomial {
                    order,
                    coefficients,
                })
            };
            match (poly("A", &mut ignored), poly("B", &mut ignored)) {
                (Some(a), Some(b)) => Some(Sip {
                    a,
                    b,
                    ap: poly("AP", &mut ignored),
                    bp: poly("BP", &mut ignored),
                }),
                _ => None,
            }
        } else {
            None
        };

        Ok(Some(WcsKeys {
            alt,
            wcsname: keys.string(&format!("WCSNAME{s}")),
            naxes,
            ctype,
            cunit,
            crpix,
            crval,
            cd,
            form,
            cdelt,
            pc,
            crota,
            celestial,
            lonpole: keys.float(&format!("LONPOLE{s}")),
            latpole: keys.float(&format!("LATPOLE{s}")),
            pv,
            radesys,
            equinox,
            sip,
            ignored,
        }))
    }

    /// The number of WCS axes.
    pub fn naxis(&self) -> usize {
        self.ctype.len()
    }

    /// `CD(i+1)_(j+1)`, the element of the CD matrix for 0-based `i`, `j`.
    pub fn cd(&self, i: usize, j: usize) -> f64 {
        self.cd[i * self.naxis() + j]
    }

    /// Whether the celestial axes are equatorial (`RA`/`DEC`), the only
    /// ones `RADESYS` and `EQUINOX` describe.
    fn equatorial(&self) -> bool {
        self.celestial
            .as_ref()
            .is_some_and(|c| axis_type(&self.ctype[c.lon]) == "RA")
    }

    /// The reference frame of equatorial axes: `RADESYSa`, or by Paper II's
    /// defaults `FK4` when `EQUINOX` is before 1984, `FK5` when it is
    /// 1984 or later, and `ICRS` without either. `None` for other axes.
    pub fn radesys_or_default(&self) -> Option<&str> {
        if !self.equatorial() {
            return None;
        }
        Some(match (&self.radesys, self.equinox) {
            (Some(r), _) => r.as_str(),
            (None, Some(e)) if e < 1984.0 => "FK4",
            (None, Some(_)) => "FK5",
            (None, None) => "ICRS",
        })
    }

    /// The equinox of equatorial axes: `EQUINOXa`, or by Paper II's
    /// defaults 1950 for `FK4`/`FK4-NO-E` and 2000 for `FK5`. `None` for
    /// `ICRS` and `GAPPT`, which have none, and for other axes.
    pub fn equinox_or_default(&self) -> Option<f64> {
        if let Some(e) = self.equinox {
            return self.equatorial().then_some(e);
        }
        match self.radesys_or_default()? {
            "FK4" | "FK4-NO-E" => Some(1950.0),
            "FK5" => Some(2000.0),
            _ => None,
        }
    }

    /// `CTYPEi` of 0-based axis `i` as written: with a `-SIP` suffix on the
    /// celestial axes when there is SIP.
    fn ctype_written(&self, i: usize) -> String {
        let mut ctype = self.ctype[i].clone();
        let celestial = self
            .celestial
            .as_ref()
            .is_some_and(|c| i == c.lon || i == c.lat);
        if self.sip.is_some() && celestial && !ctype.ends_with("-SIP") {
            ctype.push_str("-SIP");
        }
        ctype
    }

    /// The WCS as cards, the linear part as `CDi_j`: see
    /// [`WcsKeys::to_cards_as`].
    pub fn to_cards(&self) -> Result<Vec<Card>> {
        self.to_cards_as(LinearForm::Cd)
    }

    /// The WCS as cards, with the linear part in `form`:
    /// - `Cd`: every non-zero `CDi_j` (the diagonal if all are zero);
    /// - `Pc`: `CDELTi` and the `PCi_j` that differ from the identity. A WCS
    ///   read as CD is written with `CDELTi = 1` and `PCi_j = CDi_j`, as
    ///   wcslib converts it; one read as CROTA with its `CDELTi`;
    /// - `Crota`: `CDELTi` and the `CROTAi` read, only for a WCS read in
    ///   that form (an error otherwise, or for an alternate).
    ///
    /// `WCSAXESa`, `WCSNAMEa`, `CTYPEi` (with `-SIP` on the celestial axes
    /// when there is SIP), `CUNITi` when given, `CRPIXi`, `CRVALi`,
    /// `LONPOLE`/`LATPOLE`/`PVi_m`/`RADESYS`/`EQUINOX` when given, then every
    /// SIP coefficient. Each keyword carries the alternate letter.
    pub fn to_cards_as(&self, form: LinearForm) -> Result<Vec<Card>> {
        let n = self.naxis();
        let s: String = self.alt.map(String::from).unwrap_or_default();
        let mut cards = Vec::new();
        let mut push = |key: String, value: Value| -> Result<()> {
            cards.push(Card::new(&key, value)?);
            Ok(())
        };
        push(format!("WCSAXES{s}"), Value::Integer(n as i64))?;
        if let Some(name) = &self.wcsname {
            push(format!("WCSNAME{s}"), Value::String(name.clone()))?;
        }
        for i in 0..n {
            push(
                format!("CTYPE{}{s}", i + 1),
                Value::String(self.ctype_written(i)),
            )?;
        }
        for (i, unit) in self.cunit.iter().enumerate() {
            if !unit.is_empty() {
                push(format!("CUNIT{}{s}", i + 1), Value::String(unit.clone()))?;
            }
        }
        for (i, v) in self.crpix.iter().enumerate() {
            push(format!("CRPIX{}{s}", i + 1), Value::Float(*v))?;
        }
        for (i, v) in self.crval.iter().enumerate() {
            push(format!("CRVAL{}{s}", i + 1), Value::Float(*v))?;
        }
        match form {
            LinearForm::Cd => {
                let all_zero = self.cd.iter().all(|&v| v == 0.0);
                for i in 0..n {
                    for j in 0..n {
                        let v = self.cd(i, j);
                        if v != 0.0 || (all_zero && i == j) {
                            push(format!("CD{}_{}{s}", i + 1, j + 1), Value::Float(v))?;
                        }
                    }
                }
            }
            LinearForm::Pc => {
                let (cdelt, pc) = match self.form {
                    LinearForm::Pc => (self.cdelt.clone(), self.pc.clone()),
                    LinearForm::Cd => (vec![1.0; n], self.cd.clone()),
                    LinearForm::Crota => {
                        let mut pc = self.cd.clone();
                        for i in 0..n {
                            for j in 0..n {
                                pc[i * n + j] /= self.cdelt[i];
                            }
                        }
                        (self.cdelt.clone(), pc)
                    }
                };
                for (i, v) in cdelt.iter().enumerate() {
                    push(format!("CDELT{}{s}", i + 1), Value::Float(*v))?;
                }
                for i in 0..n {
                    for j in 0..n {
                        let v = pc[i * n + j];
                        if v != if i == j { 1.0 } else { 0.0 } {
                            push(format!("PC{}_{}{s}", i + 1, j + 1), Value::Float(v))?;
                        }
                    }
                }
            }
            LinearForm::Crota => {
                let rho = match (self.form, self.alt, self.crota) {
                    (LinearForm::Crota, None, Some(rho)) => rho,
                    _ => {
                        return Err(Error::InvalidHeader(
                            "only a WCS read with CROTA can be written with CROTA",
                        ))
                    }
                };
                for (i, v) in self.cdelt.iter().enumerate() {
                    push(format!("CDELT{}", i + 1), Value::Float(*v))?;
                }
                let lat = self.celestial.as_ref().map_or(1, |c| c.lat);
                push(format!("CROTA{}", lat + 1), Value::Float(rho))?;
            }
        }
        if let Some(v) = self.lonpole {
            push(format!("LONPOLE{s}"), Value::Float(v))?;
        }
        if let Some(v) = self.latpole {
            push(format!("LATPOLE{s}"), Value::Float(v))?;
        }
        for pv in &self.pv {
            push(format!("PV{}_{}{s}", pv.axis, pv.m), Value::Float(pv.value))?;
        }
        if let Some(r) = &self.radesys {
            push(format!("RADESYS{s}"), Value::String(r.clone()))?;
        }
        if let Some(e) = self.equinox {
            push(format!("EQUINOX{s}"), Value::Float(e))?;
        }
        if let (Some(sip), None) = (&self.sip, self.alt) {
            let polys = [
                ("A", Some(&sip.a)),
                ("B", Some(&sip.b)),
                ("AP", sip.ap.as_ref()),
                ("BP", sip.bp.as_ref()),
            ];
            for (name, poly) in polys {
                let Some(poly) = poly else { continue };
                push(format!("{name}_ORDER"), Value::Integer(poly.order as i64))?;
                for &(p, q, v) in &poly.coefficients {
                    push(format!("{name}_{p}_{q}"), Value::Float(v))?;
                }
            }
        }
        Ok(cards)
    }
}

/// Conversion into the `wcs` crate's `WCSParams`, the CDS projection engine.
#[cfg(feature = "cds-wcs")]
mod cds {
    use super::{Error, Result, WcsKeys};
    use alloc::format;
    use alloc::string::String;
    use alloc::vec::Vec;
    use serde::de::{self, value::MapDeserializer, Deserializer, IntoDeserializer, Visitor};
    use serde::Deserialize;

    /// One `WCSParams` field's value.
    enum Field {
        Float(f64),
        Int(i64),
        Text(String),
    }

    struct FieldDeserializer(Field);

    impl<'de> Deserializer<'de> for FieldDeserializer {
        type Error = de::value::Error;

        fn deserialize_any<V: Visitor<'de>>(
            self,
            visitor: V,
        ) -> core::result::Result<V::Value, Self::Error> {
            match self.0 {
                Field::Float(v) => visitor.visit_f64(v),
                Field::Int(v) => visitor.visit_i64(v),
                Field::Text(v) => visitor.visit_string(v),
            }
        }

        fn deserialize_option<V: Visitor<'de>>(
            self,
            visitor: V,
        ) -> core::result::Result<V::Value, Self::Error> {
            visitor.visit_some(self)
        }

        serde::forward_to_deserialize_any! {
            bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
            bytes byte_buf unit unit_struct newtype_struct seq tuple
            tuple_struct map struct enum identifier ignored_any
        }
    }

    impl<'de> IntoDeserializer<'de, de::value::Error> for Field {
        type Deserializer = FieldDeserializer;

        fn into_deserializer(self) -> FieldDeserializer {
            FieldDeserializer(self)
        }
    }

    /// `WCSParams` for this WCS, for `wcs::WCS::new`.
    ///
    /// The `wcs` crate projects images whose celestial axes are pixel axes 1
    /// (longitude) and 2 (latitude), of 2 to 4 axes; it reads up to three
    /// axes' `CRPIXi`/`CRVALi`/`CDi_j`, `PV1_0`–`PV1_2` and
    /// `PV2_0`–`PV2_20`. Anything else is an error rather than silently
    /// dropped. The linear part goes over as the CD matrix, and
    /// `RADESYS`/`EQUINOX`/`LONPOLE`/`LATPOLE` as given. Pixels count from
    /// 1, as in FITS. The `wcs` crate (0.4.2) converts a galactic WCS's
    /// coordinates to ICRS and gives every other frame's as they are.
    ///
    /// A WCS with SIP distortion is an error: `wcs` 0.4.2 evaluates SIP
    /// polynomials with `mapproj` 0.4.0's `SipCoeff::p`, which squares its
    /// running powers (`u`, `u²`, `u⁴`, …) and never forms the terms
    /// without both `u` and `v` (`A_0_0`, `A_1_0`, …), so its positions
    /// would be wrong. `WcsKeys::sip` still holds every coefficient. (The
    /// fix is merged in `mapproj`'s repository but not released.)
    ///
    /// Pixel to world (`WCS::unproj`) agrees with astropy. World to pixel
    /// (`WCS::proj`) in `wcs` 0.4.2 is wrong when the CD matrix isn't
    /// symmetric, up to 40 px on fits-test-cases' `wcs/` fixtures, because
    /// `mapproj` 0.4.0 swaps the off-diagonal terms of its inverse; that fix
    /// is still an open pull request there.
    impl TryFrom<&WcsKeys> for cds_wcs::WCSParams {
        type Error = Error;

        fn try_from(keys: &WcsKeys) -> Result<Self> {
            let celestial = keys
                .celestial
                .as_ref()
                .ok_or(Error::InvalidHeader("the wcs crate needs celestial axes"))?;
            if (celestial.lon, celestial.lat) != (0, 1) {
                return Err(Error::InvalidHeader(
                    "the wcs crate needs longitude on axis 1 and latitude on axis 2",
                ));
            }
            if !(2..=4).contains(&keys.naxes.len()) {
                return Err(Error::InvalidHeader("the wcs crate needs NAXIS 2 to 4"));
            }
            if keys.sip.is_some() {
                return Err(Error::InvalidHeader(
                    "the wcs crate 0.4 evaluates SIP distortion incorrectly",
                ));
            }
            let mut fields: Vec<(String, Field)> = Vec::new();
            fields.push(("naxis".into(), Field::Int(keys.naxes.len() as i64)));
            for (i, &len) in keys.naxes.iter().enumerate() {
                fields.push((format!("naxis{}", i + 1), Field::Int(len as i64)));
            }
            let n = keys.naxis().min(3);
            for i in 0..n {
                let axis = i + 1;
                fields.push((format!("ctype{axis}"), Field::Text(keys.ctype_written(i))));
                fields.push((format!("crpix{axis}"), Field::Float(keys.crpix[i])));
                fields.push((format!("crval{axis}"), Field::Float(keys.crval[i])));
                for j in 0..n {
                    fields.push((format!("cd{axis}_{}", j + 1), Field::Float(keys.cd(i, j))));
                }
            }
            for pv in &keys.pv {
                let supported = match pv.axis {
                    1 => pv.m <= 2,
                    2 => pv.m <= 20,
                    _ => false,
                };
                if !supported {
                    return Err(Error::InvalidHeader(
                        "a PVi_m the wcs crate has no field for",
                    ));
                }
                fields.push((format!("pv{}_{}", pv.axis, pv.m), Field::Float(pv.value)));
            }
            let optional = [
                ("lonpole", keys.lonpole),
                ("latpole", keys.latpole),
                ("equinox", keys.equinox),
            ];
            for (name, value) in optional {
                if let Some(v) = value {
                    fields.push((name.into(), Field::Float(v)));
                }
            }
            if let Some(r) = &keys.radesys {
                fields.push(("radesys".into(), Field::Text(r.clone())));
            }
            // `WCSParams` deserializes from the FITS keyword names.
            let fields = fields.into_iter().map(|(k, v)| (k.to_uppercase(), v));
            let map = MapDeserializer::<_, de::value::Error>::new(fields);
            cds_wcs::WCSParams::deserialize(map)
                .map_err(|_| Error::InvalidHeader("WCS keywords the wcs crate can't take"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cards(entries: &[(&str, Value)]) -> Vec<Card> {
        entries
            .iter()
            .map(|(k, v)| Card::new(k, v.clone()).unwrap())
            .collect()
    }

    fn f(v: f64) -> Value {
        Value::Float(v)
    }

    fn text(s: &str) -> Value {
        Value::String(String::from(s))
    }

    fn tan() -> Vec<(&'static str, Value)> {
        vec![
            ("NAXIS", Value::Integer(2)),
            ("NAXIS1", Value::Integer(300)),
            ("NAXIS2", Value::Integer(200)),
            ("CTYPE1", text("RA---TAN")),
            ("CTYPE2", text("DEC--TAN")),
            ("CRPIX1", f(100.0)),
            ("CRPIX2", f(50.0)),
            ("CRVAL1", f(10.0)),
            ("CRVAL2", f(20.0)),
        ]
    }

    fn keys(extra: &[(&str, Value)]) -> WcsKeys {
        let mut entries = tan();
        entries.extend_from_slice(extra);
        WcsKeys::from_cards(&cards(&entries), None)
            .unwrap()
            .unwrap()
    }

    #[test]
    fn cd_wins_over_pc_cdelt_and_crota() {
        let k = keys(&[
            ("CD1_1", f(-2e-4)),
            ("CD2_2", f(3e-4)),
            ("PC1_1", f(0.9)),
            ("CDELT1", f(-1e-3)),
            ("CROTA2", f(30.0)),
        ]);
        assert_eq!(k.form, LinearForm::Cd);
        assert_eq!(k.cd, [-2e-4, 0.0, 0.0, 3e-4]);
        assert_eq!(k.ignored, ["CDELT1", "PC1_1", "CROTA2"]);
    }

    #[test]
    fn pc_wins_over_crota_and_defaults_to_the_identity() {
        let k = keys(&[("PC1_2", f(0.1)), ("CDELT1", f(-1e-3)), ("CROTA2", f(30.0))]);
        assert_eq!(k.form, LinearForm::Pc);
        assert_eq!(k.cd, [-1e-3, -1e-4, 0.0, 1.0]);
        assert_eq!(k.ignored, ["CROTA2"]);
        let bare = keys(&[]);
        assert_eq!(
            (bare.form, bare.cd.as_slice()),
            (LinearForm::Pc, &[1.0, 0.0, 0.0, 1.0][..])
        );
    }

    #[test]
    fn crota_of_the_latitude_axis_applies_and_others_are_ignored() {
        let rho = 30f64.to_radians();
        let k = keys(&[
            ("CDELT1", f(-1e-3)),
            ("CDELT2", f(2e-3)),
            ("CROTA2", f(30.0)),
        ]);
        assert_eq!((k.form, k.crota), (LinearForm::Crota, Some(30.0)));
        let want = [
            -1e-3 * rho.cos(),
            -2e-3 * rho.sin(),
            -1e-3 * rho.sin(),
            2e-3 * rho.cos(),
        ];
        for (got, want) in k.cd.iter().zip(want) {
            assert!((got - want).abs() < 1e-18);
        }
        // CROTA1 rotates nothing when latitude is axis 2.
        let k = keys(&[("CDELT1", f(-1e-3)), ("CROTA1", f(30.0))]);
        assert_eq!(
            (k.form, k.cd.as_slice()),
            (LinearForm::Pc, &[-1e-3, 0.0, 0.0, 1.0][..])
        );
        assert_eq!(k.ignored, ["CROTA1"]);
    }

    #[test]
    fn frame_defaults_follow_paper_ii() {
        assert_eq!(keys(&[]).radesys_or_default(), Some("ICRS"));
        assert_eq!(keys(&[]).equinox_or_default(), None);
        let fk4 = keys(&[("EQUINOX", f(1950.0))]);
        assert_eq!(
            (fk4.radesys_or_default(), fk4.equinox_or_default()),
            (Some("FK4"), Some(1950.0))
        );
        let fk5 = keys(&[("EPOCH", f(2000.0))]);
        assert_eq!(
            (fk5.radesys_or_default(), fk5.equinox_or_default()),
            (Some("FK5"), Some(2000.0))
        );
        let given = keys(&[("RADESYS", text("FK5"))]);
        assert_eq!(given.equinox_or_default(), Some(2000.0));
        let mut galactic = tan();
        galactic[3].1 = text("GLON-TAN");
        galactic[4].1 = text("GLAT-TAN");
        let g = WcsKeys::from_cards(&cards(&galactic), None)
            .unwrap()
            .unwrap();
        assert_eq!(g.celestial.as_ref().map(|c| (c.lon, c.lat)), Some((0, 1)));
        assert_eq!(
            (g.radesys_or_default(), g.equinox_or_default()),
            (None, None)
        );
    }

    #[test]
    fn sip_keeps_every_term_up_to_its_order() {
        let k = keys(&[
            ("A_ORDER", Value::Integer(2)),
            ("A_0_0", f(0.5)),
            ("A_1_0", f(1e-3)),
            ("A_0_2", f(2e-6)),
            ("A_3_0", f(9.0)),
            ("B_ORDER", Value::Integer(1)),
            ("B_0_1", f(-1e-3)),
        ]);
        let sip = k.sip.as_ref().unwrap();
        assert_eq!(
            sip.a.coefficients,
            [(0, 0, 0.5), (0, 2, 2e-6), (1, 0, 1e-3)]
        );
        assert_eq!(
            (sip.a.coefficient(0, 0), sip.a.coefficient(1, 1)),
            (0.5, 0.0)
        );
        assert_eq!(sip.b.coefficients, [(0, 1, -1e-3)]);
        assert_eq!((sip.ap.is_none(), sip.bp.is_none()), (true, true));
        assert_eq!(k.ignored, ["A_3_0"]);
        let written: Vec<String> = k
            .to_cards()
            .unwrap()
            .iter()
            .map(|c| String::from(c.keyword_str()))
            .collect();
        assert!(written.contains(&String::from("A_0_0")));
        let ctype1 = &k.to_cards().unwrap()[1];
        assert_eq!(ctype1.value, Some(text("RA---TAN-SIP")));
    }

    #[test]
    fn alternates_have_their_own_keys_and_no_crota() {
        let mut entries = tan();
        entries.extend([
            ("CROTA2", f(30.0)),
            ("CTYPE1B", text("DETX")),
            ("CRVAL1B", f(1.5)),
            ("CDELT1B", f(0.25)),
        ]);
        let cards = cards(&entries);
        assert_eq!(WcsKeys::alternates(&cards), ['B']);
        let b = WcsKeys::from_cards(&cards, Some('B')).unwrap().unwrap();
        assert_eq!((b.naxis(), b.form), (2, LinearForm::Pc));
        assert_eq!(
            (b.ctype[0].as_str(), b.crval[0], b.cd(0, 0)),
            ("DETX", 1.5, 0.25)
        );
        assert!(b.celestial.is_none());
        let written = b.to_cards().unwrap();
        assert!(written.iter().all(|c| c.keyword_str().ends_with('B')));
        assert!(b.to_cards_as(LinearForm::Crota).is_err());
        assert_eq!(WcsKeys::from_cards(&cards, Some('C')).unwrap(), None);
        assert!(WcsKeys::from_cards(&cards, Some('b')).is_err());
    }

    #[test]
    fn a_header_without_wcs_keywords_has_no_wcs() {
        let plain = cards(&[("NAXIS", Value::Integer(2)), ("OBJECT", text("M13"))]);
        assert_eq!(WcsKeys::from_cards(&plain, None).unwrap(), None);
        assert!(WcsKeys::alternates(&plain).is_empty());
    }
}
