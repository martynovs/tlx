//! A blocked Bloom filter of fingerprints (one per run of the disk tier).
//!
//! Every fingerprint sets `K` bits of one 512-bit block (a cache line on
//! most machines, half of one on Apple's): a lookup touches one block.
//! The block and the bits within it come from mixes of all the bits of the
//! fingerprint (fingerprints are uniform, but test models' need not be).
//! With `BITS_PER_FP` = 10 bits per fingerprint the false-positive rate is
//! about 1%.

use std::sync::atomic::{AtomicU64, Ordering};

/// Bits of the filter per fingerprint.
pub(super) const BITS_PER_FP: usize = 10;

/// Bits set per fingerprint.
const K: u32 = 7;

/// Words of a block.
const BLOCK_WORDS: usize = 8;

pub(super) struct Bloom {
    words: Box<[AtomicU64]>,
    blocks: u64,
}

impl Bloom {
    /// A filter for `n` fingerprints.
    pub(super) fn new(n: usize) -> Bloom {
        let blocks = (n.saturating_mul(BITS_PER_FP).div_ceil(BLOCK_WORDS * 64)).max(1);
        Bloom { words: (0..blocks * BLOCK_WORDS).map(|_| AtomicU64::new(0)).collect(), blocks: blocks as u64 }
    }

    /// The memory of the filter.
    pub(super) fn bytes(&self) -> usize {
        self.words.len() * 8
    }

    /// The memory of a filter for `n` fingerprints.
    pub(super) fn bytes_for(n: usize) -> usize {
        (n.saturating_mul(BITS_PER_FP).div_ceil(BLOCK_WORDS * 64)).max(1) * BLOCK_WORDS * 8
    }

    /// The first word of `fp`'s block, and the bits within the block (as
    /// `K` 9-bit positions packed in a word).
    #[inline]
    fn locate(&self, fp: u64) -> (usize, u64) {
        use tlx_base::hash::mix64 as mix;
        let h = mix(fp);
        let block = ((h as u128 * self.blocks as u128) >> 64) as usize;
        (block * BLOCK_WORDS, mix(h))
    }

    /// Add `fp` (several threads may add at once).
    pub(super) fn insert(&self, fp: u64) {
        let (base, mut h) = self.locate(fp);
        let mut masks = [0u64; BLOCK_WORDS];
        for _ in 0..K {
            let bit = (h & 511) as usize;
            masks[bit >> 6] |= 1 << (bit & 63);
            h >>= 9;
        }
        for (i, m) in masks.into_iter().enumerate() {
            if m != 0 {
                self.words[base + i].fetch_or(m, Ordering::Relaxed);
            }
        }
    }

    /// Whether `fp` may have been added (false: it was not).
    #[inline]
    pub(super) fn maybe(&self, fp: u64) -> bool {
        let (base, mut h) = self.locate(fp);
        for _ in 0..K {
            let bit = (h & 511) as usize;
            if self.words[base + (bit >> 6)].load(Ordering::Relaxed) & (1 << (bit & 63)) == 0 {
                return false;
            }
            h >>= 9;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(i: u64) -> u64 {
        i.wrapping_mul(0xD6E8_FEB8_6659_FD93).rotate_left(17) ^ 0x5555
    }

    /// No false negatives; about 1% false positives.
    #[test]
    fn no_false_negatives_few_false_positives() {
        // Miri is ~100x slower; the false-positive bound scales with n.
        let n = if cfg!(miri) { 10_000 } else { 100_000 };
        let b = Bloom::new(n);
        assert_eq!(b.bytes(), Bloom::bytes_for(n));
        for i in 0..n as u64 {
            b.insert(fp(i));
        }
        assert!((0..n as u64).all(|i| b.maybe(fp(i))));
        let fpos = (n as u64..2 * n as u64).filter(|&i| b.maybe(fp(i))).count();
        assert!(fpos < n / 50, "{fpos} false positives in {n}");
        // Clustered fingerprints too.
        let b = Bloom::new(1000);
        for i in 0..1000u64 {
            b.insert(i << 40);
        }
        assert!((0..1000u64).all(|i| b.maybe(i << 40)));
        let fpos = (0..1000u64).filter(|&i| b.maybe((i << 40) | 1)).count();
        assert!(fpos < 40, "{fpos} false positives in 1000");
    }
}
