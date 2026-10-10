//! A block store: one spill file of blocks (see the module doc of `io`).

use std::fs::{File, OpenOptions};
use std::os::unix::fs::FileExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use super::{disk_reserve, disk_space, Ctx, Dir, IoError};

/// The unit of checksums: every page of a block has one.
pub const PAGE: usize = 4096;

/// Where a block is: its offset and length in its store, and the checksums
/// of its pages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockRef {
    pub off: u64,
    pub len: usize,
    sums: Box<[u32]>,
}

/// A file of blocks.
pub struct BlockStore {
    file: File,
    path: PathBuf,
    /// The end of the file: blocks are appended at offsets reserved here.
    end: AtomicU64,
    /// The bytes of the blocks not deleted.
    live: AtomicU64,
    ctx: Arc<Ctx>,
    /// (Keeps the directory while the store exists.)
    _dir: Arc<Dir>,
}

impl BlockStore {
    /// A new file at `path`; with `nocache`, written and read past the page
    /// cache where the system allows (macOS).
    pub(super) fn create(path: PathBuf, dir: Arc<Dir>, ctx: Arc<Ctx>, nocache: bool) -> Result<BlockStore, IoError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| IoError(format!("cannot create the spill file {}: {e}", path.display())))?;
        #[cfg(target_os = "macos")]
        if nocache {
            use std::os::fd::AsRawFd;
            // (Best effort: only a hint to the page cache.)
            // SAFETY: a valid descriptor.
            unsafe { libc::fcntl(file.as_raw_fd(), libc::F_NOCACHE, 1) };
        }
        #[cfg(not(target_os = "macos"))]
        let _ = nocache;
        Ok(BlockStore { file, path, end: AtomicU64::new(0), live: AtomicU64::new(0), ctx, _dir: dir })
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// The bytes of the blocks not deleted.
    pub fn bytes(&self) -> u64 {
        self.live.load(Ordering::Relaxed)
    }

    /// Check that `len` more bytes may be written (`-maxdisk`, the free
    /// space, the testing hook) and count them.
    fn reserve(&self, len: u64) -> Result<(), IoError> {
        let ctx = &*self.ctx;
        let cfg = &ctx.cfg;
        let written = ctx.written.fetch_add(len, Ordering::Relaxed) + len;
        if cfg.fail_after.is_some_and(|n| written > n) {
            return Err(IoError(format!(
                "cannot write the spill file {}: injected write error (SpillConfig::fail_after)",
                self.path.display()
            )));
        }
        let used = ctx.used.fetch_add(len, Ordering::Relaxed) + len;
        if let Some(max) = cfg.maxdisk.filter(|&m| used > m) {
            ctx.used.fetch_sub(len, Ordering::Relaxed);
            return Err(IoError(format!(
                "the spill files would take more than -maxdisk {} (in {}); give tlx more disk (-maxdisk) or memory (-maxmem)",
                size(max),
                self.dir_display()
            )));
        }
        if let Some((avail, total)) = disk_space(&self.path) {
            let keep = disk_reserve(total);
            if avail < len + keep {
                ctx.used.fetch_sub(len, Ordering::Relaxed);
                return Err(IoError(format!(
                    "no space left for the spill files in {}: {} free, {} to write, tlx keeps {} free; free some disk space, give another directory (-metadir) or more memory (-maxmem)",
                    self.dir_display(),
                    size(avail),
                    size(len),
                    size(keep)
                )));
            }
        }
        ctx.peak.fetch_max(used, Ordering::Relaxed);
        Ok(())
    }

    fn dir_display(&self) -> String {
        self.path.parent().map_or_else(String::new, |p| p.display().to_string())
    }

    /// Append a block; where it is. Several threads may append at once.
    pub fn append(&self, data: &[u8]) -> Result<BlockRef, IoError> {
        let r = self.reserve(data.len() as u64).and_then(|()| {
            let off = self.end.fetch_add(data.len() as u64, Ordering::Relaxed);
            self.file.write_all_at(data, off).map_err(|e| {
                self.ctx.used.fetch_sub(data.len() as u64, Ordering::Relaxed);
                IoError(format!("cannot write the spill file {}: {e}", self.path.display()))
            })?;
            Ok(off)
        });
        match r {
            Ok(off) => {
                self.live.fetch_add(data.len() as u64, Ordering::Relaxed);
                self.ctx.blocks.fetch_add(1, Ordering::Relaxed);
                Ok(BlockRef { off, len: data.len(), sums: data.chunks(PAGE).map(checksum).collect() })
            }
            Err(e) => Err(self.ctx.fail(e)),
        }
    }

    /// Read block `r` whole (checksums verified).
    pub fn read(&self, r: &BlockRef) -> Result<Vec<u8>, IoError> {
        let mut out = vec![0u8; r.len];
        self.read_into(r, &mut out)?;
        Ok(out)
    }

    /// Read block `r` into `out` (of its length; checksums verified).
    pub fn read_into(&self, r: &BlockRef, out: &mut [u8]) -> Result<(), IoError> {
        assert_eq!(out.len(), r.len, "read_into: wrong length");
        self.read_pages(r, 0, out)?;
        for (i, page) in out.chunks(PAGE).enumerate() {
            self.verify(r, i, page)?;
        }
        Ok(())
    }

    /// Read `out.len()` bytes at offset `at` of block `r` (reading and
    /// verifying the pages they are in).
    pub fn read_range(&self, r: &BlockRef, at: usize, out: &mut [u8]) -> Result<(), IoError> {
        if out.is_empty() {
            return Ok(());
        }
        assert!(at + out.len() <= r.len, "read_range: out of the block");
        let first = at / PAGE;
        let last = (at + out.len() - 1) / PAGE;
        let start = first * PAGE;
        let end = ((last + 1) * PAGE).min(r.len);
        if start == at && end == at + out.len() {
            // (Whole pages: read in place.)
            self.read_pages(r, start, out)?;
            for (k, page) in out.chunks(PAGE).enumerate() {
                self.verify(r, first + k, page)?;
            }
            return Ok(());
        }
        let mut buf = vec![0u8; end - start];
        self.read_pages(r, start, &mut buf)?;
        for (k, page) in buf.chunks(PAGE).enumerate() {
            self.verify(r, first + k, page)?;
        }
        out.copy_from_slice(&buf[at - start..at - start + out.len()]);
        Ok(())
    }

    fn read_pages(&self, r: &BlockRef, start: usize, buf: &mut [u8]) -> Result<(), IoError> {
        self.file
            .read_exact_at(buf, r.off + start as u64)
            .map_err(|e| self.ctx.fail(IoError(format!("cannot read the spill file {}: {e}", self.path.display()))))?;
        self.ctx.read.fetch_add(buf.len() as u64, Ordering::Relaxed);
        Ok(())
    }

    /// A read-only mapping of the bytes `lo..hi` of the file (all written:
    /// a range of blocks), if the system gives one. Reading it may stop the
    /// process (SIGBUS) where reading the file would fail.
    pub fn map(&self, lo: u64, hi: u64) -> Option<Mapping> {
        use std::os::fd::AsRawFd;
        if hi <= lo {
            return None;
        }
        // SAFETY: no preconditions.
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(1) as u64;
        let base = lo / page * page;
        let len = usize::try_from(hi - base).ok()?;
        // SAFETY: a valid descriptor; a new mapping (no address given) of
        // bytes the file has.
        let ptr =
            unsafe { libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ, libc::MAP_SHARED, self.file.as_raw_fd(), base as libc::off_t) };
        if ptr == libc::MAP_FAILED {
            return None;
        }
        // Lookups read single pages at random: a page fault that is not
        // in the page cache should read that page, not the system's
        // read-around window (on Linux 128 KiB: 32 times the bytes read,
        // and the pages read around evict others; best effort).
        // SAFETY: advice on the mapping just made.
        unsafe { libc::madvise(ptr, len, libc::MADV_RANDOM) };
        Some(Mapping { ptr: ptr.cast(), len, base })
    }

    /// Verify page `page` of block `r` (read as `data`) against its
    /// checksum.
    pub fn verify_page(&self, r: &BlockRef, page: usize, data: &[u8]) -> Result<(), IoError> {
        self.verify(r, page, data)
    }

    fn verify(&self, r: &BlockRef, page: usize, data: &[u8]) -> Result<(), IoError> {
        if checksum(data) == r.sums[page] {
            return Ok(());
        }
        Err(self.ctx.fail(IoError(format!(
            "the spill file {} is corrupt: checksum mismatch at offset {}",
            self.path.display(),
            r.off + (page * PAGE) as u64
        ))))
    }

    /// Delete block `r`: its space is given back (the whole pages it
    /// covers, where the file system supports it).
    pub fn delete(&self, r: BlockRef) {
        self.live.fetch_sub(r.len as u64, Ordering::Relaxed);
        self.ctx.used.fetch_sub(r.len as u64, Ordering::Relaxed);
        let start = r.off.next_multiple_of(PAGE as u64);
        let end = (r.off + r.len as u64) / PAGE as u64 * PAGE as u64;
        if end > start {
            punch_hole(&self.file, start, end - start);
        }
    }
}

impl Drop for BlockStore {
    fn drop(&mut self) {
        self.ctx.used.fetch_sub(self.live.load(Ordering::Relaxed), Ordering::Relaxed);
        let _ = std::fs::remove_file(&self.path);
    }
}

/// A read-only mapping of a part of a block store (`BlockStore::map`).
pub struct Mapping {
    ptr: *const u8,
    len: usize,
    /// The file offset of its first byte.
    base: u64,
}

// SAFETY: read-only memory, unmapped only when dropped.
unsafe impl Send for Mapping {}
// SAFETY: as above.
unsafe impl Sync for Mapping {}

impl Mapping {
    /// The `len` bytes at offset `off` of the file (within the mapping).
    #[inline]
    pub fn bytes(&self, off: u64, len: usize) -> &[u8] {
        let at = (off - self.base) as usize;
        assert!(off >= self.base && at + len <= self.len, "Mapping::bytes: out of the mapping");
        // SAFETY: within the mapping, which lives as long as `self`.
        unsafe { std::slice::from_raw_parts(self.ptr.add(at), len) }
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: the mapping made by `BlockStore::map`; no borrow of it
        // outlives `self`.
        unsafe { libc::munmap(self.ptr as *mut libc::c_void, self.len) };
    }
}

/// `b` bytes for a message: in MiB, or KiB below 1 MiB.
fn size(b: u64) -> String {
    if b >= 1 << 20 {
        format!("{} MiB", b >> 20)
    } else {
        format!("{} KiB", b >> 10)
    }
}

/// Give back the space of `len` bytes at `off` (page aligned), keeping the
/// file's size; best effort.
fn punch_hole(file: &File, off: u64, len: u64) {
    use std::os::fd::AsRawFd;
    #[cfg(target_os = "macos")]
    {
        let arg = libc::fpunchhole_t { fp_flags: 0, reserved: 0, fp_offset: off as libc::off_t, fp_length: len as libc::off_t };
        // SAFETY: a valid descriptor and argument.
        unsafe { libc::fcntl(file.as_raw_fd(), libc::F_PUNCHHOLE, &arg) };
    }
    #[cfg(target_os = "linux")]
    {
        // SAFETY: a valid descriptor.
        unsafe {
            libc::fallocate(
                file.as_raw_fd(),
                libc::FALLOC_FL_PUNCH_HOLE | libc::FALLOC_FL_KEEP_SIZE,
                off as libc::off_t,
                len as libc::off_t,
            )
        };
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (file, off, len);
    }
}

/// The checksum of a page: a multiplicative hash of its words, folded to
/// 32 bits (detects torn, misplaced and corrupted pages; not
/// cryptographic).
pub(crate) fn checksum(data: &[u8]) -> u32 {
    const K: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut h = 0x243F_6A88_85A3_08D3u64 ^ data.len() as u64;
    let mut words = data.chunks_exact(8);
    for w in &mut words {
        h = (h ^ u64::from_le_bytes(w.try_into().unwrap())).wrapping_mul(K).rotate_left(29);
    }
    let rest = words.remainder();
    if !rest.is_empty() {
        let mut w = [0u8; 8];
        w[..rest.len()].copy_from_slice(rest);
        h = (h ^ u64::from_le_bytes(w)).wrapping_mul(K).rotate_left(29);
    }
    h ^= h >> 32;
    h.wrapping_mul(K) as u32 ^ (h >> 17) as u32
}
