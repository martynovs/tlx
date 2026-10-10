//! Bitset operations on encoded values (`u64` word arrays).
//! Bit `r` set means the element of rank `r` is present.

/// Ranks of the elements of a bitset, ascending.
pub fn bit_ranks(w: &[u64]) -> impl Iterator<Item = usize> + '_ {
    w.iter().enumerate().flat_map(|(i, &word)| {
        let mut b = word;
        std::iter::from_fn(move || {
            (b != 0).then(|| {
                let r = i * 64 + b.trailing_zeros() as usize;
                b &= b - 1;
                r
            })
        })
    })
}

/// Whether rank `r` is in the bitset (false outside the universe).
pub fn bit_test(w: &[u64], r: i64) -> bool {
    r >= 0 && (r as usize) < w.len() * 64 && (w[r as usize / 64] >> (r % 64)) & 1 == 1
}

/// Add rank `r` (checked by the caller to be inside the universe).
pub fn bit_set(w: &mut [u64], r: usize) {
    w[r / 64] |= 1 << (r % 64);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitsets() {
        let mut w = [0u64; 2];
        for r in [0, 5, 63, 64, 100] {
            bit_set(&mut w, r);
        }
        assert_eq!(bit_ranks(&w).collect::<Vec<_>>(), [0, 5, 63, 64, 100]);
        assert!(bit_test(&w, 64) && !bit_test(&w, 65) && !bit_test(&w, -1) && !bit_test(&w, 128));
    }
}
