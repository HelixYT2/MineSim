//! `LegacyRandomSource`: the 48-bit linear congruential generator behind
//! `RandomSource.create(seed)`, which every entity's `random` field uses.
//!
//! `SingleThreadedRandomSource` and `ThreadSafeLegacyRandomSource` run the identical algorithm
//! (they differ only in how they guard the seed against concurrent use), so they are the same
//! type here.

use crate::gaussian::MarsagliaPolarGaussian;
use crate::{RandomSource, DOUBLE_UNIT, FLOAT_UNIT};
use ms_numerics::{hotspot, mth};

const MULTIPLIER: u64 = 25214903917;
const INCREMENT: u64 = 11;
const MASK: u64 = (1 << 48) - 1;

/// `LegacyRandomSource` (a.k.a. `SingleThreadedRandomSource`, `ThreadSafeLegacyRandomSource`).
#[derive(Clone, Debug)]
pub struct LegacyRandomSource {
    seed: u64,
    gaussian: MarsagliaPolarGaussian,
}

/// `SingleThreadedRandomSource` is [`LegacyRandomSource`] without the atomics.
pub type SingleThreadedRandomSource = LegacyRandomSource;

impl LegacyRandomSource {
    /// `new LegacyRandomSource(long)`.
    pub fn new(seed: i64) -> Self {
        Self {
            seed: scramble(seed),
            gaussian: MarsagliaPolarGaussian::new(),
        }
    }

    /// `BitRandomSource.next(bits)`: advances the LCG and returns its top `bits` bits.
    pub fn next(&mut self, bits: u32) -> i32 {
        self.seed = self.seed.wrapping_mul(MULTIPLIER).wrapping_add(INCREMENT) & MASK;
        (self.seed >> (48 - bits)) as i32
    }

    /// `LegacyRandomSource.fork()`: a new source seeded by the next long.
    pub fn fork(&mut self) -> LegacyRandomSource {
        LegacyRandomSource::new(self.next_long())
    }

    /// `LegacyRandomSource.forkPositional()`: the factory is seeded by the next long.
    pub fn fork_positional(&mut self) -> LegacyPositionalRandomFactory {
        LegacyPositionalRandomFactory::new(self.next_long())
    }

    /// `nextGaussian()` with an explicit logarithm (see [`MarsagliaPolarGaussian`]).
    pub fn next_gaussian_with_log(&mut self, log: fn(f64) -> f64) -> f64 {
        let mut gaussian = self.gaussian;
        let value = gaussian.next_gaussian(|| self.next_double(), log);
        self.gaussian = gaussian;
        value
    }
}

fn scramble(seed: i64) -> u64 {
    (seed as u64 ^ MULTIPLIER) & MASK
}

impl RandomSource for LegacyRandomSource {
    fn set_seed(&mut self, seed: i64) {
        self.seed = scramble(seed);
        self.gaussian.reset();
    }

    fn next_int(&mut self) -> i32 {
        self.next(32)
    }

    fn next_int_bound(&mut self, bound: i32) -> i32 {
        assert!(bound > 0, "Bound must be positive");
        if bound & (bound - 1) == 0 {
            return ((i64::from(bound) * i64::from(self.next(31))) >> 31) as i32;
        }
        loop {
            let j = self.next(31);
            let k = j % bound;
            if j.wrapping_sub(k).wrapping_add(bound - 1) >= 0 {
                return k;
            }
        }
    }

    fn next_long(&mut self) -> i64 {
        let hi = self.next(32);
        let lo = self.next(32);
        (i64::from(hi) << 32).wrapping_add(i64::from(lo))
    }

    fn next_boolean(&mut self) -> bool {
        self.next(1) != 0
    }

    fn next_float(&mut self) -> f32 {
        self.next(24) as f32 * FLOAT_UNIT
    }

    fn next_double(&mut self) -> f64 {
        let hi = i64::from(self.next(26));
        let lo = i64::from(self.next(27));
        ((hi << 27) + lo) as f64 * DOUBLE_UNIT
    }

    fn next_gaussian(&mut self) -> f64 {
        self.next_gaussian_with_log(hotspot::log)
    }
}

/// `LegacyRandomSource.LegacyPositionalRandomFactory`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LegacyPositionalRandomFactory {
    seed: i64,
}

impl LegacyPositionalRandomFactory {
    pub const fn new(seed: i64) -> Self {
        Self { seed }
    }

    /// `at(x, y, z)`: a source seeded from the block position hash.
    pub fn at(&self, x: i32, y: i32, z: i32) -> LegacyRandomSource {
        LegacyRandomSource::new(mth::get_seed(x, y, z) ^ self.seed)
    }

    /// `fromHashOf(String)`: `String.hashCode()` xor the factory seed.
    pub fn from_hash_of(&self, name: &str) -> LegacyRandomSource {
        let hash = i64::from(crate::support::java_string_hash(name));
        LegacyRandomSource::new(hash ^ self.seed)
    }

    /// `fromSeed(long)`.
    pub fn from_seed(&self, seed: i64) -> LegacyRandomSource {
        LegacyRandomSource::new(seed)
    }
}
