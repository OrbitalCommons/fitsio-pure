//! Random-groups primary HDUs, the layout UVFITS uses.
//!
//! A random-groups HDU (`GROUPS = T`, `NAXIS1 = 0`) holds `GCOUNT` groups,
//! each `PCOUNT` parameters followed by a data array shaped by `NAXIS2`
//! onwards. [`group_params`] reads the parameters' names and scaling
//! (`PTYPEn`, `PSCALn`, `PZEROn`), and [`GroupsWriter`] streams such an HDU
//! to any [`Write`] sink a group at a time, as cfitsio's `ffphpr` and
//! `ffpgpe` write one, without holding the file in memory.
//!
//! ```
//! use fitsio_pure::groups::{GroupParam, GroupsWriter};
//!
//! // Two groups of three parameters and a 2x2 data array.
//! let params = [
//!     GroupParam::new("UU"),
//!     GroupParam::new("DATE").with_zero(2456528.5),
//!     GroupParam::new("DATE"),
//! ];
//! let mut out = Vec::new();
//! let mut writer = GroupsWriter::new(&mut out, -32, &[2, 2], &params, 2, &[])?;
//! writer.write_group(&[0.5f32, 0.25, 0.0, 1.0, 2.0, 3.0, 4.0])?;
//! // A group written short is zero-filled, and so are groups never written.
//! writer.finish()?;
//! # Ok::<(), fitsio_pure::Error>(())
//! ```

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{Error, Result};
use crate::header::Card;
use crate::image_writer::{ImageWriter, Sample, DEFAULT_CHUNK_BYTES};
use crate::io::Write;
use crate::value::Value;

/// One random-groups parameter: its name and how its stored values scale.
///
/// UVFITS names parameters such as `UU`, `VV`, `WW`, `BASELINE` and `DATE`,
/// and may use a name twice: the Julian date is split across two `DATE`
/// parameters, only the first with a zero point. So parameters are kept in
/// order, names may repeat, and a scale or zero may be absent.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupParam {
    /// `PTYPEn`, the parameter's name.
    pub ptype: String,
    /// `PSCALn`, or `None` when the header has none (a scale of 1).
    pub pscal: Option<f64>,
    /// `PZEROn`, or `None` when the header has none (a zero of 0).
    pub pzero: Option<f64>,
}

impl GroupParam {
    /// A parameter called `ptype`, with neither `PSCALn` nor `PZEROn`.
    pub fn new(ptype: impl Into<String>) -> Self {
        GroupParam {
            ptype: ptype.into(),
            pscal: None,
            pzero: None,
        }
    }

    /// Set `PSCALn`.
    pub fn with_scale(mut self, pscal: f64) -> Self {
        self.pscal = Some(pscal);
        self
    }

    /// Set `PZEROn`.
    pub fn with_zero(mut self, pzero: f64) -> Self {
        self.pzero = Some(pzero);
        self
    }

    /// The physical value of a stored value: `stored * PSCALn + PZEROn`.
    pub fn physical(&self, stored: f64) -> f64 {
        stored * self.pscal.unwrap_or(1.0) + self.pzero.unwrap_or(0.0)
    }

    /// The stored value of a physical value: `(physical - PZEROn) / PSCALn`.
    pub fn stored(&self, physical: f64) -> f64 {
        (physical - self.pzero.unwrap_or(0.0)) / self.pscal.unwrap_or(1.0)
    }
}

/// The `pcount` group parameters `cards` describe, in order: `PTYPEn`
/// (empty when missing), `PSCALn` and `PZEROn` for `n` in `1..=pcount`.
pub fn group_params(cards: &[Card], pcount: usize) -> Vec<GroupParam> {
    let find = |keyword: &str| {
        cards
            .iter()
            .find(|c| c.keyword_str() == keyword)
            .and_then(|c| c.value.as_ref())
    };
    let number = |keyword: &str| match find(keyword) {
        Some(Value::Float(f)) => Some(*f),
        Some(Value::Integer(i)) => Some(*i as f64),
        _ => None,
    };
    (1..=pcount)
        .map(|n| GroupParam {
            ptype: match find(&format!("PTYPE{n}")) {
                Some(Value::String(s)) => String::from(s.trim_end()),
                _ => String::new(),
            },
            pscal: number(&format!("PSCAL{n}")),
            pzero: number(&format!("PZERO{n}")),
        })
        .collect()
}

/// The header cfitsio's `ffphpr` writes for a random-groups primary HDU,
/// followed by each parameter's `PTYPEn`, `PSCALn` and `PZEROn`.
///
/// `naxes` are the data array's axes, `NAXIS2` onwards; `NAXIS1` is 0.
pub fn groups_header(
    bitpix: i64,
    naxes: &[usize],
    params: &[GroupParam],
    gcount: usize,
) -> Result<Vec<Card>> {
    if !matches!(bitpix, 8 | 16 | 32 | 64 | -32 | -64) {
        return Err(Error::InvalidBitpix(bitpix));
    }
    let card = |keyword: &str, value: Value, comment: &str| -> Result<Card> {
        let mut card = Card::new(keyword, value)?;
        if !comment.is_empty() {
            card.comment = Some(String::from(comment));
        }
        Ok(card)
    };
    let commentary = |text: &str| Card {
        keyword: *b"COMMENT ",
        value: None,
        comment: Some(String::from(text)),
    };
    let mut cards = Vec::new();
    cards.push(card(
        "SIMPLE",
        Value::Logical(true),
        "file does conform to FITS standard",
    )?);
    cards.push(card(
        "BITPIX",
        Value::Integer(bitpix),
        "number of bits per data pixel",
    )?);
    cards.push(card(
        "NAXIS",
        Value::Integer(naxes.len() as i64 + 1),
        "number of data axes",
    )?);
    let axes = core::iter::once(0).chain(naxes.iter().copied());
    for (i, len) in axes.enumerate() {
        let comment = format!("length of data axis {}", i + 1);
        cards.push(card(
            &format!("NAXIS{}", i + 1),
            Value::Integer(len as i64),
            &comment,
        )?);
    }
    cards.push(card(
        "EXTEND",
        Value::Logical(true),
        "FITS dataset may contain extensions",
    )?);
    cards.push(card(
        "GROUPS",
        Value::Logical(true),
        "random group records are present",
    )?);
    cards.push(card(
        "PCOUNT",
        Value::Integer(params.len() as i64),
        "number of random group parameters",
    )?);
    cards.push(card(
        "GCOUNT",
        Value::Integer(gcount as i64),
        "number of random groups",
    )?);
    cards.push(commentary(
        "  FITS (Flexible Image Transport System) format is defined in 'Astronomy",
    ));
    cards.push(commentary(
        "  and Astrophysics', volume 376, page 359; bibcode: 2001A&A...376..359H",
    ));
    for (i, param) in params.iter().enumerate() {
        let n = i + 1;
        cards.push(card(
            &format!("PTYPE{n}"),
            Value::String(param.ptype.clone()),
            "",
        )?);
        if let Some(pscal) = param.pscal {
            cards.push(card(&format!("PSCAL{n}"), Value::Float(pscal), "")?);
        }
        if let Some(pzero) = param.pzero {
            cards.push(card(&format!("PZERO{n}"), Value::Float(pzero), "")?);
        }
    }
    Ok(cards)
}

/// Streams a random-groups primary HDU to a [`Write`] sink.
///
/// The constructor writes the header. Each [`write_group`](Self::write_group)
/// then writes the next group's values, its parameters followed by its data
/// array, as cfitsio's `ffpgpe` writes them from the group's first element.
/// Values past the end of a group run on into the next, and a group left
/// short is zero-filled, as cfitsio leaves values it was never given.
/// [`finish`](Self::finish) zero-fills any groups not written, up to
/// `GCOUNT`, pads the data unit to its block, and returns the sink, so
/// extension HDUs such as a UVFITS antenna table can follow.
///
/// Values are stored as they are given, with no `PSCALn`/`PZEROn` applied,
/// as `ffpgpe` stores them; [`GroupParam::stored`] converts a physical
/// parameter value. Only one conversion buffer is held, so a UVFITS file
/// of any size is written in a fixed amount of memory.
pub struct GroupsWriter<W: Write> {
    samples: ImageWriter<W>,
    group_len: u64,
    gcount: u64,
}

impl<W: Write> GroupsWriter<W> {
    /// Write the header for `gcount` groups of `params` and a data array of
    /// axes `naxes` (`NAXIS2` onwards), as [`groups_header`] builds it,
    /// followed by `extra` cards, and get ready for the first group.
    pub fn new(
        inner: W,
        bitpix: i64,
        naxes: &[usize],
        params: &[GroupParam],
        gcount: usize,
        extra: &[Card],
    ) -> Result<Self> {
        let mut cards = groups_header(bitpix, naxes, params, gcount)?;
        cards.extend_from_slice(extra);
        let overflow = || Error::InvalidHeader("random-groups data size overflows");
        let data_len = naxes
            .iter()
            .try_fold(1u64, |acc, &n| acc.checked_mul(n as u64))
            .ok_or_else(overflow)?;
        let group_len = data_len
            .checked_add(params.len() as u64)
            .ok_or_else(overflow)?;
        let expected = group_len.checked_mul(gcount as u64).ok_or_else(overflow)?;
        let samples = ImageWriter::start(inner, &cards, bitpix, expected, DEFAULT_CHUNK_BYTES)?;
        Ok(GroupsWriter {
            samples,
            group_len,
            gcount: gcount as u64,
        })
    }

    /// The number of values in a group: `PCOUNT` plus the data array's.
    pub fn group_len(&self) -> u64 {
        self.group_len
    }

    /// The number of groups written so far, each counted once any of its
    /// values are.
    pub fn groups_written(&self) -> u64 {
        self.samples
            .samples_written()
            .div_ceil(self.group_len.max(1))
    }

    /// Write the next group from its first value: `values` are its
    /// parameters, then its data array. More values than a group holds run
    /// on into the following groups, and the last group they reach is
    /// zero-filled to its end. `T` must match `BITPIX`. Writing past
    /// `GCOUNT` groups is an error, and nothing from that call is written.
    pub fn write_group<T: Sample>(&mut self, values: &[T]) -> Result<()> {
        if values.is_empty() {
            return Ok(());
        }
        let groups = (values.len() as u64).div_ceil(self.group_len.max(1));
        if self.groups_written() + groups > self.gcount {
            return Err(Error::InvalidHeader("more groups than GCOUNT declares"));
        }
        self.samples.write_samples(values)?;
        let short = groups * self.group_len - values.len() as u64;
        self.samples.write_zeros(short)
    }

    /// Zero-fill the groups not written, up to `GCOUNT`, pad the data unit
    /// to its block, flush, and return the sink.
    pub fn finish(mut self) -> Result<W> {
        let left = self.samples.samples_expected() - self.samples.samples_written();
        self.samples.write_zeros(left)?;
        self.samples.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hdu::{parse_fits, HduInfo};
    use alloc::vec;

    fn params() -> Vec<GroupParam> {
        vec![
            GroupParam::new("UU").with_scale(1.0).with_zero(0.0),
            GroupParam::new("BASELINE"),
            GroupParam::new("DATE").with_scale(1.0).with_zero(2456528.5),
            GroupParam::new("DATE").with_scale(1.0),
        ]
    }

    #[test]
    fn groups_round_trip_through_the_parser() {
        let mut out = Vec::new();
        let mut writer = GroupsWriter::new(&mut out, -32, &[3, 2], &params(), 3, &[]).unwrap();
        assert_eq!(writer.group_len(), 10);
        let full: Vec<f32> = (1..=10).map(|v| v as f32).collect();
        writer.write_group(&full).unwrap();
        writer.write_group(&[7.0f32, 8.0]).unwrap();
        assert_eq!(writer.groups_written(), 2);
        writer.finish().unwrap();
        assert_eq!(out.len() % crate::BLOCK_SIZE, 0);

        let parsed = parse_fits(&out).unwrap();
        let hdu = parsed.primary();
        let HduInfo::RandomGroups {
            bitpix,
            naxes,
            pcount,
            gcount,
        } = &hdu.info
        else {
            panic!("expected random groups, got {:?}", hdu.info);
        };
        assert_eq!(
            (*bitpix, naxes.as_slice(), *pcount, *gcount),
            (-32, &[3, 2][..], 4, 3)
        );
        assert_eq!(hdu.data_len, 30 * 4);
        assert_eq!(group_params(&hdu.cards, *pcount), params());

        let data: Vec<f32> = out[hdu.data_start..hdu.data_start + hdu.data_len]
            .chunks_exact(4)
            .map(|b| f32::from_be_bytes(b.try_into().unwrap()))
            .collect();
        let mut want = full.clone();
        want.extend([7.0, 8.0]);
        want.resize(30, 0.0);
        assert_eq!(data, want);
    }

    #[test]
    fn values_run_on_into_the_next_group() {
        let mut out = Vec::new();
        let mut writer =
            GroupsWriter::new(&mut out, 16, &[2], &[GroupParam::new("A")], 3, &[]).unwrap();
        writer.write_group(&[1i16, 2, 3, 4]).unwrap();
        assert_eq!(writer.groups_written(), 2);
        // A third group would need four.
        assert!(writer.write_group(&[1i16, 2, 3, 4]).is_err());
        writer.write_group(&[5i16]).unwrap();
        assert!(writer.write_group(&[6i16]).is_err());
        writer.finish().unwrap();
        let hdu = parse_fits(&out).unwrap().hdus.remove(0);
        let data: Vec<i16> = out[hdu.data_start..hdu.data_start + hdu.data_len]
            .chunks_exact(2)
            .map(|b| i16::from_be_bytes([b[0], b[1]]))
            .collect();
        assert_eq!(data, [1, 2, 3, 4, 0, 0, 5, 0, 0]);
    }

    #[test]
    fn the_wrong_sample_type_is_an_error() {
        let mut out = Vec::new();
        let mut writer = GroupsWriter::new(&mut out, -32, &[2], &[], 1, &[]).unwrap();
        assert!(writer.write_group(&[1.0f64, 2.0]).is_err());
    }

    #[test]
    fn physical_values_apply_scale_and_zero() {
        let date = GroupParam::new("DATE").with_scale(2.0).with_zero(10.0);
        assert_eq!(date.physical(3.0), 16.0);
        assert_eq!(date.stored(16.0), 3.0);
        assert_eq!(GroupParam::new("UU").physical(3.0), 3.0);
    }
}
