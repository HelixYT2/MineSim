//! State hashing for differential testing against the real game.
//!
//! [`StateBuf`] serializes per-tick state into a fixed big-endian byte layout and hashes it.
//! Floats are written as raw IEEE-754 bits, so NaN compares by bit pattern and `-0.0` differs
//! from `0.0`; integers are two's-complement. The Java trace tool and the simulator must emit
//! identical bytes for identical state — that byte layout is the comparison contract
//! (`docs/contract.md`, `contract-v1`).
//!
//! [`player`] holds the frozen per-tick serialization of the player state and the rolling hash
//! used by the golden lock.

#![forbid(unsafe_code)]

pub mod player;

use xxhash_rust::xxh3::xxh3_64_with_seed;

/// Changing this invalidates every previously recorded golden hash.
pub const HASH_SEED: u64 = 0x4d49_4e45_5349_4d00;

/// `H = xxh3_64(seed = HASH_SEED, bytes)` — the hash of an already-serialized state.
pub fn hash_bytes(bytes: &[u8]) -> u64 {
    xxh3_64_with_seed(bytes, HASH_SEED)
}

#[derive(Clone, Debug, Default)]
pub struct StateBuf {
    bytes: Vec<u8>,
}

impl StateBuf {
    pub fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    pub fn push_f64(&mut self, v: f64) -> &mut Self {
        self.bytes.extend_from_slice(&v.to_bits().to_be_bytes());
        self
    }

    pub fn push_f32(&mut self, v: f32) -> &mut Self {
        self.bytes.extend_from_slice(&v.to_bits().to_be_bytes());
        self
    }

    pub fn push_i64(&mut self, v: i64) -> &mut Self {
        self.bytes.extend_from_slice(&v.to_be_bytes());
        self
    }

    pub fn push_i32(&mut self, v: i32) -> &mut Self {
        self.bytes.extend_from_slice(&v.to_be_bytes());
        self
    }

    pub fn push_u64(&mut self, v: u64) -> &mut Self {
        self.bytes.extend_from_slice(&v.to_be_bytes());
        self
    }

    pub fn push_u32(&mut self, v: u32) -> &mut Self {
        self.bytes.extend_from_slice(&v.to_be_bytes());
        self
    }

    pub fn push_u8(&mut self, v: u8) -> &mut Self {
        self.bytes.push(v);
        self
    }

    pub fn push_bool(&mut self, v: bool) -> &mut Self {
        self.bytes.push(v as u8);
        self
    }

    /// A string as a `u16` big-endian byte length followed by its UTF-8 bytes.
    ///
    /// # Panics
    /// If the string is longer than 65535 bytes (registry ids never are).
    pub fn push_str(&mut self, v: &str) -> &mut Self {
        let len = u16::try_from(v.len()).expect("string longer than 65535 bytes");
        self.bytes.extend_from_slice(&len.to_be_bytes());
        self.bytes.extend_from_slice(v.as_bytes());
        self
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    pub fn hash(&self) -> u64 {
        hash_bytes(&self.bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> StateBuf {
        let mut b = StateBuf::new();
        b.push_f64(0.08)
            .push_f64(-0.0)
            .push_f64(78.4)
            .push_f32(-90.0)
            .push_f32(0.0)
            .push_bool(true)
            .push_bool(false)
            .push_i32(i32::MIN)
            .push_i64(0x0123_4567_89ab_cdef)
            .push_u64(0x0000_5dee_ce66_d00b);
        b
    }

    #[test]
    fn serialization_is_stable() {
        assert_eq!(sample().bytes(), sample().bytes());
        assert_eq!(sample().hash(), sample().hash());
    }

    #[test]
    fn floats_use_big_endian_raw_bits() {
        let mut b = StateBuf::new();
        b.push_f64(0.08);
        assert_eq!(b.bytes(), &0.08_f64.to_bits().to_be_bytes());

        let mut r = StateBuf::new();
        r.push_f32(-90.0);
        assert_eq!(r.bytes(), &(-90.0_f32).to_bits().to_be_bytes());
    }

    #[test]
    fn nan_compares_by_bits() {
        let nan = f64::from_bits(0x7ff8_0000_0000_0000);
        assert_eq!(
            StateBuf::new().push_f64(nan).hash(),
            StateBuf::new().push_f64(nan).hash()
        );
    }

    #[test]
    fn strings_and_bytes_are_length_prefixed() {
        let mut b = StateBuf::new();
        b.push_u8(0xab).push_str("minecraft:speed");
        let mut want = vec![0xab, 0x00, 0x0f];
        want.extend_from_slice(b"minecraft:speed");
        assert_eq!(b.bytes(), want.as_slice());
        // Prefixing prevents ("ab", "c") colliding with ("a", "bc").
        let mut x = StateBuf::new();
        x.push_str("ab").push_str("c");
        let mut y = StateBuf::new();
        y.push_str("a").push_str("bc");
        assert_ne!(x.bytes(), y.bytes());
    }

    #[test]
    fn negative_zero_is_distinct() {
        let mut neg = StateBuf::new();
        neg.push_f64(-0.0);
        let mut pos = StateBuf::new();
        pos.push_f64(0.0);
        assert_ne!(neg.bytes(), pos.bytes());
    }
}
