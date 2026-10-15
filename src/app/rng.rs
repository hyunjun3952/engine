pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Rng(seed)
    }

    // The next 24 random bits.
    fn bits(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        self.0 >> 8
    }

    pub fn f32(&mut self) -> f32 {
        self.bits() as f32 / (1u32 << 24) as f32
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }

    /// A whole number in `0..n`, to pick from a list; panics when `n` is 0.
    pub fn below(&mut self, n: usize) -> usize {
        assert!(n > 0, "Rng::below needs a positive bound");
        ((self.bits() as u64 * n as u64) >> 24) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sequence_for_a_seed_is_fixed_across_builds() {
        // Replays and saved seeds depend on these numbers never changing.
        let mut r = Rng::new(1);
        assert_eq!([r.f32(), r.f32()], [0.2364555, 0.36927062]);
    }

    #[test]
    fn same_seed_same_sequence_and_values_stay_in_range() {
        let (mut a, mut b) = (Rng::new(9), Rng::new(9));
        for _ in 0..1000 {
            let x = a.range(-2.0, 3.0);
            assert_eq!(x, b.range(-2.0, 3.0));
            assert!((-2.0..3.0).contains(&x));
        }
    }

    #[test]
    fn below_stays_under_its_bound_and_reaches_every_value() {
        let mut r = Rng::new(5);
        let mut seen = [0; 3];
        for _ in 0..300 {
            seen[r.below(3)] += 1;
        }
        assert!(seen.iter().all(|&n| n > 60), "{seen:?}");
        assert_eq!(r.below(1), 0);
    }
}
