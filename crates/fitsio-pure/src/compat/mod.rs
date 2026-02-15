pub mod errors;
pub mod fitsfile;
pub mod hdu;
pub mod headers;
pub mod images;
pub mod tables;

#[cfg(test)]
mod tests {
    #[test]
    fn reexports_core() {
        assert_eq!(crate::BLOCK_SIZE, 2880);
    }
}
