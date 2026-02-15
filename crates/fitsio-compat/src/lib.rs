pub use fitsio_pure;

pub mod errors;
pub mod fitsfile;
pub mod hdu;
pub mod headers;
pub mod images;
pub mod tables;

// FitsRow -- placeholder for future derive-macro based row reading (C6)

#[cfg(test)]
mod tests {
    #[test]
    fn reexports_core() {
        assert_eq!(fitsio_pure::BLOCK_SIZE, 2880);
    }
}
