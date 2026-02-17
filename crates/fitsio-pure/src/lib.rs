#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod bintable;
pub mod block;
pub mod endian;
pub mod error;
pub mod extension;
pub mod hdu;
pub mod header;
pub mod image;
pub mod io;
pub mod primary;
pub mod table;
pub mod tiled;
pub mod value;

pub use block::{BLOCK_SIZE, CARDS_PER_BLOCK, CARD_SIZE};
pub use error::{Error, Result};

#[cfg(feature = "compat")]
pub mod compat;
