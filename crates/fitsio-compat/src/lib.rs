pub use fitsio_pure;

#[cfg(test)]
mod tests {
    #[test]
    fn reexports_core() {
        assert_eq!(fitsio_pure::BLOCK_SIZE, 2880);
    }
}
