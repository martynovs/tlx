//! An append-only sequence in fixed-size blocks that can be spilled to disk
//! (trace logs, liveness edge logs).

use std::sync::Arc;

use super::{as_bytes_mut, BlockRef, BlockStore, IoError, Pending, Plain, Prefetch, Spill};

/// A block of a `SpillVec`.
enum Slot<T> {
    Mem(Vec<T>),
    /// Handed to the writer thread.
    Pending(Arc<Pending>),
    Disk(BlockRef),
}

/// An append-only sequence of plain values in blocks of `2^shift` values
/// (the first block grows to that size: small sequences stay small).
/// Appending never moves values. Blocks can be written to a store
/// (`spill`) and are read back transparently (`get`, `iter`, `drain`);
/// a spilled block that has to change is read back into memory first.
pub struct SpillVec<T> {
    blocks: Vec<Slot<T>>,
    len: usize,
    shift: u32,
    /// The blocks before this one are not in memory (`spill` starts here).
    first_mem: usize,
    /// The blocks being written.
    pending: Vec<usize>,
    /// The last block, partial, was spilled (`spill` with `all`): `unseal`
    /// before appending.
    sealed: bool,
    /// The store of the spilled blocks (created with the first one), and
    /// the name of its file.
    store: Option<Arc<BlockStore>>,
    name: String,
}

impl<T: Plain> SpillVec<T> {
    /// A sequence in blocks of about `block_bytes` bytes (a power of two
    /// of values, at least 16), spilled to a file named after `name`.
    pub fn new(block_bytes: usize, name: impl Into<String>) -> SpillVec<T> {
        let n = (block_bytes / std::mem::size_of::<T>()).max(16);
        let shift = usize::BITS - 1 - n.leading_zeros();
        SpillVec { blocks: Vec::new(), len: 0, shift, first_mem: 0, pending: Vec::new(), sealed: false, store: None, name: name.into() }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Values per block.
    #[inline]
    pub fn block_len(&self) -> usize {
        1 << self.shift
    }

    /// The number of values of block `k`.
    fn len_of(&self, k: usize) -> usize {
        (self.len - (k << self.shift)).min(self.block_len())
    }

    /// Append `x`; its index. (Not after the partial last block was
    /// spilled: see `unseal`.)
    #[inline]
    pub fn push(&mut self, x: T) -> usize {
        let cap = self.block_len();
        match self.blocks.last_mut() {
            Some(Slot::Mem(b)) if b.len() < cap => b.push(x),
            _ => self.push_block(x),
        }
        self.len += 1;
        self.len - 1
    }

    #[cold]
    #[inline(never)]
    fn push_block(&mut self, x: T) {
        assert!(!self.sealed, "SpillVec::push after its partial last block was spilled (unseal first)");
        let mut b = if self.blocks.is_empty() { Vec::new() } else { Vec::with_capacity(self.block_len()) };
        b.push(x);
        self.blocks.push(Slot::Mem(b));
    }

    /// Before appending to a sequence whose partial last block was
    /// spilled: read that block back.
    pub fn unseal(&mut self) -> Result<(), IoError> {
        if self.sealed {
            self.load(self.blocks.len() - 1)?;
            self.sealed = false;
        }
        Ok(())
    }

    /// Value `i`.
    #[inline]
    pub fn get(&self, i: usize) -> Result<T, IoError> {
        assert!(i < self.len, "SpillVec::get out of bounds");
        let (k, j) = (i >> self.shift, i & (self.block_len() - 1));
        match &self.blocks[k] {
            Slot::Mem(b) => Ok(b[j]),
            Slot::Pending(p) => {
                let size = std::mem::size_of::<T>();
                let mut x = [T::default()];
                if p.with_data(|d| as_bytes_mut(&mut x).copy_from_slice(&d[j * size..(j + 1) * size])).is_some() {
                    return Ok(x[0]);
                }
                let r = p.wait()?;
                self.read_one(&r, j)
            }
            Slot::Disk(r) => self.read_one(r, j),
        }
    }

    fn read_one(&self, r: &BlockRef, j: usize) -> Result<T, IoError> {
        let mut x = [T::default()];
        let size = std::mem::size_of::<T>();
        self.store.as_ref().expect("spilled block without a store").read_range(r, j * size, as_bytes_mut(&mut x))?;
        Ok(x[0])
    }

    /// Set value `i` (its block is read back into memory if spilled).
    pub fn set(&mut self, i: usize, x: T) -> Result<(), IoError> {
        assert!(i < self.len, "SpillVec::set out of bounds");
        let (k, j) = (i >> self.shift, i & (self.block_len() - 1));
        let b = self.load(k)?;
        b[j] = x;
        Ok(())
    }

    /// Block `k` in memory (read back if spilled; its disk copy deleted).
    fn load(&mut self, k: usize) -> Result<&mut Vec<T>, IoError> {
        let n = self.len_of(k);
        let slot = std::mem::replace(&mut self.blocks[k], Slot::Mem(Vec::new()));
        let v = match slot {
            Slot::Mem(b) => b,
            Slot::Pending(p) => {
                self.pending.retain(|&i| i != k);
                match p.settle::<Vec<T>>() {
                    Ok(r) => self.read_block(r, n, true)?,
                    Err((v, _)) => v,
                }
            }
            Slot::Disk(r) => self.read_block(r, n, true)?,
        };
        let mut v = v;
        v.reserve_exact(self.block_len() - v.len());
        self.blocks[k] = Slot::Mem(v);
        self.first_mem = self.first_mem.min(k);
        match &mut self.blocks[k] {
            Slot::Mem(b) => Ok(b),
            _ => unreachable!(),
        }
    }

    /// The `n` values of spilled block `r` (deleted from the store if
    /// `delete`).
    fn read_block(&self, r: BlockRef, n: usize, delete: bool) -> Result<Vec<T>, IoError> {
        let store = self.store.as_ref().expect("spilled block without a store");
        let mut v = vec![T::default(); n];
        store.read_into(&r, as_bytes_mut(&mut v))?;
        if delete {
            store.delete(r);
        }
        Ok(v)
    }

    /// Hand the blocks in memory to the writer: the full ones, and with
    /// `all` the last one too even if partial. Then `poll`.
    pub fn spill(&mut self, spill: &Spill, all: bool) -> Result<(), IoError> {
        let nblocks = self.blocks.len();
        let mut first_mem = nblocks;
        for k in self.first_mem..nblocks {
            let n = self.len_of(k);
            let last = k + 1 == nblocks;
            if !matches!(self.blocks[k], Slot::Mem(_)) {
                continue;
            }
            if n < self.block_len() && !(all && last) || n == 0 {
                first_mem = first_mem.min(k);
                continue;
            }
            if self.store.is_none() {
                match spill.store(&self.name) {
                    Ok(s) => self.store = Some(s),
                    Err(e) => {
                        self.first_mem = self.first_mem.min(k);
                        return Err(e);
                    }
                }
            }
            let Slot::Mem(b) = std::mem::replace(&mut self.blocks[k], Slot::Mem(Vec::new())) else { unreachable!() };
            let p = spill.write_async(self.store.as_ref().unwrap(), Box::new(b));
            self.blocks[k] = Slot::Pending(p);
            self.pending.push(k);
            if last && n < self.block_len() {
                self.sealed = true;
            }
        }
        self.first_mem = first_mem;
        self.poll()
    }

    /// Blocks written: their memory is freed (`Disk`). A failed write: the
    /// block stays in memory, and the error is returned.
    pub fn poll(&mut self) -> Result<(), IoError> {
        let mut err = None;
        let mut i = 0;
        while i < self.pending.len() {
            let k = self.pending[i];
            let Slot::Pending(p) = &self.blocks[k] else { unreachable!() };
            let p = p.clone();
            match p.try_settle::<Vec<T>>() {
                Some(Ok(r)) => self.blocks[k] = Slot::Disk(r),
                Some(Err((v, e))) => {
                    self.blocks[k] = Slot::Mem(v);
                    self.first_mem = self.first_mem.min(k);
                    if k + 1 == self.blocks.len() {
                        self.sealed = false;
                    }
                    err.get_or_insert(e);
                }
                None => {
                    i += 1;
                    continue;
                }
            }
            self.pending.swap_remove(i);
        }
        err.map_or(Ok(()), Err)
    }

    /// Wait for the writes in progress (then as `poll`).
    pub fn settle(&mut self) -> Result<(), IoError> {
        for &k in &self.pending {
            if let Slot::Pending(p) = &self.blocks[k] {
                let _ = p.wait();
            }
        }
        self.poll()
    }

    /// The memory held (blocks in memory, or still being written).
    pub fn bytes(&self) -> usize {
        let size = std::mem::size_of::<T>();
        self.blocks
            .iter()
            .map(|s| match s {
                Slot::Mem(b) => b.capacity() * size,
                Slot::Pending(_) => self.block_len() * size,
                Slot::Disk(_) => 0,
            })
            .sum()
    }

    /// The number of blocks on disk (or being written).
    pub fn spilled_blocks(&self) -> usize {
        self.blocks.iter().filter(|s| !matches!(s, Slot::Mem(_))).count()
    }

    /// The values in order (spilled blocks are read back, the next one
    /// ahead with `spill`'s reader thread). A read error ends the
    /// iteration; `Iter::error` tells.
    pub fn iter<'a>(&'a self, spill: Option<&'a Spill>) -> Iter<'a, T> {
        Iter { src: Src::Borrowed(self), spill, next: 0, cur: Cur::None, ptr: std::ptr::null(), len: 0, pos: 0, ahead: None, error: None }
    }

    /// `iter` from value `start` on (the blocks before its block are not
    /// read).
    pub fn iter_from<'a>(&'a self, start: usize, spill: Option<&'a Spill>) -> Iter<'a, T> {
        let k = (start >> self.shift).min(self.blocks.len());
        let mut it = Iter {
            src: Src::Borrowed(self),
            spill,
            next: k,
            cur: Cur::None,
            ptr: std::ptr::null(),
            len: 0,
            pos: 0,
            ahead: None,
            error: None,
        };
        for _ in (k << self.shift)..start.min(self.len) {
            it.next();
        }
        it
    }

    /// `iter`, freeing the blocks once read.
    pub fn drain(self, spill: Option<&Spill>) -> Iter<'_, T> {
        Iter { src: Src::Owned(self), spill, next: 0, cur: Cur::None, ptr: std::ptr::null(), len: 0, pos: 0, ahead: None, error: None }
    }
}

enum Src<'a, T> {
    Borrowed(&'a SpillVec<T>),
    Owned(SpillVec<T>),
}

enum Cur<'a, T> {
    None,
    Borrowed(&'a [T]),
    Owned(Vec<T>),
}

/// The values of a `SpillVec`, in order (see `SpillVec::iter`).
pub struct Iter<'a, T> {
    src: Src<'a, T>,
    spill: Option<&'a Spill>,
    /// The next block.
    next: usize,
    cur: Cur<'a, T>,
    /// The values of `cur` (not changed while they are read), and the
    /// position of the next one.
    ptr: *const T,
    len: usize,
    pos: usize,
    /// Block `next`, being read ahead.
    ahead: Option<Arc<Prefetch>>,
    error: Option<IoError>,
}

impl<T: Plain> Iter<'_, T> {
    /// The read error that ended the iteration, if any.
    pub fn error(&self) -> Option<&IoError> {
        self.error.as_ref()
    }

    fn vec(&self) -> &SpillVec<T> {
        match &self.src {
            Src::Borrowed(v) => v,
            Src::Owned(v) => v,
        }
    }

    /// Where block `k` is on disk, if it is (waiting for its write); `None`
    /// if its values are in memory. A write that failed leaves the block's
    /// data in memory, so it is `None` too: only a read of a block that
    /// really is on disk can fail, and that error is the read's own
    /// (propagated by the caller).
    fn on_disk(&self, k: usize) -> Option<BlockRef> {
        match &self.vec().blocks[k] {
            Slot::Mem(_) => None,
            Slot::Pending(p) => p.wait().ok(),
            Slot::Disk(r) => Some(r.clone()),
        }
    }

    /// Load block `next` into `cur`.
    fn load_next(&mut self) -> Result<bool, IoError> {
        let k = self.next;
        if k >= self.vec().blocks.len() {
            return Ok(false);
        }
        self.next += 1;
        self.pos = 0;
        let n = self.vec().len_of(k);
        let ahead = self.ahead.take();
        self.cur = match self.on_disk(k) {
            Some(r) => {
                let bytes = match ahead {
                    Some(p) => p.wait()?,
                    None => self.vec().store.as_ref().unwrap().read(&r)?,
                };
                let mut v = vec![T::default(); n];
                as_bytes_mut(&mut v).copy_from_slice(&bytes[..n * std::mem::size_of::<T>()]);
                if let Src::Owned(sv) = &self.src {
                    sv.store.as_ref().unwrap().delete(r);
                }
                Cur::Owned(v)
            }
            None => match &mut self.src {
                Src::Borrowed(sv) => match &sv.blocks[k] {
                    Slot::Mem(b) => Cur::Borrowed(b),
                    Slot::Pending(p) => {
                        let mut v = vec![T::default(); n];
                        p.with_data(|d| as_bytes_mut(&mut v).copy_from_slice(&d[..n * std::mem::size_of::<T>()]));
                        Cur::Owned(v)
                    }
                    Slot::Disk(_) => unreachable!(),
                },
                Src::Owned(sv) => match std::mem::replace(&mut sv.blocks[k], Slot::Mem(Vec::new())) {
                    Slot::Mem(b) => Cur::Owned(b),
                    Slot::Pending(p) => Cur::Owned(p.take_back()),
                    Slot::Disk(_) => unreachable!(),
                },
            },
        };
        (self.ptr, self.len) = match &self.cur {
            Cur::None => (std::ptr::null(), 0),
            Cur::Borrowed(b) => (b.as_ptr(), b.len()),
            Cur::Owned(v) => (v.as_ptr(), v.len()),
        };
        // Read the next block ahead.
        if let (Some(spill), true) = (self.spill, self.next < self.vec().blocks.len()) {
            if let Slot::Disk(r) = &self.vec().blocks[self.next] {
                self.ahead = Some(spill.prefetch(self.vec().store.as_ref().unwrap(), r));
            }
        }
        Ok(true)
    }
}

impl<T: Plain> Iter<'_, T> {
    /// `next` at the end of a block.
    #[inline(never)]
    fn next_block(&mut self) -> Option<T> {
        while self.error.is_none() {
            match self.load_next() {
                Ok(true) if self.len > 0 => {
                    self.pos = 1;
                    // SAFETY: `ptr` points to the `len` values of `cur`.
                    return Some(unsafe { *self.ptr });
                }
                Ok(true) => {}
                Ok(false) => return None,
                Err(e) => {
                    self.cur = Cur::None;
                    (self.ptr, self.len) = (std::ptr::null(), 0);
                    self.error = Some(e);
                }
            }
        }
        None
    }
}

impl<T: Plain> Iterator for Iter<'_, T> {
    type Item = T;

    #[inline]
    fn next(&mut self) -> Option<T> {
        if self.pos < self.len {
            // SAFETY: `ptr` points to the `len` values of `cur`, which does
            // not change until the next block is loaded.
            let x = unsafe { *self.ptr.add(self.pos) };
            self.pos += 1;
            return Some(x);
        }
        self.next_block()
    }
}

#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;
    use std::os::unix::fs::FileExt;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::io::{Spill, SpillConfig};

    use super::{Slot, SpillVec};

    fn test_dir(name: &str) -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("tlx-spillvec-test-{}-{name}-{n}", std::process::id()))
    }

    /// 1000 values in blocks of 128: the first three write, the rest fail
    /// (`fail_after`) and stay in memory.
    fn spilled_with_a_failed_write(name: &str) -> (Spill, SpillVec<u64>) {
        let spill = Spill::new(SpillConfig { dir: Some(test_dir(name)), fail_after: Some(3 * 1024), ..Default::default() });
        let mut v: SpillVec<u64> = SpillVec::new(1024, "log");
        for i in 0..1000u64 {
            v.push(i * 3);
        }
        let _ = v.spill(&spill, false);
        (spill, v)
    }

    /// The failed writes are still `Slot::Pending` (nothing polled them):
    /// the iteration reads them from memory and does not report an error
    /// (only their presence on disk is `None`, not a failure).
    #[test]
    fn iter_reads_memory_after_a_failed_write() {
        let (spill, v) = spilled_with_a_failed_write("failed-write");
        // Nothing polled the vec: the seven full blocks are still
        // `Pending`, their write done (blocks 0..3) or failed (3..7).
        assert_eq!(v.spilled_blocks(), 7);
        assert!(v.blocks[..7].iter().all(|s| matches!(s, Slot::Pending(_))));
        let mut it = v.iter(Some(&spill));
        let all: Vec<u64> = it.by_ref().collect();
        assert!(it.error().is_none(), "{:?}", it.error());
        assert_eq!(all, (0..1000u64).map(|i| i * 3).collect::<Vec<_>>());
        assert_eq!(spill.counters().read, 3 * 1024);
    }

    /// A read of a block that really is on disk still fails the iteration.
    #[test]
    fn iter_propagates_a_store_read_error() {
        let (spill, mut v) = spilled_with_a_failed_write("read-error");
        assert!(v.settle().is_err());
        let Slot::Disk(r) = &v.blocks[1] else { panic!("block 1 was not written") };
        let r = r.clone();
        // Flip a byte of block 1 behind the store's back.
        let f = OpenOptions::new().write(true).open(v.store.as_ref().unwrap().path()).unwrap();
        f.write_all_at(&[0xA5], r.off + 8).unwrap();
        // Block 0 reads fine; block 1 is read ahead by the reader thread,
        // whose error ends the iteration.
        let mut it = v.iter(Some(&spill));
        let got: Vec<u64> = it.by_ref().collect();
        let e = it.error().expect("the corrupt block must end the iteration");
        assert!(e.0.contains("checksum mismatch"), "{e}");
        assert_eq!(got, (0..128u64).map(|i| i * 3).collect::<Vec<_>>());
    }
}
