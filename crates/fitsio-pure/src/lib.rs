//! Pure Rust FITS file reader/writer.
//!
//! Parse FITS files with [`hdu::parse_fits`], then read image pixels via
//! [`image::read_image_data`] or table columns via [`bintable`] and [`table`].
//! Tile-compressed images (RICE_1 / GZIP_1) are handled transparently through
//! the [`tiled`] module.
//!
//! The core library is `no_std`-compatible (requires `alloc`). Enable the
//! `compat` feature for a drop-in replacement of the `fitsio` crate API.
#![cfg_attr(not(feature = "std"), no_std)]
#![warn(missing_docs)]

extern crate alloc;

/// Binary table (BINTABLE) column parsing and data extraction.
pub mod bintable;
/// FITS 2880-byte block utilities and constants.
pub mod block;
/// HDU checksum computation and encoding (CHECKSUM/DATASUM).
pub mod checksum;
/// Big-endian byte conversion helpers for FITS data types.
pub mod endian;
/// Error types used throughout the crate.
pub mod error;
/// Extension HDU (IMAGE/TABLE/BINTABLE) header parsing.
pub mod extension;
/// Top-level FITS parsing: HDU discovery and metadata extraction.
pub mod hdu;
/// Header card parsing and serialization.
pub mod header;
/// Image pixel data reading and type conversion.
pub mod image;
/// Minimal `Read`/`Write`/`Seek` traits for `no_std` environments.
pub mod io;
/// Primary HDU header parsing and construction.
pub mod primary;
/// ASCII table (TABLE) column parsing and data extraction.
pub mod table;
/// Tile-compressed image decompression (RICE_1, GZIP_1).
pub mod tiled;
/// FITS header value representation (integer, float, string, logical).
pub mod value;

pub use block::{BLOCK_SIZE, CARDS_PER_BLOCK, CARD_SIZE};
pub use error::{Error, Result};

/// Compatibility layer mirroring the `fitsio` crate API.
#[cfg(feature = "compat")]
pub mod compat;
