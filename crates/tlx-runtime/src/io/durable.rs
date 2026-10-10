//! Durable files: syncing data to the medium, and the commit protocol of a
//! directory that is replaced as a whole (checkpoints, `DirCommit`).
//!
//! The sync policy (docs/scale.md §9.2): every file is handed to the
//! device with a plain `fsync` (`sync_file`), and a whole set of files is
//! made durable by one drive-cache flush of their directory
//! (`sync_dir_full`) just before it is published. Spill files are scratch
//! data and are never synced.
//!
//! Why `sync_file` is not `File::sync_all`: on Apple platforms std's
//! `sync_all` is `fcntl(F_FULLFSYNC)`, a drive-cache flush. Flushing per file
//! costs a drive round trip each, and the concurrent flushes of one drive
//! serialize: on macOS that dominated the checkpoint before the sync policy below. The
//! per-file sync must stay a plain `fsync(2)`; only the one flush per commit
//! (`flush_device`) uses `sync_all`.

use std::fs::File;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;

use super::IoError;

/// Hand `f`'s data to the device: plain `fsync(2)` (on Linux this is a full
/// barrier; on macOS it stops at the drive's write cache, and `flush_device`
/// pushes the whole cache to the medium).
///
/// A checkpoint has many files (the runs and parts of the fingerprint set,
/// the lists of the queue, one stream per worker and kind). Flushing the
/// drive's cache per file — `F_FULLFSYNC`, what this used to do, and what
/// std's `File::sync_all` does on macOS — costs a drive round trip each and
/// the concurrent flushes of one drive serialize; on macOS it dominated the
/// checkpoint (docs/scale.md §9.2). Plain `fsync` is enough here because the
/// cache is per device, not per file: one flush after all of them makes all
/// of them durable.
pub fn sync_file(f: &File) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        // SAFETY: a valid descriptor.
        if unsafe { libc::fsync(f.as_raw_fd()) } == 0 {
            return Ok(());
        }
        Err(std::io::Error::last_os_error())
    }
    #[cfg(not(unix))]
    {
        f.sync_all()
    }
}

/// Flush the device's write cache to the medium (`F_FULLFSYNC` on macOS,
/// whose `fsync` stops at the drive: std's `File::sync_all` is exactly that
/// call there).
///
/// fcntl(2): `F_FULLFSYNC` "does the same thing as fsync(2) then asks the
/// drive to flush all buffered data to the permanent storage device ... data
/// that had been fsync'd on the same device before is guaranteed to be
/// persisted when this call returns". One call therefore covers every file
/// already `fsync`ed on the device, whatever its descriptor: the guarantee is
/// per device, not per file. The same man page notes the operation "may take
/// quite a while", which is why there is exactly one per commit. On Linux
/// `fsync` is already this barrier, so there is nothing to do.
pub fn flush_device(f: &File) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        f.sync_all()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = f;
        Ok(())
    }
}

/// Sync directory `d` and flush the device once: its entries (the new files,
/// the renames) and the data of every file in it are durable when this
/// returns (best effort: errors are ignored).
pub fn sync_dir_full(d: &Path) {
    if let Ok(f) = File::open(d) {
        let _ = sync_file(&f);
        let _ = flush_device(&f);
    }
}

/// Held while a `DirCommit` renames its directories, and by the signal path
/// (`super::cleanup_dir_on_signal`, for good: the process then exits), so
/// that a signal never sees a commit half done.
pub(crate) static COMMIT: Mutex<()> = Mutex::new(());

/// A directory `<name>` in `parent` that is replaced as a whole: written as
/// `<name>.tmp`, then committed (the previous one renamed `<name>.old`
/// meanwhile, then removed). A process killed at any moment leaves the
/// previous directory or the new one, complete (`roll_forward` repairs a
/// kill between the two renames).
///
/// The commit is durable before and after the renames (`sync_dir_full`): a
/// crash never leaves a published directory whose data did not reach the
/// medium. Its files must have been `sync_file`d by the writer.
#[derive(Clone, Debug)]
pub struct DirCommit {
    cur: PathBuf,
    tmp: PathBuf,
    old: PathBuf,
}

impl DirCommit {
    pub fn new(parent: &Path, name: &str) -> DirCommit {
        DirCommit { cur: parent.join(name), tmp: parent.join(format!("{name}.tmp")), old: parent.join(format!("{name}.old")) }
    }

    /// The committed directory.
    pub fn cur(&self) -> &Path {
        &self.cur
    }

    /// The directory being written.
    pub fn tmp(&self) -> &Path {
        &self.tmp
    }

    /// The previous directory, while the next one is committed.
    pub fn old(&self) -> &Path {
        &self.old
    }

    /// Start writing a new directory: `tmp`, empty (what an abandoned one
    /// left is removed).
    pub fn begin(&self) -> Result<&Path, IoError> {
        let _ = std::fs::remove_dir_all(&self.tmp);
        std::fs::create_dir_all(&self.tmp).map_err(|e| IoError(format!("cannot create {}: {e}", self.tmp.display())))?;
        Ok(&self.tmp)
    }

    /// `tmp` is complete: make it durable, then the committed directory
    /// (the previous one goes; not while a signal cleans up). On an error
    /// the caller removes `tmp` (`abort`); the previous directory is kept
    /// (as `cur`, or as `old` if the second rename failed).
    pub fn commit(&self) -> Result<(), IoError> {
        sync_dir_full(&self.tmp);
        let _commit = COMMIT.lock();
        let _ = std::fs::remove_dir_all(&self.old);
        if self.cur.exists() {
            std::fs::rename(&self.cur, &self.old).map_err(|e| IoError(format!("cannot rename {}: {e}", self.cur.display())))?;
        }
        std::fs::rename(&self.tmp, &self.cur).map_err(|e| IoError(format!("cannot rename {}: {e}", self.tmp.display())))?;
        if let Some(parent) = self.cur.parent() {
            sync_dir_full(parent);
        }
        let _ = std::fs::remove_dir_all(&self.old);
        Ok(())
    }

    /// Abandon the directory being written.
    pub fn abort(&self) {
        let _ = std::fs::remove_dir_all(&self.tmp);
    }

    /// After a process was killed between the renames of a commit: the
    /// previous directory back in place. Whether a committed directory
    /// exists.
    pub fn roll_forward(&self) -> bool {
        if !self.cur.is_dir() && self.old.is_dir() {
            let _ = std::fs::rename(&self.old, &self.cur);
        }
        self.cur.is_dir()
    }

    /// Remove what is not committed (`tmp` and `old`).
    pub fn remove_uncommitted(&self) {
        let _ = std::fs::remove_dir_all(&self.tmp);
        let _ = std::fs::remove_dir_all(&self.old);
    }

    /// Remove all of it.
    pub fn remove_all(&self) {
        for d in [&self.cur, &self.tmp, &self.old] {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn write(dir: &Path, name: &str, text: &str) {
        let f = File::create(dir.join(name)).unwrap();
        std::io::Write::write_all(&mut &f, text.as_bytes()).unwrap();
        sync_file(&f).unwrap();
    }

    fn read(dir: &Path, name: &str) -> Option<String> {
        std::fs::read_to_string(dir.join(name)).ok()
    }

    #[test]
    fn names_follow_the_checkpoint_layout() {
        let c = DirCommit::new(Path::new("/m"), "checkpoint");
        assert_eq!(
            (c.cur(), c.tmp(), c.old()),
            (Path::new("/m/checkpoint"), Path::new("/m/checkpoint.tmp"), Path::new("/m/checkpoint.old"))
        );
    }

    #[test]
    fn commit_replaces_the_previous_directory() {
        let p = tlx_testkit::TempDir::new("commit");
        let c = DirCommit::new(&p, "ck");
        // A first commit, without a previous directory.
        let tmp = c.begin().unwrap().to_path_buf();
        write(&tmp, "a", "1");
        c.commit().unwrap();
        assert_eq!(read(c.cur(), "a").as_deref(), Some("1"));
        assert!(!c.tmp().exists() && !c.old().exists());
        // `begin` starts from an empty directory.
        std::fs::create_dir_all(c.tmp()).unwrap();
        write(c.tmp(), "stale", "x");
        c.begin().unwrap();
        assert_eq!(std::fs::read_dir(c.tmp()).unwrap().count(), 0);
        write(c.tmp(), "b", "2");
        c.commit().unwrap();
        assert_eq!(read(c.cur(), "a"), None);
        assert_eq!(read(c.cur(), "b").as_deref(), Some("2"));
        assert!(!c.tmp().exists() && !c.old().exists());
        // An abandoned one leaves the committed one alone.
        write(c.begin().unwrap(), "c", "3");
        c.abort();
        assert!(!c.tmp().exists());
        assert_eq!(read(c.cur(), "b").as_deref(), Some("2"));
        c.remove_all();
        assert!(!c.cur().exists());
    }

    #[test]
    fn roll_forward_after_a_kill_between_the_renames() {
        let p = tlx_testkit::TempDir::new("roll");
        let c = DirCommit::new(&p, "ck");
        assert!(!c.roll_forward());
        // Killed after `cur` -> `old`, before `tmp` -> `cur`.
        std::fs::create_dir_all(c.old()).unwrap();
        write(c.old(), "a", "1");
        std::fs::create_dir_all(c.tmp()).unwrap();
        assert!(c.roll_forward());
        assert_eq!(read(c.cur(), "a").as_deref(), Some("1"));
        c.remove_uncommitted();
        assert!(c.cur().is_dir() && !c.tmp().exists() && !c.old().exists());
    }

    #[test]
    fn syncs_succeed_on_files_and_directories() {
        let p = tlx_testkit::TempDir::new("sync");
        let f = File::create(p.join("f")).unwrap();
        sync_file(&f).unwrap();
        flush_device(&f).unwrap();
        sync_dir_full(&p);
    }
}
