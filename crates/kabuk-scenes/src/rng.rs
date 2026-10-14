//! A small seeded random source (xorshift64*), so a scene can be replayed in tests and
//! each monitor's scene differs from the others.

#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        // A zero state would stay zero.
        Rng(seed ^ 0x9E37_79B9_7F4A_7C15 | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// In `[0, 1)`.
    pub fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// In `[lo, hi)`.
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + self.f64() * (hi - lo)
    }

    /// In `0..n` (0 when `n` is 0).
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: the same seed repeats, values stay in range, a zero seed still moves.
    use super::*;

    #[test]
    fn seeded_values_repeat_and_stay_in_range() {
        let (mut a, mut b) = (Rng::new(7), Rng::new(7));
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
            let f = a.f64();
            b.f64();
            assert!((0.0..1.0).contains(&f));
            assert!(a.below(5) < 5);
            b.below(5);
        }
        let mut z = Rng::new(0);
        assert_ne!(z.next_u64(), z.next_u64());
        assert_eq!(Rng::new(1).below(0), 0);
        let r = Rng::new(3).range(2.2, 4.6);
        assert!((2.2..4.6).contains(&r));
    }
}
