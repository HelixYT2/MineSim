//! The game's random number generators.
//!
//! `java.util.Random` is the 48-bit linear congruential generator the JVM ships. Its output is
//! reproduced here exactly, including the rejection loop in bounded `nextInt` and the bit layout
//! of the float and double draws; the tests check it against sequences dumped from the JVM.
//!
//! Two pieces are intentionally absent for now: Xoroshiro128++ (the `RandomSource` used since
//! 1.18), which needs the Minecraft jar to validate its specific seeding, and `nextGaussian`,
//! which needs the fdlibm `log` port.

#![forbid(unsafe_code)]

const MULTIPLIER: u64 = 0x5_DEEC_E66D;
const INCREMENT: u64 = 0xB;
const MASK: u64 = (1 << 48) - 1;
const DOUBLE_UNIT: f64 = 1.0 / (1u64 << 53) as f64;

pub struct JavaRandom {
    seed: u64,
}

impl JavaRandom {
    pub fn new(seed: i64) -> Self {
        Self {
            seed: (seed as u64 ^ MULTIPLIER) & MASK,
        }
    }

    pub fn set_seed(&mut self, seed: i64) {
        self.seed = (seed as u64 ^ MULTIPLIER) & MASK;
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_jvm_sequences() {
        let csv = include_str!("../testdata/java_random.csv");
        let mut rng = JavaRandom::new(0);
        let mut current: Option<i64> = None;

        for line in csv.lines().skip(1) {
            let mut cols = line.split(',');
            let seed: i64 = cols.next().unwrap().parse().unwrap();
            let op = cols.next().unwrap();
            let arg: i32 = cols.next().unwrap().parse().unwrap();
            let want: u64 = cols.next().unwrap().parse().unwrap();

            if current != Some(seed) {
                rng = JavaRandom::new(seed);
                current = Some(seed);
            }

            let got = match op {
                "int" => u64::from(rng.next_int() as u32),
                "intb" => u64::from(rng.next_int_bound(arg) as u32),
                "long" => rng.next_long() as u64,
                "float" => u64::from(rng.next_float().to_bits()),
                "double" => rng.next_double().to_bits(),
                "bool" => u64::from(rng.next_bool()),
                other => panic!("unknown op {other}"),
            };
            assert_eq!(got, want, "seed={seed} op={op} arg={arg}");
        }
    }
}
