pub struct XorShift64Star(u64);

impl XorShift64Star {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    pub fn below(&mut self, limit: u64) -> u64 {
        self.next_u64() % limit
    }
    pub fn below_or_zero(&mut self, limit: u64) -> u64 {
        if limit == 0 {
            0
        } else {
            self.below(limit)
        }
    }
    pub fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below_or_zero((hi - lo + 1) as u64) as i64
    }
    pub fn chance(&mut self, numerator: u64, denominator: u64) -> bool {
        self.below(denominator) < numerator
    }
    pub fn chance_inclusive(&mut self, probability: f64) -> bool {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64 <= probability
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_sequence_and_sampling() {
        let mut rng = XorShift64Star::new(1);
        assert_eq!((0..3).map(|_| rng.next_u64()).collect::<Vec<_>>(), [5180492295206395165, 12380297144915551517, 13389498078930870103]);
        let mut rng = XorShift64Star::new(1);
        assert_eq!(rng.below_or_zero(0), 0);
        assert_eq!(rng.below(10), 5);
        assert_eq!(rng.range(-10, 10), -10 + (12380297144915551517u64 % 21) as i64);
        assert_eq!(rng.chance(7, 10), 13389498078930870103u64 % 10 < 7);
        assert!(XorShift64Star::new(0).chance_inclusive(0.0));
        assert!(!XorShift64Star::new(0).chance(0, 100));
    }
}
