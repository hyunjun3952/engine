use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

const MULTIPLIER: u64 = 0x517c_c1b7_2722_0a95;

/// A multiplicative hasher for keys the engine makes itself (entities, ids, grid cells). SipHash defends against
/// hostile keys, which these are not, and took about 8 percent of a physics step. The same keys always hash the same
/// way, which also keeps iteration order repeatable from run to run.
#[derive(Default)]
pub(crate) struct Fx(u64);

impl Fx {
    fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(MULTIPLIER);
    }
}

impl Hasher for Fx {
    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut word = [0; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            self.add(u64::from_le_bytes(word));
        }
    }

    fn write_u8(&mut self, i: u8) {
        self.add(i.into());
    }

    fn write_u32(&mut self, i: u32) {
        self.add(i.into());
    }

    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }

    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }

    // The multiply leaves the low bits depending only on the low bits of the input, so rotate the good ones down.
    fn finish(&self) -> u64 {
        self.0.rotate_left(26)
    }
}

pub(crate) type FxMap<K, V> = HashMap<K, V, BuildHasherDefault<Fx>>;
pub(crate) type FxSet<K> = HashSet<K, BuildHasherDefault<Fx>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_of_every_width_round_trip_and_a_run_of_ids_spreads_over_the_table() {
        let mut map = FxMap::default();
        for i in 0..1000u64 {
            map.insert((i, i * 3), i);
        }
        assert!((0..1000u64).all(|i| map.get(&(i, i * 3)) == Some(&i)));
        let mut set = FxSet::default();
        set.extend(["a", "b", "ab", "ba", ""]);
        assert_eq!(set.len(), 5);
        // Sequential ids must not all land in the same few buckets: the low bits of the hashes should vary.
        let low: FxSet<u64> = (0..256u64)
            .map(|i| {
                let mut h = Fx::default();
                h.write_u64(i);
                h.finish() & 63
            })
            .collect();
        assert!(low.len() >= 48, "only {} of 64 low-bit values used", low.len());
    }
}
