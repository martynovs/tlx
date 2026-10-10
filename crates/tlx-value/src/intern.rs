//! Global string interner. Strings and model-value names are `Sym`s; their
//! order is the order in which they were first interned (TLC's UniqueString
//! token order, see tlc-semantics.md §1.3). Callers that care about matching
//! TLC's CHOOSE results must intern in TLC's order (cfg values first, then
//! source tokens in parse order) before evaluating anything.
//!
//! Implementation: the text -> id index is split into `SHARD_COUNT`
//! independently locked open-addressing tables, chosen by the text's hash, so
//! threads that intern new strings concurrently (a spec that builds strings,
//! many workers) rarely meet on a lock, and a table only rehashes its own
//! share. Each thread also keeps a small direct-mapped cache of symbols it
//! interned, which serves repeated strings without any lock. Ids come from one
//! global counter, taken under the shard lock: on one thread they follow the
//! call order, exactly as with a single lock (concurrent threads interleave,
//! as they always did). The id -> text table is an append-only segmented
//! array that is read without any lock (`as_str` is lock-free).

use std::alloc::Layout;
use std::cell::Cell;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};

/// Interned string. `Ord` is intern order, not lexicographic order.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Sym(u32);

impl Sym {
    /// Intern `s` (returns the existing symbol if already interned).
    pub fn new(s: &str) -> Sym {
        // Interned symbols never change, so a per-thread cache is always
        // valid; it keeps the shared locks out of the steady state. A slot
        // holds `tag << 32 | id` like the shard tables.
        thread_local! {
            static CACHE: [Cell<u64>; CACHE_SLOTS] = const { [const { Cell::new(EMPTY) }; CACHE_SLOTS] };
        }
        let hash = tlx_base::hash::hash_bytes(s.as_bytes());
        let slot = hash as usize & (CACHE_SLOTS - 1);
        let cached = CACHE.with(|c| c[slot].get());
        if cached != EMPTY && (cached >> 32) as u32 == tag_of(hash) && entry(cached as u32).text == s {
            return Sym(cached as u32);
        }
        let sym = Sym::new_global(s, hash);
        CACHE.with(|c| c[slot].set(slot_of(sym.0, hash)));
        sym
    }

    fn new_global(s: &str, hash: u64) -> Sym {
        let mut index = lock(&SHARDS[shard_of(hash)]);
        if let Some(id) = index.find(s, hash) {
            return Sym(id);
        }
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        assert!(id < u32::MAX, "interner overflow");
        let text: &'static str = Box::leak(s.to_owned().into_boxed_str());
        // Publish the entry before the id becomes reachable through a shard.
        // SAFETY: `id` is fresh (just taken from the counter), and this
        // thread holds a shard lock.
        unsafe { write_entry(id, Entry { text, hash, mv_type: mv_type_of(text) }) };
        index.insert(id, hash);
        Sym(id)
    }

    /// Look up an arbitrary raw id, synchronizing with all in-flight writers.
    /// Returns `None` if no symbol has been published for this id.
    pub fn try_from_id(id: u32) -> Option<Sym> {
        // NEXT is advanced before write_entry: the counter alone is not a
        // publication check. Holding every shard waits for those writes.
        let _all = lock_all();
        (id < NEXT.load(Ordering::Relaxed)).then_some(Sym(id))
    }

    /// The symbol whose `id()` is `id`.
    ///
    /// # Safety
    /// `id` must be the `id()` of a `Sym` of this process (or of one that
    /// `restore` re-created), obtained by this thread through some
    /// synchronization with the thread that interned it: then its entry is
    /// written and visible (see `entry`).
    pub unsafe fn from_id(id: u32) -> Sym {
        debug_assert!(id < NEXT.load(Ordering::Relaxed), "invalid Sym id {id}");
        Sym(id)
    }

    /// The interned text (lives for the rest of the process).
    pub fn as_str(self) -> &'static str {
        entry(self.0).text
    }

    pub fn id(self) -> u32 {
        self.0
    }

    /// Deterministic 64-bit hash of the text (independent of intern order).
    pub fn text_hash(self) -> u64 {
        entry(self.0).hash
    }

    /// TLC model-value type: `Some(c)` for names of the form `c_...`
    /// (length > 2 and second character `_`), `None` for untyped names.
    pub fn model_value_type(self) -> Option<char> {
        char::from_u32(entry(self.0).mv_type)
    }
}

impl std::fmt::Debug for Sym {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}

fn mv_type_of(s: &str) -> u32 {
    let mut chars = s.chars();
    let (Some(c0), Some(c1)) = (chars.next(), chars.next()) else { return u32::MAX };
    if c1 == '_' && chars.next().is_some() {
        c0 as u32
    } else {
        u32::MAX // not a valid char => None
    }
}

/// Number of interned symbols (ids `0..len()` are valid).
pub fn len() -> usize {
    // With every shard locked no id is between allocation and its entry
    // write: every id below the counter is written and visible here.
    let _all = lock_all();
    NEXT.load(Ordering::Relaxed) as usize
}

/// The texts of all interned symbols in id (intern) order, for checkpoints.
/// Taken while other threads intern, the result is a prefix of the table.
pub fn snapshot() -> Vec<&'static str> {
    let n = len() as u32;
    (0..n).map(|id| entry(id).text).collect()
}

/// Re-intern `texts` so that `texts[i]` gets id `i` (recover: reproduces the
/// intern order, hence the value order, of the run that took the snapshot).
/// The current table must be a prefix of `texts`; normally it is empty
/// because recover restores the table before anything else interns. If the
/// order cannot be reproduced, the error names the first conflicting id.
pub fn restore<S: AsRef<str>>(texts: &[S]) -> Result<(), String> {
    for (i, t) in texts.iter().enumerate() {
        let t = t.as_ref();
        let sym = Sym::new(t);
        if sym.id() as usize != i {
            let found = if i < len() { format!("{:?}", entry(i as u32).text) } else { "unassigned".to_string() };
            return Err(format!(
                "cannot restore the string intern order: id {i} should be {t:?} but is {found} ({t:?} has id {})",
                sym.id()
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------

struct Entry {
    text: &'static str,
    hash: u64,
    mv_type: u32,
}

/// Slots of the per-thread cache (a power of two; 32 KiB per thread).
const CACHE_SLOTS: usize = 4096;
/// Number of shards of the text -> id index (a power of two): enough that
/// workers interning new strings rarely wait for each other.
const SHARD_BITS: u32 = 10;
const SHARD_COUNT: usize = 1 << SHARD_BITS;
/// A free slot (cache and shard tables): id `u32::MAX` is never assigned.
const EMPTY: u64 = u64::MAX;

/// The next id to assign (the number of ids handed out so far). Only
/// incremented under a shard lock.
static NEXT: AtomicU32 = AtomicU32::new(0);

/// A shard's lock and table, alone on its cache lines (no false sharing).
#[repr(align(128))]
struct Shard(Mutex<Index>);

#[allow(clippy::declare_interior_mutable_const)]
const NEW_SHARD: Shard = Shard(Mutex::new(Index { slots: Vec::new(), len: 0 }));
static SHARDS: [Shard; SHARD_COUNT] = [NEW_SHARD; SHARD_COUNT];

/// The shard of a text hash: its top bits (independent of the tag, the low
/// half, which also places a symbol in its table and in the thread cache).
#[inline]
fn shard_of(hash: u64) -> usize {
    (hash >> (64 - SHARD_BITS)) as usize
}

#[inline]
fn tag_of(hash: u64) -> u32 {
    hash as u32
}

/// A table (or cache) slot: the hash's tag, checked before the text, and the id.
#[inline]
fn slot_of(id: u32, hash: u64) -> u64 {
    (tag_of(hash) as u64) << 32 | id as u64
}

fn lock(shard: &Shard) -> MutexGuard<'_, Index> {
    shard.0.lock().unwrap_or_else(|e| e.into_inner())
}

/// Lock every shard, in index order (the only place that holds more than one
/// lock): no interning is in progress while the guards live.
fn lock_all() -> Vec<MutexGuard<'static, Index>> {
    SHARDS.iter().map(lock).collect()
}

/// Open-addressing (linear probing) table of ids, slots as in `slot_of`.
/// A slot's position follows from its tag: growing never reads an entry.
struct Index {
    slots: Vec<u64>,
    len: usize,
}

impl Index {
    fn find(&self, s: &str, hash: u64) -> Option<u32> {
        if self.slots.is_empty() {
            return None;
        }
        let mask = self.slots.len() - 1;
        let tag = tag_of(hash);
        let mut i = tag as usize & mask;
        loop {
            let slot = self.slots[i];
            if slot == EMPTY {
                return None;
            }
            if (slot >> 32) as u32 == tag && entry(slot as u32).text == s {
                return Some(slot as u32);
            }
            i = (i + 1) & mask;
        }
    }

    /// Add `id` (absent; its entry is written), whose text hash is `hash`.
    fn insert(&mut self, id: u32, hash: u64) {
        // Load factor at most 1/2: short probe sequences, even for misses.
        if (self.len + 1) * 2 > self.slots.len() {
            self.grow();
        }
        self.place(slot_of(id, hash));
        self.len += 1;
    }

    fn place(&mut self, slot: u64) {
        let mask = self.slots.len() - 1;
        let mut i = (slot >> 32) as usize & mask;
        while self.slots[i] != EMPTY {
            i = (i + 1) & mask;
        }
        self.slots[i] = slot;
    }

    fn grow(&mut self) {
        let cap = (self.slots.len() * 2).max(16);
        let old = std::mem::replace(&mut self.slots, vec![EMPTY; cap]);
        for slot in old.into_iter().filter(|&s| s != EMPTY) {
            self.place(slot);
        }
    }
}

/// Segment `k` holds ids `2^k - 1 .. 2^(k+1) - 1` (capacity `2^k`).
const SEGMENTS: usize = 32;
#[allow(clippy::declare_interior_mutable_const)]
const NULL_SEG: AtomicPtr<Entry> = AtomicPtr::new(std::ptr::null_mut());
static SEGS: [AtomicPtr<Entry>; SEGMENTS] = [NULL_SEG; SEGMENTS];

#[inline]
fn locate(id: u32) -> (usize, usize) {
    let n = id as u64 + 1;
    let k = 63 - n.leading_zeros() as usize;
    (k, (n - (1u64 << k)) as usize)
}

/// # Safety
/// Must be called once per id, before the id is published in a shard (and
/// so before any `Sym` with this id exists), by a thread holding a shard
/// lock (whose release makes the write visible to that shard's readers).
unsafe fn write_entry(id: u32, e: Entry) {
    let (k, off) = locate(id);
    let mut seg = SEGS[k].load(Ordering::Acquire);
    if seg.is_null() {
        // Threads holding different shard locks may race to allocate the
        // same segment: the first one installs it, the others free theirs.
        let layout = Layout::array::<Entry>(1usize << k).expect("interner segment size");
        // SAFETY: the layout's size is nonzero (at least one entry).
        let fresh = unsafe { std::alloc::alloc(layout) } as *mut Entry;
        if fresh.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        seg = match SEGS[k].compare_exchange(std::ptr::null_mut(), fresh, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => fresh,
            Err(installed) => {
                // SAFETY: allocated above with this layout, never shared.
                unsafe { std::alloc::dealloc(fresh as *mut u8, layout) };
                installed
            }
        };
    }
    // SAFETY: `off < 2^k`, the segment's length; no reader can access this
    // entry yet (no `Sym` with this id exists), and no other writer has this
    // id (ids come from the counter once each).
    unsafe { seg.add(off).write(e) };
}

#[inline]
fn entry(id: u32) -> &'static Entry {
    let (k, off) = locate(id);
    let seg = SEGS[k].load(Ordering::Acquire);
    assert!(!seg.is_null(), "invalid Sym");
    // SAFETY: `off < 2^k`, the segment's length. A `Sym` can only be
    // obtained from `Sym::new` (or from the id of one, `Sym::from_id`,
    // whose contract requires the same), which initializes the entry
    // before returning (and before publishing the id in a shard); entries
    // are never modified afterwards.
    // The entry write happens-before this read: this thread got the id from
    // the writing thread through some synchronization, from a shard under
    // its lock (released by the writer after writing), or from its own cache
    // (filled from one of those).
    unsafe { &*seg.add(off) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Threads interning overlapping sets of new strings concurrently agree
    /// on the symbols and read back their texts (a stress test of the
    /// lock-free id -> text table; most useful under a sanitizer).
    #[test]
    fn concurrent_interning() {
        // Miri interleaves the threads faithfully but runs ~1000x slower; the
        // thread structure is what exercises the lock-free table, so shrink
        // the per-thread count under Miri only.
        const N: usize = if cfg!(miri) { 64 } else { 3000 };
        let syms: Vec<Vec<Sym>> = std::thread::scope(|sc| {
            let hs: Vec<_> = (0..8)
                .map(|t| {
                    sc.spawn(move || {
                        let mut out = vec![None; N];
                        for j in 0..N {
                            let i = (j * 7 + t * 13) % N;
                            let s = Sym::new(&format!("intern-test-{i}"));
                            assert_eq!(s.as_str(), format!("intern-test-{i}"));
                            out[i] = Some(s);
                        }
                        out.into_iter().map(Option::unwrap).collect::<Vec<_>>()
                    })
                })
                .collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        for v in &syms[1..] {
            assert_eq!(v, &syms[0]);
        }
        for (i, s) in syms[0].iter().enumerate() {
            assert_eq!(s.as_str(), format!("intern-test-{i}"));
        }
    }

    /// Ids follow the call order on one thread, and repeated or evicted
    /// strings (more than the thread cache holds) keep their symbols.
    #[test]
    fn order_and_identity() {
        const N: usize = if cfg!(miri) { 100 } else { 3 * CACHE_SLOTS };
        let first: Vec<Sym> = (0..N).map(|i| Sym::new(&format!("order-test-{i}"))).collect();
        assert!(first.windows(2).all(|w| w[0] < w[1]), "ids not in call order");
        let again: Vec<Sym> = (0..N).rev().map(|i| Sym::new(&format!("order-test-{i}"))).collect();
        assert!(first.iter().eq(again.iter().rev()));
        // Another thread (empty cache) finds the same symbols in the shards.
        std::thread::scope(|sc| {
            sc.spawn(|| {
                for (i, s) in first.iter().enumerate() {
                    assert_eq!(Sym::new(&format!("order-test-{i}")), *s);
                }
            });
        });
        let snap = snapshot();
        for (i, s) in first.iter().enumerate() {
            assert_eq!(snap[s.id() as usize], format!("order-test-{i}"));
        }
    }

    /// A snapshot taken while threads intern is a prefix of the table.
    #[test]
    fn snapshot_while_interning() {
        const N: usize = if cfg!(miri) { 50 } else { 20000 };
        std::thread::scope(|sc| {
            for t in 0..4 {
                sc.spawn(move || {
                    for i in 0..N {
                        Sym::new(&format!("snap-test-{t}-{i}"));
                    }
                });
            }
            for _ in 0..if cfg!(miri) { 2 } else { 50 } {
                let snap = snapshot();
                for (id, text) in snap.iter().enumerate() {
                    assert_eq!(entry(id as u32).text, *text);
                }
            }
        });
    }

    /// Reserve an id under a shard lock, then ask another thread to decode
    /// it before publishing. Checking NEXT alone would expose unwritten data.
    #[test]
    fn checked_id_waits_for_publication() {
        let text = "checked-reserved-entry";
        let hash = tlx_base::hash::hash_bytes(text.as_bytes());
        std::thread::scope(|scope| {
            let mut index = lock(&SHARDS[shard_of(hash)]);
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let (ready, started) = std::sync::mpsc::channel();
            let reader = scope.spawn(move || {
                ready.send(()).unwrap();
                Sym::try_from_id(id).unwrap().as_str()
            });
            started.recv().unwrap();
            // SAFETY: id was allocated under this shard lock and has never
            // been published. The checked reader must wait for this lock.
            unsafe { write_entry(id, Entry { text, hash, mv_type: mv_type_of(text) }) };
            index.insert(id, hash);
            drop(index);
            assert_eq!(reader.join().unwrap(), text);
        });
        assert!(Sym::try_from_id(u32::MAX).is_none());
    }

    #[test]
    fn segments() {
        assert_eq!(locate(0), (0, 0));
        assert_eq!(locate(1), (1, 0));
        assert_eq!(locate(2), (1, 1));
        assert_eq!(locate(3), (2, 0));
        assert_eq!(locate(u32::MAX - 1), (31, (1 << 31) - 1));
    }
}
