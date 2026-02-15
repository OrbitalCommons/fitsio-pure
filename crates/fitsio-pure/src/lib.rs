#![cfg_attr(not(feature = "std"), no_std)]

/// FITS block size in bytes.
pub const BLOCK_SIZE: usize = 2880;

/// FITS card (keyword record) size in bytes.
pub const CARD_SIZE: usize = 80;

/// Number of cards per block.
pub const CARDS_PER_BLOCK: usize = BLOCK_SIZE / CARD_SIZE;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_constants() {
        assert_eq!(BLOCK_SIZE, 2880);
        assert_eq!(CARD_SIZE, 80);
        assert_eq!(CARDS_PER_BLOCK, 36);
        assert_eq!(CARDS_PER_BLOCK * CARD_SIZE, BLOCK_SIZE);
    }
}
