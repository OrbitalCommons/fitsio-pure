/// FITS block size in bytes (each logical record is one block).
pub const BLOCK_SIZE: usize = 2880;

/// FITS card (keyword record) size in bytes.
pub const CARD_SIZE: usize = 80;

/// Number of cards that fit in a single block.
pub const CARDS_PER_BLOCK: usize = BLOCK_SIZE / CARD_SIZE;

/// Padding byte used for header blocks (ASCII space).
pub const HEADER_PAD_BYTE: u8 = 0x20;

/// Padding byte used for data blocks (zero).
pub const DATA_PAD_BYTE: u8 = 0x00;

/// Returns the number of FITS blocks required to hold `num_bytes` bytes.
///
/// A FITS file is organized in units of 2880 bytes. This computes the ceiling
/// division: 0 bytes requires 0 blocks, 1 byte requires 1 block, 2880 bytes
/// requires 1 block, 2881 bytes requires 2 blocks, etc.
pub const fn blocks_needed(num_bytes: usize) -> usize {
    if num_bytes == 0 {
        return 0;
    }
    num_bytes.div_ceil(BLOCK_SIZE)
}

/// Returns the total byte length (in whole blocks) required to hold `num_bytes`.
///
/// This is simply `blocks_needed(num_bytes) * BLOCK_SIZE`.
pub const fn padded_byte_len(num_bytes: usize) -> usize {
    blocks_needed(num_bytes) * BLOCK_SIZE
}

/// Copies `src` into the beginning of `dest` and fills the remaining bytes of
/// `dest` with `pad_byte`.
///
/// `dest` must be at least as large as `src`. This is a general-purpose helper
/// used by the header and data padding routines.
///
/// # Panics
///
/// Panics if `dest.len() < src.len()`.
fn copy_and_pad(dest: &mut [u8], src: &[u8], pad_byte: u8) {
    let len = src.len();
    dest[..len].copy_from_slice(src);
    let remaining = &mut dest[len..];
    let mut i = 0;
    while i < remaining.len() {
        remaining[i] = pad_byte;
        i += 1;
    }
}

/// Writes `src` into `dest`, padding any trailing bytes in the final block with
/// ASCII spaces (0x20) as required for FITS header blocks.
///
/// `dest` must have length equal to `padded_byte_len(src.len())`.
///
/// # Panics
///
/// Panics if `dest` is not the correct padded length.
pub fn pad_header_blocks(dest: &mut [u8], src: &[u8]) {
    assert_eq!(
        dest.len(),
        padded_byte_len(src.len()),
        "dest length must equal the padded block length of src"
    );
    copy_and_pad(dest, src, HEADER_PAD_BYTE);
}

/// Writes `src` into `dest`, padding any trailing bytes in the final block with
/// zero bytes (0x00) as required for FITS data blocks.
///
/// `dest` must have length equal to `padded_byte_len(src.len())`.
///
/// # Panics
///
/// Panics if `dest` is not the correct padded length.
pub fn pad_data_blocks(dest: &mut [u8], src: &[u8]) {
    assert_eq!(
        dest.len(),
        padded_byte_len(src.len()),
        "dest length must equal the padded block length of src"
    );
    copy_and_pad(dest, src, DATA_PAD_BYTE);
}

/// Reads exactly one 2880-byte block from `src` starting at the given block
/// index and copies it into `dest`.
///
/// # Panics
///
/// Panics if `dest.len() != BLOCK_SIZE` or if `src` does not contain enough
/// bytes for the requested block.
pub fn read_block(dest: &mut [u8; BLOCK_SIZE], src: &[u8], block_index: usize) {
    let start = block_index * BLOCK_SIZE;
    let end = start + BLOCK_SIZE;
    assert!(
        src.len() >= end,
        "src does not contain block {}",
        block_index
    );
    dest.copy_from_slice(&src[start..end]);
}

/// Writes exactly one 2880-byte block from `block` into `dest` at the given
/// block index.
///
/// # Panics
///
/// Panics if `dest` does not have enough room to hold the block at the
/// requested index.
pub fn write_block(dest: &mut [u8], block: &[u8; BLOCK_SIZE], block_index: usize) {
    let start = block_index * BLOCK_SIZE;
    let end = start + BLOCK_SIZE;
    assert!(
        dest.len() >= end,
        "dest does not have room for block {}",
        block_index
    );
    dest[start..end].copy_from_slice(block);
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- blocks_needed ----

    #[test]
    fn blocks_needed_zero() {
        assert_eq!(blocks_needed(0), 0);
    }

    #[test]
    fn blocks_needed_one_byte() {
        assert_eq!(blocks_needed(1), 1);
    }

    #[test]
    fn blocks_needed_exactly_one_block() {
        assert_eq!(blocks_needed(BLOCK_SIZE), 1);
    }

    #[test]
    fn blocks_needed_one_over() {
        assert_eq!(blocks_needed(BLOCK_SIZE + 1), 2);
    }

    #[test]
    fn blocks_needed_exactly_two_blocks() {
        assert_eq!(blocks_needed(2 * BLOCK_SIZE), 2);
    }

    #[test]
    fn blocks_needed_partial() {
        assert_eq!(blocks_needed(100), 1);
        assert_eq!(blocks_needed(2879), 1);
        assert_eq!(blocks_needed(2881), 2);
        assert_eq!(blocks_needed(5760), 2);
        assert_eq!(blocks_needed(5761), 3);
    }

    // ---- padded_byte_len ----

    #[test]
    fn padded_byte_len_zero() {
        assert_eq!(padded_byte_len(0), 0);
    }

    #[test]
    fn padded_byte_len_aligned() {
        assert_eq!(padded_byte_len(BLOCK_SIZE), BLOCK_SIZE);
        assert_eq!(padded_byte_len(2 * BLOCK_SIZE), 2 * BLOCK_SIZE);
    }

    #[test]
    fn padded_byte_len_unaligned() {
        assert_eq!(padded_byte_len(1), BLOCK_SIZE);
        assert_eq!(padded_byte_len(BLOCK_SIZE + 1), 2 * BLOCK_SIZE);
    }

    // ---- constants ----

    #[test]
    fn constant_relationships() {
        assert_eq!(BLOCK_SIZE, 2880);
        assert_eq!(CARD_SIZE, 80);
        assert_eq!(CARDS_PER_BLOCK, 36);
        assert_eq!(CARDS_PER_BLOCK * CARD_SIZE, BLOCK_SIZE);
    }

    // ---- header padding ----

    #[test]
    fn header_pad_full_block() {
        let src = [0x41u8; BLOCK_SIZE]; // 'A' repeated
        let mut dest = [0u8; BLOCK_SIZE];
        pad_header_blocks(&mut dest, &src);
        assert_eq!(&dest[..], &src[..]);
    }

    #[test]
    fn header_pad_partial_block() {
        let src = [0x41u8; 80]; // one card worth of 'A'
        let mut dest = [0u8; BLOCK_SIZE];
        pad_header_blocks(&mut dest, &src);
        assert_eq!(&dest[..80], &src[..]);
        // Remaining bytes should be ASCII space (0x20)
        for &b in &dest[80..] {
            assert_eq!(b, HEADER_PAD_BYTE);
        }
    }

    #[test]
    fn header_pad_empty() {
        let src: &[u8] = &[];
        let mut dest: [u8; 0] = [];
        pad_header_blocks(&mut dest, src);
        // No bytes to check, just verifying it doesn't panic.
    }

    #[test]
    fn header_pad_multi_block() {
        let src = [0x42u8; BLOCK_SIZE + 100];
        let mut dest = [0u8; 2 * BLOCK_SIZE];
        pad_header_blocks(&mut dest, &src);
        assert_eq!(&dest[..BLOCK_SIZE + 100], &src[..]);
        for &b in &dest[BLOCK_SIZE + 100..] {
            assert_eq!(b, HEADER_PAD_BYTE);
        }
    }

    // ---- data padding ----

    #[test]
    fn data_pad_full_block() {
        let src = [0xFFu8; BLOCK_SIZE];
        let mut dest = [0xAA; BLOCK_SIZE];
        pad_data_blocks(&mut dest, &src);
        assert_eq!(&dest[..], &src[..]);
    }

    #[test]
    fn data_pad_partial_block() {
        let src = [0xFFu8; 100];
        let mut dest = [0xAA; BLOCK_SIZE];
        pad_data_blocks(&mut dest, &src);
        assert_eq!(&dest[..100], &src[..]);
        for &b in &dest[100..] {
            assert_eq!(b, DATA_PAD_BYTE);
        }
    }

    #[test]
    fn data_pad_empty() {
        let src: &[u8] = &[];
        let mut dest: [u8; 0] = [];
        pad_data_blocks(&mut dest, src);
    }

    #[test]
    fn data_pad_multi_block() {
        let src = [0xABu8; BLOCK_SIZE + 500];
        let mut dest = [0u8; 2 * BLOCK_SIZE];
        pad_data_blocks(&mut dest, &src);
        assert_eq!(&dest[..BLOCK_SIZE + 500], &src[..]);
        for &b in &dest[BLOCK_SIZE + 500..] {
            assert_eq!(b, DATA_PAD_BYTE);
        }
    }

    // ---- read_block / write_block ----

    #[test]
    fn read_block_first() {
        let mut data = [0u8; 2 * BLOCK_SIZE];
        for (i, b) in data.iter_mut().enumerate() {
            *b = (i % 256) as u8;
        }
        let mut block = [0u8; BLOCK_SIZE];
        read_block(&mut block, &data, 0);
        assert_eq!(&block[..], &data[..BLOCK_SIZE]);
    }

    #[test]
    fn read_block_second() {
        let mut data = [0u8; 2 * BLOCK_SIZE];
        for (i, b) in data.iter_mut().enumerate() {
            *b = (i % 256) as u8;
        }
        let mut block = [0u8; BLOCK_SIZE];
        read_block(&mut block, &data, 1);
        assert_eq!(&block[..], &data[BLOCK_SIZE..2 * BLOCK_SIZE]);
    }

    #[test]
    #[should_panic(expected = "src does not contain block")]
    fn read_block_out_of_bounds() {
        let data = [0u8; BLOCK_SIZE];
        let mut block = [0u8; BLOCK_SIZE];
        read_block(&mut block, &data, 1);
    }

    #[test]
    fn write_block_round_trip() {
        let mut original = [0u8; BLOCK_SIZE];
        for (i, b) in original.iter_mut().enumerate() {
            *b = (i % 256) as u8;
        }

        let mut buffer = [0u8; 3 * BLOCK_SIZE];
        write_block(&mut buffer, &original, 1);

        let mut readback = [0u8; BLOCK_SIZE];
        read_block(&mut readback, &buffer, 1);
        assert_eq!(&readback[..], &original[..]);
    }

    #[test]
    #[should_panic(expected = "dest does not have room for block")]
    fn write_block_out_of_bounds() {
        let block = [0u8; BLOCK_SIZE];
        let mut dest = [0u8; BLOCK_SIZE];
        write_block(&mut dest, &block, 1);
    }

    // ---- mismatched dest size panics ----

    #[test]
    #[should_panic(expected = "dest length must equal the padded block length")]
    fn header_pad_wrong_dest_size() {
        let src = [0u8; 100];
        let mut dest = [0u8; 100]; // should be BLOCK_SIZE
        pad_header_blocks(&mut dest, &src);
    }

    #[test]
    #[should_panic(expected = "dest length must equal the padded block length")]
    fn data_pad_wrong_dest_size() {
        let src = [0u8; 100];
        let mut dest = [0u8; 100]; // should be BLOCK_SIZE
        pad_data_blocks(&mut dest, &src);
    }
}
