pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Rng(seed)
    }

    pub fn f32(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_sequence_and_values_stay_in_range() {
        let (mut a, mut b) = (Rng::new(9), Rng::new(9));
        for _ in 0..1000 {
            let x = a.range(-2.0, 3.0);
            assert_eq!(x, b.range(-2.0, 3.0));
            assert!((-2.0..3.0).contains(&x));
        }
    }
}
