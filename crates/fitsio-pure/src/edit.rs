//! Updating header cards of a FITS file in place.
//!
//! [`update_card`](crate::edit::update_card) changes one keyword of one HDU by rewriting only header
//! bytes: the matching 80-byte card is overwritten, or a new card takes the
//! slot where `END` was and `END` moves down one. The data unit is never
//! read or written, so stamping a keyword onto a multi-gigabyte file costs a
//! few header blocks of I/O.
//!
//! Keywords that describe the data layout or its interpretation (`BITPIX`,
//! `NAXISn`, `BZERO`, `TFORMn`, …) are refused, since changing them in place
//! would misdescribe the data. `CHECKSUM` and `DATASUM` are refused too; an
//! existing `CHECKSUM` is left as it was and no longer matches the header
//! after an update, as with cfitsio until `fits_update_chksum` is called.
//!
//! These writes are not atomic: they edit the file where it is, which is the
//! point. Replacing or inserting a card writes at most two adjacent cards, but
//! a crash partway through [`update_card_growing`](crate::edit::update_card_growing)
//! leaves the file corrupt. To rewrite a whole file atomically, use
//! [`write_atomic`](crate::io::write_atomic).

use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::vec;
use std::vec::Vec;

use crate::block::{BLOCK_SIZE, CARD_SIZE};
use crate::error::{Error, Result};
use crate::header::{format_card, format_end_card, parse_card, Card};
use crate::stream::FitsReader;
use crate::value::Value;

/// Bytes moved at a time when [`update_card_growing`] shifts the file.
const SHIFT_CHUNK: usize = 1 << 20;

/// The longest string, with each `'` counted twice, that fits in one card.
const MAX_CARD_STRING: usize = 68;

/// How [`update_card`] changed the header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardUpdate {
    /// An existing card with the keyword was overwritten.
    Replaced,
    /// The card was written where `END` was, and `END` moved down one slot.
    Inserted,
    /// The header had no free slot, so it grew by one block and everything
    /// after it moved down (only from [`update_card_growing`]).
    Grown,
}

/// Sets `card` in the header of HDU `hdu_index` (0 is the primary) of the
/// FITS file in `file`, changing only header bytes.
///
/// If the header has a valued card with the same keyword (compared ignoring
/// ASCII case), the first such card is overwritten. Otherwise the card is
/// inserted before `END`. `COMMENT`, `HISTORY` and blank-keyword cards are
/// always inserted.
///
/// The card is formatted with [`format_card`]. Errors, leaving the file
/// unchanged, when:
/// - the keyword is structural (see the [module docs](crate::edit)), `HIERARCH`,
///   `CONTINUE` or `END`;
/// - a string value needs `CONTINUE` cards, or the card to replace is a long
///   string continued over `CONTINUE` cards;
/// - the HDU does not exist;
/// - the card must be inserted and the header's last block has no free slot.
///   [`update_card_growing`] handles that case instead.
pub fn update_card<F: Read + Write + Seek>(
    file: &mut F,
    hdu_index: usize,
    card: &Card,
) -> Result<CardUpdate> {
    update(file, hdu_index, card, false)
}

/// Like [`update_card`], but when the header is full, grows it by one block
/// and moves the rest of the file down by 2880 bytes, as cfitsio does.
///
/// Growing rewrites everything after the header, so it costs as much I/O as
/// the rest of the file, and an interrupted write leaves the file corrupt.
/// The data bytes themselves are unchanged, only moved.
pub fn update_card_growing<F: Read + Write + Seek>(
    file: &mut F,
    hdu_index: usize,
    card: &Card,
) -> Result<CardUpdate> {
    update(file, hdu_index, card, true)
}

/// [`update_card`] on the file at `path`.
pub fn update_card_in_file(
    path: impl AsRef<Path>,
    hdu_index: usize,
    card: &Card,
) -> Result<CardUpdate> {
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let result = update_card(&mut file, hdu_index, card)?;
    file.sync_data()?;
    Ok(result)
}

fn update<F: Read + Write + Seek>(
    file: &mut F,
    hdu_index: usize,
    card: &Card,
    grow: bool,
) -> Result<CardUpdate> {
    let commentary = card.is_commentary();
    check_card(card, commentary)?;
    let formatted = format_card(card);
    parse_card(&formatted)?;

    let (header_start, header_len) = locate_header(file, hdu_index)?;
    let mut raw = vec![0u8; header_len];
    file.seek(SeekFrom::Start(header_start))?;
    file.read_exact(&mut raw)?;
    let cards: Vec<&[u8]> = raw.chunks_exact(CARD_SIZE).collect();
    let end = cards
        .iter()
        .position(|c| &c[..8] == b"END     ")
        .ok_or(Error::InvalidHeader("missing END card"))?;

    let at = |slot: usize| header_start + (slot * CARD_SIZE) as u64;

    if !commentary {
        let existing = cards[..end]
            .iter()
            .position(|c| c[..8].eq_ignore_ascii_case(&card.keyword) && c[8] == b'=');
        if let Some(slot) = existing {
            if cards.get(slot + 1).is_some_and(|c| &c[..8] == b"CONTINUE") {
                return Err(Error::InvalidHeader(
                    "cannot update a long-string (CONTINUE) card in place",
                ));
            }
            file.seek(SeekFrom::Start(at(slot)))?;
            file.write_all(&formatted)?;
            file.flush()?;
            return Ok(CardUpdate::Replaced);
        }
    }

    if end + 1 < cards.len() {
        file.seek(SeekFrom::Start(at(end)))?;
        file.write_all(&formatted)?;
        file.write_all(&format_end_card())?;
        file.flush()?;
        return Ok(CardUpdate::Inserted);
    }
    if !grow {
        return Err(Error::InvalidHeader(
            "header is full; inserting a card would move the data",
        ));
    }

    let header_end = header_start + header_len as u64;
    shift_tail(file, header_end)?;
    let mut block = vec![b' '; BLOCK_SIZE];
    block[..CARD_SIZE].copy_from_slice(&format_end_card());
    file.seek(SeekFrom::Start(at(end)))?;
    file.write_all(&formatted)?;
    file.seek(SeekFrom::Start(header_end))?;
    file.write_all(&block)?;
    file.flush()?;
    Ok(CardUpdate::Grown)
}

/// Rejects cards that cannot be written in place.
fn check_card(card: &Card, commentary: bool) -> Result<()> {
    let keyword = card.keyword_str().to_ascii_uppercase();
    if is_structural(&keyword) {
        return Err(Error::InvalidHeader(
            "structural keywords cannot be updated in place",
        ));
    }
    match (&card.value, commentary) {
        (Some(_), true) | (None, false) => Err(Error::InvalidValue),
        (Some(Value::String(s)), false) if s.len() + s.matches('\'').count() > MAX_CARD_STRING => {
            Err(Error::InvalidValue)
        }
        _ => Ok(()),
    }
}

/// Keywords whose value describes the data unit's layout or meaning, or the
/// header's own structure.
fn is_structural(keyword: &str) -> bool {
    const FIXED: &[&str] = &[
        "SIMPLE", "XTENSION", "BITPIX", "NAXIS", "END", "BZERO", "BSCALE", "BLANK", "PCOUNT",
        "GCOUNT", "GROUPS", "TFIELDS", "THEAP", "CHECKSUM", "DATASUM", "CONTINUE", "HIERARCH",
        "ZIMAGE", "ZSIMPLE", "ZTENSION", "ZEXTEND", "ZBITPIX", "ZNAXIS", "ZPCOUNT", "ZGCOUNT",
        "ZCMPTYPE", "ZQUANTIZ", "ZDITHER0", "ZBLANK", "ZSCALE", "ZZERO", "ZTHEAP", "ZHECKSUM",
        "ZDATASUM",
    ];
    const INDEXED: &[&str] = &[
        "NAXIS", "TFORM", "TBCOL", "TSCAL", "TZERO", "TNULL", "TDIM", "PTYPE", "PSCAL", "PZERO",
        "ZNAXIS", "ZTILE", "ZNAME", "ZVAL", "ZFORM", "ZCTYP",
    ];
    FIXED.contains(&keyword)
        || INDEXED.iter().any(|prefix| {
            keyword
                .strip_prefix(prefix)
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
}

/// Finds HDU `hdu_index` by reading headers and seeking past data units.
/// Returns the header's offset and length in bytes.
fn locate_header<F: Read + Seek>(file: &mut F, hdu_index: usize) -> Result<(u64, usize)> {
    file.seek(SeekFrom::Start(0))?;
    let mut reader = FitsReader::seekable(&mut *file)?;
    for _ in 0..hdu_index {
        if reader.next_hdu()?.is_none() {
            return Err(Error::InvalidHeader("HDU index out of range"));
        }
    }
    let hdu = reader
        .next_hdu()?
        .ok_or(Error::InvalidHeader("HDU index out of range"))?;
    Ok((hdu.header_start as u64, hdu.data_start - hdu.header_start))
}

/// Moves every byte from `from` to the end of the file down by one block,
/// copying backwards from the end so nothing is overwritten before it moves.
fn shift_tail<F: Read + Write + Seek>(file: &mut F, from: u64) -> Result<()> {
    let len = file.seek(SeekFrom::End(0))?;
    let mut buf = vec![0u8; SHIFT_CHUNK];
    let mut end = len;
    while end > from {
        let start = end.saturating_sub(SHIFT_CHUNK as u64).max(from);
        let chunk = &mut buf[..(end - start) as usize];
        file.seek(SeekFrom::Start(start))?;
        file.read_exact(chunk)?;
        file.seek(SeekFrom::Start(start + BLOCK_SIZE as u64))?;
        file.write_all(chunk)?;
        end = start;
    }
    Ok(())
}
