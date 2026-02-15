//! Trait abstractions for reading/writing FITS data in both `std` and `no_std` contexts.
//!
//! When the `std` feature is enabled, this module re-exports the standard library's
//! I/O traits and types directly. In `no_std` mode, minimal equivalents are provided
//! that support the subset of functionality required for FITS operations.

#[cfg(feature = "std")]
#[allow(unused_imports)]
pub use std::io::{Cursor, Read, Result, Seek, SeekFrom, Write};

// ── no_std: provide our own implementations ──

#[cfg(not(feature = "std"))]
mod nostd {
    /// Minimal I/O error type for `no_std` environments.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum IoError {
        /// An operation attempted to read or seek past the end of available data.
        UnexpectedEof,
        /// A write was attempted on a fixed-size buffer that has no remaining capacity.
        WriteZero,
        /// A seek landed on a negative absolute position.
        InvalidSeek,
    }

    impl core::fmt::Display for IoError {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            match self {
                IoError::UnexpectedEof => write!(f, "unexpected end of file"),
                IoError::WriteZero => write!(f, "write zero"),
                IoError::InvalidSeek => write!(f, "invalid seek to negative position"),
            }
        }
    }

    /// Convenience result type that uses [`IoError`].
    pub type Result<T> = core::result::Result<T, IoError>;

    /// Describes a position to seek to within a stream.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum SeekFrom {
        /// Seek to an absolute byte offset from the start.
        Start(u64),
        /// Seek relative to the current position (may be negative).
        Current(i64),
        /// Seek relative to the end of the stream (may be negative).
        End(i64),
    }

    /// Read bytes from a source.
    pub trait Read {
        /// Pull some bytes from this source into the specified buffer, returning
        /// how many bytes were read.
        fn read(&mut self, buf: &mut [u8]) -> Result<usize>;

        /// Read the exact number of bytes required to fill `buf`.
        ///
        /// Returns `Err(IoError::UnexpectedEof)` if the source runs out of data
        /// before `buf` is completely filled.
        fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
            let mut offset = 0;
            while offset < buf.len() {
                match self.read(&mut buf[offset..])? {
                    0 => return Err(IoError::UnexpectedEof),
                    n => offset += n,
                }
            }
            Ok(())
        }
    }

    /// Write bytes to a destination.
    pub trait Write {
        /// Write a buffer into this writer, returning how many bytes were written.
        fn write(&mut self, buf: &[u8]) -> Result<usize>;

        /// Flush any buffered output.
        fn flush(&mut self) -> Result<()>;

        /// Attempt to write an entire buffer into this writer.
        ///
        /// Returns `Err(IoError::WriteZero)` if the writer cannot accept any more
        /// bytes before the buffer is fully consumed.
        fn write_all(&mut self, buf: &[u8]) -> Result<()> {
            let mut offset = 0;
            while offset < buf.len() {
                match self.write(&buf[offset..])? {
                    0 => return Err(IoError::WriteZero),
                    n => offset += n,
                }
            }
            Ok(())
        }
    }

    /// Seek to a position within a stream.
    pub trait Seek {
        /// Seek to the given position, returning the new absolute offset from the
        /// start of the stream.
        fn seek(&mut self, pos: SeekFrom) -> Result<u64>;
    }

    /// A cursor wrapping a byte buffer, providing [`Read`], [`Write`], and [`Seek`].
    ///
    /// This is the `no_std` equivalent of `std::io::Cursor`.
    #[derive(Debug, Clone)]
    pub struct Cursor<T> {
        inner: T,
        pos: u64,
    }

    impl<T> Cursor<T> {
        /// Create a new cursor wrapping the given byte buffer.
        pub fn new(inner: T) -> Self {
            Cursor { inner, pos: 0 }
        }

        /// Return the current byte offset of the cursor.
        pub fn position(&self) -> u64 {
            self.pos
        }

        /// Set the cursor position.
        pub fn set_position(&mut self, pos: u64) {
            self.pos = pos;
        }

        /// Consume the cursor, returning the wrapped buffer.
        pub fn into_inner(self) -> T {
            self.inner
        }

        /// Borrow the wrapped buffer.
        pub fn get_ref(&self) -> &T {
            &self.inner
        }

        /// Mutably borrow the wrapped buffer.
        pub fn get_mut(&mut self) -> &mut T {
            &mut self.inner
        }
    }

    // ── Read for Cursor<&[u8]> ──

    impl Read for Cursor<&[u8]> {
        fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
            let start = self.pos as usize;
            if start >= self.inner.len() {
                return Ok(0);
            }
            let available = &self.inner[start..];
            let n = buf.len().min(available.len());
            buf[..n].copy_from_slice(&available[..n]);
            self.pos += n as u64;
            Ok(n)
        }
    }

    // ── Seek helper ──

    fn compute_seek_pos(current: u64, len: u64, from: SeekFrom) -> Result<u64> {
        let new_pos: i64 = match from {
            SeekFrom::Start(offset) => offset as i64,
            SeekFrom::Current(offset) => current as i64 + offset,
            SeekFrom::End(offset) => len as i64 + offset,
        };
        if new_pos < 0 {
            return Err(IoError::InvalidSeek);
        }
        Ok(new_pos as u64)
    }

    // ── Seek for Cursor<&[u8]> ──

    impl Seek for Cursor<&[u8]> {
        fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
            let new_pos = compute_seek_pos(self.pos, self.inner.len() as u64, pos)?;
            self.pos = new_pos;
            Ok(new_pos)
        }
    }

    // ── Read / Write / Seek for Cursor<Vec<u8>> ──

    extern crate alloc;

    impl Read for Cursor<alloc::vec::Vec<u8>> {
        fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
            let start = self.pos as usize;
            if start >= self.inner.len() {
                return Ok(0);
            }
            let available = &self.inner[start..];
            let n = buf.len().min(available.len());
            buf[..n].copy_from_slice(&available[..n]);
            self.pos += n as u64;
            Ok(n)
        }
    }

    impl Write for Cursor<alloc::vec::Vec<u8>> {
        fn write(&mut self, buf: &[u8]) -> Result<usize> {
            let start = self.pos as usize;
            // Extend if writing past the current end.
            if start + buf.len() > self.inner.len() {
                self.inner.resize(start + buf.len(), 0);
            }
            self.inner[start..start + buf.len()].copy_from_slice(buf);
            self.pos += buf.len() as u64;
            Ok(buf.len())
        }

        fn flush(&mut self) -> Result<()> {
            Ok(())
        }
    }

    impl Seek for Cursor<alloc::vec::Vec<u8>> {
        fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
            let new_pos = compute_seek_pos(self.pos, self.inner.len() as u64, pos)?;
            self.pos = new_pos;
            Ok(new_pos)
        }
    }
}

#[cfg(not(feature = "std"))]
pub use nostd::*;

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_read_basic() {
        let data = b"hello world";
        let mut cursor = Cursor::new(&data[..]);
        let mut buf = [0u8; 5];
        let n = cursor.read(&mut buf).unwrap();
        assert_eq!(n, 5);
        assert_eq!(&buf, b"hello");
    }

    #[test]
    fn cursor_read_exact_success() {
        let data = b"abcdef";
        let mut cursor = Cursor::new(&data[..]);
        let mut buf = [0u8; 6];
        cursor.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"abcdef");
    }

    #[test]
    fn cursor_read_exact_short_read() {
        let data = b"abc";
        let mut cursor = Cursor::new(&data[..]);
        let mut buf = [0u8; 6];
        let result = cursor.read_exact(&mut buf);
        assert!(result.is_err());
    }

    #[test]
    fn cursor_read_at_eof_returns_zero() {
        let data = b"hi";
        let mut cursor = Cursor::new(&data[..]);
        let mut buf = [0u8; 10];
        let _ = cursor.read(&mut buf).unwrap();
        let n = cursor.read(&mut buf).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn cursor_seek_from_start() {
        let data = b"abcdefgh";
        let mut cursor = Cursor::new(&data[..]);
        let pos = cursor.seek(SeekFrom::Start(3)).unwrap();
        assert_eq!(pos, 3);
        let mut buf = [0u8; 2];
        cursor.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"de");
    }

    #[test]
    fn cursor_seek_from_current() {
        let data = b"abcdefgh";
        let mut cursor = Cursor::new(&data[..]);
        cursor.seek(SeekFrom::Start(2)).unwrap();
        let pos = cursor.seek(SeekFrom::Current(3)).unwrap();
        assert_eq!(pos, 5);
        let mut buf = [0u8; 1];
        cursor.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"f");
    }

    #[test]
    fn cursor_seek_from_current_negative() {
        let data = b"abcdefgh";
        let mut cursor = Cursor::new(&data[..]);
        cursor.seek(SeekFrom::Start(5)).unwrap();
        let pos = cursor.seek(SeekFrom::Current(-2)).unwrap();
        assert_eq!(pos, 3);
        let mut buf = [0u8; 1];
        cursor.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"d");
    }

    #[test]
    fn cursor_seek_from_end() {
        let data = b"abcdefgh";
        let mut cursor = Cursor::new(&data[..]);
        let pos = cursor.seek(SeekFrom::End(-3)).unwrap();
        assert_eq!(pos, 5);
        let mut buf = [0u8; 3];
        cursor.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"fgh");
    }

    #[test]
    fn cursor_seek_end_zero() {
        let data = b"abcd";
        let mut cursor = Cursor::new(&data[..]);
        let pos = cursor.seek(SeekFrom::End(0)).unwrap();
        assert_eq!(pos, 4);
        let mut buf = [0u8; 1];
        let n = cursor.read(&mut buf).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn cursor_vec_write_and_read_back() {
        let mut cursor = Cursor::new(Vec::<u8>::new());
        cursor.write_all(b"fits data").unwrap();
        cursor.seek(SeekFrom::Start(0)).unwrap();
        let mut buf = [0u8; 9];
        cursor.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"fits data");
    }

    #[test]
    fn cursor_vec_write_extends_buffer() {
        let mut cursor = Cursor::new(Vec::<u8>::new());
        cursor.write_all(b"hello").unwrap();
        assert_eq!(cursor.get_ref().len(), 5);
        cursor.write_all(b" world").unwrap();
        assert_eq!(cursor.get_ref().len(), 11);
        assert_eq!(cursor.get_ref(), b"hello world");
    }

    #[test]
    fn cursor_vec_overwrite_middle() {
        let mut cursor = Cursor::new(Vec::<u8>::new());
        cursor.write_all(b"aaabbbccc").unwrap();
        cursor.seek(SeekFrom::Start(3)).unwrap();
        cursor.write_all(b"XXX").unwrap();
        assert_eq!(cursor.get_ref(), b"aaaXXXccc");
    }

    #[test]
    fn cursor_vec_seek_past_end_then_write() {
        let mut cursor = Cursor::new(Vec::<u8>::new());
        cursor.write_all(b"ab").unwrap();
        cursor.seek(SeekFrom::Start(5)).unwrap();
        cursor.write_all(b"c").unwrap();
        assert_eq!(cursor.get_ref().len(), 6);
        assert_eq!(cursor.get_ref()[0], b'a');
        assert_eq!(cursor.get_ref()[1], b'b');
        assert_eq!(cursor.get_ref()[2], 0);
        assert_eq!(cursor.get_ref()[3], 0);
        assert_eq!(cursor.get_ref()[4], 0);
        assert_eq!(cursor.get_ref()[5], b'c');
    }

    #[test]
    fn cursor_vec_flush_is_noop() {
        let mut cursor = Cursor::new(Vec::<u8>::new());
        cursor.flush().unwrap();
    }

    #[test]
    fn cursor_position_tracking() {
        let data = b"abcdef";
        let mut cursor = Cursor::new(&data[..]);
        assert_eq!(cursor.position(), 0);
        let mut buf = [0u8; 3];
        cursor.read_exact(&mut buf).unwrap();
        assert_eq!(cursor.position(), 3);
        cursor.seek(SeekFrom::Start(1)).unwrap();
        assert_eq!(cursor.position(), 1);
    }

    #[test]
    fn cursor_into_inner() {
        let data = vec![1u8, 2, 3];
        let cursor = Cursor::new(data);
        let inner = cursor.into_inner();
        assert_eq!(inner, vec![1, 2, 3]);
    }

    #[test]
    fn cursor_set_position() {
        let data = b"abcdef";
        let mut cursor = Cursor::new(&data[..]);
        cursor.set_position(4);
        assert_eq!(cursor.position(), 4);
        let mut buf = [0u8; 2];
        cursor.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"ef");
    }

    #[cfg(not(feature = "std"))]
    #[test]
    fn seek_to_negative_position_fails() {
        let data = b"abc";
        let mut cursor = Cursor::new(&data[..]);
        let result = cursor.seek(SeekFrom::Current(-1));
        assert!(result.is_err());
    }

    #[cfg(not(feature = "std"))]
    #[test]
    fn io_error_display() {
        let e = IoError::UnexpectedEof;
        assert_eq!(format!("{e}"), "unexpected end of file");
        let e = IoError::WriteZero;
        assert_eq!(format!("{e}"), "write zero");
        let e = IoError::InvalidSeek;
        assert_eq!(format!("{e}"), "invalid seek to negative position");
    }
}
