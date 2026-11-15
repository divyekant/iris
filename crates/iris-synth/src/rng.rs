//! SplitMix64. Small, fast and the same on every platform.

pub struct Rng(u64);

impl Rng {
    /// One independent stream for a (thread, position, purpose) key.
    pub fn stream(seed: u64, thread: u32, pos: u16, purpose: u8) -> Rng {
        let key = (thread as u64) << 24 | (pos as u64) << 8 | purpose as u64;
        let mut r = Rng(seed ^ key.wrapping_mul(0xd6e8_feb8_6659_fd93));
        r.next();
        r
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A number in `0..n`. `n` must not be 0.
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// True with a chance of `per_mille` in 1000.
    pub fn chance(&mut self, per_mille: u64) -> bool {
        self.below(1000) < per_mille
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }
}
