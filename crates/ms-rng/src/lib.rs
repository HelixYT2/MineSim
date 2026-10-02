//! The game's random number generators, bit-exact.
//!
//! * [`JavaRandom`]: `java.util.Random`, the 48-bit LCG the JVM ships.
//! * [`LegacyRandomSource`]: `net.minecraft.world.level.levelgen.LegacyRandomSource`, the
//!   generator behind `RandomSource.create(seed)`. Every entity's `random` field is one of these
//!   (vanilla seeds it from `RandomSupport.generateUniqueSeed()`, i.e. non-deterministically; the
//!   simulator seeds it explicitly).
//! * [`XoroshiroRandomSource`]: the Xoroshiro128++ generator used by world generation, with
//!   [`support`] (`RandomSupport` seeding: Stafford mix, 128-bit upgrade, MD5 `seedFromHashOf`).
//!
//! All draw methods follow the game's exact semantics (`nextInt(bound)` rejection loops,
//! `nextFloat`/`nextDouble` bit layouts, `triangle`, `consumeCount`, `fork`, positional
//! factories). `nextGaussian` is the Marsaglia polar method; the game calls `Math.log` for it,
//! which on HotSpot/x86_64 is *not* `StrictMath.log` (see [`ms_numerics::hotspot`] for the
//! platform notes), while `java.util.Random` uses `StrictMath.log` (fdlibm,
//! [`ms_numerics::fdlibm::log`]).
//!
//! Every generator is validated against sequences recorded from the real classes; see
//! `crates/ms-rng/testdata/` and `tools/refgen/README.md`.
//!
//! Invalid arguments that make the Java methods throw (`nextInt(bound <= 0)`,
//! `nextInt(origin >= bound)`) panic here.

#![forbid(unsafe_code)]

mod gaussian;
mod legacy;
pub mod support;
mod xoroshiro;

pub use gaussian::MarsagliaPolarGaussian;
pub use legacy::{LegacyPositionalRandomFactory, LegacyRandomSource, SingleThreadedRandomSource};
pub use xoroshiro::{
    Xoroshiro128PlusPlus, XoroshiroPositionalRandomFactory, XoroshiroRandomSource,
};

use ms_numerics::fdlibm;

const MULTIPLIER: u64 = 0x5_DEEC_E66D;
const INCREMENT: u64 = 0xB;
const MASK: u64 = (1 << 48) - 1;

/// `BitRandomSource.DOUBLE_MULTIPLIER` as the game's bytecode uses it: the double 2^-53 (the
/// float literal `1.110223E-16F` widened).
pub(crate) const DOUBLE_UNIT: f64 = 1.110_223_024_625_156_5e-16;
/// `BitRandomSource.FLOAT_MULTIPLIER`: the float 2^-24.
pub(crate) const FLOAT_UNIT: f32 = 5.960_464_5e-8;

/// `net.minecraft.util.RandomSource`: the draw methods every game generator implements, with the
/// interface's default methods.
pub trait RandomSource {
    /// `setSeed(long)`; also discards a cached gaussian.
    fn set_seed(&mut self, seed: i64);
    /// `nextInt()`.
    fn next_int(&mut self) -> i32;
    /// `nextInt(bound)`; panics when `bound <= 0` (Java throws).
    fn next_int_bound(&mut self, bound: i32) -> i32;
    /// `nextLong()`.
    fn next_long(&mut self) -> i64;
    /// `nextBoolean()`.
    fn next_boolean(&mut self) -> bool;
    /// `nextFloat()`: a multiple of 2^-24 in `[0, 1)`.
    fn next_float(&mut self) -> f32;
    /// `nextDouble()`: a multiple of 2^-53 in `[0, 1)`.
    fn next_double(&mut self) -> f64;
    /// `nextGaussian()`: the polar method with the game's `Math.log`.
    fn next_gaussian(&mut self) -> f64;

    /// `nextIntBetweenInclusive(min, max)` = `nextInt(max - min + 1) + min`.
    fn next_int_between_inclusive(&mut self, min: i32, max: i32) -> i32 {
        self.next_int_bound(max.wrapping_sub(min).wrapping_add(1))
            .wrapping_add(min)
    }

    /// `nextInt(origin, bound)` = `origin + nextInt(bound - origin)`; panics unless
    /// `origin < bound`.
    fn next_int_range(&mut self, origin: i32, bound: i32) -> i32 {
        assert!(origin < bound, "bound - origin is non positive");
        origin.wrapping_add(self.next_int_bound(bound.wrapping_sub(origin)))
    }

    /// `triangle(double mode, double deviation)` = `mode + deviation * (nextDouble() - nextDouble())`.
    fn triangle(&mut self, mode: f64, deviation: f64) -> f64 {
        let a = self.next_double();
        let b = self.next_double();
        mode + deviation * (a - b)
    }

    /// `triangle(float mode, float deviation)`.
    fn triangle_f32(&mut self, mode: f32, deviation: f32) -> f32 {
        let a = self.next_float();
        let b = self.next_float();
        mode + deviation * (a - b)
    }

    /// `consumeCount(count)`: draws and discards `count` values (`nextInt()` each; the
    /// Xoroshiro source advances its generator directly).
    fn consume_count(&mut self, count: i32) {
        for _ in 0..count {
            self.next_int();
        }
    }
}

/// `java.util.Random`.
#[derive(Clone, Debug)]
pub struct JavaRandom {
    seed: u64,
    gaussian: MarsagliaPolarGaussian,
}

impl JavaRandom {
    pub fn new(seed: i64) -> Self {
        Self {
            seed: (seed as u64 ^ MULTIPLIER) & MASK,
            gaussian: MarsagliaPolarGaussian::new(),
        }
    }

    /// `setSeed`: also discards a cached gaussian.
    pub fn set_seed(&mut self, seed: i64) {
        self.seed = (seed as u64 ^ MULTIPLIER) & MASK;
        self.gaussian.reset();
    }

    fn next(&mut self, bits: u32) -> i32 {
        self.seed = self.seed.wrapping_mul(MULTIPLIER).wrapping_add(INCREMENT) & MASK;
        (self.seed >> (48 - bits)) as i32
    }

    pub fn next_int(&mut self) -> i32 {
        self.next(32)
    }

    pub fn next_int_bound(&mut self, bound: i32) -> i32 {
        assert!(bound > 0, "bound must be positive");
        let m = bound - 1;
        if bound & m == 0 {
            return ((i64::from(bound) * i64::from(self.next(31))) >> 31) as i32;
        }
        let mut u = self.next(31);
        loop {
            let r = u % bound;
            if u.wrapping_sub(r).wrapping_add(m) >= 0 {
                return r;
            }
            u = self.next(31);
        }
    }

    pub fn next_long(&mut self) -> i64 {
        let hi = i64::from(self.next(32));
        let lo = i64::from(self.next(32));
        (hi << 32).wrapping_add(lo)
    }

    pub fn next_bool(&mut self) -> bool {
        self.next(1) != 0
    }

    pub fn next_float(&mut self) -> f32 {
        self.next(24) as f32 / (1u32 << 24) as f32
    }

    pub fn next_double(&mut self) -> f64 {
        let hi = i64::from(self.next(26));
        let lo = i64::from(self.next(27));
        ((hi << 27) + lo) as f64 * DOUBLE_UNIT
    }

    /// `Random.nextGaussian()`: the polar method with `StrictMath.log` / `StrictMath.sqrt`
    /// (fdlibm), so it is exact on every platform.
    pub fn next_gaussian(&mut self) -> f64 {
        let mut gaussian = self.gaussian;
        let value = gaussian.next_gaussian(|| self.next_double(), fdlibm::log);
        self.gaussian = gaussian;
        value
    }
}
