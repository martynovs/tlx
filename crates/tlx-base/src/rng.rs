//! Seed-reproducible SplitMix64: the shared 64-bit core sequence for test
//! and simulation generators, including its resumable state.
//!
//! `next_u64` is the core progression meant for reuse; a caller that needs a
//! different bounded-sampling policy keeps its own wrapper and only calls
//! this method (CODE-14). `below` is this module's own policy (modulo
//! reduction, used by the runtime simulation) and must not be substituted for
//! a consumer's policy under the same generator name.

/// SplitMix64 state (the raw 64-bit stream; the field is public so a caller
/// can snapshot and resume the sequence).
#[derive(Clone, Debug)]
pub struct SplitMix64(pub u64);

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// The next value of the shared SplitMix64 progression.
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        crate::hash::mix64(self.0)
    }

    /// Uniform in `0..bound` by modulo reduction (the runtime simulation
    /// policy; see the module note on sampling policies).
    #[inline]
    pub fn below(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulation_sequence_and_resumption() {
        let mut rng = SplitMix64::new(0);
        for expected in [0xe220a8397b1dcdaf, 0x6e789e6aa1b965f4, 0x06c45d188009454f] {
            assert_eq!(rng.next_u64(), expected);
        }
        let mut resumed = SplitMix64(rng.0);
        for bound in [1, 2, 7, usize::MAX] {
            assert_eq!(rng.below(bound), (resumed.next_u64() % bound as u64) as usize);
        }
        let mut wrapped = SplitMix64(u64::MAX);
        wrapped.next_u64();
        assert_eq!(wrapped.0, 0x9e37_79b9_7f4a_7c14);
    }

    #[test]
    fn core_sequence_is_stable_for_zero_and_max_seeds() {
        for (seed, expected) in [
            (0u64, [0xe220a8397b1dcdaf, 0x6e789e6aa1b965f4, 0x06c45d188009454f]),
            (u64::MAX, [0xe4d971771b652c20, 0xe99ff867dbf682c9, 0x382ff84cb27281e9]),
        ] {
            let mut rng = SplitMix64::new(seed);
            let got: Vec<u64> = (0..3).map(|_| rng.next_u64()).collect();
            assert_eq!(got, expected, "seed {seed}");
        }
    }
}
