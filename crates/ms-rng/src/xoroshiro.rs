//! `Xoroshiro128PlusPlus` and `XoroshiroRandomSource`: the generator behind world generation
//! since 1.18 (not used by entities, which use [`crate::LegacyRandomSource`]).

use crate::gaussian::MarsagliaPolarGaussian;
use crate::support::{self, upgrade_seed_to_128bit, Seed128bit, GOLDEN_RATIO_64, SILVER_RATIO_64};
use crate::{RandomSource, DOUBLE_UNIT, FLOAT_UNIT};
use ms_numerics::{hotspot, mth};

/// `Xoroshiro128PlusPlus`: the raw 128-bit generator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Xoroshiro128PlusPlus {
    seed_lo: u64,
    seed_hi: u64,
}

impl Xoroshiro128PlusPlus {
    /// `new Xoroshiro128PlusPlus(long, long)`: an all-zero state (which would be stuck) is
    /// replaced by the golden/silver ratio constants.
    pub fn new(lo: i64, hi: i64) -> Self {
        if lo | hi == 0 {
            return Self {
                seed_lo: GOLDEN_RATIO_64 as u64,
                seed_hi: SILVER_RATIO_64 as u64,
            };
        }
        Self {
            seed_lo: lo as u64,
            seed_hi: hi as u64,
        }
    }

    pub fn from_seed(seed: Seed128bit) -> Self {
        Self::new(seed.seed_lo, seed.seed_hi)
    }

    /// `Xoroshiro128PlusPlus.nextLong()`.
    pub fn next_long(&mut self) -> i64 {
        let l = self.seed_lo;
        let mut m = self.seed_hi;
        let n = l.wrapping_add(m).rotate_left(17).wrapping_add(l);
        m ^= l;
        self.seed_lo = l.rotate_left(49) ^ m ^ (m << 21);
        self.seed_hi = m.rotate_left(28);
        n as i64
    }
}

/// `XoroshiroRandomSource`.
#[derive(Clone, Debug)]
pub struct XoroshiroRandomSource {
    generator: Xoroshiro128PlusPlus,
    gaussian: MarsagliaPolarGaussian,
}

impl XoroshiroRandomSource {
    /// `new XoroshiroRandomSource(long)`: the seed is widened and mixed.
    pub fn new(seed: i64) -> Self {
        Self::from_seed128(upgrade_seed_to_128bit(seed))
    }

    /// `new XoroshiroRandomSource(RandomSupport.Seed128bit)`.
    pub fn from_seed128(seed: Seed128bit) -> Self {
        Self::from_generator(Xoroshiro128PlusPlus::from_seed(seed))
    }

    /// `new XoroshiroRandomSource(long, long)`.
    pub fn from_halves(lo: i64, hi: i64) -> Self {
        Self::from_generator(Xoroshiro128PlusPlus::new(lo, hi))
    }

    fn from_generator(generator: Xoroshiro128PlusPlus) -> Self {
        Self {
            generator,
            gaussian: MarsagliaPolarGaussian::new(),
        }
    }

    /// `XoroshiroRandomSource.fork()`: a new source seeded by the next two longs (low, high).
    pub fn fork(&mut self) -> XoroshiroRandomSource {
        let lo = self.generator.next_long();
        let hi = self.generator.next_long();
        Self::from_halves(lo, hi)
    }

    /// `XoroshiroRandomSource.forkPositional()`.
    pub fn fork_positional(&mut self) -> XoroshiroPositionalRandomFactory {
        let lo = self.generator.next_long();
        let hi = self.generator.next_long();
        XoroshiroPositionalRandomFactory::new(lo, hi)
    }

    /// `nextBits(bits)`: the top `bits` bits of the next long.
    fn next_bits(&mut self, bits: u32) -> u64 {
        (self.generator.next_long() as u64) >> (64 - bits)
    }

    /// `nextGaussian()` with an explicit logarithm (see [`MarsagliaPolarGaussian`]).
    pub fn next_gaussian_with_log(&mut self, log: fn(f64) -> f64) -> f64 {
        let mut gaussian = self.gaussian;
        let value = gaussian.next_gaussian(|| self.next_double(), log);
        self.gaussian = gaussian;
        value
    }
}

impl RandomSource for XoroshiroRandomSource {
    fn set_seed(&mut self, seed: i64) {
        self.generator = Xoroshiro128PlusPlus::from_seed(upgrade_seed_to_128bit(seed));
        self.gaussian.reset();
    }

    fn next_int(&mut self) -> i32 {
        self.generator.next_long() as i32
    }

    fn next_int_bound(&mut self, bound: i32) -> i32 {
        assert!(bound > 0, "Bound must be positive");
        let bound_u = bound as u32;
        let mut l = u64::from(self.next_int() as u32);
        let mut m = l * u64::from(bound_u);
        let mut n = m & 0xffff_ffff;
        if n < u64::from(bound_u) {
            // Integer.remainderUnsigned(~bound + 1, bound)
            let threshold = u64::from((!bound_u).wrapping_add(1) % bound_u);
            while n < threshold {
                l = u64::from(self.next_int() as u32);
                m = l * u64::from(bound_u);
                n = m & 0xffff_ffff;
            }
        }
        (m >> 32) as i32
    }

    fn next_long(&mut self) -> i64 {
        self.generator.next_long()
    }

    fn next_boolean(&mut self) -> bool {
        self.generator.next_long() & 1 != 0
    }

    fn next_float(&mut self) -> f32 {
        self.next_bits(24) as f32 * FLOAT_UNIT
    }

    fn next_double(&mut self) -> f64 {
        self.next_bits(53) as f64 * DOUBLE_UNIT
    }

    fn next_gaussian(&mut self) -> f64 {
        self.next_gaussian_with_log(hotspot::log)
    }

    /// `XoroshiroRandomSource.consumeCount`: advances the generator `count` times.
    fn consume_count(&mut self, count: i32) {
        for _ in 0..count {
            self.generator.next_long();
        }
    }
}

/// `XoroshiroRandomSource.XoroshiroPositionalRandomFactory`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XoroshiroPositionalRandomFactory {
    seed_lo: i64,
    seed_hi: i64,
}

impl XoroshiroPositionalRandomFactory {
    pub const fn new(seed_lo: i64, seed_hi: i64) -> Self {
        Self { seed_lo, seed_hi }
    }

    /// `at(x, y, z)`: a source seeded from the block position hash.
    pub fn at(&self, x: i32, y: i32, z: i32) -> XoroshiroRandomSource {
        let m = mth::get_seed(x, y, z) ^ self.seed_lo;
        XoroshiroRandomSource::from_halves(m, self.seed_hi)
    }

    /// `fromHashOf(String)`.
    pub fn from_hash_of(&self, name: &str) -> XoroshiroRandomSource {
        let seed = support::seed_from_hash_of(name).xor(self.seed_lo, self.seed_hi);
        XoroshiroRandomSource::from_seed128(seed)
    }

    /// `fromSeed(long)`.
    pub fn from_seed(&self, seed: i64) -> XoroshiroRandomSource {
        XoroshiroRandomSource::from_halves(seed ^ self.seed_lo, seed ^ self.seed_hi)
    }
}
