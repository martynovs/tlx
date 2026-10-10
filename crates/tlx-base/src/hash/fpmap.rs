//! `FpMap`: a map from fingerprints (`u64` keys that are hashes already) to
//! `u32` values, such as the node ids of a behavior graph.

use super::INTEGER_MULTIPLIER;

/// A map from fingerprints to `u32` values: an open-addressing table
/// (linear probing, at most 3/4 full), 12 bytes per slot. The slot of a key
/// is taken from the high half of one multiplication (fingerprints are
/// hashes already).
///
/// `u32::MAX` marks an empty slot: it is not a value.
#[derive(Default)]
pub struct FpMap {
    keys: Vec<u64>,
    vals: Vec<u32>,
    len: usize,
}

/// An empty slot of `FpMap::vals`.
const EMPTY: u32 = u32::MAX;

impl FpMap {
    /// An empty map (it allocates on the first insertion).
    pub fn new() -> FpMap {
        FpMap::default()
    }

    /// An empty map that holds `n` entries without growing (at most half
    /// full then).
    pub fn with_capacity(n: usize) -> FpMap {
        let cap = Self::slots(n);
        FpMap { keys: vec![0; cap], vals: vec![EMPTY; cap], len: 0 }
    }

    /// The slots of `with_capacity(n)`.
    fn slots(n: usize) -> usize {
        (2 * n).next_power_of_two().max(16)
    }

    /// The memory of `with_capacity(n)`, in bytes (`bytes`).
    pub fn bytes_with_capacity(n: usize) -> usize {
        12 * Self::slots(n)
    }

    /// The number of entries.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    fn slot(fp: u64, mask: usize) -> usize {
        (fp.wrapping_mul(INTEGER_MULTIPLIER) >> 32) as usize & mask
    }

    /// The slot of `fp`: its entry, or the empty slot where it goes. The
    /// table must have an empty slot.
    #[inline]
    fn find(&self, fp: u64) -> usize {
        let mask = self.vals.len() - 1;
        let mut i = Self::slot(fp, mask);
        while self.vals[i] != EMPTY && self.keys[i] != fp {
            i = (i + 1) & mask;
        }
        i
    }

    /// The value of `fp`, if any.
    #[inline]
    pub fn get(&self, fp: u64) -> Option<u32> {
        if self.vals.is_empty() {
            return None;
        }
        match self.vals[self.find(fp)] {
            EMPTY => None,
            v => Some(v),
        }
    }

    /// Insert `fp -> v` if `fp` has no value: false (and the value left as
    /// it is) if it has one.
    pub fn insert_new(&mut self, fp: u64, v: u32) -> bool {
        let mut new = false;
        self.get_or_insert_with(fp, || {
            new = true;
            v
        });
        new
    }

    /// The value of `fp`, inserting `f()` first if it has none. The value
    /// can be changed in place (never to `u32::MAX`).
    #[inline]
    pub fn get_or_insert_with(&mut self, fp: u64, f: impl FnOnce() -> u32) -> &mut u32 {
        if 4 * (self.len + 1) > 3 * self.vals.len() {
            self.grow();
        }
        let i = self.find(fp);
        if self.vals[i] == EMPTY {
            let v = f();
            debug_assert_ne!(v, EMPTY, "u32::MAX is not a value of an FpMap");
            self.keys[i] = fp;
            self.vals[i] = v;
            self.len += 1;
        }
        &mut self.vals[i]
    }

    fn grow(&mut self) {
        let cap = (2 * self.vals.len()).max(1024);
        let keys = std::mem::replace(&mut self.keys, vec![0; cap]);
        let vals = std::mem::replace(&mut self.vals, vec![EMPTY; cap]);
        for (fp, v) in keys.into_iter().zip(vals).filter(|&(_, v)| v != EMPTY) {
            let i = self.find(fp);
            self.keys[i] = fp;
            self.vals[i] = v;
        }
    }

    /// The memory of the table, in bytes.
    pub fn bytes(&self) -> usize {
        12 * self.vals.len()
    }
}

#[cfg(test)]
mod tests {
    use super::FpMap;

    #[test]
    fn empty_map_finds_nothing() {
        let m = FpMap::new();
        assert_eq!(m.get(0), None);
        assert_eq!(m.get(42), None);
        assert!(m.is_empty());
    }

    #[test]
    fn insert_new_keeps_the_first_value() {
        let mut m = FpMap::with_capacity(4);
        assert!(m.insert_new(7, 1));
        assert!(!m.insert_new(7, 2));
        assert_eq!(m.get(7), Some(1));
        assert_eq!(m.len(), 1);
        // Key 0 is a key like any other.
        assert!(m.insert_new(0, 3));
        assert_eq!(m.get(0), Some(3));
    }

    #[test]
    fn values_change_in_place() {
        let mut m = FpMap::new();
        let next = m.len() as u32;
        *m.get_or_insert_with(5, || next) |= 1 << 31;
        assert_eq!(m.get(5), Some(1 << 31));
        let v = m.get_or_insert_with(5, || unreachable!("present"));
        assert_eq!(*v, 1 << 31);
    }

    /// Ids numbered in order of insertion survive growth; colliding slots
    /// (keys equal in their high bits after the multiplication) probe on.
    #[test]
    fn growth_keeps_every_entry() {
        let mut m = FpMap::new();
        let keys: Vec<u64> = (0..100_000u64).map(|i| i.wrapping_mul(0x2545_f491_4f6c_dd1d) ^ (i << 40)).collect();
        for &k in &keys {
            let n = m.len() as u32;
            m.get_or_insert_with(k, || n);
        }
        assert_eq!(m.len(), keys.len());
        for (i, &k) in keys.iter().enumerate() {
            assert_eq!(m.get(k), Some(i as u32));
        }
        assert_eq!(m.get(u64::MAX), None);
        // A table sized for its entries does not grow.
        let mut f = FpMap::with_capacity(keys.len());
        let cap = f.bytes();
        assert_eq!(cap, FpMap::bytes_with_capacity(keys.len()));
        for (i, &k) in keys.iter().enumerate() {
            assert!(f.insert_new(k, i as u32));
        }
        assert_eq!(f.bytes(), cap);
    }
}
