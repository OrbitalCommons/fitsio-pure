//! Atomic, durable file replacement.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// How many temporary names to try before giving up.
const MAX_ATTEMPTS: u32 = 64;

/// Distinguishes temporary names created by one process.
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A file that replaces its target only when [`commit`](Self::commit) is called.
///
/// Bytes are written to a temporary file next to the target, named
/// `.<target name>.<unique>.tmp`. `commit` flushes it, `sync_all`s it, renames
/// it over the target, and on Unix fsyncs the directory, so after a crash the
/// target holds either its old content or the complete new content, never a
/// partial write. Dropping an `AtomicFile` without committing removes the
/// temporary file and leaves the target untouched.
///
/// On Unix, replacing an existing file keeps its permission bits. A new file
/// gets the same permissions [`std::fs::write`] would give it. If the target
/// is a symbolic link, the file it points to is replaced and the link is kept.
/// Ownership and hard links to the old file are not carried over.
///
/// ```no_run
/// use std::io::Write;
/// use fitsio_pure::io::AtomicFile;
///
/// let mut file = AtomicFile::new("out.fits")?;
/// file.write_all(b"...")?;
/// file.commit()?;
/// # Ok::<(), std::io::Error>(())
/// ```
#[derive(Debug)]
pub struct AtomicFile {
    file: Option<BufWriter<File>>,
    temp_path: PathBuf,
    target: PathBuf,
}

impl AtomicFile {
    /// Create a temporary file that will replace `path` on commit.
    ///
    /// Fails if `path` has no file name or its directory is not writable.
    pub fn new<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        let target = resolve_target(path.as_ref());
        let name = target
            .file_name()
            .ok_or_else(|| std::io::Error::new(ErrorKind::InvalidInput, "path has no file name"))?;
        let dir = match target.parent() {
            Some(dir) if !dir.as_os_str().is_empty() => dir,
            _ => Path::new("."),
        };

        let mut attempt = 0;
        let (file, temp_path) = loop {
            let mut temp_name = std::ffi::OsString::from(".");
            temp_name.push(name);
            temp_name.push(format!(".{}.tmp", unique_suffix()));
            let temp_path = dir.join(temp_name);
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp_path)
            {
                Ok(file) => break (file, temp_path),
                Err(e) if e.kind() == ErrorKind::AlreadyExists && attempt < MAX_ATTEMPTS => {
                    attempt += 1;
                }
                Err(e) => return Err(e),
            }
        };

        let atomic = AtomicFile {
            file: Some(BufWriter::new(file)),
            temp_path,
            target,
        };
        #[cfg(unix)]
        if let Ok(metadata) = std::fs::metadata(&atomic.target) {
            atomic.file_ref().set_permissions(metadata.permissions())?;
        }
        Ok(atomic)
    }

    /// The path that [`commit`](Self::commit) replaces.
    pub fn target(&self) -> &Path {
        &self.target
    }

    /// The temporary file being written.
    pub fn temp_path(&self) -> &Path {
        &self.temp_path
    }

    /// Flush and `sync_all` the temporary file, rename it over the target,
    /// and on Unix fsync the target's directory.
    ///
    /// On error before the rename the target is untouched and the temporary
    /// file is removed.
    pub fn commit(mut self) -> std::io::Result<()> {
        let writer = self.file.take().expect("file is present until commit");
        let file = writer.into_inner().map_err(|e| e.into_error())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&self.temp_path, &self.target)?;
        // The temporary name is gone; nothing for Drop to clean up.
        self.temp_path = PathBuf::new();
        #[cfg(unix)]
        {
            let dir = match self.target.parent() {
                Some(dir) if !dir.as_os_str().is_empty() => dir,
                _ => Path::new("."),
            };
            File::open(dir)?.sync_all()?;
        }
        Ok(())
    }

    #[cfg(unix)]
    fn file_ref(&self) -> &File {
        self.file
            .as_ref()
            .expect("file is present until commit")
            .get_ref()
    }

    fn writer(&mut self) -> &mut BufWriter<File> {
        self.file.as_mut().expect("file is present until commit")
    }
}

impl Write for AtomicFile {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.writer().write(buf)
    }

    fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        self.writer().write_all(buf)
    }

    /// Flushes buffered bytes to the temporary file. The target is not
    /// touched until [`commit`](AtomicFile::commit).
    fn flush(&mut self) -> std::io::Result<()> {
        self.writer().flush()
    }
}

impl Drop for AtomicFile {
    fn drop(&mut self) {
        // Close the handle before removing, which Windows requires.
        drop(self.file.take());
        if !self.temp_path.as_os_str().is_empty() {
            let _ = std::fs::remove_file(&self.temp_path);
        }
    }
}

/// Atomically replace the file at `path` with `bytes`.
///
/// Equivalent to [`AtomicFile::new`], `write_all`, then
/// [`AtomicFile::commit`]: on any error the existing file is left as it was.
pub fn write_atomic<P: AsRef<Path>>(path: P, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = AtomicFile::new(path)?;
    file.write_all(bytes)?;
    file.commit()
}

/// Follow a symbolic link so the file it points to is replaced, not the link.
fn resolve_target(path: &Path) -> PathBuf {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
        }
        _ => path.to_path_buf(),
    }
}

fn unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}.{count}.{nanos:08x}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image_writer::ImageWriter;

    /// A sink that fails once `left` bytes have been accepted.
    struct FailAfter<W> {
        inner: W,
        left: usize,
    }

    impl<W: Write> Write for FailAfter<W> {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if self.left == 0 {
                return Err(std::io::Error::other("injected failure"));
            }
            let n = buf.len().min(self.left);
            self.left -= n;
            self.inner.write(&buf[..n])
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.inner.flush()
        }
    }

    fn dir_entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn commit_replaces_the_target() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.fits");
        std::fs::write(&path, b"old").unwrap();

        let mut file = AtomicFile::new(&path).unwrap();
        let temp = file.temp_path().to_path_buf();
        assert_eq!(temp.parent(), Some(dir.path()));
        assert!(temp
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with(".out.fits."));
        file.write_all(b"new content").unwrap();
        file.flush().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"old");
        file.commit().unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"new content");
        assert_eq!(dir_entries(dir.path()), ["out.fits"]);
    }

    #[test]
    fn commit_creates_a_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new.fits");
        write_atomic(&path, b"abc").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"abc");
        assert_eq!(dir_entries(dir.path()), ["new.fits"]);
    }

    #[test]
    fn drop_without_commit_removes_the_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.fits");
        std::fs::write(&path, b"original").unwrap();

        let mut file = AtomicFile::new(&path).unwrap();
        file.write_all(&[7u8; 100_000]).unwrap();
        assert!(file.temp_path().exists());
        drop(file);

        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        assert_eq!(dir_entries(dir.path()), ["out.fits"]);
    }

    #[test]
    fn failure_mid_stream_leaves_the_target_intact() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("image.fits");
        let original = crate::image::build_image_hdu(
            16,
            &[2, 2],
            &crate::image::ImageData::I16(vec![1, 2, 3, 4]),
        )
        .unwrap();
        std::fs::write(&path, &original).unwrap();

        // Fail partway through the data unit of a 1000 x 700 f32 image.
        let sink = FailAfter {
            inner: AtomicFile::new(&path).unwrap(),
            left: 2880 + 1_000_000,
        };
        let mut writer = ImageWriter::with_chunk_size(
            sink,
            &crate::primary::build_primary_header(-32, &[1000, 700]).unwrap(),
            64 * 1024,
        )
        .unwrap();
        let row = vec![1.5f32; 1000];
        let err = (0..700).try_for_each(|_| writer.write_samples(&row));
        assert!(matches!(err, Err(crate::Error::Io(_))));
        drop(writer);

        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(dir_entries(dir.path()), ["image.fits"]);
    }

    #[test]
    fn streamed_image_commits_through_atomic_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("image.fits");
        std::fs::write(&path, b"old").unwrap();

        let pixels: Vec<f32> = (0..35).map(|i| i as f32 * 0.5).collect();
        let mut writer =
            ImageWriter::primary(AtomicFile::new(&path).unwrap(), -32, &[7, 5], &[]).unwrap();
        writer.write_samples(&pixels[..10]).unwrap();
        writer.write_samples(&pixels[10..]).unwrap();
        writer.finish().unwrap().commit().unwrap();

        let expected =
            crate::image::build_image_hdu(-32, &[7, 5], &crate::image::ImageData::F32(pixels))
                .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), expected);
        assert_eq!(dir_entries(dir.path()), ["image.fits"]);
    }

    #[test]
    fn concurrent_files_get_distinct_temp_names() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.fits");
        let a = AtomicFile::new(&path).unwrap();
        let b = AtomicFile::new(&path).unwrap();
        assert_ne!(a.temp_path(), b.temp_path());
        drop((a, b));
        assert!(dir_entries(dir.path()).is_empty());
    }

    #[test]
    fn path_without_file_name_is_rejected() {
        let err = AtomicFile::new("/").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
    }

    #[test]
    fn missing_directory_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(write_atomic(dir.path().join("nope/out.fits"), b"x").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn replacing_keeps_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.fits");
        std::fs::write(&path, b"old").unwrap();
        for mode in [0o600, 0o640, 0o755] {
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            write_atomic(&path, b"new").unwrap();
            let got = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(got, mode);
        }
    }

    #[cfg(unix)]
    #[test]
    fn new_file_gets_default_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let plain = dir.path().join("plain.fits");
        let atomic = dir.path().join("atomic.fits");
        std::fs::write(&plain, b"x").unwrap();
        write_atomic(&atomic, b"x").unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&atomic), mode(&plain));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_target_is_replaced_and_link_kept() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.fits");
        let link = dir.path().join("link.fits");
        std::fs::write(&real, b"old").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();

        write_atomic(&link, b"new").unwrap();

        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read(&real).unwrap(), b"new");
        assert_eq!(dir_entries(dir.path()), ["link.fits", "real.fits"]);
    }
}
