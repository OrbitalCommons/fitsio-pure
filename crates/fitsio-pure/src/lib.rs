#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod block;
pub mod endian;
pub mod error;
pub mod header;
pub mod io;
pub mod value;

pub use block::{BLOCK_SIZE, CARDS_PER_BLOCK, CARD_SIZE};
pub use error::{Error, Result};
