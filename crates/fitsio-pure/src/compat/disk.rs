//! Reading a FITS file on disk at offsets, with few, large requests.

use std::collections::VecDeque;
use std::fs::File;
use std::sync::{Arc, Mutex, PoisonError};

/// A read-only file on disk, read at offsets as cfitsio reads one.
///
/// Reads are positional (`pread` on Unix, `seek_read` on Windows), so
/// threads reading different parts of one file don't wait for each other.
/// A read of at least [`DiskFile::BLOCK`] bytes goes straight to the file.
/// A smaller one, such as a table cell or a group's parameters, is served
/// from the block it falls in, read whole and kept with the last few read, as
/// cfitsio's buffers keep them, so reading a table a cell at a time costs a
/// read per block rather than one per cell.
pub(crate) struct DiskFile {
    file: PositionalFile,
    len: usize,
    /// Recently read blocks, by block number, most recent first.
    blocks: Mutex<VecDeque<(usize, Arc<[u8]>)>>,
}

impl DiskFile {
    /// The size of the blocks small reads are served from.
    pub(crate) const BLOCK: usize = 64 * 1024;
    /// How many blocks are kept: 1 MiB of them.
    const KEPT: usize = 16;

    pub(crate) fn new(file: File, len: usize) -> DiskFile {
        DiskFile {
            file: PositionalFile::new(file),
            len,
            blocks: Mutex::default(),
        }
    }

    /// Fill `buf` with the file's bytes from `offset`.
    pub(crate) fn read_at(&self, offset: usize, buf: &mut [u8]) -> crate::Result<()> {
        if offset
            .checked_add(buf.len())
            .is_none_or(|end| end > self.len)
        {
            return Err(crate::Error::UnexpectedEof);
        }
        if buf.len() >= Self::BLOCK {
            return self.read_exact_at(offset, buf);
        }
        let mut done = 0;
        while done < buf.len() {
            let at = offset + done;
            let block = self.block(at / Self::BLOCK)?;
            let from = &block[at % Self::BLOCK..];
            let n = from.len().min(buf.len() - done);
            buf[done..done + n].copy_from_slice(&from[..n]);
            done += n;
        }
        Ok(())
    }

    /// Fill `buf` with the file's bytes from `offset`, for a small read
    /// whose neighbours won't be read soon, such as one group's parameters
    /// out of groups longer than a block. It is served from a kept block if
    /// there is one, and otherwise read exactly, without reading or keeping
    /// the rest of its block.
    pub(crate) fn read_sparse_at(&self, offset: usize, buf: &mut [u8]) -> crate::Result<()> {
        let end = offset.checked_add(buf.len());
        let first_block = offset / Self::BLOCK;
        let all_kept = end.is_some_and(|end| {
            let blocks = self.lock();
            (first_block..end.div_ceil(Self::BLOCK))
                .all(|number| blocks.iter().any(|(n, _)| *n == number))
        });
        if !all_kept && end.is_some_and(|end| end <= self.len) {
            return self.read_exact_at(offset, buf);
        }
        self.read_at(offset, buf)
    }

    /// Block `number`, from those kept or read from the file.
    fn block(&self, number: usize) -> crate::Result<Arc<[u8]>> {
        if let Some(block) = self.kept(number) {
            return Ok(block);
        }
        // Read without holding the lock, so other threads' reads of kept
        // blocks don't wait for this one.
        let start = number * Self::BLOCK;
        let mut block = vec![0; Self::BLOCK.min(self.len - start)];
        self.read_exact_at(start, &mut block)?;
        let block: Arc<[u8]> = block.into();
        let mut blocks = self.lock();
        if !blocks.iter().any(|(n, _)| *n == number) {
            blocks.push_front((number, block.clone()));
            blocks.truncate(Self::KEPT);
        }
        Ok(block)
    }

    /// Block `number` if it is kept, made the most recent.
    fn kept(&self, number: usize) -> Option<Arc<[u8]>> {
        let mut blocks = self.lock();
        let i = blocks.iter().position(|(n, _)| *n == number)?;
        let block = blocks.remove(i)?;
        blocks.push_front(block.clone());
        Some(block.1)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<(usize, Arc<[u8]>)>> {
        // The blocks are only ever whole, so a panic elsewhere can't leave
        // them inconsistent.
        self.blocks.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn read_exact_at(&self, offset: usize, buf: &mut [u8]) -> crate::Result<()> {
        self.file
            .read_exact_at(offset as u64, buf)
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::UnexpectedEof => crate::Error::UnexpectedEof,
                _ => crate::Error::Io(e),
            })
    }
}

/// A file read at offsets without moving a shared position, where the
/// platform can; elsewhere, seeking and reading under a lock.
struct PositionalFile {
    #[cfg(any(unix, windows))]
    file: File,
    #[cfg(not(any(unix, windows)))]
    file: Mutex<File>,
}

impl PositionalFile {
    fn new(file: File) -> PositionalFile {
        PositionalFile {
            #[cfg(any(unix, windows))]
            file,
            #[cfg(not(any(unix, windows)))]
            file: Mutex::new(file),
        }
    }

    #[cfg(unix)]
    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> std::io::Result<()> {
        std::os::unix::fs::FileExt::read_exact_at(&self.file, buf, offset)
    }

    #[cfg(windows)]
    fn read_exact_at(&self, mut offset: u64, mut buf: &mut [u8]) -> std::io::Result<()> {
        use std::os::windows::fs::FileExt;
        while !buf.is_empty() {
            match self.file.seek_read(buf, offset) {
                Ok(0) => return Err(std::io::ErrorKind::UnexpectedEof.into()),
                Ok(n) => {
                    buf = &mut buf[n..];
                    offset += n as u64;
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    #[cfg(not(any(unix, windows)))]
    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> std::io::Result<()> {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = self.file.lock().unwrap_or_else(PoisonError::into_inner);
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_of(len: usize) -> (tempfile::NamedTempFile, Vec<u8>) {
        let bytes: Vec<u8> = (0..len).map(|i| (i * 7 % 251) as u8).collect();
        let mut file = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut file, &bytes).unwrap();
        (file, bytes)
    }

    #[test]
    fn reads_of_any_size_and_place_read_the_files_bytes() {
        let len = 5 * DiskFile::BLOCK + 123;
        let (tmp, bytes) = file_of(len);
        let disk = DiskFile::new(File::open(tmp.path()).unwrap(), len);
        let block = DiskFile::BLOCK;
        for (offset, n) in [
            (0, 1),
            (17, 300),
            (block - 5, 10),
            (block - 5, 2 * block + 10),
            (3 * block, block),
            (len - 1, 1),
            (len - 200, 200),
            (0, len),
            (len, 0),
        ] {
            let mut buf = vec![0; n];
            disk.read_at(offset, &mut buf).unwrap();
            assert_eq!(buf, bytes[offset..offset + n], "{offset} + {n}");
        }
        // Many small reads, through more blocks than are kept, and back.
        for offset in (0..len - 20)
            .step_by(997)
            .chain((0..len - 20).step_by(4999).rev())
        {
            let mut buf = [0; 20];
            disk.read_at(offset, &mut buf).unwrap();
            assert_eq!(buf, bytes[offset..offset + 20]);
        }
    }

    #[test]
    fn a_read_past_the_end_is_unexpected_eof() {
        let (tmp, _) = file_of(1000);
        let disk = DiskFile::new(File::open(tmp.path()).unwrap(), 1000);
        for (offset, n) in [(990, 11), (1001, 0), (0, 1001), (usize::MAX, 1)] {
            let mut buf = vec![0; n];
            assert!(matches!(
                disk.read_at(offset, &mut buf),
                Err(crate::Error::UnexpectedEof)
            ));
        }
    }

    #[test]
    fn sparse_reads_read_just_their_bytes_or_use_kept_blocks() {
        let len = 4 * DiskFile::BLOCK + 99;
        let (tmp, bytes) = file_of(len);
        let disk = DiskFile::new(File::open(tmp.path()).unwrap(), len);
        let kept =
            |disk: &DiskFile| -> Vec<usize> { disk.lock().iter().map(|(n, _)| *n).collect() };
        let mut params = [0; 20];
        for offset in (0..len - 20).step_by(36_884) {
            disk.read_sparse_at(offset, &mut params).unwrap();
            assert_eq!(params, bytes[offset..offset + 20]);
        }
        assert!(kept(&disk).is_empty());
        // Once a block is kept, sparse reads in it come from it.
        let mut cell = [0; 8];
        disk.read_at(DiskFile::BLOCK + 10, &mut cell).unwrap();
        disk.read_sparse_at(DiskFile::BLOCK + 500, &mut params)
            .unwrap();
        assert_eq!(params, bytes[DiskFile::BLOCK + 500..][..20]);
        // Across a kept and an unkept block, and past the end.
        let at = 2 * DiskFile::BLOCK - 10;
        disk.read_sparse_at(at, &mut params).unwrap();
        assert_eq!(params, bytes[at..at + 20]);
        assert_eq!(kept(&disk), [1]);
        assert!(matches!(
            disk.read_sparse_at(len - 10, &mut params),
            Err(crate::Error::UnexpectedEof)
        ));
    }

    #[test]
    fn small_reads_keep_their_blocks() {
        let len = 3 * DiskFile::BLOCK;
        let (tmp, _) = file_of(len);
        let disk = DiskFile::new(File::open(tmp.path()).unwrap(), len);
        let mut cell = [0; 8];
        for offset in (0..len - 8).step_by(100) {
            disk.read_at(offset, &mut cell).unwrap();
        }
        let kept: Vec<usize> = disk.lock().iter().map(|(n, _)| *n).collect();
        assert_eq!(kept, [2, 1, 0]);
    }
}
